//! HID Provider 运行时：1 枚举线程 + N 读取线程。
//!
//! **品牌无关**：设备匹配、帧解析、充电标志全部来自 [`BrandSpec`] 与 [`Registry`]，无品牌分支，新增品牌不改这里。
//!
//! 隔离靠**时间上界**而非线程池：`read_timeout = 120ms`，实测拔线后句柄立刻失效，故一个接收器挂死只会拖慢自己的读取线程。
//!
//! 三条实测结论：1. **禁止** `open(vid, pid)`：会静默打开错误集合（枚举顺序里第一个 `0x0C/0x01` 集合），拿到「成功但读不到」的句柄；2. 集合选择须「VID/PID 精确 → 品牌声明的 UsagePage」两步，型号表只按 `VID:PID` 提供型号，不含接口号；3. 只写通道不得用读句柄打开，主动查询（若品牌声明）走写通道。
//!
//! # 设备类型识别（与电量采集解耦）
//! 分类只用枚举结果里的字段（`usage_page` / `usage` / `VID` / `PID`），不额外开句柄、不发光报文、不改读取线程，对采集节奏与续航零影响。判定是纯函数（[`voltnest_provider::classify`]),本文件只做：1. 把同一 `VID:PID` 的全部接口集合聚合成证据（多接口≠多设备）；2. 从型号表取「有出处」的厂商声明（`deviceType` 恰为规范类型词时）。枚举不到标准 Usage 且型号表无类型声明时结论 `Unknown`——这就是「不猜」的全部承诺。
//!
//! # Linux（hidraw 后端）
//! Linux 走 hidapi 的静态 hidraw 后端（特性 `linux-static-hidraw`），集合路径形如 `/dev/hidraw3`：
//! 1. **权限**：读写 `/dev/hidraw*` 需要当前用户被放行，发行版通常靠 `plugdev` 组或 udev 规则做到。**本实现不安装任何 udev 规则**；权限缺失时 `open_path` 失败并上报 `access_denied`，不会静默。
//! 2. **能力**：hidraw 从报告描述符解析出真实 `usage_page`/`usage`（只有 libusb 后端恒为 0）；这两项是集合选择的唯一判据，`interface_number` 不再参与匹配。
//! 3. **无设备**：机器上没有 `/dev/hidraw*` 时枚举为空——不报错、不 panic、不阻塞，只打一条诊断后继续按 [`PRESENCE_POLL_MS`] 轮询。
//! 4. **构建期依赖**：hidapi 的 hidraw 后端用 pkg-config 探测 `libudev`，故构建机须装 libudev 开发文件（Debian / Ubuntu `libudev-dev`，Fedora `systemd-devel`）；运行期只用到系统已装的 `libudev.so.1`。

use crate::brands::{Brand, Registry};
use crate::spec::{BrandSpec, ReplyTransport};
use crate::{
    PRESENCE_POLL_MS, LISTEN_READ_TIMEOUT_MS, READ_BUFFER_LEN, READ_TIMEOUT_MS, RETRY_GAP_MS, frame,
};
use hidapi::{HidApi, HidDevice};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use voltnest_provider::{
    DeviceClassification, DeviceFact, ErrorCode, FAMILY_HID_VBAS, HID_VBAS, HidEvidence, Provider,
    ProviderEvent, ProviderProfile, RemovalReason, StableId, Transport, ValidReading,
    VerifiedMapping, classify_hid,
};

/// `hid-vbas` 家族的 Provider 实现。
#[derive(Debug, Clone, Copy)]
pub struct HidProvider {
    profile: &'static ProviderProfile,
}

impl HidProvider {
    /// 使用家族级能力声明 [`HID_VBAS`]。
    pub fn new() -> Self {
        Self { profile: &HID_VBAS }
    }
}

