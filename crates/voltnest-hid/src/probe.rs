//! 生成上报数据用的 HID 诊断面：枚举全部集合 + 录制原始报告字节。
//!
//! 与 [`crate::runtime`] 的分工：那个只服务型号表里已知的设备并产出电量读数；本模块面向
//! 「型号表里没有、自动识别失败」的设备，不匹配品牌、不解析帧、不猜字段，只按顺序记录
//! 「哪个集合、什么时刻、收发到哪些字节」。因此不依赖 [`crate::spec::BrandSpec`]。
//!
//! 枚举与录制只有 Windows / Linux 实现，其它平台返回 [`CaptureError::Unsupported`]；
//! 帧结构、分组与字节分析与平台无关。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::READ_BUFFER_LEN;
use crate::hex;

/// 一次录制最多记录的帧数（默认值，可由调用方下调）。
pub const CAPTURE_MAX_FRAMES: usize = 2_000;

/// 录制时长下界。
pub const CAPTURE_MIN_DURATION_MS: u64 = 1_000;

/// 录制时长上界。
pub const CAPTURE_MAX_DURATION_MS: u64 = 300_000;

/// 主动查询报文的默认发送间隔。
pub const CAPTURE_DEFAULT_WRITE_INTERVAL_MS: u64 = 1_000;

/// 单次读取的阻塞上界：决定「停止」指令与时长到点的响应速度。
pub const CAPTURE_READ_TIMEOUT_MS: i32 = 100;

/// 一个 HID 集合声明的 Usage。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Usage {
    /// UsagePage（如 `0xFFB2`、`0x0001`）。
    pub page: u16,
    /// Usage（如 `0x0001`、`0x0002`）。
    pub usage: u16,
}

impl Usage {
    /// 是否落在标准 UsagePage 段（`< 0xFF00`）。标准页通常不承载厂商私有电量协议。
    pub const fn is_standard(self) -> bool {
        self.page < 0xFF00
    }
}

impl core::fmt::Display for Usage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "0x{:04X}/0x{:04X}", self.page, self.usage)
    }
}

/// 枚举结果里的一台（或一个集合）HID 设备。
///
/// 唯一标识是 `path`：同一物理设备的多个 Usage 在 Linux hidraw 上共用一条 path，必须合并。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CollectionInfo {
    /// 平台路径（`\\?\hid#vid_...` / `/dev/hidraw3`），用于打开与去重。
    pub path: String,
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// 产品名（驱动/固件给什么就是什么，可能为空）。
    pub product: Option<String>,
    /// 厂商名。
    pub manufacturer: Option<String>,
    /// 序列号（同型号多只设备时唯一的区分手段）。
    pub serial: Option<String>,
    /// 接口号；后端不提供时为 `-1`。
    pub interface_number: i32,
    /// 该 path 上声明过的全部 Usage（已排序去重）。
    pub usages: Vec<Usage>,
}

impl CollectionInfo {
    /// `0xVVVV:0xPPPP` 形式的标识。
    pub fn vid_pid(&self) -> String {
        format!("0x{:04X}:0x{:04X}", self.vid, self.pid)
    }

