//! 托盘图标：每个可见设备一个环形电量图（§12）。
//!
//! 设备图标用 32×32 RGBA 手工光栅化，不引入图形渲染依赖（AA-4）；语义与前端 `batteryColor()` 一一对应：在线主题色、<20% 橙、<10% 红、充电绿弧加环心闪电、等待读数蓝、过期琥珀、读取失败橙，离线但物理存在时为灰环加横线。图标**存在性只由物理存在决定**：`visibleDeviceIds` 已由核心按 `present && !hidden` 截断到 4 个算好，这里只跟随、不二次判断。
//!
//! # 基础图标（应用图标）何时显现
//! 窗口关掉只剩托盘时，**一张图标都没有等于进程失联**（打不开面板也无法退出），故托盘必须始终至少有一张可用图标。占位图标即**应用图标本身**（与任务栏/开始菜单同 PNG）：无设备时用户看到「应用在跑」而非需解读的小图形；手绘蓝环只留给「设备已在列表、还没拿到读数」那一种状态。它仅在「没有设备图标」时现身：有设备图标就让位（电量环本身就是入口，再挂一个会像第二台设备）；设备全拔或全被隐藏时再回来。每张图标右键菜单一致（打开面板 / 退出 VoltNest），任意一张都能退出。

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

/// 与前端 `lib/format.ts` 色值逐字对应（托盘与面板须能并排看）。充电时两侧都用 `EMERALD`（即面板 `CHARGING_GREEN`），靠闪电形状而非颜色表达「在充电」。
const EMERALD: [u8; 3] = [16, 185, 129];
const ORANGE: [u8; 3] = [249, 115, 22];
const RED: [u8; 3] = [239, 68, 68];
const AMBER: [u8; 3] = [245, 158, 11];
const SKY: [u8; 3] = [56, 189, 248];
const GRAY: [u8; 3] = [148, 163, 184];

/// 充电闪电：lucide `zap` 折点近似（24×24，形心 12,12）。原图带圆角描边，这里直接连圆弧端点 —— 32px 上差不到半像素，却省掉贝塞尔光栅化器。
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

/// 还没拿到任何读数时，整环按这个透明度画：蓝得看得见，又明显不是一次真实读数。
const PENDING_ALPHA: f32 = 0.7;

/// 应用图标：`bundle.icon` 里 32×32 那张，与任务栏/开始菜单同源。无设备时显示它而非另画图形：用户认的是应用图标，托盘里出现同款方块即说明「这只是应用本身」。
const APP_ICON_PNG: &[u8] = include_bytes!("../icons/32x32.png");

/// 每张图标「本该显示」的 tooltip：气泡提示只占用 6 秒，之后要原样还回去。
static BASELINE_TOOLTIPS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// 基础图标当前是否露在托盘里（隐身的图标不能当气泡的落点）。
static BASE_VISIBLE: AtomicBool = AtomicBool::new(true);

/// 解出打包的应用图标。解不开（文件坏掉）时退回蓝色等待环：难看总比托盘空着好，且只是占位图、不影响任何设备图标。
fn app_icon() -> Image<'static> {
    Image::from_bytes(APP_ICON_PNG).unwrap_or_else(|err| {
        log::warn!("解码应用图标失败，退回等待环: {err}");
        ring_image(None, false, DeviceState::Unknown)
    })
}

/// 安装托盘：菜单事件处理器 + 基础图标。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    // 菜单事件全局分发（任何图标的菜单走同一监听器列表），故只注册一次：各图标各自注册会让一次「退出」被调用 N 次。
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

/// 基础图标显隐：有设备图标就隐身让位，没有就出来占位。只切可见性不重建：重建会让图标跳到托盘末尾甚至溢出区，比多看一眼占位图标更难受。
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
    if let Err(err) = build_tray(app, BASE_TRAY_ID, app_icon(), tooltip) {
        log::warn!("创建基础托盘图标失败: {err}");
        return None;
    }
    app.tray_by_id(&TrayIconId::new(BASE_TRAY_ID))
}

/// 建一张托盘图标：统一菜单 + 左键开面板。菜单与图标均为主线程资源（Tauri `Menu` Drop 也要回主线程），故整段搬到主线程做：`sync` 跑在宿主线程，而 `run_on_main_thread` 在主线程就地执行、不会自锁。
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

/// 托盘气泡模式：把提示写进对应图标 tooltip，6 秒后恢复原样。返回是否真的挂上 —— 没有可用图标时不假装发出去（诊断页要能分辨）。
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

/// tooltip 文案：`型号 · 92%` / `型号 · —（离线）` / `型号 · 87%（上次读数）`。读数与图标共用同一个 `displayed_battery()`：图标画了 87% 蓝弧，悬停文字就不能还写「—」，否则用户以为百分比凭空来的。`extra > 0` 表示还有设备没进托盘（§12「还有 N 台」），挂最后一张图标上；整串（含后缀）≤128 字符。
fn tooltip_for(device: &DeviceDto, extra: usize) -> String {
    let reading = displayed_battery(device);
    let battery = match reading {
        Some(r) => format!("{}%", r.percent),
        None => "—".to_string(),
    };
    let mut body = format!("{} · {battery}", device.display_name);
    // 缓存值必须标明来源，不能和实时读数看起来一样。
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

/// 托盘用的一次读数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reading {
    /// 百分比。
    percent: u8,
    /// 本次会话是否还没拿到实时读数（来自 `state.json` 缓存）。
    from_cache: bool,
}

