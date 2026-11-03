//! HID Provider 运行时：1 个枚举线程 + N 个读取线程。
//!
//! 品牌无关：设备匹配、帧解析、充电标志全部来自 [`BrandSpec`] 与 [`Registry`]。
//! 隔离靠读超时上界而非线程池，一个接收器挂死只会拖慢自己的读取线程。
//! 只按「VID/PID 精确 → 品牌声明的 UsagePage」两步选集合，不使用 `open(vid, pid)`。
//!
//! 设备类型识别只用枚举字段（纯函数 [`voltnest_provider::classify`]），不额外开句柄、
//! 不发报文；证据不足时结论为 `Unknown`。
//!
//! Linux 走 hidraw 后端：读写 `/dev/hidraw*` 需当前用户被放行（本实现不安装 udev 规则），
//! 权限缺失时 `open_path` 失败并上报 `access_denied`；无设备时枚举为空并按
//! [`PRESENCE_POLL_MS`] 轮询。构建期需 libudev 开发文件。

use crate::brands::{Brand, Registry};
use crate::spec::{BrandSpec, ReplyTransport, RequestTemplate};
use crate::table::Entry;
use crate::{
    LISTEN_READ_TIMEOUT_MS, PRESENCE_POLL_MS, READ_BUFFER_LEN, READ_TIMEOUT_MS, RETRY_GAP_MS, frame,
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
        let mut empty_reported = false;

        while !stop.load(Ordering::Relaxed) {
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

            for (key, target) in &found {
                if announced.insert(key.clone()) {
                    let fact = DeviceFact::new(
                        target.stable_id.clone(),
                        Transport::Hid,
                        FAMILY_HID_VBAS,
                        target.display_name.clone(),
                        target.model_name_raw.clone(),
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
                        let _ = tx.send(ProviderEvent::Unreadable {
                            stable_id: target.stable_id.clone(),
                            code: open_error_code(&e),
                            detail: Some(format!("open_path 失败: {e}")),
                        });
                    }
                }
            }

            let gone: Vec<String> = sessions
                .keys()
                .filter(|k| !found.contains_key(*k))
                .cloned()
                .collect();
            for key in gone {
                if let Some(session) = sessions.remove(&key) {
                    session.stop.store(true, Ordering::Relaxed);
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

        for (_, session) in sessions.drain() {
            session.stop.store(true, Ordering::Relaxed);
            let _ = session.join.join();
        }
    }
}

/// 一条采集会话的句柄：停止标志 + 线程句柄。
struct SessionHandle {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

struct HidTarget {
    stable_id: StableId,
    /// 厂商 ID。
    vid: u16,
    /// 产品 ID。
    pid: u16,
    path: CString,
    display_name: String,
    /// 未经品牌前缀修饰的型号原文（`modelKey` 来源）。
    model_name_raw: String,
    /// 命中的品牌（决定字段位置与查询方式）。
    brand: &'static BrandSpec,
    /// 类型识别结果（来自枚举元数据 + 型号表声明）。
    classification: DeviceClassification,
}

/// 把同一 `VID:PID` 全部接口的 Usage 聚合成一份证据。
///
/// 聚合键是 `VID:PID`，故多接口不会被当成多台设备。Usage 字段是否可用由
/// [`HidEvidence::push_collection`] 判定，平台差异不在这里分支。
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

/// 生成一台设备的分类：Usage 证据 + 有出处的厂商声明（`deviceType` 恰为规范类型词时生效）。
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

/// 枚举结果里的一条集合记录。
///
/// 同一平台节点在 hidraw 上会展开成多条共用 `path` 的记录，收敛成这个平台无关结构后
/// 集合选择才能写成可测的纯函数。
struct EnumeratedCollection {
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
    path: CString,
    /// 设备自报的产品名（展示名与型号来源之一）。
    product: Option<String>,
    /// 设备自报的厂商名（展示名的品牌前缀）。
    manufacturer: Option<String>,
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
            product: info.product_string().map(str::to_string),
            manufacturer: info.manufacturer_string().map(str::to_string),
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

/// 「添加设备」用的候选：与自动识别共用同一套筛选。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CandidateDevice {
    /// 稳定标识。
    pub stable_id: String,
    /// 展示名（品牌 + 型号）。
    pub display_name: String,
    /// `0xVVVV:0xPPPP`。
    pub vid_pid: String,
}

/// 枚举当前接入、且能通过自动识别筛选的设备。
///
/// 筛选逻辑与 [`select_targets`] 完全一致（型号表命中 → 品牌协议 → UsagePage），
/// 保证「手动添加」列出来的正是自动识别会认的那批设备。
pub fn list_candidates() -> Result<Vec<CandidateDevice>, String> {
    let api = HidApi::new().map_err(|e| format!("初始化 HID 枚举失败：{e}"))?;
    let usages = usage_index(&api);
    let (found, _conflicts) = collect_targets(&api, &usages);
    let mut out: Vec<CandidateDevice> = found
        .into_values()
        .map(|target| CandidateDevice {
            stable_id: target.stable_id.as_str().to_string(),
            display_name: target.display_name,
            vid_pid: format!("0x{:04X}:0x{:04X}", target.vid, target.pid),
        })
        .collect();
    out.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.stable_id.cmp(&b.stable_id))
    });
    Ok(out)
}