impl Default for HidProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for HidProvider {
    fn profile(&self) -> &'static ProviderProfile {
        self.profile
    }

    fn run(self: Box<Self>, tx: Sender<ProviderEvent>, stop: Arc<AtomicBool>) {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                let _ = tx.send(ProviderEvent::TransportDown {
                    code: ErrorCode::Internal,
                    detail: format!("HidApi::new 失败: {e}"),
                });
                return;
            }
        };

        let mut sessions: HashMap<String, SessionHandle> = HashMap::new();
        let mut announced: HashSet<String> = HashSet::new();
        // 只在「空 → 非空 / 非空 → 空」切换时打诊断，避免每 5s 刷屏。
        let mut empty_reported = false;

        while !stop.load(Ordering::Relaxed) {
            // 重新枚举（hidapi 重读设备列表），打在接收器上、不经 2.4G 链路，不影响鼠标续航——别为「怕唤醒鼠标」改成事件订阅，那是后续升级路径而非 v1 修复。
            if let Err(e) = api.refresh_devices() {
                let _ = tx.send(ProviderEvent::TransportDown {
                    code: ErrorCode::Internal,
                    detail: format!("hidapi 枚举失败: {e}"),
                });
                if !backoff_sleep(&stop, PRESENCE_POLL_MS) {
                    break;
                }
                continue;
            }

            // 枚举为空不是错误：Linux 上常见于「没有 /dev/hidraw*」或「当前用户不可读」。这里只打一条状态
            // 切换诊断，**不**派发 TransportDown（那会让上层误判传输层坏了、把既有设备全转 Offline），
            // 也不提前退出——继续按 PRESENCE_POLL_MS 轮询，插上设备即恢复。
            let enumeration_empty = api.device_list().next().is_none();
            if enumeration_empty && !empty_reported {
                eprintln!(
                    "voltnest-hid: 枚举无 HID 设备（Linux: 无 /dev/hidraw* 或当前用户不可读；无设备属正常状态）"
                );
                empty_reported = true;
            } else if !enumeration_empty && empty_reported {
                eprintln!("voltnest-hid: 枚举恢复，HID 设备列表非空");
                empty_reported = false;
            }

            let (found, conflicts) = collect_targets(&api, &usage_index(&api));
            for (key, detail) in conflicts {
                if let Some(id) = StableId::parse(&key) {
                    let _ = tx.send(ProviderEvent::BindingConflict {
                        stable_id: id,
                        detail,
                    });
                }
            }

            // 1) 新出现的设备：先上报事实，再尝试打开
            for (key, target) in &found {
                if announced.insert(key.clone()) {
                    let fact = DeviceFact::new(
                        target.stable_id.clone(),
                        Transport::Hid,
                        FAMILY_HID_VBAS,
                        target.display_name.clone(),
                        target.display_name.clone(),
                    )
                    .with_path(target.path.to_string_lossy().into_owned())
                    .with_classification(target.classification.clone());
                    let _ = tx.send(ProviderEvent::Arrived(fact));
                }
                if sessions.contains_key(key) {
                    continue;
                }
                match api.open_path(&target.path) {
                    Ok(dev) => {
                        let reader_stop = Arc::new(AtomicBool::new(false));
                        let flag = Arc::clone(&reader_stop);
                        let reader_tx = tx.clone();
                        let stable_id = target.stable_id.clone();
                        let brand = target.brand;
                        let name = format!("hid-read-{key}");
                        match thread::Builder::new().name(name).spawn(move || {
                            read_loop(dev, stable_id, brand, reader_tx, flag);
                        }) {
                            Ok(join) => {
                                sessions.insert(
                                    key.clone(),
                                    SessionHandle {
                                        stop: reader_stop,
                                        join,
                                    },
                                );
                            }
                            Err(e) => {
                                let _ = tx.send(ProviderEvent::Unreadable {
                                    stable_id: target.stable_id.clone(),
                                    code: ErrorCode::Internal,
                                    detail: Some(format!("创建读取线程失败: {e}")),
                                });
                                eprintln!("无法为 {key} 创建读取线程: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        // 官方驱动运行时共享打开可行（实测）；失败多是被独占。hidapi 原文进诊断：`access_denied` 不足以区分「被独占」与「系统权限策略拦截」。
                        let _ = tx.send(ProviderEvent::Unreadable {
                            stable_id: target.stable_id.clone(),
                            code: open_error_code(&e),
                            detail: Some(format!("open_path 失败: {e}")),
                        });
                    }
                }
            }

            // 2) 物理消失：枚举不再返回 → 关闭句柄 + Removed（唯一销毁路径）
            let gone: Vec<String> = sessions
                .keys()
                .filter(|k| !found.contains_key(*k))
                .cloned()
                .collect();
            for key in gone {
                if let Some(session) = sessions.remove(&key) {
                    session.stop.store(true, Ordering::Relaxed);
                    // 读取线程最多阻塞 READ_TIMEOUT_MS，join 上界由此保证。
                    let _ = session.join.join();
                    announced.remove(&key);
                    if let Some(id) = StableId::parse(&key) {
                        let _ = tx.send(ProviderEvent::Removed {
                            stable_id: id,
                            reason: RemovalReason::EnumerationGone,
                        });
                    }
                }
            }

            // 3) 读线程已自行退出（错误已上报）：丢弃会话，下一轮重开
            let dead: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| s.join.is_finished())
                .map(|(k, _)| k.clone())
                .collect();
            for key in dead {
                if let Some(session) = sessions.remove(&key) {
                    let _ = session.join.join();
                }
            }

            if !backoff_sleep(&stop, PRESENCE_POLL_MS) {
                break;
            }
        }

        // 退出：先置位再 join，保证不泄漏句柄与线程。
        for (_, session) in sessions.drain() {
            session.stop.store(true, Ordering::Relaxed);
            let _ = session.join.join();
        }
    }
}