/// 与前端 `displayedBattery()` 同规则：首读未到（`state == Unknown`）时用 `lastKnownBattery` 撑住图标与 tooltip，免得启动瞬间托盘变成无比例、无读数的空环。`Unavailable`/`Offline` 不算 —— 那是「确定读不到」，不能假装还有实时读数。
fn displayed_battery(device: &DeviceDto) -> Option<Reading> {
    if let Some(percent) = device.battery {
        return Some(Reading {
            percent,
            from_cache: false,
        });
    }
    match device.state {
        DeviceState::Unknown => device.last_known_battery.map(|percent| Reading {
            percent,
            from_cache: true,
        }),
        _ => None,
    }
}

/// 环的颜色：除充电外与前端 `batteryColor()` 逐条对应（§12）。只有 `Online`/`Stale` 有「当前读数」可按阈值着色，其余状态沿用状态色（即面板「等待读数」蓝环、「读取失败」橙环那套）。充电不看电量高低一律绿：闪电已说清「在充电」、弧长继续表达充到哪了；面板同色（`CHARGING_GREEN`），两侧一致。
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

/// 生成一张 32×32 环形电量图：`battery == None` 等待环（整圈状态色、透明度压低一档）；`offline` 灰环+横线（物理存在链路不在）；`unknown` 蓝环（与面板「等待读数」同色，启动回填也走这条）；`charging` 绿弧+环心闪电，**弧长仍随电量走**、只把环收窄给闪电让位；其余按 20%/10% 分档着色。
pub fn ring_image(battery: Option<u8>, charging: bool, state: DeviceState) -> Image<'static> {
    let size = ICON_SIZE as f32;
    let center = size / 2.0;
    let outer = size * 0.44;

    let plug = is_charging_glyph(charging, state);
    // 闪电要占环心，环太厚就会贴上去糊成一团；但也别细到看不出弧长，
    // 32px 上 0.12×size ≈ 3.8px，仍是任务栏里一眼能读出的粗细。
    let thickness = if plug { size * 0.12 } else { size * 0.155 };
    let inner = outer - thickness;

    let rgb = ring_tone(battery, charging, state);
    let draw_line = state == DeviceState::Offline;
    // 一圈都没有读数（刚启动、还没回填到缓存值）：整环用状态色画满，
    // 这样托盘里始终是「一块看得懂的颜色」，而不是只剩灰轨道。
    let pending = battery.is_none() && !draw_line;

    // 充电也照实画弧：闪电负责说「在充电」，弧长继续交代「充到哪了」。
    // 只画满环会让 5% 和 95% 长得一模一样，进度就没了。
    let ratio = if state == DeviceState::Offline || pending {
        1.0
    } else {
        battery
            .map(|v| f32::from(v.min(100)) / 100.0)
            .unwrap_or(0.0)
    };
    let arc_alpha = if pending { PENDING_ALPHA } else { 1.0 };
    // 24 单位的闪电画成 0.56×size 高，正好落在收窄后的环心里（尖角离内圈约 1.5px）。
    let bolt_scale = size * 0.028;

    // 3×3 超采样：32px 上不做抗锯齿的环会明显锯齿。
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
            // 轨道只画在有过采样命中的地方，避免整块半透明。
            let track_a = track_hits as f32 / total;
            let arc_a = arc_hits as f32 / total;
            let line_a = line_hits as f32 / total;
            let bolt_a = bolt_hits as f32 / total;
            // 闪电与环同色，直接并进电弧那一层；两者几何上不重叠。
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
            device_type: "mouse".into(),
            classification_method: "verified_device_mapping".into(),
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
    fn base_icon_is_the_app_icon() {
        // 没有设备时托盘该显示应用图标本身（和任务栏同一个 PNG），不是手绘的电量环。
        let img = app_icon();
        assert_eq!((img.width(), img.height()), (ICON_SIZE, ICON_SIZE));
        let rgba = img.rgba();
        let opaque = rgba.chunks(4).filter(|p| p[3] > 200).count();
        assert!(
            opaque > 900,
            "应用图标是实心圆角方块，覆盖面积远大于电量环：{opaque}"
        );
        // 顶端正中：电量环在这里是透明的（圆环上边缘落在 y≈2），应用图标的深色底是实心。
        assert_eq!(
            rgba[(16 * 4) + 3],
            255,
            "顶端实心说明用的是应用图标，而不是空心环"
        );
    }

    #[test]
    fn waiting_icon_is_the_panel_blue() {
        // 面板「等待读数」是 #38bdf8；刚启动、还没发现设备时托盘必须是同一个蓝。
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
        // 半透明像素做混合除法时会有一两级的取整误差，按容差比色。
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
        // 环心（半径 0.5×center 以内）在不充电时必须是空的。
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
        // 只量环上的不透明像素（环内半径 0.6×中心距以内是闪电的地盘，排除掉）。
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
            Some(Reading {
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
        // 环上画了 87% 的蓝弧，悬停文字就不能还写着「—」。
        let mut waiting = device(None, DeviceState::Unknown);
        waiting.last_known_battery = Some(87);
        assert_eq!(tooltip_for(&waiting, 0), "AJ159 NL · 87%（上次读数）");

        // 有实时读数时不该多出「上次读数」的尾巴。
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