/// 按「VID/PID 精确 → 品牌声明的 UsagePage（+ 可选 Usage）」筛选候选集合。
///
/// 返回 `(本轮候选, 冲突告警)`：同一轮出现两个相同 `stableId` 但 `path` 不同时只保留先发现的
/// 一条并上报冲突；`path` 相同是同一物理集合的多条 Usage 记录，不算冲突。
fn select_targets(
    entries: &[EnumeratedCollection],
    usages: &HashMap<(u16, u16), HidEvidence>,
) -> (HashMap<String, HidTarget>, Vec<(String, String)>) {
    let registry = Registry::embedded();
    let mut found: HashMap<String, HidTarget> = HashMap::new();
    let mut conflicts: Vec<(String, String)> = Vec::new();

    // 两遍：先按主页面，再按备用页面补「主页面零命中」的型号。
    for alt in [false, true] {
        for info in entries {
            consider_target(info, alt, registry, usages, &mut found, &mut conflicts);
        }
    }
    (found, conflicts)
}

/// 单条枚举记录参与一次候选筛选。
///
/// `alt = true` 表示这一遍只认品牌声明的备用 UsagePage，且不得覆盖已选中的集合。
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
    let Some(spec) = brand.spec.for_device(entry) else {
        return;
    };
    let page_ok = if alt {
        spec.usage_page_alt == Some(info.usage_page)
    } else {
        spec.usage_page == 0 || spec.usage_page == info.usage_page
    };
    if !page_ok {
        return;
    }
    if !alt && spec.usage.is_some_and(|usage| usage != info.usage) {
        return;
    }
    let stable_id = StableId::hid(vid, pid);
    let key = stable_id.as_str().to_string();
    if let Some(existing) = found.get(&key) {
        if alt {
            return;
        }
        if existing.path == info.path {
            return;
        }
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
    let model = model_name(spec, info, entry);
    found.insert(
        key,
        HidTarget {
            stable_id,
            vid,
            pid,
            path: info.path.clone(),
            display_name: compose_display_name(
                clean(info.manufacturer.as_deref()),
                model.as_deref(),
                vid,
                pid,
            ),
            model_name_raw: model.unwrap_or_default(),
            brand: spec,
            classification: classify_target(brand, spec, vid, pid, usages),
        },
    );
}

/// 去掉首尾空白；空白串视为没有。
fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

/// 型号名：品牌声明优先产品名时用设备自报的产品名，否则用型号表型号；两者都缺失为 `None`。
fn model_name(spec: &BrandSpec, info: &EnumeratedCollection, entry: &Entry) -> Option<String> {
    let table = clean(entry.name.as_deref());
    let product = clean(info.product.as_deref());
    let picked = if spec.name_from_product {
        product.or(table)
    } else {
        table
    };
    picked.map(str::to_string)
}

