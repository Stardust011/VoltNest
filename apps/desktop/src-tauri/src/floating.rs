//! 悬浮窗运行时：拉起 `--floating-host` 子进程，下发呈现单元，回收布局改动。
//!
//! Tauri 用 tao、Slint 用 winit，两套事件循环都要独占主线程，所以悬浮窗跑在同一个 exe 的
//! 子进程里（`VoltNest.exe --floating-host`）。父子之间只用一条 NDJSON 管道：下行全量单元
//! 快照，上行布局改动与「用户隐藏了哪个窗口」。
//!
//! 每个单元的身份是**锚点 stableId**（`CardDto.anchorId`：用户合并取配置主设备、自动合并
//! 取 HID 一侧、单设备取自身）。settings、托盘勾选、子进程窗口、前端菜单全都用这一个键，
//! 所以多单元显隐天然互不影响——全局没有任何「悬浮窗总开关」。

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use voltnest_core::{DeviceDto, DeviceState, Snapshot};
use voltnest_floating::proto::{ChildMsg, HostMsg, PROTOCOL_VERSION, UnitWire};
use voltnest_storage::{FloatingSettings, Settings};

use crate::commands::write_settings_try;
use crate::dto::{FloatingStateDto, FloatingUnitDto};
use crate::host::{FloatingEvent, HostEvent, HostState, lock};

/// 悬浮窗状态事件名（与前端 `FLOATING_EVENT` 一字不差）。
pub const FLOATING_EVENT: &str = "state://floating";

/// 子进程启动参数。
pub const HOST_FLAG: &str = "--floating-host";

/// 没有可见窗口超过这个时长就回收子进程。
const IDLE_RETIRE: Duration = Duration::from_secs(60);

/// 布局改动攒这么久再落盘（拖动/滑块连续变化只写一次）。
const FLUSH_AFTER: Duration = Duration::from_millis(500);

/// 连续启动失败多少次后进入降级（不再自动重试）。
const MAX_FAILURES: u32 = 5;

/// [`HOST_FLAG`] 的窗口子进程不需要控制台。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 运行期状态：子进程句柄、上次下发的单元、各种定时器。
#[derive(Default)]
pub struct Runtime {
    child: Option<ChildHandle>,
    seq: u64,
    units: Vec<UnitWire>,
    last_sent: Vec<UnitWire>,
    failures: u32,
    retry_at: Option<Instant>,
    idle_since: Option<Instant>,
    flush_at: Option<Instant>,
    degraded: bool,
    rev: u64,
}

struct ChildHandle {
    child: Child,
    stdin: Sender<String>,
    pid: u32,
}

impl Drop for ChildHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 解析当前快照里的呈现单元，并合上各自的悬浮窗偏好。纯函数，便于单测。
pub fn resolve_units(snap: &Snapshot, floating: &FloatingSettings) -> Vec<UnitWire> {
    let mut units: Vec<UnitWire> = Vec::new();
    let mut covered: BTreeSet<&str> = BTreeSet::new();

    for card in &snap.cards {
        for id in &card.member_device_ids {
            covered.insert(id.as_str());
        }
        let Some(device) = snap
            .devices
            .iter()
            .find(|d| d.stable_id == card.primary_device_id)
        else {
            continue;
        };
        units.push(build_unit(
            &card.anchor_id,
            &card.display_name,
            device,
            floating,
        ));
    }

    for device in &snap.devices {
        if covered.contains(device.stable_id.as_str()) {
            continue;
        }
        units.push(build_unit(
            &device.stable_id,
            &device.display_name,
            device,
            floating,
        ));
    }

    // 顺序跟设备列表一致：前端行、托盘图标、悬浮窗都用同一套次序。
    let order: Vec<&str> = snap.devices.iter().map(|d| d.stable_id.as_str()).collect();
    units.sort_by_key(|unit| {
        order
            .iter()
            .position(|id| *id == unit.anchor)
            .unwrap_or(usize::MAX)
    });
    units
}