    /// 列表里显示的名称：产品名优先，缺失时退回 VID:PID（不编造型号）。
    pub fn display_name(&self) -> String {
        self.product
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.vid_pid())
    }

    /// 全部 Usage 的紧凑写法（`0xFFB2/0x0001, 0x0001/0x0002`）。
    pub fn usage_summary(&self) -> String {
        if self.usages.is_empty() {
            return "（未声明）".to_string();
        }
        self.usages
            .iter()
            .map(Usage::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// 是否只有标准 Usage（帮助用户判断「这是鼠标键盘的标准集合，不是私有通道」）。
    pub fn only_standard_usages(&self) -> bool {
        !self.usages.is_empty() && self.usages.iter().all(|usage| usage.is_standard())
    }

    /// 记录/展示时代表的那个 Usage：优先厂商私有页（`>= 0xFF00`），否则取第一个。
    pub fn primary_usage(&self) -> Option<Usage> {
        self.usages
            .iter()
            .copied()
            .find(|usage| !usage.is_standard())
            .or_else(|| self.usages.first().copied())
    }
}

/// 枚举的原始一条记录（一个集合），暴露出来便于单测覆盖合并逻辑。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RawCollection {
    /// 平台路径。
    pub path: String,
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// 产品名。
    pub product: Option<String>,
    /// 厂商名。
    pub manufacturer: Option<String>,
    /// 序列号。
    pub serial: Option<String>,
    /// 接口号。
    pub interface_number: i32,
    /// UsagePage。
    pub usage_page: u16,
    /// Usage。
    pub usage: u16,
}

/// 把枚举记录按 `path` 合并成设备列表，并按 `(VID, PID, 产品名, path)` 排序。
pub fn merge_collections(raw: Vec<RawCollection>) -> Vec<CollectionInfo> {
    let mut out: Vec<CollectionInfo> = Vec::with_capacity(raw.len());
    for item in raw {
        let usage = Usage {
            page: item.usage_page,
            usage: item.usage,
        };
        match out.iter_mut().find(|known| known.path == item.path) {
            Some(known) => {
                if !known.usages.contains(&usage) {
                    known.usages.push(usage);
                }
            }
            None => out.push(CollectionInfo {
                path: item.path,
                vid: item.vid,
                pid: item.pid,
                product: item.product,
                manufacturer: item.manufacturer,
                serial: item.serial,
                interface_number: item.interface_number,
                usages: vec![usage],
            }),
        }
    }
    for info in &mut out {
        info.usages.sort();
        info.usages.dedup();
    }
    out.sort_by(|a, b| {
        (a.vid, a.pid, a.display_name(), &a.path).cmp(&(b.vid, b.pid, b.display_name(), &b.path))
    });
    out
}

/// 同一 VID:PID 的全部集合 —— 用户视角的「一台设备」。
///
/// 复合 HID 会同时暴露标准集合与厂商私有集合，先按 VID:PID 收敛，用户再选具体集合。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetGroup {
    /// 厂商 ID。
    pub vid: u16,
    /// 产品 ID。
    pub pid: u16,
    /// 设备名（产品名，缺失时为空串 —— 展示时用 [`TargetGroup::display_name`]）。
    pub name: String,
    /// 厂商名（取第一个非空值）。
    pub manufacturer: Option<String>,
    /// 序列号（去重、升序：同型号多只设备靠它区分）。
    pub serials: Vec<String>,
    /// 该 VID:PID 下的全部集合。
    pub collections: Vec<CollectionInfo>,
}

impl TargetGroup {
    /// `0xVVVV:0xPPPP`。
    pub fn vid_pid(&self) -> String {
        format!("0x{:04X}:0x{:04X}", self.vid, self.pid)
    }

    /// 列表显示名：产品名优先，缺失时退回 VID:PID（不编造型号）。
    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.vid_pid()
        } else {
            name.to_string()
        }
    }

    /// 序列号展示：多个时用 `、` 连接，没有则 `None`。
    pub fn serial_summary(&self) -> Option<String> {
        if self.serials.is_empty() {
            None
        } else {
            Some(self.serials.join("、"))
        }
    }

    /// 私有集合（UsagePage `>= 0xFF00`）的数量 —— 界面据此提示「先看私有集合」。
    pub fn private_collections(&self) -> usize {
        self.collections
            .iter()
            .filter(|collection| !collection.only_standard_usages())
            .count()
    }
}

