//! 托盘图标：每个可见设备一个环形电量图（32×32 RGBA 手工光栅化，配色与前端 `batteryColor()` 一致）。
//!
//! 没有设备图标时露基础图标（应用图标），有设备图标就让位；设备全部离线时也回到应用图标。每张图标挂同一份菜单（打开面板 / 退出）。
//!
//! Linux（AppIndicator）限制：`set_tooltip` 为空实现，「气泡模式」返回 [`BubbleOutcome::UnsupportedPlatform`]；`TrayIconEvent` 不派发，左键开面板仅 Windows 生效。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, TrayIconId,
};
use tauri::{AppHandle, Manager};

use voltnest_core::{DeviceDto, DeviceState, Snapshot};

use crate::host::{HostState, lock};

/// 基础托盘图标的 id（没有设备图标时占位，点它打开面板）。
pub const BASE_TRAY_ID: &str = "voltnest";
/// 图标边长。
pub const ICON_SIZE: u32 = 32;
/// tooltip 上限。
const TOOLTIP_LIMIT: usize = 128;
/// 托盘气泡（tooltip 提示）的持续时长。
const BUBBLE_MS: u64 = 6_000;

const MENU_OPEN: &str = "tray:open";
const MENU_QUIT: &str = "tray:quit";

/// 与前端 `lib/format.ts` 色值一一对应；充电统一用 `EMERALD`，靠闪电形状表达「在充电」。
const EMERALD: [u8; 3] = [16, 185, 129];
const ORANGE: [u8; 3] = [249, 115, 22];
const RED: [u8; 3] = [239, 68, 68];
const AMBER: [u8; 3] = [245, 158, 11];
const SKY: [u8; 3] = [56, 189, 248];
const GRAY: [u8; 3] = [148, 163, 184];

/// 充电闪电：lucide `zap` 折点近似（24×24，形心 12,12）。
const BOLT: [(f32, f32); 12] = [
    (15.914, 4.0),
    (13.44, 2.439),
    (4.44, 11.439),
    (5.5, 14.0),
    (9.502, 14.0),
    (9.973, 14.666),
    (8.086, 20.0),
    (10.561, 21.56),
    (19.561, 12.56),
    (18.5, 10.0),
    (14.503, 10.0),
    (14.031, 9.333),
];

/// 等待读数时整环的透明度：看得见，但明显不是一次真实读数。
const PENDING_ALPHA: f32 = 0.7;

/// 应用图标：`bundle.icon` 里 32×32 那张，无设备时占位。
const APP_ICON_PNG: &[u8] = include_bytes!("../icons/32x32.png");

/// 每张图标应有的 tooltip（气泡提示 6 秒后据此还原）。
static BASELINE_TOOLTIPS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// 基础图标当前是否可见（隐身的图标不能当气泡的落点）。
static BASE_VISIBLE: AtomicBool = AtomicBool::new(true);

/// 解出打包的应用图标；解码失败退回蓝色等待环。
fn app_icon() -> Image<'static> {
    Image::from_bytes(APP_ICON_PNG).unwrap_or_else(|err| {
        log::warn!("解码应用图标失败，退回等待环: {err}");
        ring_image(None, false, DeviceState::Unknown)
    })
}

/// 安装托盘：菜单事件处理器 + 基础图标。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    app.on_menu_event(|app, event| match event.id.as_ref() {
        MENU_OPEN => show_main(app),
        MENU_QUIT => quit(app),
        _ => {}
    });

    build_tray(app, BASE_TRAY_ID, app_icon(), "VoltNest")
}

/// 显示并聚焦主窗口（二次启动、托盘点击、菜单项都走这里）。
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 退出进程：任意一张图标的右键菜单都能到这儿。
fn quit(app: &AppHandle) {
    app.exit(0);
}

