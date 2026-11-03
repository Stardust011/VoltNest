//! 生成上报数据：枚举候选集合、录制原始 HID 报告、整理成可直接贴进 issue 的报告。
//!
//! 抓包自成临时句柄，独立于采集线程。帧数上限 [`CAPTURE_MAX_FRAMES`]、正文字符预算 [`REPORT_MAX_CHARS`]，超限时在报告中明示；复制的是正文，落盘文件才是完整数据。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use voltnest_hid::hex;
use voltnest_hid::probe::{
    self, CAPTURE_DEFAULT_WRITE_INTERVAL_MS, CAPTURE_MAX_FRAMES, CaptureRequest, CaptureSummary,
    CapturedFrame, CollectionInfo, Direction,
};
use voltnest_provider::clock;
use voltnest_storage::{DataPaths, ManualDevice, ManualDeviceFile, ManualDeviceStore, atomic};

use crate::dto::{
    CaptureEventDto, CaptureFrameDto, CaptureStartDto, CaptureStartResultDto, CaptureSummaryDto,
    HidCollectionDto, HidTargetDto, ManualDeviceDto, ReportDto, ReportFormDto, ReportSampleDto,
};
use crate::host::lock;

/// 抓包进度事件名（与前端 `CAPTURE_EVENT` 一字不差）。
pub const CAPTURE_EVENT: &str = "probe://capture";

/// 一批进度事件里最多携带的帧数。
const FRAME_BATCH: usize = 24;

/// 进度事件的最小间隔：设备狂发时也不要一个字节一个事件地打 IPC。
const EMIT_INTERVAL_MS: u64 = 150;

/// 报告原始数据块里最多列出的帧组数。
const REPORT_MAX_FRAME_GROUPS: usize = 500;

/// 报告正文字符预算（GitHub issue 正文上限 65536 字符，留出余量）。
pub const REPORT_MAX_CHARS: usize = 60_000;

/// 一次抓包会话。
struct Session {
    /// 实际打开的集合（宿主重新枚举后的结果，不是前端传上来的那份）。
    device: CollectionInfo,
    /// 停止标志：`stop_hid_capture` 与窗口关闭都走它。
    stop: Arc<AtomicBool>,
    /// 已录到的帧（报告的数据源）。
    frames: Mutex<Vec<CapturedFrame>>,
    /// 结束时的统计。
    summary: Mutex<Option<CaptureSummary>>,
    /// 抓取结束时刻（UTC 毫秒）：采样时间要如实记录，不能用「生成报告的时刻」冒充。
    finished_at_ms: Mutex<Option<u64>>,
}

/// 生成上报数据相关的跨命令状态（不参与采集生命周期）。
pub struct ProbeState {
    paths: DataPaths,
    store: ManualDeviceStore,
    /// 手动添加的设备（内存副本；落盘由 `store` 负责）。
    manual: Mutex<ManualDeviceFile>,
    /// 进行中 / 刚结束的抓包会话。
    sessions: Mutex<HashMap<u64, Arc<Session>>>,
    next_id: AtomicU64,
}

impl ProbeState {
    /// 载入手动设备清单。文件损坏时只记一条日志，不阻塞启动。
    pub fn new(paths: DataPaths, now_ms: u64) -> Self {
        let store = ManualDeviceStore::new(paths.clone());
        let loaded = store.load(now_ms);
        if let Some(path) = &loaded.corrupt_backup {
            log::warn!(
                "manual_devices.json 损坏，已存档到 {}，本次以空表启动",
                path.display()
            );
        }
        if let Some(path) = &loaded.unsupported_backup {
            log::warn!(
                "manual_devices.json 版本过高，已存档到 {}，本次以空表启动",
                path.display()
            );
        }
        Self {
            paths,
            store,
            manual: Mutex::new(loaded.file),
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        }
    }

    fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 取一份会话句柄（不存在时给出可操作的提示）。
    fn session(&self, session_id: u64) -> Result<Arc<Session>, String> {
        lock(&self.sessions)
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "抓包会话已结束，请重新录制一段数据".to_string())
    }
}

/// 列出设备（按 VID:PID 聚合的集合组）。枚举为空不是错误。
#[tauri::command]
pub fn list_hid_targets() -> Result<Vec<HidTargetDto>, String> {
    let devices = probe::enumerate().map_err(|err| err.to_string())?;
    Ok(probe::group_targets(devices)
        .into_iter()
        .map(HidTargetDto::from)
        .collect())
}