/// 一条采集会话的句柄：停止标志 + 线程句柄。
///
/// 与 `voltnest_ble::runtime::SessionHandle` 职责一致（曾名 `Session`）。
struct SessionHandle {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

struct HidTarget {
    stable_id: StableId,
    path: CString,
    display_name: String,
    /// 命中的品牌（决定字段位置与查询方式）。
    brand: &'static BrandSpec,
    /// 类型识别结果（来自枚举元数据 + 型号表声明）。
    classification: DeviceClassification,
}

/// 把**同一 VID:PID 的全部接口**的 Usage 聚合成一份证据。
///
/// 遍历整张枚举表而非只看命中的集合：设备可能是复合 HID（厂商私有集合在 MI_01、标准鼠标集合在 MI_00），只看一个接口会漏标准证据。聚合键是 `VID:PID`（= `stableId` 粒度），故**多接口不会被当成多台设备**。
///
/// `usage_page()` / `usage()` 在 Linux **libusb** 后端恒为 0（hidapi 官方文档）；Windows 的 hid.dll 与 Linux 的 **hidraw** 后端都从报告描述符解析出真实值，故这两类平台的 Usage 规则必须照常生效。字段到底可不可用由 [`HidEvidence::push_collection`] 判定（`usage_page == 0` 即不可用），平台差异不在这里分支。
fn usage_index(api: &HidApi) -> HashMap<(u16, u16), HidEvidence> {
    let mut index: HashMap<(u16, u16), HidEvidence> = HashMap::new();
    for info in api.device_list() {
        let key = (info.vendor_id(), info.product_id());
        index
            .entry(key)
            .or_insert_with(|| HidEvidence::new(key.0, key.1))
            .push_collection(info.usage_page(), info.usage());
    }
    index
}

/// 生成一台设备的分类：先收集 Usage 候选，再叠加**有出处**的厂商声明。型号表映射仅当 `deviceType` 恰好是规范类型词时生效（见 [`crate::table::Entry::declared_device_type`]），来源串写进证据便于事后核对。
fn classify_target(
    brand: &Brand,
    spec: &'static BrandSpec,
    vid: u16,
    pid: u16,
    usages: &HashMap<(u16, u16), HidEvidence>,
) -> DeviceClassification {
    let Some(entry) = brand.table.lookup(vid, pid) else {
        return DeviceClassification::unknown();
    };
    let verified = entry.declared_device_type().map(|device_type| {
        VerifiedMapping::new(
            device_type,
            format!(
                "{} · {} deviceType={}",
                brand.table.source, spec.id, entry.device_type
            ),
        )
    });
    let fallback = HidEvidence::new(vid, pid);
    let evidence = usages.get(&(vid, pid)).unwrap_or(&fallback);
    classify_hid(evidence, verified.as_ref())
}

/// 枚举结果里的一条**集合记录**。
///
/// `device_list()` 的每一项对应一个 HID 集合，但两个后端的展开方式不同：Windows 上每个集合是独立设备记录；
/// Linux hidraw 上同一个 `/dev/hidraw*` 节点里的多个 Usage 会展开成多条记录、**共用同一 `path`**。
/// 先把枚举收敛成这个平台无关的最小结构，集合选择才能写成可测的纯函数。
struct EnumeratedCollection {
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
    path: CString,
}

/// 把 hidapi 的枚举结果收敛成 [`EnumeratedCollection`] 列表。
fn enum_entries(api: &HidApi) -> Vec<EnumeratedCollection> {
    api.device_list()
        .map(|info| EnumeratedCollection {
            vid: info.vendor_id(),
            pid: info.product_id(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            path: info.path().to_owned(),
        })
        .collect()
}

/// 用当前枚举结果筛选候选集合。
fn collect_targets(
    api: &HidApi,
    usages: &HashMap<(u16, u16), HidEvidence>,
) -> (HashMap<String, HidTarget>, Vec<(String, String)>) {
    select_targets(&enum_entries(api), usages)
}

/// 按「VID/PID 精确 → 品牌声明的 UsagePage（+ 可选 Usage）」筛选候选集合。返回 `(本轮候选, 冲突告警)`：同一轮出现两个相同 `stableId`（两只同型号接收器）时只保留先发现的一条并上报 `BindingConflict`。
///
/// 平台差异在这里收口：`stableId` 相同但 `path` 相同 = 同一物理集合的多条 Usage 记录（hidraw 特性），不算冲突；只有 `path` 不同才是真的两只同型号接收器。
fn select_targets(
    entries: &[EnumeratedCollection],
    usages: &HashMap<(u16, u16), HidEvidence>,
) -> (HashMap<String, HidTarget>, Vec<(String, String)>) {
    let registry = Registry::embedded();
    let mut found: HashMap<String, HidTarget> = HashMap::new();
    let mut conflicts: Vec<(String, String)> = Vec::new();

    // 两遍：先按主页面，再按备用页面补「主页面零命中」的型号。
    // 分遍是为了让「两个页面都存在」的设备稳定选主页面 —— 单遍的话结果取决于枚举顺序，
    // 先枚举到备用页面就会把它当主页面，还会把真正的主页面报成「同 VID:PID 冲突」。
    for alt in [false, true] {
        for info in entries {
            consider_target(info, alt, registry, usages, &mut found, &mut conflicts);
        }
    }
    (found, conflicts)
}

/// 单条枚举记录参与一次候选筛选。
///
/// `alt = true` 表示这一遍只认品牌声明的**备用** UsagePage，且不得覆盖已选中的集合。
fn consider_target(
    info: &EnumeratedCollection,
    alt: bool,
    registry: &Registry,
    usages: &HashMap<(u16, u16), HidEvidence>,
    found: &mut HashMap<String, HidTarget>,
    conflicts: &mut Vec<(String, String)>,
) {
    let (vid, pid) = (info.vid, info.pid);
    let Some(brand) = registry
        .brands()
        .iter()
        .find(|b| b.table.lookup(vid, pid).is_some())
    else {
        return;
    };
    let Some(entry) = brand.table.lookup(vid, pid) else {
        return;
    };
    // 品牌可声明「按型号表字段选协议」的钩子（达尔优按 `fwType`、雷柏按 `driver` 分派）；
    // 没有钩子的品牌原样返回自己的 spec，判定与引入钩子前完全一致。
    let Some(spec) = brand.spec.for_device(entry) else {
        return;
    };
    // `usage_page == 0` 是通配：官方按「集合实现了哪个报告 ID」选，而 hidapi 枚举拿不到
    // 报告描述符里的报告 ID 列表，故这类设备只能接受任意集合。
    let page_ok = if alt {
        spec.usage_page_alt == Some(info.usage_page)
    } else {
        spec.usage_page == 0 || spec.usage_page == info.usage_page
    };
    if !page_ok {
        return;
    }
    // 品牌声明了 Usage 就只在它上面打开：同一 UsagePage 下可能并存「配置页 / 升级页」，
    // 只按 UsagePage 匹配会按枚举顺序随机选中一个。
    if spec.usage.is_some_and(|usage| usage != info.usage) {
        return;
    }
    let stable_id = StableId::hid(vid, pid);
    let key = stable_id.as_str().to_string();
    if let Some(existing) = found.get(&key) {
        // 备用页面这一遍永远让位给主页面。
        if alt {
            return;
        }
        if existing.path == info.path {
            return;
        }
        // 通配 UsagePage 的品牌（迈从音频）本来就不知道哪个集合是对的：同一台设备的多个集合
        // 逐个枚举是常态，不是「两只同型号接收器」，不报冲突。
        if spec.usage_page == 0 {
            return;
        }
        if !conflicts.iter().any(|(k, _)| k == &key) {
            conflicts.push((
                key.clone(),
                format!(
                    "同一轮枚举出现多个相同 VID/PID 的 0x{:04X}/0x{:04X}（UsagePage/Usage）集合；保留先发现的一条",
                    spec.usage_page, info.usage
                ),
            ));
        }
        return;
    }
    found.insert(
        key,
        HidTarget {
            stable_id,
            path: info.path.clone(),
            display_name: entry
                .name
                .clone()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("0x{vid:04X}:0x{pid:04X}")),
            brand: spec,
            classification: classify_target(brand, spec, vid, pid, usages),
        },
    );
}

/// 读取线程：按品牌描述解码；仅当品牌声明了查询模板时才发报文。
fn read_loop(
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
) {
    match brand.transport {
        // 纯监听不向设备发任何报文：即使 spec 误写了 `request` 也不发（`Listen` 的语义优先）。
        ReplyTransport::Listen => read_input_loop(dev, stable_id, brand, tx, stop, None),
        // output 查询：有没有 `request` 决定「查完再读」还是「只读」。
        ReplyTransport::InterruptQuery => {
            read_input_loop(dev, stable_id, brand, tx, stop, brand.request)
        }
        ReplyTransport::FeatureQuery { reply_len } => {
            read_feature_query_loop(dev, stable_id, brand, reply_len, tx, stop)
        }
        ReplyTransport::FeaturePoll { reply_len } => {
            read_feature_poll_loop(dev, stable_id, brand, reply_len, tx, stop)
        }
    }
}

/// 处理主动 output 查询和被动 input 读取；`request=None` 即纯监听。
fn read_input_loop(
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
    request: Option<crate::spec::RequestTemplate>,
) {
    let mut buf = [0u8; READ_BUFFER_LEN];
    let mut next_query = request.map(|_| Instant::now());
    let mut reassembler = frame::Reassembler::new();

    while !stop.load(Ordering::Relaxed) {
        if let Some(req) = request
            && next_query.is_some_and(|t| Instant::now() >= t)
        {
            // 新一次查询前先丢半包：半包拼出来的电量不可信。
            if brand.reassembly.is_some() {
                reassembler.reset();
            }
            let _ = dev.write(&req.build(brand.frame.report_id));
            next_query = req
                .interval_ms
                .map(|ms| Instant::now() + Duration::from_millis(ms));
        }

        match dev.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => {}
            Ok(n) => {
                let report = &buf[..n];
                // 实时上报通道：只认电量帧。输入报告里混着位移、媒体键等无关帧，
                // 把它们送去解析只会得到一堆 `cmd_mismatch`，所以这里直接丢。
                if brand.listen.is_some()
                    && let Some(reading) = frame::decode_listen(brand, report)
                {
                    if !send_reading(&tx, &stable_id, reading) {
                        return;
                    }
                    continue;
                }
                // 多包应答通道：命令字不同的包（含上面那种监听帧）在这里被过滤掉。
                if brand.reassembly.is_some() {
                    if let Some(assembled) = reassembler.feed(brand, report, Instant::now())
                        && let Some(reading) = frame::decode_assembled(brand, &assembled)
                        && !send_reading(&tx, &stable_id, reading)
                    {
                        return;
                    }
                    continue;
                }
                // output 查询通道：命令字对不上的帧不是本次查询的应答（官方也按命令字过滤）。
                if matches!(brand.transport, ReplyTransport::InterruptQuery)
                    && !frame::matches_cmd(brand, report)
                {
                    continue;
                }
                if !publish_report(&stable_id, brand, report, &tx) {
                    return;
                }
            }
            Err(_) => {
                send_link_lost(&tx, stable_id);
                return;
            }
        }
    }
}