/// 按快照同步设备托盘图标：增、改、删。
pub fn sync_tray_icons(app: &AppHandle, shared: &Arc<HostState>, snap: &Snapshot) {
    let mut known = lock(&shared.tray_ids);
    let visible: &[String] = if all_devices_offline(snap) {
        &[]
    } else {
        &snap.tray.visible_device_ids
    };
    let truncated = snap.devices.len().saturating_sub(visible.len());
    let mut wanted: Vec<String> = Vec::with_capacity(visible.len());

    for (idx, stable_id) in visible.iter().enumerate() {
        let Some(device) = snap.devices.iter().find(|d| &d.stable_id == stable_id) else {
            continue;
        };
        let tray_id = tray_id_for(stable_id);
        let icon = ring_image(
            displayed_battery(device).map(|r| r.percent),
            device.is_charging == Some(true),
            device.state,
        );
        let extra = if idx + 1 == visible.len() {
            truncated
        } else {
            0
        };
        let tooltip = tooltip_for(device, extra);

        match app.tray_by_id(&TrayIconId::new(&tray_id)) {
            Some(tray) => {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_tooltip(Some(&tooltip));
                remember_tooltip(&tray_id, &tooltip);
            }
            None => {
                if let Err(err) = build_tray(app, &tray_id, icon, &tooltip) {
                    log::warn!("创建托盘图标失败（{stable_id}）: {err}");
                    continue;
                }
            }
        }
        wanted.push(stable_id.clone());
    }

    for stale in known.iter().filter(|id| !wanted.contains(id)) {
        remove_tray(app, &tray_id_for(stale));
    }
    let has_device_icons = !wanted.is_empty();
    *known = wanted;
    drop(known);

    update_base(app, snap, has_device_icons);
}

/// 全部设备都在 `Offline`（空列表与 `Unknown`/`Unavailable` 不算）。
fn all_devices_offline(snap: &Snapshot) -> bool {
    !snap.devices.is_empty() && snap.devices.iter().all(|d| d.state == DeviceState::Offline)
}

/// 基础图标显隐：有设备图标就隐身让位，没有就出来占位（只切可见性，不重建）。
fn update_base(app: &AppHandle, snap: &Snapshot, has_device_icons: bool) {
    let tooltip = summary_tooltip(snap);
    let visible = !has_device_icons;
    let Some(base) = base_tray(app, &tooltip) else {
        return;
    };

    if current_tooltip(BASE_TRAY_ID).as_deref() != Some(tooltip.as_str()) {
        let _ = base.set_tooltip(Some(&tooltip));
        remember_tooltip(BASE_TRAY_ID, &tooltip);
    }
    if BASE_VISIBLE.swap(visible, Ordering::Relaxed) != visible {
        let _ = base.set_visible(visible);
    }
}

/// 拿基础图标；万一它不在（install 失败过）就补建一张。
fn base_tray(app: &AppHandle, tooltip: &str) -> Option<TrayIcon> {
    if let Some(base) = app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID)) {
        return Some(base);
    }
    if let Err(err) = build_tray(app, BASE_TRAY_ID, app_icon(), tooltip) {
        log::warn!("创建基础托盘图标失败: {err}");
        return None;
    }
    app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID))
}

/// 建一张托盘图标：统一菜单 + 左键开面板。菜单与图标为主线程资源，故整段搬到主线程执行。
fn build_tray(
    app: &AppHandle,
    tray_id: &str,
    icon: Image<'static>,
    tooltip: &str,
) -> tauri::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    let id = tray_id.to_string();
    let tip = tooltip.to_string();

    app.run_on_main_thread(move || {
        let built = tray_menu(&handle).and_then(|menu| {
            TrayIconBuilder::with_id(&id)
                .icon(icon)
                .tooltip(&tip)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => show_main(tray.app_handle()),
                    _ => {}
                })
                .build(&handle)
                .map(|_| ())
        });
        let _ = tx.send(built);
    })?;
    rx.recv()
        .map_err(|_| tauri::Error::FailedToReceiveMessage)??;

    remember_tooltip(tray_id, tooltip);
    Ok(())
}

/// 每张图标都带一份同样的菜单：任意一张右键都能打开面板或退出。
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, MENU_OPEN, "打开面板", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出 VoltNest", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&open, &separator, &quit])
}

/// 移除图标，顺手丢掉它的 tooltip 记录。
fn remove_tray(app: &AppHandle, tray_id: &str) {
    app.remove_tray_by_id(&TrayIconId::new(tray_id));
    lock(&BASELINE_TOOLTIPS).remove(tray_id);
}