/// 开始抓取。立即返回会话号，数据通过 [`CAPTURE_EVENT`] 分批推送。
#[tauri::command]
pub fn start_hid_capture(
    app: AppHandle,
    request: CaptureStartDto,
    state: State<'_, Arc<ProbeState>>,
) -> Result<CaptureStartResultDto, String> {
    let write_frames = parse_write_frames(&request.write_frames)?;
    let device = probe::enumerate()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|candidate| candidate.path == request.path)
        .ok_or_else(|| "设备已断开或集合路径已变化，请点「刷新」后重新选择".to_string())?;

    let mut capture = CaptureRequest::new(device.path.clone());
    capture.duration_ms = request.duration_ms;
    capture.write_frames = write_frames;
    capture.write_interval_ms = request
        .write_interval_ms
        .unwrap_or(CAPTURE_DEFAULT_WRITE_INTERVAL_MS);
    capture.max_frames = CAPTURE_MAX_FRAMES;

    let session_id = state.next_session_id();
    let session = Arc::new(Session {
        device: device.clone(),
        stop: Arc::new(AtomicBool::new(false)),
        frames: Mutex::new(Vec::new()),
        summary: Mutex::new(None),
        finished_at_ms: Mutex::new(None),
    });
    lock(&state.sessions).insert(session_id, Arc::clone(&session));

    let duration_ms = capture.clamped_duration_ms();
    let max_frames = capture.clamped_max_frames();
    let handle = app.clone();
    let worker = Arc::clone(&session);
    let spawned = thread::Builder::new()
        .name(format!("hid-capture-{session_id}"))
        .spawn(move || run_capture(handle, session_id, capture, worker));
    if let Err(err) = spawned {
        lock(&state.sessions).remove(&session_id);
        return Err(format!("无法启动抓包线程：{err}"));
    }

    Ok(CaptureStartResultDto {
        session_id,
        device: HidCollectionDto::from(device),
        duration_ms,
        max_frames,
    })
}

/// 请求停止（已录到的数据保留，仍可生成报告）。
#[tauri::command]
pub fn stop_hid_capture(session_id: u64, state: State<'_, Arc<ProbeState>>) -> Result<(), String> {
    let session = state.session(session_id)?;
    session.stop.store(true, Ordering::Relaxed);
    Ok(())
}

/// 丢弃会话（关闭面板时调用）：停止线程并释放帧内存。
#[tauri::command]
pub fn discard_hid_capture(session_id: u64, state: State<'_, Arc<ProbeState>>) {
    if let Some(session) = lock(&state.sessions).remove(&session_id) {
        session.stop.store(true, Ordering::Relaxed);
    }
}

/// 生成 issue 报告：只返回正文，不写磁盘（保存时才落盘）。
#[tauri::command]
pub fn generate_issue_report(
    form: ReportFormDto,
    state: State<'_, Arc<ProbeState>>,
) -> Result<ReportDto, String> {
    let (device, samples) = collect_samples(&state, &form.samples)?;
    let markdown = build_report(&device, &samples, &form, clock::now_utc_ms(), None);
    Ok(ReportDto { markdown })
}

/// 保存为「手动添加的设备」：同一份报告同时落盘，记录里只存文件路径。
#[tauri::command]
pub fn save_manual_device(
    form: ReportFormDto,
    state: State<'_, Arc<ProbeState>>,
) -> Result<ManualDeviceDto, String> {
    let (device, samples) = collect_samples(&state, &form.samples)?;
    let now_ms = clock::now_utc_ms();
    let path = report_path(&state.paths, &device, now_ms);
    let hint = path.display().to_string();
    let markdown = build_report(&device, &samples, &form, now_ms, Some(&hint));
    atomic::write_atomic(&path, markdown.as_bytes())
        .map_err(|err| format!("写入报告失败：{err}"))?;

    let usage = device.primary_usage();
    let entry = ManualDevice {
        id: format!("hid:{:04x}:{:04x}", device.vid, device.pid),
        name: display_name(&form, &device),
        vid: device.vid,
        pid: device.pid,
        usage_page: usage.map(|u| u.page).unwrap_or(0),
        usage: usage.map(|u| u.usage).unwrap_or(0),
        path: device.path.clone(),
        product: device.product.clone(),
        serial: device.serial.clone(),
        note: form.note.trim().to_string(),
        added_at_ms: now_ms,
        last_capture_at_ms: samples.iter().map(|sample| sample.captured_at_ms).max(),
        captured_frames: samples.iter().map(|sample| sample.frames.len()).sum(),
        samples: samples
            .iter()
            .map(|sample| voltnest_storage::ManualSample {
                battery_percent: sample.battery,
                frames: sample.frames.len(),
                captured_at_ms: sample.captured_at_ms,
                extra: Default::default(),
            })
            .collect(),
        report_path: Some(hint),
        extra: Default::default(),
    };

    let mut next = lock(&state.manual).clone();
    next.upsert(entry.clone());
    state
        .store
        .save(&next)
        .map_err(|err| format!("保存手动设备失败：{err}"))?;
    *lock(&state.manual) = next;
    log::info!(
        "已保存手动设备 {}（VID:PID 0x{:04X}:0x{:04X}，{} 次采样 / {} 帧）",
        entry.name,
        entry.vid,
        entry.pid,
        entry.samples.len(),
        entry.captured_frames
    );
    Ok(ManualDeviceDto::from(&entry))
}

/// 手动添加的设备列表（新添加的在前）。
#[tauri::command]
pub fn list_manual_devices(state: State<'_, Arc<ProbeState>>) -> Vec<ManualDeviceDto> {
    let mut list: Vec<ManualDeviceDto> = lock(&state.manual)
        .devices
        .iter()
        .map(ManualDeviceDto::from)
        .collect();
    list.sort_by_key(|device| std::cmp::Reverse(device.added_at_ms));
    list
}

/// 报告文件是否可删：必须在 `reports/` 下且为 `.md`（记录可被手工编辑，防删任意文件）。
fn removable_report_path(reports: &Path, stored: Option<&str>) -> Option<PathBuf> {
    let path = PathBuf::from(stored?);
    let under_reports = path.starts_with(reports);
    let is_markdown = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    (under_reports && is_markdown).then_some(path)
}