fn build_unit(
    anchor: &str,
    title: &str,
    device: &DeviceDto,
    floating: &FloatingSettings,
) -> UnitWire {
    let window = floating.window(anchor);
    let battery = device.battery.or(device.last_known_battery);
    UnitWire {
        anchor: anchor.to_string(),
        title: title.to_string(),
        device_type: device.device_type.clone(),
        battery: battery
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "—".to_string()),
        level: battery
            .map(|percent| f32::from(percent) / 100.0)
            .unwrap_or(0.0),
        status: status_text(device),
        offline: !device.state.is_active(),
        visible: window.visible,
        x: window.x,
        y: window.y,
        side: window.side,
        transparency: window.transparency,
    }
}

fn status_text(device: &DeviceDto) -> String {
    let link = if device.transport == "hid" {
        "2.4G"
    } else {
        "蓝牙"
    };
    match device.state {
        DeviceState::Online => format!("已连接 · {link}"),
        DeviceState::Stale => format!("数据过期 · {link}"),
        DeviceState::Unavailable => "读取失败".to_string(),
        DeviceState::Offline => "离线".to_string(),
        DeviceState::Unknown => format!("等待读数 · {link}"),
    }
}

/// 每次发布快照后调用：必要时拉起子进程、下发单元、收尾落盘与事件。
pub fn sync(app: &AppHandle, shared: &Arc<HostState>, snap: &Snapshot) {
    let floating = lock(&shared.settings).floating.clone();
    let units = resolve_units(snap, &floating);
    let any_visible = units.iter().any(|unit| unit.visible);

    let mut rt = lock(&shared.floating);
    let units_changed = rt.units != units;
    rt.units = units;

    if any_visible {
        rt.idle_since = None;
        ensure_child(&mut rt, &shared.tx);
    } else {
        retire_if_idle(&mut rt);
    }

    if rt.units != rt.last_sent {
        // 先取出写端再改 rt，避免同时持有可变与不可变借用。
        if let Some(stdin) = rt.child.as_ref().map(|child| child.stdin.clone()) {
            rt.seq += 1;
            let msg = HostMsg::Sync {
                v: PROTOCOL_VERSION,
                seq: rt.seq,
                units: rt.units.clone(),
            };
            let _ = stdin.send(voltnest_floating::proto::encode(&msg));
            rt.last_sent = rt.units.clone();
        }
    }

    let flush_due = rt.flush_at.is_some_and(|at| Instant::now() >= at);
    if flush_due {
        rt.flush_at = None;
    }
    drop(rt);

    if flush_due {
        save_settings(shared);
    }
    if units_changed {
        emit_state(app, shared);
    }
}

/// 显式设置某个单元的显隐。
pub fn set_visible(
    app: &AppHandle,
    shared: &Arc<HostState>,
    anchor: &str,
    visible: bool,
) -> Result<(), String> {
    if !lock(&shared.floating)
        .units
        .iter()
        .any(|unit| unit.anchor == anchor)
    {
        return Err(format!("未知的悬浮窗单元：{anchor}"));
    }

    let key = anchor.to_string();
    write_settings_try(shared, |settings| {
        settings
            .floating
            .windows
            .entry(key.clone())
            .or_default()
            .visible = visible;
        Ok(())
    })?;

    // 用户显式操作时解除降级，给他一次重试的机会。
    {
        let mut rt = lock(&shared.floating);
        rt.degraded = false;
        rt.failures = 0;
        rt.retry_at = None;
    }

    let snap = crate::host::snapshot(shared);
    sync(app, shared, &snap);

    if visible && lock(&shared.floating).child.is_none() {
        let key = anchor.to_string();
        let _ = write_settings_try(shared, |settings| {
            if let Some(window) = settings.floating.windows.get_mut(&key) {
                window.visible = false;
            }
            Ok(())
        });
        return Err("悬浮窗进程没能启动，详情见诊断日志".to_string());
    }

    // 让托盘勾选、前端菜单跟着刷新。
    let _ = shared.tx.send(HostEvent::Wake);
    Ok(())
}

/// 托盘菜单用：读当前值取反。
pub fn toggle(app: &AppHandle, shared: &Arc<HostState>, anchor: &str) -> Result<(), String> {
    let current = lock(&shared.floating)
        .units
        .iter()
        .find(|unit| unit.anchor == anchor)
        .map(|unit| unit.visible)
        .unwrap_or(false);
    set_visible(app, shared, anchor, !current)
}