/// 记下这张图标应有的 tooltip，供气泡提示结束后恢复。
fn remember_tooltip(tray_id: &str, tooltip: &str) {
    lock(&BASELINE_TOOLTIPS).insert(tray_id.to_string(), tooltip.to_string());
}

/// 这张图标应有的 tooltip（当前值）。
fn current_tooltip(tray_id: &str) -> Option<String> {
    lock(&BASELINE_TOOLTIPS).get(tray_id).cloned()
}

/// 托盘气泡投递的三态结果（诊断页据此区分「平台不支持」与「没有落脚图标」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BubbleOutcome {
    /// 提示已写进对应图标的 tooltip（6 秒后还原）。
    Sent,
    /// 没有可用落脚图标：设备图标不在，基础图标也隐身。
    NoIcon,
    /// 平台的托盘后端没有 tooltip 实现（Linux / AppIndicator）。
    UnsupportedPlatform,
}

/// 托盘气泡模式：把提示写进对应图标 tooltip，6 秒后恢复。
///
/// 判定顺序：先找落脚图标（无 → [`BubbleOutcome::NoIcon`]），再看平台（Linux → [`BubbleOutcome::UnsupportedPlatform`]），最后写 tooltip → [`BubbleOutcome::Sent`]。
pub fn bubble(app: &AppHandle, stable_id: &str, text: &str) -> BubbleOutcome {
    let Some(tray) = notification_tray(app, stable_id) else {
        return BubbleOutcome::NoIcon;
    };
    if cfg!(target_os = "linux") {
        log::warn!("托盘气泡模式在 Linux 托盘后端不可用（tooltip 无实现）：请改用系统通知");
        return BubbleOutcome::UnsupportedPlatform;
    }
    let tray_id = tray.id().as_ref().to_string();
    let _ = tray.set_tooltip(Some(text));

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(BUBBLE_MS));
        let restore = lock(&BASELINE_TOOLTIPS).get(&tray_id).cloned();
        if let (Some(tray), Some(text)) = (app.tray_by_id(&TrayIconId::new(&tray_id)), restore) {
            let _ = tray.set_tooltip(Some(&text));
        }
    });
    BubbleOutcome::Sent
}

/// 低电量提示挂哪张图标：优先设备自己的，其次**露在托盘里**的基础图标。
fn notification_tray(app: &AppHandle, stable_id: &str) -> Option<TrayIcon> {
    if let Some(tray) = app.tray_by_id(&TrayIconId::new(tray_id_for(stable_id))) {
        return Some(tray);
    }
    if BASE_VISIBLE.load(Ordering::Relaxed) {
        return app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID));
    }
    None
}

/// 基础托盘图标的 tooltip（只在没有设备图标时可见）。
fn summary_tooltip(snap: &Snapshot) -> String {
    let total = snap.devices.len();
    if total == 0 {
        return "VoltNest".to_string();
    }
    if all_devices_offline(snap) {
        return format!("VoltNest · {total} 台设备（全部离线）");
    }
    if snap.tray.visible_device_ids.is_empty() {
        return format!("VoltNest · {total} 台设备（未固定在托盘）");
    }
    format!("VoltNest · {total} 台设备")
}

/// 设备托盘图标的 id：`tray:<stableId>`。
fn tray_id_for(stable_id: &str) -> String {
    format!("tray:{stable_id}")
}

/// tooltip 文案：`型号 · 92%` / `型号 · —（离线）` / `型号 · 87%（上次读数）`。`extra > 0` 表示还有设备没进托盘，挂在最后一张图标上；整串 ≤128 字符。
fn tooltip_for(device: &DeviceDto, extra: usize) -> String {
    let reading = displayed_battery(device);
    let battery = match reading {
        Some(r) => format!("{}%", r.percent),
        None => "—".to_string(),
    };
    let mut body = format!("{} · {battery}", device.display_name);
    if reading.is_some_and(|r| r.from_cache) {
        body.push_str("（上次读数）");
    }
    if device.state == DeviceState::Offline {
        body.push_str("（离线）");
    }
    let suffix = if extra > 0 {
        format!(" · 还有 {extra} 台")
    } else {
        String::new()
    };
    let budget = TOOLTIP_LIMIT.saturating_sub(suffix.chars().count());
    format!("{}{suffix}", clamp_chars(body, budget))
}