#[tauri::command]
pub fn remove_manual_device(id: String, state: State<'_, Arc<ProbeState>>) -> Result<(), String> {
    let removed = lock(&state.manual).get(&id).cloned();
    let mut next = lock(&state.manual).clone();
    if !next.remove(&id) {
        return Err("这条记录已经不存在".to_string());
    }
    state
        .store
        .save(&next)
        .map_err(|err| format!("保存手动设备失败：{err}"))?;
    *lock(&state.manual) = next;

    let Some(device) = removed else {
        return Ok(());
    };
    let Some(path) =
        removable_report_path(&state.paths.reports_dir(), device.report_path.as_deref())
    else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => log::info!("已删除报告文件 {}", path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("删除报告文件失败（{}）：{err}", path.display()),
    }
    Ok(())
}

/// 读取某条手动设备的报告正文（用于「复制报告」）。
#[tauri::command]
pub fn get_manual_report(id: String, state: State<'_, Arc<ProbeState>>) -> Result<String, String> {
    let reports = state.paths.reports_dir();
    let stored = {
        let file = lock(&state.manual);
        file.get(&id)
            .ok_or_else(|| "这条记录已经不存在".to_string())?
            .report_path
            .clone()
    };
    let Some(stored) = stored else {
        return Err("这条记录没有报告文件（可能只保存了设备信息）".to_string());
    };
    let path = PathBuf::from(&stored);
    if !path.starts_with(&reports) {
        return Err("报告路径不在数据目录内，已拒绝读取".to_string());
    }
    std::fs::read_to_string(&path).map_err(|err| format!("读取报告失败：{err}"))
}

/// 抓包线程主体：把帧流式推给前端，结束后再发一条带统计的收尾事件。
fn run_capture(app: AppHandle, session_id: u64, request: CaptureRequest, session: Arc<Session>) {
    let mut pending: Vec<CaptureFrameDto> = Vec::new();
    let mut last_emit = Instant::now();

    flush(&app, session_id, &mut pending, 0, "running", None, None);

    let session_for_sink = Arc::clone(&session);
    let result = probe::capture(&request, &session.stop, |frame| {
        let dto = CaptureFrameDto::from(&frame);
        let total = {
            let mut guard = lock(&session_for_sink.frames);
            guard.push(frame);
            guard.len()
        };
        pending.push(dto);
        if pending.len() >= FRAME_BATCH
            || last_emit.elapsed() >= Duration::from_millis(EMIT_INTERVAL_MS)
        {
            flush(&app, session_id, &mut pending, total, "running", None, None);
            last_emit = Instant::now();
        }
    });

    let total = lock(&session.frames).len();
    *lock(&session.finished_at_ms) = Some(clock::now_utc_ms());
    match result {
        Ok(summary) => {
            let status = if session.stop.load(Ordering::Relaxed) {
                "stopped"
            } else {
                "done"
            };
            *lock(&session.summary) = Some(summary.clone());
            flush(
                &app,
                session_id,
                &mut pending,
                total,
                status,
                None,
                Some(CaptureSummaryDto::from(&summary)),
            );
        }
        Err(err) => {
            let message = err.to_string();
            log::warn!("手动抓包结束（{message}），已录 {total} 帧");
            flush(
                &app,
                session_id,
                &mut pending,
                total,
                "failed",
                Some(message),
                None,
            );
        }
    }
}

/// 发一条进度事件。事件发不出去（窗口已关）不算错误。
fn flush(
    app: &AppHandle,
    session_id: u64,
    pending: &mut Vec<CaptureFrameDto>,
    total: usize,
    status: &'static str,
    message: Option<String>,
    summary: Option<CaptureSummaryDto>,
) {
    let event = CaptureEventDto {
        session_id,
        frames: std::mem::take(pending),
        total,
        status,
        message,
        summary,
    };
    if let Err(err) = app.emit(CAPTURE_EVENT, &event) {
        log::warn!("抓包进度事件发送失败：{err}");
    }
}

/// 解析用户手填的查询报文。
fn parse_write_frames(raw: &[String]) -> Result<Vec<Vec<u8>>, String> {
    let mut frames = Vec::with_capacity(raw.len());
    for (index, text) in raw.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let bytes = hex::parse(text)
            .map_err(|err| format!("第 {} 条查询报文无法解析：{err}", index + 1))?;
        if !bytes.is_empty() {
            frames.push(bytes);
        }
    }
    Ok(frames)
}

/// 一次采样在报告渲染时的全部输入。
struct SampleInput {
    /// 该次采样录到的帧（已按会话顺序）。
    frames: Vec<CapturedFrame>,
    /// 结束统计（抓取中途失败时为 `None`）。
    summary: Option<CaptureSummary>,
    /// 驱动 / 系统显示的电量。
    battery: Option<u8>,
    /// 采样完成时间（UTC 毫秒）。
    captured_at_ms: u64,
}