fn read_feature_query_loop(
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    reply_len: usize,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
) {
    let Some(req) = brand.request else {
        return;
    };
    let Some(primary) = brand.frame.report_id else {
        return;
    };
    let mut reply = vec![0u8; reply_len];
    // 同一固件家族在不同连接方式下报告 ID 不同（达尔优自研：有线 `0x00` / 2.4G 接收器 `0x07`，
    // 网页驱动按 `WirelessFlag` 选择），而枚举结果里拿不到该标志 —— 故按「主 → 备」尝试，
    // 任一 ID 读出有效帧即锁定，不来回抖动。
    let mut report_id = primary;
    let mut next_query = Instant::now();
    let mut buf = [0u8; READ_BUFFER_LEN];
    while !stop.load(Ordering::Relaxed) {
        // 声明了实时上报通道且还没到下次查询时间：先听设备自己报，不打扰它。
        // 官方驱动正是这么做的：连上时查询一次拿到权威值，之后靠输入报告实时刷新。
        if brand.listen.is_some() && Instant::now() < next_query {
            match dev.read_timeout(&mut buf, LISTEN_READ_TIMEOUT_MS) {
                Ok(0) => {}
                Ok(n) => {
                    if let Some(reading) = frame::decode_listen(brand, &buf[..n])
                        && !send_reading(&tx, &stable_id, reading)
                    {
                        return;
                    }
                }
                Err(_) => {
                    send_link_lost(&tx, stable_id);
                    return;
                }
            }
            continue;
        }

        // 一轮采集 = 首发 + `retry` 次重试（官方驱动的 `retryTime` 语义：一次未应答不代表设备不可读）。
        // 只有整轮都解不出有效帧才上报一次 `Unreadable` —— 重试过程中的失败只消耗次数，不上报。
        for attempt in 0..req.attempts() {
            if attempt > 0 && !backoff_sleep(&stop, RETRY_GAP_MS) {
                return;
            }
            let query = req.build(Some(report_id));
            if dev.send_feature_report(&query).is_err() {
                // 报告 ID 不被该集合接受时，发送会直接失败（不是「回了个空帧」）：
                // 声明了备用 ID 就先换过去试，否则整轮都是无效重试。
                if let Some(alt) = brand.fallback_report_id
                    && alt != report_id
                {
                    report_id = alt;
                    if !backoff_sleep(&stop, 50) {
                        return;
                    }
                    continue;
                }
                if !backoff_sleep(&stop, 50) {
                    return;
                }
                continue;
            }
            reply.fill(0);
            reply[0] = report_id;
            match dev.get_feature_report(&mut reply) {
                Ok(n) if n > 0 => {
                    let event = reading_event(&stable_id, brand, &reply[..n]);
                    if matches!(event, ProviderEvent::Unreadable { .. }) {
                        // 解不出有效帧且还有备用 ID：先换 ID 重问，不把这次失败当设备错误上报。
                        // 换 ID 会消耗一次尝试次数，避免「主备都不应答」时无限重问。
                        if let Some(alt) = brand.fallback_report_id
                            && alt != report_id
                        {
                            report_id = alt;
                            continue;
                        }
                        // 还有重试次数：留给下一轮尝试。
                        if attempt + 1 < req.attempts() {
                            continue;
                        }
                    }
                    if tx.send(event).is_err() {
                        return;
                    }
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    send_link_lost(&tx, stable_id);
                    return;
                }
            }
        }
        if let Some(ms) = req.interval_ms {
            next_query = Instant::now() + Duration::from_millis(ms);
        }
        // 纯查询通道（无实时上报）在这里等间隔；有实时上报的通道由上面的读分支自然阻塞。
        if brand.listen.is_none() && !backoff_sleep(&stop, req.interval_ms.unwrap_or(0)) {
            return;
        }
    }
}