/// 处理子进程上报。
pub fn handle(shared: &Arc<HostState>, event: FloatingEvent) {
    match event {
        FloatingEvent::Ready => {
            let mut rt = lock(&shared.floating);
            rt.failures = 0;
            rt.degraded = false;
            rt.retry_at = None;
        }
        FloatingEvent::Layout {
            anchor,
            x,
            y,
            side,
            transparency,
        } => {
            {
                let mut rt = lock(&shared.floating);
                if let Some(unit) = rt.units.iter_mut().find(|unit| unit.anchor == anchor) {
                    unit.x = Some(x);
                    unit.y = Some(y);
                    unit.side = side;
                    unit.transparency = transparency;
                }
                rt.flush_at = Some(Instant::now() + FLUSH_AFTER);
            }
            // 只改内存，等 flush 窗口统一落盘。
            let mut settings = lock(&shared.settings);
            let window = settings.floating.windows.entry(anchor).or_default();
            window.x = Some(x);
            window.y = Some(y);
            window.side = side;
            window.transparency = transparency;
        }
        FloatingEvent::Hidden { anchor } => {
            let key = anchor.clone();
            if let Err(err) = write_settings_try(shared, |settings| {
                settings
                    .floating
                    .windows
                    .entry(key.clone())
                    .or_default()
                    .visible = false;
                Ok(())
            }) {
                log::warn!("记录悬浮窗隐藏失败（{anchor}）：{err}");
            }
            let _ = shared.tx.send(HostEvent::Wake);
        }
        FloatingEvent::Log { level, message } => {
            let level = match level.as_str() {
                "error" => log::Level::Error,
                "warn" => log::Level::Warn,
                "debug" => log::Level::Debug,
                _ => log::Level::Info,
            };
            log::log!(target: "floating", level, "{message}");
        }
        FloatingEvent::Exited => {
            let mut rt = lock(&shared.floating);
            if rt.child.is_none() {
                // 是我们主动回收的，不算失败。
                return;
            }
            rt.child = None;
            rt.last_sent.clear();
            rt.failures += 1;
            if rt.failures >= MAX_FAILURES {
                rt.degraded = true;
                log::error!("悬浮窗进程连续 {} 次退出，已停止自动重启", rt.failures);
            } else {
                let delay = retry_delay(rt.failures);
                rt.retry_at = Some(Instant::now() + delay);
                log::warn!("悬浮窗进程退出，{:?} 后重试", delay);
            }
            drop(rt);
            let _ = shared.tx.send(HostEvent::Wake);
        }
    }
}

/// 当前单元（命令 `get_floating_units` 与事件 payload 共用）。
pub fn state_dto(shared: &HostState) -> FloatingStateDto {
    let rt = lock(&shared.floating);
    FloatingStateDto {
        rev: rt.rev,
        units: rt
            .units
            .iter()
            .map(|unit| FloatingUnitDto {
                anchor: unit.anchor.clone(),
                title: unit.title.clone(),
                visible: unit.visible,
                offline: unit.offline,
                side: unit.side,
                transparency: unit.transparency,
            })
            .collect(),
    }
}

/// 进程退出前收尾：让子进程自己退，等不到就杀。
pub fn shutdown(shared: &HostState) {
    let mut rt = lock(&shared.floating);
    if let Some(stdin) = rt.child.as_ref().map(|child| child.stdin.clone()) {
        rt.seq += 1;
        let msg = HostMsg::Shutdown {
            v: PROTOCOL_VERSION,
        };
        let _ = stdin.send(voltnest_floating::proto::encode(&msg));
    }
    // Drop 会 kill + wait，保证不留孤儿进程。
    rt.child = None;
}

fn emit_state(app: &AppHandle, shared: &Arc<HostState>) {
    let dto = state_dto(shared);
    if let Err(err) = app.emit(FLOATING_EVENT, &dto) {
        log::warn!("悬浮窗事件发送失败：{err}");
    }
}

fn save_settings(shared: &HostState) {
    let next: Settings = lock(&shared.settings).clone();
    if let Err(err) = shared.settings_store.save(&next) {
        log::warn!("保存悬浮窗布局失败：{err}");
    }
}