/// 把扁平集合列表聚合为「按 VID:PID 的设备列表」，并按 VID:PID 排序。
pub fn group_targets(devices: Vec<CollectionInfo>) -> Vec<TargetGroup> {
    let mut targets: Vec<TargetGroup> = Vec::new();
    for collection in devices {
        match targets
            .iter_mut()
            .find(|target| target.vid == collection.vid && target.pid == collection.pid)
        {
            Some(target) => {
                if target.name.trim().is_empty()
                    && let Some(product) = collection.product.as_deref()
                {
                    target.name = product.trim().to_string();
                }
                if target.manufacturer.is_none() {
                    target.manufacturer = collection
                        .manufacturer
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string);
                }
                if let Some(serial) = collection
                    .serial
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    target.serials.push(serial.to_string());
                }
                target.collections.push(collection);
            }
            None => {
                let serials = collection
                    .serial
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(|text| vec![text.to_string()])
                    .unwrap_or_default();
                targets.push(TargetGroup {
                    vid: collection.vid,
                    pid: collection.pid,
                    name: collection
                        .product
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    manufacturer: collection
                        .manufacturer
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string),
                    serials,
                    collections: vec![collection],
                });
            }
        }
    }
    for target in &mut targets {
        target.serials.sort();
        target.serials.dedup();
    }
    targets.sort_by_key(|target| (target.vid, target.pid));
    targets
}

/// 枚举当前的 HID 集合。
///
/// 失败只有一种：hidapi 初始化失败。枚举为空不是错误，返回空表由界面提示。
pub fn enumerate() -> Result<Vec<CollectionInfo>, CaptureError> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        enumerate_platform()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(CaptureError::Unsupported)
    }
}

/// 一帧的收发方向（从宿主视角）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 设备 → 宿主（输入报告）。
    In,
    /// 宿主 → 设备（主动发出的查询报文）。
    Out,
}

impl Direction {
    /// 报告里的写法：`IN` / `OUT`。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::In => "IN",
            Self::Out => "OUT",
        }
    }
}

/// 录制到的一帧。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CapturedFrame {
    /// 相对录制开始的毫秒数（不是系统时间：报告要能被任何人复现同一时间轴）。
    pub elapsed_ms: u64,
    /// 收发方向。
    pub direction: Direction,
    /// 原始字节（首字节通常是报告 ID）。
    pub data: Vec<u8>,
}

impl CapturedFrame {
    /// 十六进制写法。
    pub fn hex(&self) -> String {
        hex::format(&self.data)
    }
}

/// 一次录制的参数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaptureRequest {
    /// 要打开的集合路径（来自 [`CollectionInfo::path`]）。
    pub path: String,
    /// 录制时长（毫秒），会被夹到 [`CAPTURE_MIN_DURATION_MS`]..=[`CAPTURE_MAX_DURATION_MS`]。
    pub duration_ms: u64,
    /// 录制期间周期性发出的报文（可为空 = 只监听）。用于「电量要靠查询」的设备。
    pub write_frames: Vec<Vec<u8>>,
    /// 查询报文的发送间隔。
    pub write_interval_ms: u64,
    /// 最多记录的帧数，夹到 `1..=`[`CAPTURE_MAX_FRAMES`]。
    pub max_frames: usize,
}

impl CaptureRequest {
    /// 只监听、录 20 秒的默认请求。
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            duration_ms: 20_000,
            write_frames: Vec::new(),
            write_interval_ms: CAPTURE_DEFAULT_WRITE_INTERVAL_MS,
            max_frames: CAPTURE_MAX_FRAMES,
        }
    }

    /// 实际生效的时长。
    pub fn clamped_duration_ms(&self) -> u64 {
        self.duration_ms
            .clamp(CAPTURE_MIN_DURATION_MS, CAPTURE_MAX_DURATION_MS)
    }

    /// 实际生效的帧数上限。
    pub fn clamped_max_frames(&self) -> usize {
        self.max_frames.clamp(1, CAPTURE_MAX_FRAMES)
    }
}

