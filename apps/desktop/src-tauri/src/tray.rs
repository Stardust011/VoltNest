//! 托盘图标：每个可见设备一个环形电量图（§12）。
//!
//! 图标由本模块用 32×32 RGBA 手工光栅化，不引入任何图形渲染依赖（AA-4）。
//! 语义与前端 `batteryColor()` 保持一致：正常主题色、<20% 橙、<10% 红、充电青，
//! 离线但物理存在时为灰环加横线。
//!
//! 图标**存在性只由物理存在决定**：`snapshot.tray.visibleDeviceIds` 已经由核心
//! 按 `present && !hidden` 且截断到 4 个算好，这里只负责跟随，不做二次判断。

use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent, TrayIconId};
use tauri::{AppHandle, Manager};

use voltnest_core::{DeviceDto, DeviceState, Snapshot};

use crate::host::{Shared, lock};

/// 基础托盘图标的 id（点它打开面板）。
pub const BASE_TRAY_ID: &str = "voltnest";
/// 图标边长。
pub const ICON_SIZE: u32 = 32;
/// tooltip 上限（§12）。
const TOOLTIP_LIMIT: usize = 128;

const EMERALD: [u8; 3] = [16, 185, 129];
const ORANGE: [u8; 3] = [249, 115, 22];
const RED: [u8; 3] = [239, 68, 68];
const CYAN: [u8; 3] = [6, 182, 212];
const GRAY: [u8; 3] = [148, 163, 184];

/// 安装基础托盘图标。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 VoltNest", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;

    TrayIconBuilder::with_id(BASE_TRAY_ID)
        .icon(ring_image(None, false, DeviceState::Unknown))
        .tooltip("VoltNest")
        .menu(&menu)
        // 左键点图标开面板，右键才弹菜单，和系统托盘惯例一致。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 显示并聚焦主窗口（二次启动、托盘点击、菜单项都走这里）。
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 按快照同步设备托盘图标：增、改、删。
pub fn sync(app: &AppHandle, shared: &Arc<Shared>, snap: &Snapshot) {
    let mut known = lock(&shared.tray_ids);
    let mut wanted: Vec<String> = Vec::with_capacity(snap.tray.visible_device_ids.len());

    for stable_id in &snap.tray.visible_device_ids {
        let Some(device) = snap.devices.iter().find(|d| &d.stable_id == stable_id) else {
            // 快照自洽性由核心保证；真出现不一致时跳过而不是 panic。
            continue;
        };
        let tray_id = TrayIconId::new(tray_id_for(stable_id));
        let icon = ring_image(
            device.battery,
            device.is_charging == Some(true),
            device.state,
        );
        let tooltip = tooltip_for(device);

        match app.tray_by_id(&tray_id) {
            Some(tray) => {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_tooltip(Some(&tooltip));
            }
            None => {
                if let Err(err) = TrayIconBuilder::with_id(tray_id)
                    .icon(icon)
                    .tooltip(tooltip)
                    .build(app)
                {
                    log::warn!("创建托盘图标失败（{stable_id}）: {err}");
                    continue;
                }
            }
        }
        wanted.push(stable_id.clone());
    }

    for stale in known.iter().filter(|id| !wanted.contains(id)) {
        app.remove_tray_by_id(&TrayIconId::new(tray_id_for(stale)));
    }
    *known = wanted;
}

/// 设备托盘图标的 id：`tray:<stableId>`。
fn tray_id_for(stable_id: &str) -> String {
    format!("tray:{stable_id}")
}

/// tooltip 文案：`型号 · 92%` / `型号 · —`，截断到 128 字符。
fn tooltip_for(device: &DeviceDto) -> String {
    let battery = match device.battery {
        Some(v) => format!("{v}%"),
        None => "—".to_string(),
    };
    let mut text = format!("{} · {battery}", device.display_name);
    if device.state == DeviceState::Offline {
        text.push_str("（离线）");
    }
    if text.chars().count() > TOOLTIP_LIMIT {
        text = text.chars().take(TOOLTIP_LIMIT - 1).collect();
        text.push('…');
    }
    text
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
            provider: "hid-nearlink-ajazz".into(),
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

    #[test]
    fn tooltip_is_bounded() {
        let mut d = device(Some(92), DeviceState::Online);
        assert_eq!(tooltip_for(&d), "AJ159 NL · 92%");
        d.display_name = "型号".repeat(100);
        let text = tooltip_for(&d);
        assert!(text.chars().count() <= TOOLTIP_LIMIT);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn tooltip_marks_offline_and_null_battery() {
        let d = device(None, DeviceState::Offline);
        assert_eq!(tooltip_for(&d), "AJ159 NL · —（离线）");
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