/// 取出全部采样，并挡住「一帧都没有」「多次采样不是同一集合」这类必然出错的报告。
fn collect_samples(
    state: &ProbeState,
    requests: &[ReportSampleDto],
) -> Result<(CollectionInfo, Vec<SampleInput>), String> {
    if requests.is_empty() {
        return Err("还没有任何采样：请先录制一段数据再生成报告".to_string());
    }
    let mut device: Option<CollectionInfo> = None;
    let mut seen: Vec<u64> = Vec::with_capacity(requests.len());
    let mut samples = Vec::with_capacity(requests.len());
    let now_ms = clock::now_utc_ms();

    for (index, request) in requests.iter().enumerate() {
        let no = index + 1;
        if seen.contains(&request.session_id) {
            return Err(format!(
                "第 {no} 次采样与前面的采样是同一段数据，请重新录制"
            ));
        }
        seen.push(request.session_id);

        let session = state.session(request.session_id)?;
        session.stop.store(true, Ordering::Relaxed);
        let frames = lock(&session.frames).clone();
        if frames.is_empty() {
            return Err(format!(
                "第 {no} 次采样没有录到任何数据：请确认设备已连接，录制期间做点操作（按键 / 滚轮 / 插拔），或补一条查询报文后重录"
            ));
        }
        match &device {
            Some(first) if first.path != session.device.path => {
                return Err(
                    "多次采样选中的 HID 集合不一致：请在同一集合上重新采集全部样本".to_string(),
                );
            }
            Some(_) => {}
            None => device = Some(session.device.clone()),
        }
        samples.push(SampleInput {
            summary: lock(&session.summary).clone(),
            battery: request.battery_percent.map(|percent| percent.min(100)),
            captured_at_ms: lock(&session.finished_at_ms).unwrap_or(now_ms),
            frames,
        });
    }

    let device = device.ok_or_else(|| "没有可用的采样".to_string())?;
    Ok((device, samples))
}

/// 报告文件路径：`reports/hid-<vid>-<pid>-<时间戳>.md`。
fn report_path(paths: &DataPaths, device: &CollectionInfo, now_ms: u64) -> PathBuf {
    paths.reports_dir().join(format!(
        "hid-{:04x}-{:04x}-{}.md",
        device.vid,
        device.pid,
        utc_compact(now_ms)
    ))
}

/// 报告里显示的名称：用户填的优先，空白时退回 HID 产品名。
fn display_name(form: &ReportFormDto, device: &CollectionInfo) -> String {
    let typed = form.name.trim();
    if typed.is_empty() {
        device.display_name()
    } else {
        typed.to_string()
    }
}