/// 一次录制的统计结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaptureSummary {
    /// 总帧数。
    pub frames: usize,
    /// 输入帧数。
    pub input_frames: usize,
    /// 主动发出的帧数。
    pub output_frames: usize,
    /// 实际录制时长。
    pub elapsed_ms: u64,
    /// 是否因帧数上限提前结束（`true` 时报告里的数据是不完整的）。
    pub truncated: bool,
    /// 主动查询报文的发送间隔（毫秒）；没发过报文时为 `0`。
    pub write_interval_ms: u64,
    /// 主动发送失败的次数（设备不应答查询、或报文格式不被接受）。
    pub write_failures: usize,
    /// 第一次发送失败的原因（给用户看的原文）。
    pub write_error: Option<String>,
}

/// 录制失败的原因。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CaptureError {
    /// hidapi 初始化 / 打开集合失败（含被官方驱动独占）。
    Open(String),
    /// 开着的句柄读失败（通常是设备被拔掉或接收器掉电）。
    Io(String),
    /// 路径里含 NUL 字节，不可能是合法路径。
    InvalidPath,
    /// 当前平台没有实现（只有 Windows / Linux 有）。
    Unsupported,
}

impl core::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Open(detail) => write!(f, "打开设备失败：{detail}"),
            Self::Io(detail) => write!(f, "读取中断：{detail}"),
            Self::InvalidPath => write!(f, "设备路径非法"),
            Self::Unsupported => write!(f, "当前平台不支持 HID 抓包"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// 录制一段时间的原始报告。
///
/// 每收到一帧就调用一次 `sink`；即使返回 `Err`，之前 `sink` 收到的帧也有效
/// （中途拔线时用户仍能拿到拔线前的数据）。
pub fn capture(
    request: &CaptureRequest,
    stop: &AtomicBool,
    sink: impl FnMut(CapturedFrame),
) -> Result<CaptureSummary, CaptureError> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        capture_platform(request, stop, sink)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (request, stop, sink);
        Err(CaptureError::Unsupported)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn enumerate_platform() -> Result<Vec<CollectionInfo>, CaptureError> {
    let api = hidapi::HidApi::new()
        .map_err(|err| CaptureError::Open(format!("hidapi 初始化失败：{err}")))?;
    let raw: Vec<RawCollection> = api
        .device_list()
        .map(|info| RawCollection {
            path: info.path().to_string_lossy().into_owned(),
            vid: info.vendor_id(),
            pid: info.product_id(),
            product: info.product_string().map(str::to_string),
            manufacturer: info.manufacturer_string().map(str::to_string),
            serial: info.serial_number().map(str::to_string),
            interface_number: info.interface_number(),
            usage_page: info.usage_page(),
            usage: info.usage(),
        })
        .collect();
    Ok(merge_collections(raw))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn capture_platform(
    request: &CaptureRequest,
    stop: &AtomicBool,
    mut sink: impl FnMut(CapturedFrame),
) -> Result<CaptureSummary, CaptureError> {
    use std::ffi::CString;

    let path = CString::new(request.path.clone()).map_err(|_| CaptureError::InvalidPath)?;
    let api = hidapi::HidApi::new()
        .map_err(|err| CaptureError::Open(format!("hidapi 初始化失败：{err}")))?;
    let device = api
        .open_path(&path)
        .map_err(|err| CaptureError::Open(err.to_string()))?;

    let duration = Duration::from_millis(request.clamped_duration_ms());
    let max_frames = request.clamped_max_frames();
    let write_interval = (!request.write_frames.is_empty())
        .then(|| Duration::from_millis(request.write_interval_ms.max(50)));

    let started = Instant::now();
    let mut next_write = started;
    let mut buf = [0u8; READ_BUFFER_LEN];
    let mut summary = CaptureSummary {
        frames: 0,
        input_frames: 0,
        output_frames: 0,
        elapsed_ms: 0,
        truncated: false,
        write_interval_ms: write_interval.map_or(0, |interval| interval.as_millis() as u64),
        write_failures: 0,
        write_error: None,
    };
    let mut first_error: Option<CaptureError> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let elapsed = started.elapsed();
        if elapsed >= duration {
            break;
        }
        if summary.frames >= max_frames {
            summary.truncated = true;
            break;
        }

        if let Some(interval) = write_interval
            && Instant::now() >= next_write
        {
            for frame in &request.write_frames {
                match device.write(frame) {
                    Ok(_) => {
                        if summary.frames < max_frames {
                            summary.frames += 1;
                            summary.output_frames += 1;
                            sink(CapturedFrame {
                                elapsed_ms: elapsed.as_millis() as u64,
                                direction: Direction::Out,
                                data: frame.clone(),
                            });
                        }
                    }
                    Err(err) => {
                        summary.write_failures += 1;
                        summary.write_error.get_or_insert_with(|| err.to_string());
                    }
                }
            }
            next_write = Instant::now() + interval;
        }

        match device.read_timeout(&mut buf, CAPTURE_READ_TIMEOUT_MS) {
            Ok(0) => {}
            Ok(n) => {
                if summary.frames < max_frames {
                    summary.frames += 1;
                    summary.input_frames += 1;
                    sink(CapturedFrame {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        direction: Direction::In,
                        data: buf[..n].to_vec(),
                    });
                }
            }
            Err(err) => {
                first_error = Some(CaptureError::Io(err.to_string()));
                break;
            }
        }
    }

    summary.elapsed_ms = started.elapsed().as_millis() as u64;
    match first_error {
        Some(err) => Err(err),
        None => Ok(summary),
    }
}

/// 报告里的一组「连续完全相同的帧」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameGroup<'a> {
    /// 组内第一帧（保留它的时间戳）。
    pub frame: &'a CapturedFrame,
    /// 连续重复次数（`1` = 没有重复）。
    pub count: usize,
}