fn read_feature_poll_loop(
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    reply_len: usize,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
) {
    let Some(report_id) = brand.frame.report_id else {
        return;
    };
    let mut reply = vec![0u8; reply_len];
    while !stop.load(Ordering::Relaxed) {
        reply.fill(0);
        reply[0] = report_id;
        match dev.get_feature_report(&mut reply) {
            Ok(n) if n > 0 => {
                if !publish_report(&stable_id, brand, &reply[..n], &tx) {
                    return;
                }
            }
            Ok(_) => {}
            Err(_) => {
                send_link_lost(&tx, stable_id);
                return;
            }
        }
        if !backoff_sleep(&stop, brand.expected_interval_ms) {
            return;
        }
    }
}

fn publish_report(
    stable_id: &StableId,
    brand: &'static BrandSpec,
    report: &[u8],
    tx: &Sender<ProviderEvent>,
) -> bool {
    tx.send(reading_event(stable_id, brand, report)).is_ok()
}

/// 一帧 → 一个事件：解析成功是读数，失败是带成因与原始字节的不可读。
fn reading_event(stable_id: &StableId, brand: &'static BrandSpec, report: &[u8]) -> ProviderEvent {
    match frame::decode_battery_detailed(brand, report) {
        Ok(reading) => reading_event_of(stable_id, reading),
        Err(reject) => ProviderEvent::Unreadable {
            stable_id: stable_id.clone(),
            code: reject.code,
            detail: Some(reject.detail()),
        },
    }
}

/// 读数 → 事件。三处直接读（监听 / 多包组装 / 订阅）与一处解析后读共用这一条构造。
fn reading_event_of(stable_id: &StableId, reading: frame::BatteryReading) -> ProviderEvent {
    ProviderEvent::Reading(ValidReading::new(
        stable_id.clone(),
        reading.percent,
        reading.charging,
    ))
}