fn build_report(
    device: &CollectionInfo,
    samples: &[SampleInput],
    form: &ReportFormDto,
    now_ms: u64,
    report_path: Option<&str>,
) -> String {
    let name = display_name(form, device);
    let usage = device
        .primary_usage()
        .map(|usage| usage.to_string())
        .unwrap_or_else(|| "（未声明）".to_string());

    let mut out = String::with_capacity(8 * 1024);
    let _ = writeln!(out, "### 设备无法识别：{}", one_line(&name));
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "> 由 VoltNest {} 生成 · {} · 用户上报数据（未自动识别）",
        env!("CARGO_PKG_VERSION"),
        utc_stamp(now_ms)
    );
    let _ = writeln!(out);

    // 设备信息
    let _ = writeln!(out, "**设备信息**");
    let _ = writeln!(out);
    let _ = writeln!(out, "| 项目 | 值 |");
    let _ = writeln!(out, "| --- | --- |");
    let _ = writeln!(out, "| 名称 | {} |", cell(&name));
    let _ = writeln!(out, "| VID:PID | `{}` |", device.vid_pid());
    let _ = writeln!(out, "| UsagePage / Usage | `{}` |", usage);
    let _ = writeln!(out, "| 全部 Usage | `{}` |", cell(&device.usage_summary()));
    let _ = writeln!(
        out,
        "| 产品名 | {} |",
        optional_cell(device.product.as_deref())
    );
    let _ = writeln!(
        out,
        "| 厂商 | {} |",
        optional_cell(device.manufacturer.as_deref())
    );
    let _ = writeln!(
        out,
        "| 序列号 | {} |",
        optional_cell(device.serial.as_deref())
    );
    let _ = writeln!(out, "| 接口号 | `{}` |", device.interface_number);
    let _ = writeln!(out, "| HID 路径 | `{}` |", cell(&device.path));
    let _ = writeln!(out, "| 应用版本 | `{}` |", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "| 系统 | `{} / {}` |",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out);

    if device.only_standard_usages() {
        let _ = writeln!(
            out,
            "> 注意：选中的集合只声明了标准 HID 用法（鼠标 / 键盘）。厂商电量协议通常挂在 `0xFF00` 以上的 UsagePage，若下面的数据里看不到电量，请换一个集合重录。"
        );
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "**采样记录**");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| 样本 | 驱动显示电量 | 采集方式 | 录制时长 | 帧数（输入 / 输出） | 异常 |"
    );
    let _ = writeln!(out, "| ---: | --- | --- | ---: | --- | --- |");
    for (index, sample) in samples.iter().enumerate() {
        let input = sample
            .frames
            .iter()
            .filter(|frame| frame.direction == Direction::In)
            .count();
        let output = sample.frames.len() - input;
        let _ = writeln!(
            out,
            "| {} | {} | {} | {:.1} s | {} / {}（共 {}） | {} |",
            index + 1,
            battery_text(sample.battery),
            capture_mode(sample.summary.as_ref()),
            sample
                .summary
                .as_ref()
                .map_or(0, |summary| summary.elapsed_ms) as f64
                / 1000.0,
            input,
            output,
            sample.frames.len(),
            anomaly_text(sample.summary.as_ref())
        );
    }
    let _ = writeln!(out);
    if samples.len() > 1 {
        let _ = writeln!(
            out,
            "> 请对照各样本的原始数据：电量变化时取值跟着变化的字节，通常就是电量字段。"
        );
    } else {
        let _ = writeln!(
            out,
            "> 目前只有一次采样。等设备电量发生变化后，用同一集合再录一次（并在录制前填上驱动显示的新电量），两次对照才能定位电量字段。"
        );
    }
    let _ = writeln!(out);

    // 操作步骤
    let _ = writeln!(out, "**操作步骤**");
    let _ = writeln!(out);
    let steps = form.steps.trim();
    if steps.is_empty() {
        let _ = writeln!(out, "1. 打开 VoltNest → 诊断 → 生成上报数据");
        let _ = writeln!(out, "2. 选择设备「{}」及其 HID 集合", one_line(&name));
        let _ = writeln!(out, "3. 记录驱动显示的电量，录制一段数据（可多次采样）");
        let _ = writeln!(out, "4. 设备电量变化后，再按同一集合采集一次");
    } else {
        let _ = writeln!(out, "{}", steps);
    }
    let _ = writeln!(out);

    // 期望行为
    let _ = writeln!(out, "**期望行为**");
    let _ = writeln!(out);
    let expectation = form.expectation.trim();
    if expectation.is_empty() {
        let _ = writeln!(out, "设备能出现在设备列表里，并显示电量。");
    } else {
        let _ = writeln!(out, "{}", expectation);
    }
    let _ = writeln!(out);

    if !form.note.trim().is_empty() {
        let _ = writeln!(out, "**补充说明**");
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", form.note.trim());
        let _ = writeln!(out);
    }

    let mut budget = REPORT_MAX_FRAME_GROUPS;
    for (index, sample) in samples.iter().enumerate() {
        let groups = probe::group_frames(&sample.frames);
        let _ = writeln!(
            out,
            "**原始 HID 数据 — 样本 {}（{}，{}）**",
            index + 1,
            battery_text(sample.battery),
            utc_stamp(sample.captured_at_ms)
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "时间从该次录制开始计算；`×N` 表示连续 N 帧完全相同（已折叠，避免刷屏）。"
        );
        let _ = writeln!(out);

        if budget == 0 {
            let _ = writeln!(
                out,
                "> 已达到报告行数上限，本样本的原始数据未展开。{}",
                attachment_hint(report_path)
            );
            let _ = writeln!(out);
            continue;
        }

        let take = groups.len().min(budget);
        let _ = writeln!(out, "```text");
        let _ = writeln!(out, "{:>8}  {:<3}  {:<4}  数据", "t(ms)", "方向", "长度");
        for group in groups.iter().take(take) {
            let repeat = if group.count > 1 {
                format!("  ×{}", group.count)
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "{:>8}  {:<4} {:<4}  {}{}",
                group.frame.elapsed_ms,
                group.frame.direction.as_str(),
                group.frame.data.len(),
                group.frame.hex(),
                repeat
            );
        }
        let _ = writeln!(out, "```");
        let _ = writeln!(out);
        budget -= take;
        if take < groups.len() {
            let _ = writeln!(
                out,
                "> 本样本共 {} 组，受报告长度限制只列出前 {} 组（其余样本同样受限）。{}",
                groups.len(),
                take,
                attachment_hint(report_path)
            );
            let _ = writeln!(out);
        }
    }

    truncate_to_chars(out, REPORT_MAX_CHARS, report_path)
}

/// 电量的统一写法：没填就说没填，不留空单元格（空着会让人以为是解析失败）。
fn battery_text(battery: Option<u8>) -> String {
    match battery {
        Some(percent) => format!("{percent}%"),
        None => "未提供（驱动未显示）".to_string(),
    }
}

/// 采集方式一行：纯监听 / 主动查询，并区分「发了但全失败」。
fn capture_mode(summary: Option<&CaptureSummary>) -> String {
    match summary {
        Some(summary) if summary.output_frames > 0 => format!(
            "主动查询（录制期间发出 {} 帧，每 {} ms 一轮）",
            summary.output_frames,
            summary.write_interval_ms.max(1)
        ),
        Some(summary) if summary.write_failures > 0 => {
            "主动查询（报文全部发送失败，见下表「发送失败」）".to_string()
        }
        Some(_) => "纯监听（未向设备发任何报文）".to_string(),
        None => "未知（抓取未正常结束）".to_string(),
    }
}

/// 「异常」列：合并截断与发送失败，无问题时为 `—`。
fn anomaly_text(summary: Option<&CaptureSummary>) -> String {
    let Some(summary) = summary else {
        return "未知（抓取未正常结束）".to_string();
    };
    let mut notes: Vec<String> = Vec::new();
    if summary.truncated {
        notes.push(format!("数据已截断（达 {CAPTURE_MAX_FRAMES} 帧上限）"));
    }
    if summary.write_failures > 0 {
        notes.push(format!(
            "{} 次发送失败（首次：{}）",
            summary.write_failures,
            one_line(summary.write_error.as_deref().unwrap_or("未记录"))
        ));
    }
    if notes.is_empty() {
        "—".to_string()
    } else {
        notes.join("；")
    }
}