fn ensure_child(rt: &mut Runtime, tx: &Sender<HostEvent>) {
    if rt.child.is_some() || rt.degraded {
        return;
    }
    if let Some(at) = rt.retry_at
        && Instant::now() < at
    {
        return;
    }

    match spawn_child(tx.clone()) {
        Ok(handle) => {
            log::info!("悬浮窗进程已启动（pid={}）", handle.pid);
            rt.child = Some(handle);
            rt.last_sent.clear();
            rt.retry_at = None;
        }
        Err(err) => {
            log::warn!("拉起悬浮窗进程失败：{err}");
            rt.failures += 1;
            rt.retry_at = Some(Instant::now() + retry_delay(rt.failures));
            if rt.failures >= MAX_FAILURES {
                rt.degraded = true;
            }
        }
    }
}

fn retire_if_idle(rt: &mut Runtime) {
    let now = Instant::now();
    let since = *rt.idle_since.get_or_insert(now);
    if rt.child.is_some() && now.duration_since(since) >= IDLE_RETIRE {
        log::info!("没有可见的悬浮窗，回收子进程");
        rt.child = None;
        rt.last_sent.clear();
    }
}

fn retry_delay(failures: u32) -> Duration {
    let step = failures.saturating_sub(1).min(5);
    Duration::from_secs(1u64 << step)
}

fn spawn_child(tx: Sender<HostEvent>) -> std::io::Result<ChildHandle> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg(HOST_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn()?;
    let pid = child.id();
    let Some(stdin) = child.stdin.take() else {
        return Err(std::io::Error::other("子进程没有 stdin"));
    };
    let Some(stdout) = child.stdout.take() else {
        return Err(std::io::Error::other("子进程没有 stdout"));
    };

    // 写线程：宿主循环只投递，管道堵住也不会拖住快照发布。
    let (stdin_tx, stdin_rx) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("voltnest-floating-w".into())
        .spawn(move || {
            let mut out = stdin;
            for line in stdin_rx {
                let ok = out
                    .write_all(line.as_bytes())
                    .and_then(|()| out.write_all(b"\n"))
                    .and_then(|()| out.flush());
                if ok.is_err() {
                    break;
                }
            }
        })?;

    // 读线程：逐行解析成宿主事件；stdout 关闭即视为子进程退出。
    std::thread::Builder::new()
        .name("voltnest-floating-r".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                match voltnest_floating::proto::decode_child(&line) {
                    Ok(msg) => {
                        if let Some(event) = to_event(msg)
                            && tx.send(HostEvent::Floating(event)).is_err()
                        {
                            return;
                        }
                    }
                    Err(err) => log::warn!("悬浮窗上报了非法消息：{err}"),
                }
            }
            let _ = tx.send(HostEvent::Floating(FloatingEvent::Exited));
        })?;

    Ok(ChildHandle {
        child,
        stdin: stdin_tx,
        pid,
    })
}

fn to_event(msg: ChildMsg) -> Option<FloatingEvent> {
    match msg {
        ChildMsg::Ready { .. } => Some(FloatingEvent::Ready),
        ChildMsg::Layout {
            anchor,
            x,
            y,
            side,
            transparency,
            ..
        } => Some(FloatingEvent::Layout {
            anchor,
            x,
            y,
            side: side.round().clamp(0.0, u16::MAX as f32) as u16,
            transparency,
        }),
        ChildMsg::Hidden { anchor, .. } => Some(FloatingEvent::Hidden { anchor }),
        ChildMsg::Log { level, message, .. } => Some(FloatingEvent::Log { level, message }),
        ChildMsg::Bye { reason, .. } => {
            log::warn!("悬浮窗进程退出：{reason}");
            None
        }
    }
}

/// 当前悬浮窗单元（命令 `get_floating_units`）。
#[tauri::command]
pub fn get_floating_units(shared: tauri::State<'_, Arc<HostState>>) -> FloatingStateDto {
    state_dto(&shared)
}