/// 按字符数截断，超长时以「…」收尾。
fn clamp_chars(text: String, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if text.chars().count() <= limit {
        return text;
    }
    let mut out: String = text.chars().take(limit - 1).collect();
    out.push('…');
    out
}

/// 托盘要展示的一次电量读数（实时值或 `state.json` 缓存值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayedBattery {
    /// 百分比。
    percent: u8,
    /// 是否来自 `state.json` 缓存（尚未拿到实时读数）。
    from_cache: bool,
}

/// 与前端 `displayedBattery()` 同规则：首读未到时用 `lastKnownBattery` 撑住图标与 tooltip；`Unavailable`/`Offline` 不算。
fn displayed_battery(device: &DeviceDto) -> Option<DisplayedBattery> {
    if let Some(percent) = device.battery {
        return Some(DisplayedBattery {
            percent,
            from_cache: false,
        });
    }
    match device.state {
        DeviceState::Unknown => device.last_known_battery.map(|percent| DisplayedBattery {
            percent,
            from_cache: true,
        }),
        _ => None,
    }
}

/// 环的颜色：与前端 `batteryColor()` 逐条对应。只有 `Online`/`Stale` 按电量阈值着色，其余用状态色；充电一律绿。
fn ring_tone(battery: Option<u8>, charging: bool, state: DeviceState) -> [u8; 3] {
    let tone = match state {
        DeviceState::Online => EMERALD,
        DeviceState::Stale => AMBER,
        DeviceState::Unavailable => ORANGE,
        DeviceState::Offline => GRAY,
        DeviceState::Unknown => SKY,
    };
    if !matches!(state, DeviceState::Online | DeviceState::Stale) {
        return tone;
    }
    if charging {
        return EMERALD;
    }
    match battery {
        Some(v) if v < 10 => RED,
        Some(v) if v < 20 => ORANGE,
        _ => tone,
    }
}

/// 是否画「绿环 + 闪电」：充电只在有当前读数可言的两种状态下才算数。
fn is_charging_glyph(charging: bool, state: DeviceState) -> bool {
    charging && matches!(state, DeviceState::Online | DeviceState::Stale)
}

/// 生成一张 32×32 环形电量图：`None` 为等待环（整圈状态色、压低透明度）；`offline` 灰环+横线；`unknown` 蓝环；`charging` 绿弧+环心闪电（弧长仍随电量）；其余按 20%/10% 分档着色。
pub fn ring_image(battery: Option<u8>, charging: bool, state: DeviceState) -> Image<'static> {
    let size = ICON_SIZE as f32;
    let center = size / 2.0;
    let outer = size * 0.44;

    let plug = is_charging_glyph(charging, state);
    let thickness = if plug { size * 0.12 } else { size * 0.155 };
    let inner = outer - thickness;

    let rgb = ring_tone(battery, charging, state);
    let draw_line = state == DeviceState::Offline;
    let pending = battery.is_none() && !draw_line;

    let ratio = if state == DeviceState::Offline || pending {
        1.0
    } else {
        battery
            .map(|v| f32::from(v.min(100)) / 100.0)
            .unwrap_or(0.0)
    };
    let arc_alpha = if pending { PENDING_ALPHA } else { 1.0 };
    let bolt_scale = size * 0.028;

    const SS: usize = 3;
    const TRACK: [u8; 4] = [120, 130, 145, 110];
    let mut buf = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for py in 0..ICON_SIZE {
        for px in 0..ICON_SIZE {
            let mut arc_hits = 0u32;
            let mut track_hits = 0u32;
            let mut line_hits = 0u32;
            let mut bolt_hits = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    let dx = x - center;
                    let dy = y - center;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let in_ring = dist <= outer && dist >= inner;

                    if in_ring {
                        // 12 点方向顺时针的角度比例
                        let mut deg = dx.atan2(-dy).to_degrees();
                        if deg < 0.0 {
                            deg += 360.0;
                        }
                        if deg / 360.0 <= ratio {
                            arc_hits += 1;
                        } else {
                            track_hits += 1;
                        }
                    }
                    if plug && in_polygon(&BOLT, dx / bolt_scale + 12.0, dy / bolt_scale + 12.0) {
                        bolt_hits += 1;
                    }
                    if draw_line && (y - center).abs() <= 1.1 && (x - center).abs() <= outer * 0.72
                    {
                        line_hits += 1;
                    }
                }
            }

            let total = (SS * SS) as f32;
            let idx = ((py * ICON_SIZE + px) * 4) as usize;
            let track_a = track_hits as f32 / total;
            let arc_a = arc_hits as f32 / total;
            let line_a = line_hits as f32 / total;
            let bolt_a = bolt_hits as f32 / total;
            let (r, g, b, a) = blend(rgb, arc_a * arc_alpha + bolt_a, TRACK, track_a, line_a);
            if a > 0.0 {
                buf[idx] = r;
                buf[idx + 1] = g;
                buf[idx + 2] = b;
                buf[idx + 3] = (a * 255.0).round() as u8;
            }
        }
    }

    Image::new_owned(buf, ICON_SIZE, ICON_SIZE)
}