fn attachment_hint(report_path: Option<&str>) -> String {
    match report_path {
        Some(path) => format!("完整数据见报告文件，可把它作为附件上传：\n> `{path}`"),
        None => "需要完整数据时点「保存到本地」，报告文件会写到数据目录并显示路径。".to_string(),
    }
}

/// 可选字段的表格单元格：缺失写「—」，而不是留一个空白格让人以为是丢了。
fn optional_cell(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => format!("`{}`", cell(text)),
        None => "—".to_string(),
    }
}

/// 表格单元格转义：`|` 会截断单元格，换行会截断整行。
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// 单行化（用于标题与正文里嵌一段用户输入）。
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// 按字符数截断（不是字节数：中文一个字三字节，按字节切会把字切坏）。
fn truncate_to_chars(text: String, limit: usize, report_path: Option<&str>) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    let cut: String = text.chars().take(limit).collect();
    let cut = match cut.rfind('\n') {
        Some(index) => cut[..index + 1].to_string(),
        None => cut,
    };
    format!(
        "{cut}\n\n> 报告正文超过 {limit} 字符已截断。{}\n",
        attachment_hint(report_path)
    )
}

/// UTC 时间戳：`2026-10-29 03:10:44 UTC`。手写日历换算，不引入日期库。
fn utc_stamp(ms: u64) -> String {
    let (year, month, day, hour, minute, second) = civil(ms);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// 文件名用的紧凑时间戳：`20261029-031044`。
fn utc_compact(ms: u64) -> String {
    let (year, month, day, hour, minute, second) = civil(ms);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// 毫秒时间戳 → UTC 日历字段（Howard Hinnant 的 civil_from_days 算法）。
fn civil(ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let total_seconds = ms / 1_000;
    let days = (total_seconds / 86_400) as i64;
    let remainder = total_seconds % 86_400;
    let hour = (remainder / 3_600) as u32;
    let minute = ((remainder % 3_600) / 60) as u32;
    let second = (remainder % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> CollectionInfo {
        CollectionInfo {
            path: r"\\?\hid#vid_7768&pid_6426&mi_01#8&2a4f0d1&0&0000".to_string(),
            vid: 0x7768,
            pid: 0x6426,
            product: Some("AJ159 NL".to_string()),
            manufacturer: Some("AJAZZ".to_string()),
            serial: None,
            interface_number: 1,
            usages: vec![
                voltnest_hid::probe::Usage {
                    page: 0x0001,
                    usage: 0x0002,
                },
                voltnest_hid::probe::Usage {
                    page: 0xFFB2,
                    usage: 0x0001,
                },
            ],
        }
    }

    /// `samples` 只被 `collect_samples` 使用，渲染测试直接给空列表。
    fn form() -> ReportFormDto {
        ReportFormDto {
            samples: Vec::new(),
            name: "AJ159 NL".to_string(),
            steps: "1. 插上接收器\n2. 拨动滚轮".to_string(),
            expectation: "设备出现在列表里".to_string(),
            note: "电量一直显示不出来".to_string(),
        }
    }

    fn frames(values: &[(u64, Direction, &[u8])]) -> Vec<CapturedFrame> {
        values
            .iter()
            .map(|(elapsed_ms, direction, data)| CapturedFrame {
                elapsed_ms: *elapsed_ms,
                direction: *direction,
                data: data.to_vec(),
            })
            .collect()
    }

    fn sample(battery: Option<u8>, frames: Vec<CapturedFrame>) -> SampleInput {
        let input = frames
            .iter()
            .filter(|frame| frame.direction == Direction::In)
            .count();
        SampleInput {
            summary: Some(CaptureSummary {
                frames: frames.len(),
                input_frames: input,
                output_frames: frames.len() - input,
                elapsed_ms: 20_000,
                truncated: false,
                write_interval_ms: 0,
                write_failures: 0,
                write_error: None,
            }),
            battery,
            captured_at_ms: 1_700_000_000_000,
            frames,
        }
    }

    fn first_sample() -> SampleInput {
        sample(
            Some(80),
            frames(&[
                (0, Direction::In, &[0x06, 0x03, 0x50, 0x01]),
                (10, Direction::In, &[0x06, 0x03, 0x50, 0x01]),
                (20, Direction::In, &[0x06, 0x03, 0x33, 0x01]),
            ]),
        )
    }

    fn second_sample() -> SampleInput {
        sample(
            Some(75),
            frames(&[(0, Direction::In, &[0x06, 0x03, 0x2F, 0x01])]),
        )
    }

    /// 报告的核心承诺：设备标识、采样电量、步骤、原始字节都在，且能对上。
    #[test]
    fn report_contains_identity_samples_steps_and_raw_bytes() {
        let text = build_report(
            &device(),
            &[first_sample(), second_sample()],
            &form(),
            1_700_000_000_000,
            Some("C:/data/reports/hid-7768-6426.md"),
        );

        assert!(text.contains("### 设备无法识别：AJ159 NL"), "{text}");
        assert!(text.contains("| VID:PID | `0x7768:0x6426` |"), "{text}");
        assert!(
            text.contains("| UsagePage / Usage | `0xFFB2/0x0001` |"),
            "{text}"
        );
        assert!(text.contains("0x0001/0x0002, 0xFFB2/0x0001"), "{text}");
        assert!(text.contains("1. 插上接收器"), "{text}");
        assert!(text.contains("设备出现在列表里"), "{text}");
        assert!(text.contains("电量一直显示不出来"), "{text}");

        assert!(
            text.contains(
                "| 1 | 80% | 纯监听（未向设备发任何报文） | 20.0 s | 3 / 0（共 3） | — |"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "| 2 | 75% | 纯监听（未向设备发任何报文） | 20.0 s | 1 / 0（共 1） | — |"
            ),
            "{text}"
        );

        assert!(
            text.contains("**原始 HID 数据 — 样本 1（80%，2023-11-14 22:13:20 UTC）**"),
            "{text}"
        );
        assert!(
            text.contains("**原始 HID 数据 — 样本 2（75%，2023-11-14 22:13:20 UTC）**"),
            "{text}"
        );
        assert!(text.contains("06 03 50 01  ×2"), "{text}");
        assert!(text.contains("06 03 2F 01"), "{text}");
        assert!(text.contains("2023-11-14 22:13:20 UTC"), "{text}");
        assert!(text.contains("请对照各样本的原始数据"), "{text}");

        assert!(!text.contains("字节分布"), "{text}");
        assert!(!text.contains("变化最明显"), "{text}");
    }

    /// 只有一次采样时必须明确引导「电量变化后再采一次」，否则用户拿不到对照。
    #[test]
    fn single_sample_asks_for_a_second_capture() {
        let text = build_report(&device(), &[first_sample()], &form(), 0, None);
        assert!(text.contains("目前只有一次采样"), "{text}");
        assert!(text.contains("再录一次"), "{text}");
        assert!(!text.contains("请对照各样本"), "{text}");
    }

    /// 驱动没显示电量时不能编一个 0%，要明确写出来。
    #[test]
    fn missing_battery_is_stated_explicitly() {
        let text = build_report(
            &device(),
            &[sample(None, first_sample().frames)],
            &form(),
            0,
            None,
        );
        assert!(text.contains("未提供（驱动未显示）"), "{text}");
        assert!(!text.contains("| 1 | 0%"), "{text}");
        assert_eq!(anomaly_text(None), "未知（抓取未正常结束）");
        assert_eq!(battery_text(Some(37)), "37%");
    }

    /// 只有标准 Usage 时必须给出「换集合」的提示，否则用户会以为是协议不支持。
    #[test]
    fn report_warns_about_standard_only_collections() {
        let mut only_standard = device();
        only_standard.usages = vec![voltnest_hid::probe::Usage {
            page: 0x0001,
            usage: 0x0002,
        }];
        let text = build_report(&only_standard, &[first_sample()], &form(), 0, None);
        assert!(text.contains("只声明了标准 HID 用法"), "{text}");
        assert!(
            text.contains("| UsagePage / Usage | `0x0001/0x0002` |"),
            "{text}"
        );
    }

    /// 空表单要有可用的默认步骤，不能生成一份「什么都没写」的报告。
    #[test]
    fn empty_form_gets_placeholders() {
        let mut empty = form();
        empty.steps = "   ".to_string();
        empty.expectation = String::new();
        empty.note = String::new();
        let text = build_report(&device(), &[first_sample()], &empty, 0, None);
        assert!(
            text.contains("打开 VoltNest → 诊断 → 生成上报数据"),
            "{text}"
        );
        assert!(text.contains("设备能出现在设备列表里"), "{text}");
        assert!(!text.contains("**补充说明**"), "没有补充说明就别占一节");
    }

    /// 帧组过多时要明说「只列了前 N 组」，并把完整文件指出来。
    #[test]
    fn report_declares_truncation_of_long_dumps() {
        let long = sample(
            Some(60),
            (0..(REPORT_MAX_FRAME_GROUPS as u64 + 20))
                .map(|index| CapturedFrame {
                    elapsed_ms: index,
                    direction: Direction::In,
                    data: vec![0x06, (index % 200) as u8],
                })
                .collect(),
        );
        let text = build_report(&device(), &[long], &form(), 0, Some("C:/data/reports/x.md"));
        assert!(text.contains("只列出前 500 组"), "{text}");
        assert!(text.contains("C:/data/reports/x.md"), "{text}");
        assert!(
            text.chars().count() <= REPORT_MAX_CHARS + 200,
            "正文预算要守住"
        );
    }

    /// 行数预算是所有样本共享的：第二个样本不能重新领一份 500 行。
    #[test]
    fn frame_budget_is_shared_between_samples() {
        let make = |offset: u64| {
            sample(
                Some(50),
                (0..300u64)
                    .map(|index| CapturedFrame {
                        elapsed_ms: index,
                        direction: Direction::In,
                        data: vec![0x06, ((index + offset) % 250) as u8],
                    })
                    .collect(),
            )
        };
        let text = build_report(&device(), &[make(0), make(7)], &form(), 0, None);
        assert!(
            text.contains("本样本共 300 组，受报告长度限制只列出前 200 组"),
            "{text}"
        );
        let rows = text
            .lines()
            .filter(|line| matches!(line.split_whitespace().nth(1), Some("IN") | Some("OUT")))
            .count();
        assert_eq!(rows, REPORT_MAX_FRAME_GROUPS, "总行数不得超预算");
    }

    /// 正文字符预算：超长时必须截断并指向文件。
    #[test]
    fn body_is_capped_by_character_budget() {
        let long = "长".repeat(REPORT_MAX_CHARS);
        let cut = truncate_to_chars(long, 100, Some("p.md"));
        assert!(cut.chars().count() < 400, "截断后要短下来");
        assert!(cut.contains("已截断"));
        assert!(cut.contains("p.md"));
        assert_eq!(truncate_to_chars("abc".to_string(), 100, None), "abc");
        let preview = truncate_to_chars("长".repeat(400), 100, None);
        assert!(preview.contains("保存到本地"), "{preview}");
    }

    /// 只删 `reports/` 下的 `.md`：手工改过的 `manual_devices.json` 不能变成「指哪删哪」。
    #[test]
    fn only_markdown_under_reports_is_deletable() {
        let reports = Path::new("C:/data/voltnest/reports");
        assert_eq!(
            removable_report_path(reports, Some("C:/data/voltnest/reports/hid-7768-6426.md")),
            Some(PathBuf::from("C:/data/voltnest/reports/hid-7768-6426.md"))
        );
        assert_eq!(
            removable_report_path(reports, Some("C:/Windows/System32/ntoskrnl.exe")),
            None
        );
        assert_eq!(
            removable_report_path(reports, Some("C:/data/voltnest/settings.json")),
            None
        );
        // 在 reports 下但不是 Markdown
        assert_eq!(
            removable_report_path(reports, Some("C:/data/voltnest/reports/hid.json")),
            None
        );
        // 没有绑定报告文件
        assert_eq!(removable_report_path(reports, None), None);
    }

    #[test]
    fn write_frames_are_parsed_and_validated() {
        let parsed = parse_write_frames(&[
            "06 FB 00 00 00 03".to_string(),
            "   ".to_string(),
            "0x06,0xFB".to_string(),
        ])
        .expect("合法报文");
        assert_eq!(parsed.len(), 2, "空白项被跳过");
        assert_eq!(parsed[0], vec![0x06, 0xFB, 0x00, 0x00, 0x00, 0x03]);
        assert_eq!(parsed[1], vec![0x06, 0xFB]);

        let err = parse_write_frames(&["06".to_string(), "0x5".to_string()]).unwrap_err();
        assert!(err.contains("第 2 条"), "要说清是第几条出错：{err}");
    }

    #[test]
    fn utc_helpers_convert_known_instants() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_stamp(1_700_000_000_000), "2023-11-14 22:13:20 UTC");
        // 闰日：2024-02-29T00:00:00Z
        assert_eq!(utc_stamp(1_709_164_800_000), "2024-02-29 00:00:00 UTC");
        assert_eq!(utc_compact(1_700_000_000_000), "20231114-221320");
    }

    /// 用户输入里的 `|` 与换行不得破坏表格结构。
    #[test]
    fn table_cells_escape_pipes_and_newlines() {
        assert_eq!(cell("a|b\nc"), "a\\|b c");
        assert_eq!(optional_cell(None), "—");
        assert_eq!(optional_cell(Some("  ")), "—");
        assert_eq!(optional_cell(Some(" SN123 ")), "`SN123`");
    }

    /// 打包隔离：发布产物只允许带型号表，不得把测试/样例数据卷进去。
    #[test]
    fn bundle_resources_exclude_test_assets() {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let raw = std::fs::read_to_string(&config_path).expect("读取 tauri.conf.json");
        let config: serde_json::Value = serde_json::from_str(&raw).expect("解析 tauri.conf.json");
        let resources = config["bundle"]["resources"]
            .as_object()
            .expect("bundle.resources 必须是显式映射");

        assert!(!resources.is_empty(), "型号表必须随包发布，不能为空");
        for (source, target) in resources {
            assert!(
                source.ends_with("data/mappings/*.json"),
                "资源必须精确到型号表文件，禁止目录通配：{source}"
            );
            assert!(
                !source.contains("tests")
                    && !source.contains("fixtures")
                    && !source.contains("examples"),
                "测试资产不得进安装包：{source}"
            );
            assert!(
                target
                    .as_str()
                    .is_some_and(|value| value.starts_with("mappings")),
                "型号表只能落到 mappings/ 下：{target:?}"
            );
        }
    }

    /// 前端单测目录不得进入前端产物依赖图（vite 只从 index.html 起走依赖图）。
    #[test]
    fn frontend_tests_are_not_imported_by_the_app() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        let entry = std::fs::read_to_string(src.join("main.ts")).expect("读取 main.ts");
        assert!(
            !entry.contains("tests/") && !entry.contains(".spec."),
            "入口文件不得引用测试：{entry}"
        );
        let this = include_str!("probe.rs");
        let report_fn = this
            .split("fn build_report(")
            .nth(1)
            .expect("必须存在 build_report");
        let body = report_fn
            .split("fn battery_text")
            .next()
            .unwrap_or_default();
        assert!(
            !body.contains("sample_data") && !body.contains("demo") && !body.contains("fixture"),
            "报告渲染不得引用样例数据"
        );
    }
}