/// 上报一次读数；`false` = 接收端已断开，读线程应退出。
fn send_reading(
    tx: &Sender<ProviderEvent>,
    stable_id: &StableId,
    reading: frame::BatteryReading,
) -> bool {
    tx.send(reading_event_of(stable_id, reading)).is_ok()
}

/// 句柄失效（拔线 / 接收器掉电）：只报这一次，读线程随后退出。
fn send_link_lost(tx: &Sender<ProviderEvent>, stable_id: StableId) {
    let _ = tx.send(ProviderEvent::LinkLost {
        stable_id,
        code: ErrorCode::DeviceGone,
    });
}

fn open_error_code(err: &hidapi::HidError) -> ErrorCode {
    let text = err.to_string().to_lowercase();
    if text.contains("denied") || text.contains("access") || text.contains("拒绝") {
        ErrorCode::AccessDenied
    } else {
        ErrorCode::Internal
    }
}

/// 可被 `stop` 打断的睡眠；返回 `false` 表示需要退出。
fn backoff_sleep(stop: &AtomicBool, total_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(total_ms);
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !stop.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnest_provider::{ClassificationMethod, DeviceType};

    /// 已验证设备（AJ159 NL 2.4G）：型号表 `deviceType=mouse` 是有出处的声明。
    #[test]
    fn verified_device_mapping_classifies_with_source() {
        let reg = Registry::embedded();
        let (brand, _) = reg.lookup(0x7768, 0x6426).expect("已验证设备必须在表内");
        let c = classify_target(brand, brand.spec, 0x7768, 0x6426, &HashMap::new());
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);
        assert!(
            c.evidence.iter().any(|e| e.contains("config.xml")),
            "必须能追溯到来源：{:?}",
            c.evidence
        );
    }

    #[test]
    fn standard_usage_from_another_interface_wins_over_mapping() {
        // 复合 HID：厂商私有集合是命中的那个，但同 VID:PID 的另一个接口声明了
        // 标准鼠标集合 —— 聚合后被采纳为更高优先级的证据。
        let reg = Registry::embedded();
        let (brand, _) = reg.lookup(0x7768, 0x6426).unwrap();
        let mut evidence = HidEvidence::new(0x7768, 0x6426);
        evidence.push_collection(0x0001, 0x0002);
        evidence.push_collection(0xFFB2, 0x0001);
        let mut usages = HashMap::new();
        usages.insert((0x7768, 0x6426), evidence);
        let c = classify_target(brand, brand.spec, 0x7768, 0x6426, &usages);
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::HidUsage);
        assert!(c.evidence.iter().any(|e| e.contains("可核验映射")));
    }

    #[test]
    fn unlisted_vendor_usage_is_unknown() {
        // 只有厂商自定义 UsagePage：标准里没有定义，不得猜成 Mouse。
        let reg = Registry::embedded();
        let (brand, _) = reg.lookup(0x7768, 0x6426).unwrap();
        let mut evidence = HidEvidence::new(0x7768, 0x6426);
        evidence.push_collection(0xFFB2, 0x0001);
        let mut usages = HashMap::new();
        usages.insert((0x7768, 0x6426), evidence);
        let c = classify_target(brand, brand.spec, 0x7768, 0x6426, &usages);
        // 型号表仍声明 mouse，所以结论仍成立，但未命中的 Usage 只留在 skipped 里。
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);
        assert!(
            c.evidence.iter().all(|e| !e.contains("未支持")),
            "未支持的 Usage 属于 debug 信息，不得进结论证据：{:?}",
            c.evidence
        );
        assert!(c.skipped.iter().any(|e| e.contains("未支持")));
    }

    /// 真实设备端到端：`DeviceInfo` 枚举 → Usage 聚合 → 分类。
    ///
    /// `#[ignore]`：需插着已验证接收器，默认不跑。手动：`cargo test -p voltnest-hid -- --ignored real_receiver`。断言「枚举到的真实集合」而非写死期望值：该接收器同时声明鼠标与键盘集合，必走「复合集合 + 可核验映射」路径。
    #[test]
    #[ignore = "需要接上已验证的 AJAZZ 2.4G 接收器"]
    fn real_receiver_classifies_from_live_enumeration() {
        let api = HidApi::new().expect("hidapi 初始化失败");
        let usages = usage_index(&api);
        let (found, conflicts) = collect_targets(&api, &usages);
        assert!(conflicts.is_empty(), "单台设备不应触发冲突");

        let target = found
            .get("hid:7768:6426")
            .expect("未枚举到 7768:6426，请确认接收器已插入");
        let c = &target.classification;
        assert_eq!(c.device_type, DeviceType::Mouse, "证据：{:?}", c.evidence);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);

        // 枚举证据本身必须真实存在：至少一个标准鼠标集合 + 一个标准键盘集合。
        let ev = usages.get(&(0x7768, 0x6426)).expect("聚合证据缺失");
        // Windows 的 hid.dll 与 Linux 的 hidraw 后端都从报告描述符解析 Usage（只有 libusb 后端恒为 0），
        // 故「有 Usage 字段」这条断言在两个受支持后端上都成立，不因平台放宽。
        assert!(
            ev.usage_available,
            "受支持后端（Windows hid.dll / Linux hidraw）必须提供 usage 字段"
        );
        assert!(
            ev.collections
                .iter()
                .any(|c| (c.usage_page, c.usage) == (1, 2)),
            "未枚举到鼠标集合：{:?}",
            ev.collections
        );
        assert!(
            ev.collections
                .iter()
                .any(|c| (c.usage_page, c.usage) == (1, 6)),
            "未枚举到键盘集合（本机型为复合集合）：{:?}",
            ev.collections
        );
    }

    /// 构造一条枚举记录（`path` 用字符串写得直白，便于断言选中了哪个集合）。
    fn entry(vid: u16, pid: u16, usage_page: u16, usage: u16, path: &str) -> EnumeratedCollection {
        EnumeratedCollection {
            vid,
            pid,
            usage_page,
            usage,
            path: CString::new(path).expect("路径不含 NUL"),
        }
    }

    fn path_of(target: &HidTarget) -> String {
        target.path.to_str().expect("路径是 UTF-8").to_string()
    }

    /// 集合选择：只有命中品牌 UsagePage 的集合才被选中，标准鼠标集合（0x0001/0x0002）不得抢走厂商私有集合。
    #[test]
    fn selects_collection_matching_brand_usage_page() {
        let mut usages = HashMap::new();
        let mut ev = HidEvidence::new(0x7768, 0x6426);
        ev.push_collection(0x0001, 0x0002);
        ev.push_collection(0xFFB2, 0x0001);
        usages.insert((0x7768, 0x6426), ev);
        let entries = vec![
            entry(0x7768, 0x6426, 0x0001, 0x0002, "/dev/hidraw0"),
            entry(0x7768, 0x6426, 0xFFB2, 0x0001, "/dev/hidraw2"),
        ];
        let (found, conflicts) = select_targets(&entries, &usages);
        assert!(conflicts.is_empty(), "不应有冲突：{conflicts:?}");
        let target = found.get("hid:7768:6426").expect("必须选中厂商私有集合");
        assert_eq!(path_of(target), "/dev/hidraw2");
        assert_eq!(target.classification.method, ClassificationMethod::HidUsage);
    }

    /// 迈从键鼠的官方过滤器同时声明 `0xFF01` 与 `0xFF0B`：主页面零命中时必须降级到备用页面，
    /// 否则这批设备会「枚举得到但一个候选都没有」——静默消失，比读不到更难查。
    #[test]
    fn alt_usage_page_covers_devices_with_only_the_secondary_page() {
        let entries = vec![entry(0x19F5, 0xFC30, 0xFF0B, 0x0001, "/dev/hidraw7")];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(conflicts.is_empty(), "不应有冲突：{conflicts:?}");
        let target = found.get("hid:19f5:fc30").expect("备用页面必须能选中");
        assert_eq!(path_of(target), "/dev/hidraw7");
    }

    /// 两个页面都在时稳定选主页面（与枚举顺序无关），且不得把同一个 VID:PID 报成冲突。
    #[test]
    fn primary_usage_page_wins_over_secondary() {
        // 备用页面先被枚举：单遍实现会把它当成主页面，还会把真正的主页面报成冲突。
        let entries = vec![
            entry(0x19F5, 0xFC30, 0xFF0B, 0x0001, "/dev/hidraw5"),
            entry(0x19F5, 0xFC30, 0xFF01, 0x0001, "/dev/hidraw6"),
        ];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(conflicts.is_empty(), "同一台设备的两页不是冲突：{conflicts:?}");
        let target = found.get("hid:19f5:fc30").expect("必须选中一台设备");
        assert_eq!(path_of(target), "/dev/hidraw6", "主页面优先");
    }

    /// hidraw 会把同一节点的多个 Usage 展开成多条记录（共用同一 path）：那是同一物理集合，不是冲突。
    #[test]
    fn same_path_multi_usage_is_one_collection_not_a_conflict() {
        let entries = vec![
            entry(0x7768, 0x6426, 0xFFB2, 0x0001, "/dev/hidraw3"),
            entry(0x7768, 0x6426, 0xFFB2, 0x0002, "/dev/hidraw3"),
        ];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert_eq!(found.len(), 1, "多 Usage 不得变成多台设备");
        assert!(conflicts.is_empty(), "同一 path 不是冲突：{conflicts:?}");
    }

    /// 两只同型号接收器（path 不同）才是真冲突：只保留一条并上报一次。
    #[test]
    fn two_receivers_with_same_vid_pid_report_one_conflict() {
        let entries = vec![
            entry(0x7768, 0x6426, 0xFFB2, 0x0001, "/dev/hidraw3"),
            entry(0x7768, 0x6426, 0xFFB2, 0x0001, "/dev/hidraw7"),
        ];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert_eq!(found.len(), 1);
        assert_eq!(conflicts.len(), 1, "冲突只报一次：{conflicts:?}");
        assert_eq!(path_of(found.get("hid:7768:6426").unwrap()), "/dev/hidraw3");
    }

    /// 型号表只按 `VID:PID` 命中，不再带接口号：集合选择唯一判据是品牌声明的 UsagePage。
    #[test]
    fn collection_selection_uses_usage_page_only() {
        // 命中型号表但 UsagePage 不是本品牌的：不得注册（否则会打开标准鼠标集合，读到「成功但无数据」的句柄）。
        let entries = vec![entry(0x7768, 0x6426, 0x0001, 0x0002, "/dev/hidraw8")];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(found.is_empty(), "非品牌 UsagePage 不得注册");
        assert!(conflicts.is_empty());

        // 品牌 UsagePage 命中即注册，无论它挂在哪个接口上。
        let entries = vec![entry(0x7768, 0x6426, 0xFFB2, 0x0002, "/dev/hidraw9")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        assert!(found.contains_key("hid:7768:6426"));
    }

    /// 达尔优按型号表 `fwType` 分派：同一 VID 下匠盟设备只认 `0xFF02`，自研设备只认 `0x0C`。
    /// 分派失败的家族（这里用未接入的 `0x04` 表示）不注册 —— 不拿别的家族指令去猜。
    #[test]
    fn dareu_dispatches_protocol_by_fw_type() {
        // A950 PRO Mg（fwType 0x02）= 匠盟：0xFF02 命中。
        let entries = vec![entry(0x260D, 0x1117, 0xFF02, 0x0002, "/dev/hidraw4")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        let target = found.get("hid:260d:1117").expect("匠盟设备必须被选中");
        assert_eq!(target.brand.id, "dareu-jm");
        assert_eq!(target.brand.usage_page, 0xFF02);

        // Z82（fwType 0x00）= 自研 Tg：走 0x0C 通道，绝不能落到匠盟那套 0xFF02 上。
        let entries = vec![entry(0x260D, 0x0030, 0x000C, 0x0001, "/dev/hidraw5")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        let target = found.get("hid:260d:0030").expect("自研设备必须被选中");
        assert_eq!(target.brand.id, "dareu-tg");
        assert_eq!(target.brand.usage_page, 0x000C);

        // 同一台 Tg 设备出现在匠盟的 UsagePage 上：不是它的通道，不得注册。
        let entries = vec![entry(0x260D, 0x0030, 0xFF02, 0x0002, "/dev/hidraw6")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        assert!(found.is_empty(), "协议与 UsagePage 必须同时对上");
    }

    /// 雷柏 `0xFF00` 上并存「配置页(0x0002) / 升级页(0x0001 / 0x000E)」：声明了 Usage
    /// 就只在它上面打开，不能按枚举顺序随机选中升级页。
    #[test]
    fn rapoo_selects_declared_usage_not_just_usage_page() {
        let mut usages = HashMap::new();
        let mut ev = HidEvidence::new(0x24AE, 0x1410);
        ev.push_collection(0xFF00, 0x0002);
        ev.push_collection(0xFF00, 0x0001);
        ev.push_collection(0xFF00, 0x000E);
        usages.insert((0x24AE, 0x1410), ev);

        // 升级页先被枚举：仍必须选中配置页。
        let entries = vec![
            entry(0x24AE, 0x1410, 0xFF00, 0x0001, "/dev/hidraw11"),
            entry(0x24AE, 0x1410, 0xFF00, 0x0002, "/dev/hidraw12"),
        ];
        let (found, conflicts) = select_targets(&entries, &usages);
        assert!(conflicts.is_empty(), "同一设备的不同集合不是冲突");
        let target = found.get("hid:24ae:1410").expect("配置页必须被选中");
        assert_eq!(path_of(target), "/dev/hidraw12");
        assert_eq!(target.brand.id, "rapoo-nrf54l");

        // 只有升级页：不注册（打开它也读不到电量，等于「成功但无数据」）。
        let entries = vec![entry(0x24AE, 0x1410, 0xFF00, 0x000E, "/dev/hidraw13")];
        let (found, _) = select_targets(&entries, &usages);
        assert!(found.is_empty(), "升级接口不得当成电量通道");
    }

    /// 本机（无 /dev/hidraw*）与有设备的机器都必须能跑：枚举空时只返回空集合，绝不 panic、绝不虚构设备。
    #[test]
    fn live_enumeration_is_inert_without_devices() {
        let api = HidApi::new().expect("hidapi 初始化不应失败");
        let usages = usage_index(&api);
        let (found, _) = collect_targets(&api, &usages);
        if api.device_list().next().is_none() {
            assert!(found.is_empty(), "枚举为空时不得虚构设备");
            assert!(usages.is_empty(), "枚举为空时不得虚构 Usage 证据");
        }
        for (key, target) in &found {
            assert_eq!(
                StableId::parse(key).as_ref(),
                Some(&target.stable_id),
                "key 必须是可解析的 stableId：{key}"
            );
            assert!(!target.path.is_empty(), "选中的集合必须有路径");
        }
    }

    /// `run()` 在无设备机器上必须立刻可用：收到 stop 后按 backoff 步长返回，不 panic、不阻塞、
    /// 也不把「没有设备」当成传输层故障（`TransportDown` 会让上层把既有设备全转 Offline）。
    #[test]
    fn run_returns_promptly_without_devices() {
        if HidApi::new().is_err() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_run = Arc::clone(&stop);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            stop_for_run.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        Box::new(HidProvider::new()).run(tx, Arc::clone(&stop));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "run() 收到 stop 后应在 backoff 步长内返回，实际 {elapsed:?}"
        );
        let events: Vec<ProviderEvent> = rx.try_iter().collect();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProviderEvent::TransportDown { .. })),
            "无设备不是传输层故障：{events:?}"
        );
    }
}