/// 设置某个单元的悬浮窗显隐（命令 `set_floating_visible`）。
#[tauri::command]
pub fn set_floating_visible(
    app: AppHandle,
    anchor: String,
    visible: bool,
    shared: tauri::State<'_, Arc<HostState>>,
) -> Result<(), String> {
    set_visible(&app, &shared, &anchor, visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnest_core::{CardDto, DeviceDto, TraySnapshotDto};

    fn device(id: &str, kind: &str, battery: Option<u8>, state: DeviceState) -> DeviceDto {
        DeviceDto {
            stable_id: id.into(),
            transport: if id.starts_with("hid") { "hid" } else { "ble" }.into(),
            provider: "test".into(),
            display_name: format!("设备 {id}"),
            model_key: None,
            device_type: kind.into(),
            classification_method: "hydrated".into(),
            battery,
            last_known_battery: None,
            last_known_at_ms: None,
            is_charging: None,
            state,
            last_valid_at_ms: None,
            last_error: None,
        }
    }

    fn card(anchor: &str, primary: &str, members: &[&str]) -> CardDto {
        CardDto {
            card_id: format!("card:{anchor}"),
            anchor_id: anchor.into(),
            display_name: "合并组".into(),
            user_merged: true,
            primary_device_id: primary.into(),
            member_device_ids: members.iter().map(|id| (*id).to_string()).collect(),
            battery: Some(80),
            is_charging: None,
            state: DeviceState::Online,
            inactive_member_text: String::new(),
        }
    }

    fn snapshot(devices: Vec<DeviceDto>, cards: Vec<CardDto>) -> Snapshot {
        Snapshot {
            rev: 1,
            server_now_ms: 0,
            devices,
            cards,
            tray: TraySnapshotDto {
                visible_device_ids: Vec::new(),
                present: 0,
                live: 0,
                overflow: 0,
            },
        }
    }

    #[test]
    fn single_devices_become_their_own_units() {
        let snap = snapshot(
            vec![
                device("hid:7768:6426", "mouse", Some(87), DeviceState::Online),
                device(
                    "ble:01:00:00:23:04:35",
                    "keyboard",
                    None,
                    DeviceState::Offline,
                ),
            ],
            Vec::new(),
        );
        let units = resolve_units(&snap, &FloatingSettings::default());
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].anchor, "hid:7768:6426");
        assert_eq!(units[0].battery, "87%");
        assert!((units[0].level - 0.87).abs() < 1e-6);
        assert!(!units[0].offline);
        assert_eq!(units[1].battery, "—", "无读数显示占位符");
        assert!(units[1].offline);
    }

    #[test]
    fn merged_card_collapses_members_into_one_unit_keyed_by_anchor() {
        let snap = snapshot(
            vec![
                device("hid:7768:6426", "mouse", Some(90), DeviceState::Stale),
                device("ble:01:00:00:23:04:35", "mouse", None, DeviceState::Unknown),
            ],
            vec![card(
                "hid:7768:6426",
                "ble:01:00:00:23:04:35",
                &["ble:01:00:00:23:04:35", "hid:7768:6426"],
            )],
        );
        let units = resolve_units(&snap, &FloatingSettings::default());
        assert_eq!(units.len(), 1, "合并后只剩一个单元");
        assert_eq!(units[0].anchor, "hid:7768:6426", "键用锚点而不是活跃成员");
        assert_eq!(units[0].title, "合并组");
    }

    #[test]
    fn visibility_comes_from_settings_per_anchor() {
        let snap = snapshot(
            vec![
                device("hid:7768:6426", "mouse", Some(50), DeviceState::Online),
                device(
                    "ble:aa:bb:cc:dd:ee:ff",
                    "keyboard",
                    Some(60),
                    DeviceState::Online,
                ),
            ],
            Vec::new(),
        );
        let mut floating = FloatingSettings::default();
        floating.windows.insert(
            "ble:aa:bb:cc:dd:ee:ff".into(),
            voltnest_storage::FloatingWindow {
                visible: true,
                ..Default::default()
            },
        );

        let units = resolve_units(&snap, &floating);
        assert!(!units[0].visible);
        assert!(units[1].visible, "只有被打开的那个单元可见");
    }

    #[test]
    fn retry_delay_backs_off_to_thirty_seconds() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(5), Duration::from_secs(16));
        assert_eq!(retry_delay(9), Duration::from_secs(32));
    }
}