/// 射线法判断点是否落在多边形内（闪电是凹多边形，奇偶规则正好够用）。
fn in_polygon(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 合成「轨道 + 电量环 + 离线横线」三层。
fn blend(arc: [u8; 3], arc_a: f32, track: [u8; 4], track_a: f32, line_a: f32) -> (u8, u8, u8, f32) {
    let mut out = [0.0f32; 3];
    let mut alpha = 0.0f32;

    let track_alpha = track_a * (track[3] as f32 / 255.0);
    if track_alpha > 0.0 {
        for (out, t) in out.iter_mut().zip(track.iter()) {
            *out += f32::from(*t) * track_alpha;
        }
        alpha += track_alpha;
    }
    if arc_a > 0.0 {
        for (out, a) in out.iter_mut().zip(arc.iter()) {
            *out += f32::from(*a) * arc_a;
        }
        alpha += arc_a;
    }
    if line_a > 0.0 {
        for out in out.iter_mut() {
            *out = *out * (1.0 - line_a) + 30.0 * line_a;
        }
        alpha = alpha.max(line_a);
    }

    if alpha <= 0.0 {
        return (0, 0, 0, 0.0);
    }
    let a = alpha.min(1.0);
    (
        (out[0] / alpha).clamp(0.0, 255.0) as u8,
        (out[1] / alpha).clamp(0.0, 255.0) as u8,
        (out[2] / alpha).clamp(0.0, 255.0) as u8,
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(battery: Option<u8>, state: DeviceState) -> DeviceDto {
        DeviceDto {
            stable_id: "hid:7768:6426".into(),
            transport: "hid".into(),
            provider: "hid-vbas".into(),
            display_name: "AJ159 NL".into(),
            model_key: Some("aj159 nl".into()),
            device_type: "mouse".into(),
            classification_method: "verified_device_mapping".into(),
            battery,
            last_known_battery: battery,
            last_known_at_ms: None,
            is_charging: Some(false),
            state,
            last_valid_at_ms: None,
            last_error: None,
        }
    }

    fn snapshot(devices: usize, visible: usize) -> Snapshot {
        let mut snap = Snapshot {
            rev: 1,
            server_now_ms: 0,
            devices: Vec::new(),
            cards: Vec::new(),
            tray: voltnest_core::TraySnapshotDto {
                visible_device_ids: Vec::new(),
            },
        };
        snap.devices = (0..devices)
            .map(|_| device(Some(80), DeviceState::Online))
            .collect();
        snap.tray.visible_device_ids = snap
            .devices
            .iter()
            .take(visible)
            .map(|d| d.stable_id.clone())
            .collect();
        snap
    }

    #[test]
    fn tooltip_is_bounded() {
        let mut d = device(Some(92), DeviceState::Online);
        assert_eq!(tooltip_for(&d, 0), "AJ159 NL · 92%");
        d.display_name = "型号".repeat(100);
        let text = tooltip_for(&d, 0);
        assert!(text.chars().count() <= TOOLTIP_LIMIT);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn tooltip_keeps_truncation_note_even_for_long_names() {
        let mut d = device(Some(92), DeviceState::Online);
        d.display_name = "型号".repeat(100);
        let text = tooltip_for(&d, 3);
        assert!(text.chars().count() <= TOOLTIP_LIMIT, "含后缀也不能超长");
        assert!(
            text.ends_with("还有 3 台"),
            "基础灰环退位后，「还有 N 台」只能挂在设备图标上：{text}"
        );
    }

    #[test]
    fn summary_tooltip_tracks_base_icon_states() {
        assert_eq!(
            summary_tooltip(&snapshot(0, 0)),
            "VoltNest",
            "没有设备时保持朴素文案"
        );
        assert_eq!(
            summary_tooltip(&snapshot(3, 0)),
            "VoltNest · 3 台设备（未固定在托盘）",
            "设备被全部隐藏时，基础图标要说明设备还在"
        );
        assert_eq!(summary_tooltip(&snapshot(6, 6)), "VoltNest · 6 台设备");
    }

    #[test]
    fn all_offline_yields_only_the_app_icon() {
        let mut snap = snapshot(2, 2);
        assert!(!all_devices_offline(&snap));
        for d in &mut snap.devices {
            d.state = DeviceState::Offline;
            d.battery = None;
        }
        assert!(all_devices_offline(&snap), "全离线时设备图标要整体退位");
        assert_eq!(
            summary_tooltip(&snap),
            "VoltNest · 2 台设备（全部离线）",
            "应用图标顶替之后，tooltip 得说明设备还在、只是断了"
        );
    }

    #[test]
    fn all_offline_needs_every_device_and_at_least_one() {
        assert!(!all_devices_offline(&snapshot(0, 0)), "没有设备不算全离线");
        let mut mixed = snapshot(2, 2);
        mixed.devices[1].state = DeviceState::Offline;
        assert!(!all_devices_offline(&mixed), "还有在线设备就保留电量环");
        let mut waiting = snapshot(1, 1);
        waiting.devices[0].state = DeviceState::Unknown;
        assert!(!all_devices_offline(&waiting), "等待读数仍要画蓝环");
    }

    #[test]
    fn tooltip_marks_offline_and_null_battery() {
        let d = device(None, DeviceState::Offline);
        assert_eq!(tooltip_for(&d, 0), "AJ159 NL · —（离线）");
    }

    #[test]
    fn base_icon_is_the_app_icon() {
        let img = app_icon();
        assert_eq!((img.width(), img.height()), (ICON_SIZE, ICON_SIZE));
        let rgba = img.rgba();
        let opaque = rgba.chunks(4).filter(|p| p[3] > 200).count();
        assert!(
            opaque > 900,
            "应用图标是实心圆角方块，覆盖面积远大于电量环：{opaque}"
        );
        assert_eq!(
            rgba[(16 * 4) + 3],
            255,
            "顶端实心说明用的是应用图标，而不是空心环"
        );
    }

    #[test]
    fn waiting_icon_is_the_panel_blue() {
        assert_eq!(ring_tone(None, false, DeviceState::Unknown), SKY);
        assert_eq!(
            ring_tone(Some(87), false, DeviceState::Unknown),
            SKY,
            "回填的缓存读数仍属「等待读数」，不能画成在线的绿"
        );

        let img = ring_image(None, false, DeviceState::Unknown);
        let brightest = img
            .rgba()
            .chunks(4)
            .filter(|p| p[3] > 0)
            .max_by_key(|p| p[3])
            .expect("等待环不能是一张空图");
        let close = |got: u8, want: u8| (i32::from(got) - i32::from(want)).abs() <= 2;
        assert!(
            close(brightest[0], SKY[0])
                && close(brightest[1], SKY[1])
                && close(brightest[2], SKY[2]),
            "等待环应为 #38bdf8，实际 {:?}",
            &brightest[..3]
        );
        assert!(
            brightest[3] > 120 && brightest[3] < 255,
            "等待环比真实读数暗一档，别让人误读成满电：{}",
            brightest[3]
        );
    }

    #[test]
    fn waiting_with_cached_reading_draws_a_full_strength_arc() {
        let img = ring_image(Some(90), false, DeviceState::Unknown);
        let max_alpha = img.rgba().chunks(4).map(|p| p[3]).max().unwrap_or(0);
        assert_eq!(max_alpha, 255, "有缓存读数时环要画实，只有真没数据才压暗");
    }

    #[test]
    fn tones_follow_panel_battery_color() {
        assert_eq!(ring_tone(Some(96), false, DeviceState::Online), EMERALD);
        assert_eq!(ring_tone(Some(15), false, DeviceState::Online), ORANGE);
        assert_eq!(ring_tone(Some(5), false, DeviceState::Online), RED);
        assert_eq!(
            ring_tone(Some(5), true, DeviceState::Online),
            EMERALD,
            "充电不看电量分档，统一绿环 + 闪电（面板那侧同色）"
        );
        assert_eq!(ring_tone(Some(96), false, DeviceState::Stale), AMBER);
        assert_eq!(
            ring_tone(Some(96), false, DeviceState::Unavailable),
            ORANGE,
            "读取失败沿用状态色，不按电量分档"
        );
        assert_eq!(ring_tone(None, false, DeviceState::Offline), GRAY);
    }

    #[test]
    fn charging_puts_a_bolt_in_the_hole() {
        let hole_pixels = |charging: bool| {
            let img = ring_image(Some(50), charging, DeviceState::Online);
            let c = ICON_SIZE as f32 / 2.0;
            img.rgba()
                .chunks(4)
                .enumerate()
                .filter(|(i, p)| {
                    let x = (*i % ICON_SIZE as usize) as f32 + 0.5;
                    let y = (*i / ICON_SIZE as usize) as f32 + 0.5;
                    let dist = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
                    dist < c * 0.5 && p[3] > 200
                })
                .count()
        };
        assert_eq!(hole_pixels(false), 0, "不充电时环心是空的");
        assert!(hole_pixels(true) > 20, "充电时环心要画出闪电");
    }

    #[test]
    fn charging_keeps_the_progress_arc() {
        let arc = |battery: u8| {
            let img = ring_image(Some(battery), true, DeviceState::Online);
            let c = ICON_SIZE as f32 / 2.0;
            img.rgba()
                .chunks(4)
                .enumerate()
                .filter(|(i, p)| {
                    let x = (*i % ICON_SIZE as usize) as f32 + 0.5;
                    let y = (*i / ICON_SIZE as usize) as f32 + 0.5;
                    let dist = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
                    dist > c * 0.6 && p[3] > 200
                })
                .count()
        };
        assert!(
            arc(90) > arc(20),
            "充电也要看得出充到哪了：20% 和 90% 不能画成同一个环"
        );
        assert!(arc(20) > 0, "低电量时弧仍然要在，不能只剩轨道");
    }

    #[test]
    fn waiting_uses_cached_battery_but_not_after_failure() {
        let mut d = device(None, DeviceState::Unknown);
        d.last_known_battery = Some(87);
        assert_eq!(
            displayed_battery(&d),
            Some(DisplayedBattery {
                percent: 87,
                from_cache: true
            }),
            "启动回填要撑住首屏"
        );
        d.state = DeviceState::Unavailable;
        assert_eq!(
            displayed_battery(&d),
            None,
            "读取失败是「确定读不到」，不能假装还有读数"
        );
    }

    #[test]
    fn tooltip_reports_the_battery_the_ring_is_drawing() {
        let mut waiting = device(None, DeviceState::Unknown);
        waiting.last_known_battery = Some(87);
        assert_eq!(tooltip_for(&waiting, 0), "AJ159 NL · 87%（上次读数）");

        let online = device(Some(87), DeviceState::Online);
        assert_eq!(tooltip_for(&online, 0), "AJ159 NL · 87%");
    }

    #[test]
    fn ring_is_rgba_and_has_glyph() {
        let img = ring_image(Some(80), false, DeviceState::Online);
        assert_eq!(img.width(), ICON_SIZE);
        assert_eq!(img.height(), ICON_SIZE);
        let rgba = img.rgba();
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(
            rgba.chunks(4).any(|p| p[3] > 200),
            "必须画出不透明的环，而不是一张空图"
        );
        assert_eq!(rgba[3], 0, "左上角必须透明");
    }

    #[test]
    fn arc_length_follows_battery() {
        let count = |battery: u8| {
            ring_image(Some(battery), false, DeviceState::Online)
                .rgba()
                .chunks(4)
                .filter(|p| p[3] > 160)
                .count()
        };
        assert!(count(90) > count(20), "电量越高，环越长");
    }
}