/// 展示名：品牌名 + 型号；型号已含品牌时不重复。只有型号缺失才回落到 `VVVV:PPPP`。
fn compose_display_name(brand: Option<&str>, model: Option<&str>, vid: u16, pid: u16) -> String {
    let Some(model) = model else {
        return format!("{vid:04X}:{pid:04X}");
    };
    match brand {
        Some(brand) if !model.to_lowercase().starts_with(&brand.to_lowercase()) => {
            format!("{brand} {model}")
        }
        _ => model.to_string(),
    }
}

/// 换报告 ID 后的等待（毫秒）。
const REPORT_ID_SWITCH_GAP_MS: u64 = 50;

/// 档位探测最多听几帧：够容纳「一条无关推送 + 一条 base 应答」。
const PROFILE_PROBE_FRAMES: usize = 4;

/// 在报告里找出命令字为 `cmd` 的应答，返回它的 payload。
///
/// 偏移容错同 [`frame::decode_listen`]。
fn probe_payload<'a>(brand: &BrandSpec, cmd: u8, report: &'a [u8]) -> Option<&'a [u8]> {
    let layout = &brand.frame;
    let bases: &[usize] = if layout.report_id.is_some() {
        &[0]
    } else {
        &[0, 1]
    };
    for base in bases {
        if report.get(base + layout.cmd_index).copied() == Some(cmd) {
            return report.get(base + layout.payload_offset..);
        }
    }
    None
}

/// 读取线程入口：按协议形状选一条通道，退出时按原因收尾。
fn read_loop(
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut reader = Reader::new(dev, stable_id, brand, tx, stop);
    if let Err(Exit::LinkLost) = reader.run() {
        reader.link_lost();
    }
}

/// 读取线程的退出原因：两者都不算错误，区别只在要不要上报一次句柄失效。
enum Exit {
    /// 外部要求停止，或事件接收端已关闭：静默收尾。
    Stop,
    /// 句柄失效（拔线 / 接收器掉电）：先报一次 `LinkLost` 再收尾。
    LinkLost,
}

/// 一条通道的执行结果：`Ok(())` = 跑完；`Err(_)` = 该线程应当结束。
type Flow = Result<(), Exit>;

/// 推送为主的协议：多久没拿到有效读数，就值得主动问一次。
///
/// 锚点取「上次有效读数」与「上次探活」里较晚的那个：设备持续推送时永远不到这里；
/// 静默超过 `expected_interval_ms` 才探一次。探活失败不自行判死，状态由引擎按阈值链判定。
fn lazy_probe_due(
    expected_interval_ms: u64,
    last_reading: Option<Instant>,
    last_probe: Option<Instant>,
    now: Instant,
) -> bool {
    let anchor = match (last_reading, last_probe) {
        (Some(reading), Some(probe)) => Some(reading.max(probe)),
        (reading, probe) => reading.or(probe),
    };
    match anchor {
        // 还没拿到任何读数：连上就探（首帧查询）。
        None => true,
        Some(at) => {
            now.saturating_duration_since(at) >= Duration::from_millis(expected_interval_ms)
        }
    }
}

/// 一台设备的读取会话：句柄 + 身份 + 事件出口 + 停止标志。
///
/// 四种 [`ReplyTransport`] 各对应一个 `run_*` 方法，分派只在 [`Reader::run`] 一处。
struct Reader {
    dev: HidDevice,
    stable_id: StableId,
    brand: &'static BrandSpec,
    tx: Sender<ProviderEvent>,
    stop: Arc<AtomicBool>,
    /// 输入报告读缓冲：`read_input` 原地填充，调用方按实际长度切片。
    buf: [u8; READ_BUFFER_LEN],
    /// 多包应答拼接器：只有声明了 `reassembly` 的品牌会用到。
    reassembler: frame::Reassembler,
    /// 最近一次有效读数的时刻：推送型协议据此判断「要不要主动问一次」。
    last_reading: Option<Instant>,
    /// 最近一次主动探活的时刻：读数一直不来时避免每轮都发报文。
    last_probe: Option<Instant>,
    /// 最近一次读到的电量：只报充电状态的上报帧要靠它拼出一条完整读数。
    last_percent: Option<voltnest_provider::Percent>,
    /// 设备当前板载档位（探测成功前为 `None`，查询按档位 0 构造）。
    profile: Option<u8>,
}