/// 把连续完全相同的帧（方向与字节都相同）折叠成一组。展示有损，但保留重复次数。
pub fn group_frames(frames: &[CapturedFrame]) -> Vec<FrameGroup<'_>> {
    let mut groups: Vec<FrameGroup<'_>> = Vec::new();
    for frame in frames {
        match groups.last_mut() {
            Some(last)
                if last.frame.direction == frame.direction && last.frame.data == frame.data =>
            {
                last.count += 1;
            }
            _ => groups.push(FrameGroup { frame, count: 1 }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(
        path: &str,
        vid: u16,
        pid: u16,
        product: Option<&str>,
        page: u16,
        usage: u16,
    ) -> RawCollection {
        RawCollection {
            path: path.to_string(),
            vid,
            pid,
            product: product.map(str::to_string),
            manufacturer: None,
            serial: None,
            interface_number: -1,
            usage_page: page,
            usage,
        }
    }

    fn frame(elapsed_ms: u64, direction: Direction, data: &[u8]) -> CapturedFrame {
        CapturedFrame {
            elapsed_ms,
            direction,
            data: data.to_vec(),
        }
    }

    /// hidraw 会把同一节点的多个 Usage 展开成多条记录（共用 path）：必须合成一台设备。
    #[test]
    fn merges_records_sharing_a_path() {
        let devices = merge_collections(vec![
            raw(
                "/dev/hidraw3",
                0x7768,
                0x6426,
                Some("AJ159"),
                0xFFB2,
                0x0001,
            ),
            raw(
                "/dev/hidraw3",
                0x7768,
                0x6426,
                Some("AJ159"),
                0x0001,
                0x0002,
            ),
            raw(
                "/dev/hidraw3",
                0x7768,
                0x6426,
                Some("AJ159"),
                0xFFB2,
                0x0001,
            ),
        ]);
        assert_eq!(devices.len(), 1);
        assert_eq!(
            devices[0].usages,
            vec![
                Usage {
                    page: 0x0001,
                    usage: 0x0002
                },
                Usage {
                    page: 0xFFB2,
                    usage: 0x0001
                }
            ],
            "Usage 去重并排序"
        );
        assert_eq!(devices[0].usage_summary(), "0x0001/0x0002, 0xFFB2/0x0001");
        assert!(!devices[0].only_standard_usages());
    }

    /// 不同 path 是不同集合（Windows 上一个接口一条），不能被合并掉。
    #[test]
    fn keeps_distinct_paths_apart() {
        let devices = merge_collections(vec![
            raw("a", 0x1234, 0x0001, Some("X"), 0x0001, 0x0002),
            raw("b", 0x1234, 0x0001, Some("X"), 0xFF00, 0x0001),
        ]);
        assert_eq!(devices.len(), 2);
        assert!(devices[0].only_standard_usages());
        assert!(!devices[1].only_standard_usages());
    }

    /// 顺序必须与枚举顺序无关：同一个系统两次刷新得到同一份列表。
    #[test]
    fn sorts_deterministically() {
        let a = vec![
            raw("p3", 0x7768, 0x6426, Some("B"), 0xFFB2, 0x0001),
            raw("p1", 0x1234, 0x0001, None, 0x0001, 0x0002),
            raw("p2", 0x7768, 0x6426, Some("A"), 0xFFB2, 0x0001),
        ];
        let mut b = a.clone();
        b.reverse();
        let names = |list: Vec<RawCollection>| -> Vec<String> {
            merge_collections(list)
                .into_iter()
                .map(|info| info.path)
                .collect()
        };
        assert_eq!(names(a.clone()), vec!["p1", "p2", "p3"]);
        assert_eq!(names(a), names(b), "枚举顺序不得影响结果");
    }

    /// 产品名缺失时退回 VID:PID，不编造型号。
    #[test]
    fn display_name_falls_back_to_vid_pid() {
        let devices = merge_collections(vec![raw("p", 0x0ABC, 0x0DEF, Some("   "), 0, 0)]);
        assert_eq!(devices[0].display_name(), "0x0ABC:0x0DEF");
        assert_eq!(devices[0].vid_pid(), "0x0ABC:0x0DEF");
        // 没有 Usage 时也要有可读文本。
        let empty = merge_collections(vec![]);
        assert!(empty.is_empty());
        let devices = merge_collections(vec![raw("p", 1, 2, None, 0, 0)]);
        assert_eq!(devices[0].usage_summary(), "0x0000/0x0000");
    }

    #[test]
    fn request_clamps_duration_and_frames() {
        let mut request = CaptureRequest::new("/dev/hidraw0");
        assert_eq!(request.clamped_duration_ms(), 20_000);
        request.duration_ms = 0;
        assert_eq!(request.clamped_duration_ms(), CAPTURE_MIN_DURATION_MS);
        request.duration_ms = u64::MAX;
        assert_eq!(request.clamped_duration_ms(), CAPTURE_MAX_DURATION_MS);
        request.max_frames = 0;
        assert_eq!(request.clamped_max_frames(), 1);
        request.max_frames = usize::MAX;
        assert_eq!(request.clamped_max_frames(), CAPTURE_MAX_FRAMES);
        assert!(request.write_frames.is_empty(), "默认不向设备发报文");
    }

    /// 连续相同的帧折叠成一组，并保留组内首帧的时间戳（报告的时间轴锚点）。
    #[test]
    fn groups_consecutive_identical_frames() {
        let frames = vec![
            frame(0, Direction::In, &[0x06, 0x03, 0x50]),
            frame(10, Direction::In, &[0x06, 0x03, 0x50]),
            frame(20, Direction::In, &[0x06, 0x03, 0x51]),
            frame(30, Direction::In, &[0x06, 0x03, 0x50]),
            frame(40, Direction::Out, &[0x06, 0x03, 0x50]),
        ];
        let groups = group_frames(&frames);
        assert_eq!(groups.len(), 4, "方向不同不算重复");
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[0].frame.elapsed_ms, 0);
        assert_eq!(groups[1].count, 1);
        assert_eq!(groups[2].frame.elapsed_ms, 30);
        assert_eq!(groups[3].frame.direction, Direction::Out);

        // 空输入不 panic。
        assert!(group_frames(&[]).is_empty());
    }

    /// 同一 VID:PID 的多条集合必须收敛成一台设备：用户是按「设备」而不是按「通道」思考的。
    #[test]
    fn groups_collections_by_vid_pid() {
        let mut first = raw("p1", 0x7768, 0x6426, Some("AJ159 NL"), 0x0001, 0x0002);
        first.manufacturer = Some("AJAZZ".to_string());
        first.serial = Some("SN002".to_string());
        let mut second = raw("p2", 0x7768, 0x6426, Some("AJ159 NL"), 0xFFB2, 0x0001);
        second.serial = Some("SN001".to_string());
        let third = raw("p3", 0x1234, 0x0001, None, 0x0001, 0x0002);

        let targets = group_targets(merge_collections(vec![first, second, third]));
        assert_eq!(targets.len(), 2, "两个 VID:PID → 两台设备");
        assert_eq!(targets[0].vid_pid(), "0x1234:0x0001");
        let ajazz = &targets[1];
        assert_eq!(ajazz.vid_pid(), "0x7768:0x6426");
        assert_eq!(ajazz.collections.len(), 2);
        assert_eq!(ajazz.display_name(), "AJ159 NL");
        assert_eq!(ajazz.manufacturer.as_deref(), Some("AJAZZ"));
        assert_eq!(ajazz.serials, vec!["SN001", "SN002"], "序列号去重并排序");
        assert_eq!(ajazz.serial_summary().as_deref(), Some("SN001、SN002"));
        assert_eq!(ajazz.private_collections(), 1, "只有一个私有集合");
    }

    /// 无名设备退回 VID:PID，单集合设备也保留在 `collections` 里（前端不写两套逻辑）。
    #[test]
    fn targets_fall_back_to_vid_pid_and_keep_single_collection() {
        let targets = group_targets(merge_collections(vec![raw(
            "p",
            0x0ABC,
            0x0DEF,
            Some("   "),
            0x0001,
            0x0002,
        )]));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].display_name(), "0x0ABC:0x0DEF");
        assert_eq!(targets[0].collections.len(), 1);
        assert_eq!(targets[0].serial_summary(), None);
        assert_eq!(targets[0].private_collections(), 0);
        assert!(group_targets(vec![]).is_empty());
    }

    /// 平台门面：不支持的平台上必须给出「不支持」而不是 panic 或空成功。
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    #[test]
    fn unsupported_platforms_report_it() {
        let stop = AtomicBool::new(false);
        assert_eq!(enumerate(), Err(CaptureError::Unsupported));
        assert_eq!(
            capture(&CaptureRequest::new("x"), &stop, |_| {}),
            Err(CaptureError::Unsupported)
        );
    }

    /// 有实现的两个平台上，枚举要么成功要么给出可读的初始化错误，绝不 panic。
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn enumeration_never_panics() {
        match enumerate() {
            Ok(devices) => {
                for device in &devices {
                    assert!(!device.path.is_empty(), "枚举结果必须有路径");
                    assert!(!device.usages.is_empty(), "每个集合至少一个 Usage");
                }
            }
            Err(err) => assert!(!err.to_string().is_empty()),
        }
    }

    /// 打不开的路径必须报错（而不是无限等）：这是「选错集合」最常见的反馈路径。
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn capturing_a_missing_path_fails_fast() {
        let stop = AtomicBool::new(false);
        let mut request = CaptureRequest::new("/dev/definitely-not-a-hid-device");
        request.duration_ms = 1_000;
        let started = Instant::now();
        let result = capture(&request, &stop, |_| {});
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "打开失败必须立刻返回"
        );
    }
}
