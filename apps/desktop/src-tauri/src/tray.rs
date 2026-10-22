//! 托盘图标：每个可见设备一个环形电量图（§12）。
//!
//! 图标由本模块用 32×32 RGBA 手工光栅化，不引入任何图形渲染依赖（AA-4）。
//! 语义与前端 `batteryColor()` 保持一致：正常主题色、<20% 橙、<10% 红、充电青，
//! 离线但物理存在时为灰环加横线。
//!
//! 图标**存在性只由物理存在决定**：`snapshot.tray.visibleDeviceIds` 已经由核心
//! 按 `present && !hidden` 且截断到 4 个算好，这里只负责跟随，不做二次判断。
//!
//! # 基础图标（空心灰环）什么时候在
//!
//! 窗口关掉之后只剩托盘，**一张图标都没有就等于进程失联**：既打不开面板，也没
//! 办法退出。所以托盘里必须始终至少有一张能用的图标。
//!
//! 基础灰环因此只在「一张设备图标都没有」时占位：有设备图标出现就让位（托盘里
//! 的电量环本身就是入口，再挂一个灰环只会让人误以为是第二台设备）；设备全部拔掉
//! 或全部被用户隐藏时它再回来。
//!
//! 每张图标的右键菜单完全一致（打开面板 / 退出 VoltNest），任意一张都能退出软件。

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

use crate::host::{Shared, lock};

/// 基础托盘图标的 id（没有设备图标时占位，点它打开面板）。
pub const BASE_TRAY_ID: &str = "voltnest";
/// 图标边长。
pub const ICON_SIZE: u32 = 32;
/// tooltip 上限（§12）。
const TOOLTIP_LIMIT: usize = 128;
/// 托盘气泡（tooltip 提示）的持续时长。
const BUBBLE_MS: u64 = 6_000;

const MENU_OPEN: &str = "tray:open";
const MENU_QUIT: &str = "tray:quit";

const EMERALD: [u8; 3] = [16, 185, 129];
const ORANGE: [u8; 3] = [249, 115, 22];
const RED: [u8; 3] = [239, 68, 68];
const CYAN: [u8; 3] = [6, 182, 212];
const GRAY: [u8; 3] = [148, 163, 184];

/// 每张图标「本该显示」的 tooltip：气泡提示只占用 6 秒，之后要原样还回去。
static BASELINE_TOOLTIPS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// 基础图标当前是否露在托盘里（隐身的图标不能当气泡的落点）。
static BASE_VISIBLE: AtomicBool = AtomicBool::new(true);

/// 安装托盘：菜单事件处理器 + 基础图标。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    // 菜单事件是**全局**分发的（任何一张图标的菜单都会走同一个监听器列表）。
    // 所以只注册一次：若每张图标各自注册，点一次「退出」会被调用 N 次。
    app.on_menu_event(|app, event| match event.id.as_ref() {
        MENU_OPEN => show_main(app),
        MENU_QUIT => quit(app),
        _ => {}
    });

    build_tray(
        app,
        BASE_TRAY_ID,
        ring_image(None, false, DeviceState::Unknown),
        "VoltNest",
    )
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
pub fn sync(app: &AppHandle, shared: &Arc<Shared>, snap: &Snapshot) {
    let mut known = lock(&shared.tray_ids);
    let visible = &snap.tray.visible_device_ids;
    // 被上限截断的设备数：基础图标退位之后，「还有 N 台」只能挂在最后一张设备图标上。
    let truncated = snap.devices.len().saturating_sub(visible.len());
    let mut wanted: Vec<String> = Vec::with_capacity(visible.len());

    for (idx, stable_id) in visible.iter().enumerate() {
        let Some(device) = snap.devices.iter().find(|d| &d.stable_id == stable_id) else {
            // 快照自洽性由核心保证；真出现不一致时跳过而不是 panic。
            continue;
        };
        let tray_id = tray_id_for(stable_id);
        let icon = ring_image(
            device.battery,
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

/// 基础灰环的显隐：有设备图标就隐身让位，没有就出来占位。
///
/// 只切可见性、不销毁重建：图标一旦重建就会跑到托盘末尾（甚至被折叠进溢出区），
/// 用户眼前的东西自己跳位比多看一个灰环更难受。
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
    // `set_visible` 会让 Shell 重新注册这一项，没必要每次快照都碰它。
    if BASE_VISIBLE.swap(visible, Ordering::Relaxed) != visible {
        let _ = base.set_visible(visible);
    }
}

/// 拿基础图标；万一它不在（install 失败过）就补建一张。
fn base_tray(app: &AppHandle, tooltip: &str) -> Option<TrayIcon> {
    if let Some(base) = app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID)) {
        return Some(base);
    }
    if let Err(err) = build_tray(
        app,
        BASE_TRAY_ID,
        ring_image(None, false, DeviceState::Unknown),
        tooltip,
    ) {
        log::warn!("创建基础托盘图标失败: {err}");
        return None;
    }
    app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID))
}