impl Reader {
    fn new(
        dev: HidDevice,
        stable_id: StableId,
        brand: &'static BrandSpec,
        tx: Sender<ProviderEvent>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            dev,
            stable_id,
            brand,
            tx,
            stop,
            buf: [0u8; READ_BUFFER_LEN],
            reassembler: frame::Reassembler::new(),
            last_reading: None,
            last_probe: None,
            last_percent: None,
            profile: None,
        }
    }

    /// 一个变体一条通道，没有隐式的 `Option` 分支。
    fn run(&mut self) -> Flow {
        match self.brand.transport {
            ReplyTransport::Listen => self.run_listen(),
            ReplyTransport::InterruptQuery => self.run_interrupt_query(),
            ReplyTransport::FeatureQuery { .. } => self.run_feature_query(),
            ReplyTransport::FeaturePoll { .. } => self.run_feature_poll(),
        }
    }

    /// `Listen`：不向设备发任何报文，何时上报由设备决定。
    fn run_listen(&mut self) -> Flow {
        while !self.halted() {
            if let Some(n) = self.read_input(READ_TIMEOUT_MS)? {
                self.on_input(n)?;
            }
        }
        Ok(())
    }

    /// `InterruptQuery`：会话建立时先问一次，之后按协议声明的节奏补问。
    ///
    /// 两种节奏：`interval_ms: Some(..)` 为周期轮询；`None` 为推送为主，只在读数快过期时
    /// 探活问一次（[`Self::probe_due`]）。
    fn run_interrupt_query(&mut self) -> Flow {
        let mut next = Instant::now();
        while !self.halted() {
            if let Some(req) = self.brand.request {
                let now = Instant::now();
                let due = match req.interval_ms {
                    Some(_) => now >= next,
                    None => self.probe_due(now),
                };
                if due {
                    self.reassembler.reset();
                    self.ensure_profile()?;
                    let _ = self.dev.write(&req.build_at(
                        self.brand.frame.report_id,
                        self.profile.unwrap_or_default(),
                        self.brand.block_offset,
                    ));
                    self.last_probe = Some(now);
                    let gap = req.interval_ms.unwrap_or(self.brand.expected_interval_ms);
                    next = now + Duration::from_millis(gap);
                }
            }
            if let Some(n) = self.read_input(READ_TIMEOUT_MS)? {
                self.on_input(n)?;
            }
        }
        Ok(())
    }

    /// 推送为主的协议：读数是不是快过期了，该主动问一次。
    fn probe_due(&self, now: Instant) -> bool {
        lazy_probe_due(
            self.brand.expected_interval_ms,
            self.last_reading,
            self.last_probe,
            now,
        )
    }

    /// 读一次 base，解出当前板载档位。
    ///
    /// 每次探活都重读：用户切换档位后设备不会主动告知，而查询按档位偏移构造。
    fn ensure_profile(&mut self) -> Flow {
        let Some(probe) = self.brand.profile_probe else {
            return Ok(());
        };
        if self
            .dev
            .write(&probe.request.build(self.brand.frame.report_id))
            .is_err()
        {
            return Ok(());
        }
        for _ in 0..PROFILE_PROBE_FRAMES {
            let Some(n) = self.read_input(LISTEN_READ_TIMEOUT_MS)? else {
                break;
            };
            if let Some(payload) = probe_payload(self.brand, probe.request.cmd, &self.buf[..n]) {
                let profile = probe.resolve(payload);
                self.profile = Some(profile);
                return Ok(());
            }
        }
        Ok(())
    }

    /// `FeatureQuery`：周期发 feature 查询并读应答；声明了实时上报通道时，两次查询之间先听设备自发上报。
    fn run_feature_query(&mut self) -> Flow {
        let (Some(req), Some(primary)) = (self.brand.request, self.brand.frame.report_id) else {
            return Ok(());
        };
        let mut reply = vec![0u8; self.reply_len()];
        let mut report_id = primary;
        let mut next_query = Instant::now();
        while !self.halted() {
            let now = Instant::now();
            let due = match req.interval_ms {
                Some(_) => now >= next_query,
                None => self.probe_due(now),
            };
            if self.brand.listen.is_some() && !due {
                self.listen_once(LISTEN_READ_TIMEOUT_MS)?;
                continue;
            }
            self.last_probe = Some(now);
            self.query_round(&req, &mut reply, &mut report_id)?;
            let gap = req.interval_ms.unwrap_or(self.brand.expected_interval_ms);
            next_query = now + Duration::from_millis(gap);
            if self.brand.listen.is_none() {
                self.pause(gap)?;
            }
        }
        Ok(())
    }

    /// 一轮采集 = 首发 + `retry` 次重试：只有整轮都解不出有效帧才上报一次 `Unreadable`。
    fn query_round(&mut self, req: &RequestTemplate, reply: &mut [u8], report_id: &mut u8) -> Flow {
        for attempt in 0..req.attempts() {
            if attempt > 0 {
                self.pause(RETRY_GAP_MS)?;
            }
            if self
                .dev
                .send_feature_report(&req.build(Some(*report_id)))
                .is_err()
            {
                if let Some(alt) = self.alternate(*report_id) {
                    *report_id = alt;
                }
                self.pause(REPORT_ID_SWITCH_GAP_MS)?;
                continue;
            }
            let Some(n) = self.read_feature(reply, *report_id)? else {
                continue;
            };
            let event = reading_event(&self.stable_id, self.brand, &reply[..n]);
            if matches!(event, ProviderEvent::Unreadable { .. }) {
                if let Some(alt) = self.alternate(*report_id) {
                    *report_id = alt;
                    continue;
                }
                if attempt + 1 < req.attempts() {
                    continue;
                }
            }
            self.send(event)?;
            return Ok(());
        }
        Ok(())
    }

    /// `FeaturePoll`：不写设备，周期读取 feature 快照。
    fn run_feature_poll(&mut self) -> Flow {
        let Some(report_id) = self.brand.frame.report_id else {
            return Ok(());
        };
        let mut reply = vec![0u8; self.reply_len()];
        while !self.halted() {
            if let Some(n) = self.read_feature(&mut reply, report_id)? {
                self.send(reading_event(&self.stable_id, self.brand, &reply[..n]))?;
            }
            self.pause(self.brand.expected_interval_ms)?;
        }
        Ok(())
    }

    /// 一帧输入报告 → 最多一条事件。四条通道按声明优先级互斥：
    /// 无头上报 → 推送帧 → 多包重组 → 单帧应答。
    fn on_input(&mut self, n: usize) -> Flow {
        let report = &self.buf[..n];
        if self.brand.listen.is_some()
            && let Some(event) = frame::decode_listen(self.brand, report)
        {
            return self.on_listen(event);
        }
        if self.brand.notify.is_some()
            && let Some(reading) = frame::decode_notify(self.brand, report)
        {
            return self.emit_reading(reading);
        }
        if self.brand.reassembly.is_some() {
            if let Some(assembled) = self.reassembler.feed(self.brand, report, Instant::now())
                && let Some(reading) = frame::decode_assembled(self.brand, &assembled)
            {
                return self.emit_reading(reading);
            }
            return Ok(());
        }
        if matches!(self.brand.transport, ReplyTransport::InterruptQuery)
            && !frame::matches_cmd(self.brand, report)
        {
            return Ok(());
        }
        self.send(reading_event(&self.stable_id, self.brand, report))
    }

    /// 上报一条有效读数，并记下时刻 —— 推送型协议据此判断「要不要探活」。
    fn emit_reading(&mut self, reading: frame::BatteryReading) -> Flow {
        self.last_reading = Some(Instant::now());
        self.last_percent = Some(reading.percent);
        self.send(reading_event_of(&self.stable_id, reading))
    }

    /// 监听帧 → 事件。
    ///
    /// 只通报充电状态的帧沿用上一次电量；还没有任何电量时直接丢弃。
    fn on_listen(&mut self, event: frame::ListenEvent) -> Flow {
        match event {
            frame::ListenEvent::Reading(reading) => self.emit_reading(reading),
            frame::ListenEvent::Charging(charging) => match self.last_percent {
                Some(percent) => self.emit_reading(frame::BatteryReading {
                    percent,
                    charging: Some(charging),
                }),
                None => Ok(()),
            },
        }
    }

    /// 听一帧设备自发上报；认不出是电量帧就丢。
    fn listen_once(&mut self, timeout_ms: i32) -> Flow {
        if let Some(n) = self.read_input(timeout_ms)?
            && let Some(event) = frame::decode_listen(self.brand, &self.buf[..n])
        {
            return self.on_listen(event);
        }
        Ok(())
    }

    /// 读一帧输入报告到 `buf`；`Ok(None)` = 超时无数据。
    fn read_input(&mut self, timeout_ms: i32) -> Result<Option<usize>, Exit> {
        match self.dev.read_timeout(&mut self.buf, timeout_ms) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(n)),
            Err(_) => Err(Exit::LinkLost),
        }
    }

    /// 读一条 feature 应答；`Ok(None)` = 空应答。报告 ID 写在首字节是 hidapi 的约定。
    fn read_feature(&mut self, reply: &mut [u8], report_id: u8) -> Result<Option<usize>, Exit> {
        reply.fill(0);
        if let Some(first) = reply.first_mut() {
            *first = report_id;
        }
        match self.dev.get_feature_report(reply) {
            Ok(n) if n > 0 => Ok(Some(n)),
            Ok(_) => Ok(None),
            Err(_) => Err(Exit::LinkLost),
        }
    }

    /// 上报一条事件；接收端已断开 → 线程收尾。
    fn send(&self, event: ProviderEvent) -> Flow {
        self.tx.send(event).map_err(|_| Exit::Stop)
    }

    /// 与当前报告 ID 不同的备用 ID（`None` = 无可换）。
    fn alternate(&self, current: u8) -> Option<u8> {
        self.brand.fallback_report_id.filter(|alt| *alt != current)
    }

    /// feature 应答的读取长度，取自 [`ReplyTransport`] 的声明。
    fn reply_len(&self) -> usize {
        self.brand.transport.reply_len().unwrap_or_default()
    }

    /// 停止位是否已置。
    fn halted(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// 可被 `stop` 打断的等待。
    fn pause(&self, ms: u64) -> Flow {
        if backoff_sleep(&self.stop, ms) {
            Ok(())
        } else {
            Err(Exit::Stop)
        }
    }

    /// 上报一次句柄失效（拔线 / 接收器掉电）。
    fn link_lost(&self) {
        let _ = self.tx.send(ProviderEvent::LinkLost {
            stable_id: self.stable_id.clone(),
            code: ErrorCode::DeviceGone,
        });
    }
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
        let reg = Registry::embedded();
        let (brand, _) = reg.lookup(0x7768, 0x6426).unwrap();
        let mut evidence = HidEvidence::new(0x7768, 0x6426);
        evidence.push_collection(0xFFB2, 0x0001);
        let mut usages = HashMap::new();
        usages.insert((0x7768, 0x6426), evidence);
        let c = classify_target(brand, brand.spec, 0x7768, 0x6426, &usages);
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);
        assert!(
            c.evidence.iter().all(|e| !e.contains("未支持")),
            "未支持的 Usage 属于 debug 信息，不得进结论证据：{:?}",
            c.evidence
        );
        assert!(c.skipped.iter().any(|e| e.contains("未支持")));
    }

    /// 真实设备端到端：枚举 → Usage 聚合 → 分类。`#[ignore]`，需插着已验证接收器。
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
            product: None,
            manufacturer: None,
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

    /// 迈从 CZ/GLW 族键盘正常挂在标准页 `0x0001`，官方过滤器里还有一条 `0xFF00`（boot 模式）：
    /// 主页面零命中时必须降级到备用页面。
    #[test]
    fn alt_usage_page_covers_devices_with_only_the_secondary_page() {
        let entries = vec![entry(0x19F5, 0xFC30, 0xFF00, 0x0001, "/dev/hidraw7")];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(conflicts.is_empty(), "不应有冲突：{conflicts:?}");
        let target = found.get("hid:19f5:fc30").expect("备用页面必须能选中");
        assert_eq!(path_of(target), "/dev/hidraw7");
    }

    /// 两个页面都在时稳定选主页面（与枚举顺序无关），且不得把同一个 VID:PID 报成冲突。
    #[test]
    fn primary_usage_page_wins_over_secondary() {
        let entries = vec![
            entry(0x19F5, 0xFC30, 0xFF00, 0x0001, "/dev/hidraw5"),
            entry(0x19F5, 0xFC30, 0x0001, 0x0000, "/dev/hidraw6"),
        ];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(
            conflicts.is_empty(),
            "同一台设备的两页不是冲突：{conflicts:?}"
        );
        let target = found.get("hid:19f5:fc30").expect("必须选中一台设备");
        assert_eq!(path_of(target), "/dev/hidraw6", "主页面优先");
    }

    /// `usage` 钉死后只选 GLW 私有集合，键盘集合先枚举也抢不走。
    #[test]
    fn glw_pins_vendor_collection_not_keyboard() {
        // 键盘集合先枚举，仍须选中 `1/0`。
        let entries = vec![
            entry(0x19F5, 0xFC30, 0x0001, 0x0006, "/dev/hidraw5"),
            entry(0x19F5, 0xFC30, 0x0001, 0x0000, "/dev/hidraw6"),
        ];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(
            conflicts.is_empty(),
            "同一台设备的不同集合不是冲突：{conflicts:?}"
        );
        let target = found.get("hid:19f5:fc30").expect("必须选中一台设备");
        assert_eq!(
            path_of(target),
            "/dev/hidraw6",
            "厂商私有集合（usage=0）优先于键盘集合"
        );
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
        let entries = vec![entry(0x7768, 0x6426, 0x0001, 0x0002, "/dev/hidraw8")];
        let (found, conflicts) = select_targets(&entries, &HashMap::new());
        assert!(found.is_empty(), "非品牌 UsagePage 不得注册");
        assert!(conflicts.is_empty());

        let entries = vec![entry(0x7768, 0x6426, 0xFFB2, 0x0002, "/dev/hidraw9")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        assert!(found.contains_key("hid:7768:6426"));
    }

    /// 展示名 = 品牌名 + 型号表型号；型号原文留给 `modelKey`，不带品牌前缀。
    #[test]
    fn display_name_combines_brand_and_table_model() {
        let mut e = entry(0x7768, 0x6426, 0xFFB2, 0x0001, "/dev/hidraw3");
        e.manufacturer = Some("AJAZZ".into());
        e.product = Some("AJAZZ Receiver".into());
        let (found, _) = select_targets(&[e], &HashMap::new());
        let target = found.get("hid:7768:6426").unwrap();
        assert_eq!(target.display_name, "AJAZZ AJ159 NL");
        assert_eq!(target.model_name_raw, "AJ159 NL");
    }

    /// 迈从型号表只能给候选（共用 PID），改用设备自报的产品名；产品名已含品牌时不重复。
    #[test]
    fn mchose_display_name_prefers_product_string() {
        let mut e = entry(0x3837, 0x1015, 0xFF80, 0x0001, "/dev/hidraw4");
        e.product = Some("MCHOSE G98 V3 2.4G".into());
        e.manufacturer = Some("MCHOSE".into());
        let (found, _) = select_targets(&[e], &HashMap::new());
        let target = found.get("hid:3837:1015").unwrap();
        assert_eq!(target.display_name, "MCHOSE G98 V3 2.4G");
        assert_eq!(target.model_name_raw, "MCHOSE G98 V3 2.4G");
    }

    /// 产品名与型号都拿不到时用 `VVVV:PPPP`：大写、不带 `0x`。
    #[test]
    fn display_name_falls_back_to_bare_vid_pid() {
        let entries = vec![entry(0x3837, 0x1015, 0xFF80, 0x0001, "/dev/hidraw4")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        let target = found.get("hid:3837:1015").unwrap();
        assert_eq!(target.display_name, "3837:1015");
        assert!(target.model_name_raw.is_empty());
    }

    /// 达尔优按型号表 `fwType` 分派：同一 VID 下匠盟设备只认 `0xFF02`，自研设备只认 `0x0C`。
    /// 分派失败的家族（这里用未接入的 `0x04` 表示）不注册 —— 不拿别的家族指令去猜。
    #[test]
    fn dareu_dispatches_protocol_by_fw_type() {
        let entries = vec![entry(0x260D, 0x1117, 0xFF02, 0x0002, "/dev/hidraw4")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        let target = found.get("hid:260d:1117").expect("匠盟设备必须被选中");
        assert_eq!(target.brand.id, "dareu-jm");
        assert_eq!(target.brand.usage_page, 0xFF02);

        let entries = vec![entry(0x260D, 0x0030, 0x000C, 0x0001, "/dev/hidraw5")];
        let (found, _) = select_targets(&entries, &HashMap::new());
        let target = found.get("hid:260d:0030").expect("自研设备必须被选中");
        assert_eq!(target.brand.id, "dareu-tg");
        assert_eq!(target.brand.usage_page, 0x000C);

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

        let entries = vec![
            entry(0x24AE, 0x1410, 0xFF00, 0x0001, "/dev/hidraw11"),
            entry(0x24AE, 0x1410, 0xFF00, 0x0002, "/dev/hidraw12"),
        ];
        let (found, conflicts) = select_targets(&entries, &usages);
        assert!(conflicts.is_empty(), "同一设备的不同集合不是冲突");
        let target = found.get("hid:24ae:1410").expect("配置页必须被选中");
        assert_eq!(path_of(target), "/dev/hidraw12");
        assert_eq!(target.brand.id, "rapoo-nrf54l");

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

    /// `run()` 在无设备机器上必须立刻可用：收到 stop 后按 backoff 步长返回，不 panic、不阻塞。
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

    // ---- 推送型协议：惰性探活 ----

    /// 连上先探一次，之后只有在读数静默超过期望间隔时才再探。
    #[test]
    fn lazy_probe_waits_for_silence_before_asking_again() {
        let t0 = Instant::now();
        let expected = 30_000u64;

        // 还没拿到任何读数：连上就问。
        assert!(lazy_probe_due(expected, None, None, t0));

        assert!(!lazy_probe_due(
            expected,
            Some(t0),
            None,
            t0 + Duration::from_secs(10)
        ));
        // 静默到期望间隔：问一次。
        assert!(lazy_probe_due(
            expected,
            Some(t0),
            None,
            t0 + Duration::from_secs(30)
        ));

        let probe = t0 + Duration::from_secs(30);
        assert!(!lazy_probe_due(
            expected,
            Some(t0),
            Some(probe),
            probe + Duration::from_secs(10)
        ));
        assert!(lazy_probe_due(
            expected,
            Some(t0),
            Some(probe),
            probe + Duration::from_secs(30)
        ));

        let reading = probe + Duration::from_secs(5);
        assert!(!lazy_probe_due(
            expected,
            Some(reading),
            Some(probe),
            reading + Duration::from_secs(20)
        ));
    }
}