/// 建一张托盘图标：统一菜单 + 左键开面板。
///
/// 菜单和图标都是主线程资源（Tauri 的 `Menu` 在 Drop 里也要回主线程），所以
/// 「建菜单 + 建图标」整段搬到主线程做：`sync` 跑在宿主线程上，而
/// `run_on_main_thread` 在主线程上会就地执行，不会自锁。
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
                // 左键点图标开面板，右键才弹菜单，和系统托盘惯例一致。
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

/// 托盘气泡模式：把提示写进对应图标的 tooltip，6 秒后恢复原样。
///
/// 返回是否真的挂上了 —— 没有可用图标时不要假装发出去（诊断页要能分辨）。
pub fn bubble(app: &AppHandle, stable_id: &str, text: &str) -> bool {
    let Some(tray) = alert_tray(app, stable_id) else {
        return false;
    };
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
    true
}

/// 低电量提示挂哪张图标：优先设备自己的，其次**露在托盘里**的基础图标。
fn alert_tray(app: &AppHandle, stable_id: &str) -> Option<TrayIcon> {
    if let Some(tray) = app.tray_by_id(&TrayIconId::new(tray_id_for(stable_id))) {
        return Some(tray);
    }
    // 基础图标隐身时不能当落点：提示写进 tooltip 也没人看得见。
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
    // 有设备却一张图标都没有：只可能是用户把它们全隐藏了。
    if snap.tray.visible_device_ids.is_empty() {
        return format!("VoltNest · {total} 台设备（未固定在托盘）");
    }
    format!("VoltNest · {total} 台设备")
}

/// 设备托盘图标的 id：`tray:<stableId>`。
fn tray_id_for(stable_id: &str) -> String {
    format!("tray:{stable_id}")
}

/// tooltip 文案：`型号 · 92%` / `型号 · —（离线）`。
///
/// `extra > 0` 表示还有设备没放进托盘（§12「还有 N 台」），挂在最后一张图标上；
/// 整串（含后缀）始终不超过 128 字符。
fn tooltip_for(device: &DeviceDto, extra: usize) -> String {
    let battery = match device.battery {
        Some(v) => format!("{v}%"),
        None => "—".to_string(),
    };
    let mut body = format!("{} · {battery}", device.display_name);
    if device.state == DeviceState::Offline {
        body.push_str("（离线）");
    }
    let suffix = if extra > 0 {
        format!(" · 还有 {extra} 台")
    } else {
        String::new()
    };
    // 先给后缀留出位置，长设备名不会把「还有 N 台」挤掉。
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

/// 生成一张 32×32 的环形电量图。
///
/// - `battery == None`：空心环（无读数 / 尚未发现设备）；
/// - `offline`：灰环 + 横线（物理存在但链路不在）；
/// - `charging`：青色整环；
/// - 其余按 20% / 10% 分档着色。
pub fn ring_image(battery: Option<u8>, charging: bool, state: DeviceState) -> Image<'static> {
    let size = ICON_SIZE as f32;
    let center = size / 2.0;
    let outer = size * 0.44;
    let inner = outer - size * 0.155;

    let (rgb, draw_line) = if state == DeviceState::Offline {
        (GRAY, true)
    } else if charging {
        (CYAN, false)
    } else {
        match battery {
            Some(v) if v < 10 => (RED, false),
            Some(v) if v < 20 => (ORANGE, false),
            _ => (EMERALD, false),
        }
    };

    let ratio = if state == DeviceState::Offline {
        1.0
    } else {
        battery
            .map(|v| f32::from(v.min(100)) / 100.0)
            .unwrap_or(0.0)
    };

    // 3×3 超采样：32px 上不做抗锯齿的环会明显锯齿。
    const SS: usize = 3;
    const TRACK: [u8; 4] = [120, 130, 145, 110];
    let mut buf = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for py in 0..ICON_SIZE {
        for px in 0..ICON_SIZE {
            let mut arc_hits = 0u32;
            let mut track_hits = 0u32;
            let mut line_hits = 0u32;
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
                    if draw_line && (y - center).abs() <= 1.1 && (x - center).abs() <= outer * 0.72
                    {
                        line_hits += 1;
                    }
                }
            }

            let total = (SS * SS) as f32;
            let idx = ((py * ICON_SIZE + px) * 4) as usize;
            // 轨道只画在有过采样命中的地方，避免整块半透明。
            let track_a = track_hits as f32 / total;
            let arc_a = arc_hits as f32 / total;
            let line_a = line_hits as f32 / total;
            let (r, g, b, a) = blend(rgb, arc_a, TRACK, track_a, line_a);
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
        // 横线用深色，压在环上表示「链路不在」。
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
            battery,
            last_known_battery: battery,
            last_known_at_ms: None,
            is_charging: Some(false),
            state,
            last_valid_at_ms: None,
            last_push_at_ms: None,
            last_error: None,
        }
    }

    fn snapshot(devices: usize, visible: usize) -> Snapshot {
        let mut snap = Snapshot {
            rev: 1,
            server_now_ms: 0,
            devices: Vec::new(),
            cards: Vec::new(),
            tray: voltnest_core::TrayDto {
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
    fn tooltip_marks_offline_and_null_battery() {
        let d = device(None, DeviceState::Offline);
        assert_eq!(tooltip_for(&d, 0), "AJ159 NL · —（离线）");
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
        // 四角是透明的：托盘图标不该有方块底。
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
