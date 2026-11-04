//! 一个事件循环托管 N 个悬浮窗。
//!
//! 每个「呈现单元」（一台设备或一个合并组）对应一个独立的 `MainWindow` 实例，属性、
//! 拖拽锚点、定时器全部 per-instance，所以隐藏/移动其中一个不会波及其它窗口。
//!
//! 关键：用 [`slint::run_event_loop_until_quit`] 而不是 `run_event_loop`。后者在最后一个
//! 窗口隐藏时会直接退出事件循环（进程随之结束），表现为「关掉一个窗，其它窗跟着消失，
//! 再点开关 1 秒后才闪现」。

use crate::geometry::{
    MENU_GAP, MENU_H, MENU_W, MIN_SIDE, cascade_position, drag_delta, resized_side, screen_cursor,
};
use crate::proto::{ChildMsg, HostMsg, PROTOCOL_VERSION, UnitWire};
use crate::ui::MainWindow;
use slint::{ComponentHandle, PhysicalPosition, Timer, TimerMode};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

/// 子进程 → 宿主的唯一写出口。
///
/// 真正的写发生在独立线程里，UI 线程只投递，管道另一端卡住也不会冻住界面。
#[derive(Clone)]
pub(crate) struct ChildWriter {
    tx: Option<Sender<String>>,
}

impl ChildWriter {
    /// 写 stdout 的后台线程；宿主逐行读走。
    pub(crate) fn stdout() -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::Write;
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for line in rx {
                if writeln!(out, "{line}").is_err() {
                    break;
                }
                let _ = out.flush();
            }
        });
        Self { tx: Some(tx) }
    }

    /// 演示模式：消息直接丢弃（standalone 时不该往终端刷 NDJSON）。
    pub(crate) fn null() -> Self {
        Self { tx: None }
    }

    /// 发送一条消息；写线程已退出时静默忽略。
    pub(crate) fn send(&self, msg: &ChildMsg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(crate::proto::encode(msg));
        }
    }
}

/// 本轮要对某个锚点做的事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// 建窗口并显示。
    Create,
    /// 已存在但被隐藏：重新摆位并显示。
    Show,
    /// 只更新属性。
    Update,
    /// 隐藏但保留实例。
    Hide,
    /// 关窗并销毁实例。
    Close,
}

/// 求本轮动作。`prev` = 当前窗口及其可见性，`next` = 宿主期望的可见性。
pub(crate) fn plan(
    prev: &BTreeMap<String, bool>,
    next: &BTreeMap<String, bool>,
) -> Vec<(String, Action)> {
    let mut actions = Vec::new();
    for (anchor, &visible) in next {
        match prev.get(anchor) {
            None if visible => actions.push((anchor.clone(), Action::Create)),
            None => {}
            Some(&was) if was != visible => {
                actions.push((
                    anchor.clone(),
                    if visible { Action::Show } else { Action::Hide },
                ));
            }
            Some(_) => actions.push((anchor.clone(), Action::Update)),
        }
    }
    for anchor in prev.keys() {
        if !next.contains_key(anchor) {
            actions.push((anchor.clone(), Action::Close));
        }
    }
    actions
}

/// 一个窗口实例及其存活期资源。
struct WindowSlot {
    ui: MainWindow,
    /// 透明度去抖定时器，必须由 slot 持有，否则会被立刻析构。
    _debounce: Rc<Timer>,
    /// 演示心跳（仅 Demo 模式）。
    demo: Option<Timer>,
}

/// 会话：所有窗口 + 与宿主的通道。
pub(crate) struct Session {
    writer: ChildWriter,
    seq: u64,
    slots: BTreeMap<String, WindowSlot>,
    /// 没有存过坐标时，级联默认位的序号。
    cascade: i32,
}

impl Session {
    /// 建一个空会话。
    pub(crate) fn new(writer: ChildWriter) -> Self {
        Self {
            writer,
            seq: 0,
            slots: BTreeMap::new(),
            cascade: 0,
        }
    }

    /// 处理一条宿主消息。
    pub(crate) fn apply(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Sync { v, seq, units } => {
                if v != PROTOCOL_VERSION {
                    self.writer.send(&ChildMsg::Bye {
                        v: PROTOCOL_VERSION,
                        reason: format!("protocol_mismatch: host={v}"),
                    });
                    let _ = slint::quit_event_loop();
                    return;
                }
                if seq <= self.seq {
                    return;
                }
                self.seq = seq;
                self.sync(units);
            }
            HostMsg::Shutdown { .. } => {
                let _ = slint::quit_event_loop();
            }
        }
    }

    /// 全量同步：按 [`plan`] 求差，只动有变化的窗口。
    fn sync(&mut self, units: Vec<UnitWire>) {
        let prev: BTreeMap<String, bool> = self
            .slots
            .iter()
            .map(|(anchor, slot)| (anchor.clone(), slot.ui.window().is_visible()))
            .collect();
        let next: BTreeMap<String, bool> = units
            .iter()
            .map(|unit| (unit.anchor.clone(), unit.visible))
            .collect();

        for (anchor, action) in plan(&prev, &next) {
            let unit = units.iter().find(|unit| unit.anchor == anchor);
            match (action, unit) {
                (Action::Create, Some(unit)) => self.create(unit),
                (Action::Show, Some(unit)) => self.show_unit(&anchor, unit),
                (Action::Hide, _) => self.hide(&anchor),
                (Action::Update, Some(unit)) => self.refresh(&anchor, unit),
                (Action::Close, _) => self.close(&anchor),
                // 计划与单元表不一致（不可能发生）：当作关闭处理，避免泄漏窗口。
                (_, None) => self.close(&anchor),
            }
        }
    }

    fn create(&mut self, unit: &UnitWire) {
        let ui = match MainWindow::new() {
            Ok(ui) => ui,
            Err(err) => {
                log::error!("创建悬浮窗失败（{}）: {err}", unit.anchor);
                return;
            }
        };

        let scale = ui.window().scale_factor();
        let side = apply_unit(&ui, unit);

        // 位置：配置里有就用配置，没有就退到级联默认位。
        let position = match (unit.x, unit.y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => monitor_rect(&ui).map(|screen| {
                let side_px = (side * scale).round() as i32;
                cascade_position(screen, side_px, self.cascade)
            }),
        };
        if let Some((x, y)) = position {
            ui.window().set_position(PhysicalPosition::new(x, y));
        }
        self.cascade += 1;

        // 注：任务栏图标的隐藏靠 `lib.rs::select_backend()` 里的窗口属性钩子（`skip_taskbar`），
        // 在窗口创建时就生效，这里不需要额外处理。

        let debounce = self.install_handlers(&ui, unit.anchor.clone());
        if let Err(err) = ui.show() {
            log::error!("显示悬浮窗失败（{}）: {err}", unit.anchor);
            return;
        }

        self.slots.insert(
            unit.anchor.clone(),
            WindowSlot {
                ui,
                _debounce: debounce,
                demo: None,
            },
        );
    }

    /// 从隐藏恢复：按配置重新摆位，再显示。
    fn show_unit(&mut self, anchor: &str, unit: &UnitWire) {
        self.refresh(anchor, unit);
        let Some(slot) = self.slots.get(anchor) else {
            return;
        };
        if let (Some(x), Some(y)) = (unit.x, unit.y) {
            slot.ui.window().set_position(PhysicalPosition::new(x, y));
        }
        if let Err(err) = slot.ui.show() {
            log::error!("显示悬浮窗失败（{anchor}）: {err}");
        }
    }

    /// 只刷新文案与尺寸/透明度，不动可见性。
    fn refresh(&self, anchor: &str, unit: &UnitWire) {
        let Some(slot) = self.slots.get(anchor) else {
            return;
        };
        let ui = &slot.ui;
        ui.set_device_name(unit.title.as_str().into());
        ui.set_device_type(unit.device_type.as_str().into());
        ui.set_device_battery(unit.battery.as_str().into());
        ui.set_device_status(unit.status.as_str().into());
        ui.set_level(unit.level.clamp(0.0, 1.0));

        let side = f32::from(unit.side).clamp(MIN_SIDE, crate::geometry::MAX_SIDE);
        if (ui.get_card_side() - side).abs() > 0.5 {
            ui.set_card_side(side);
            ui.window().set_size(slint::LogicalSize::new(side, side));
        }
        let transparency = f32::from(unit.transparency).clamp(0.0, 100.0);
        if (ui.get_transparency() - transparency).abs() > 0.5 {
            ui.set_transparency(transparency);
        }
    }

    fn hide(&self, anchor: &str) {
        if let Some(slot) = self.slots.get(anchor) {
            // 收起菜单再藏，否则下次显示时窗口还停在「卡片 ∪ 面板」的尺寸上。
            dismiss_menu(&slot.ui);
            let _ = slot.ui.hide();
        }
    }

    fn close(&mut self, anchor: &str) {
        if let Some(slot) = self.slots.remove(anchor) {
            let _ = slot.ui.hide();
            // 组件在这里 drop，窗口随之销毁；其它窗口不受影响。
        }
    }

    /// 装手势回调，返回需要随 slot 保活的透明度去抖定时器。
    fn install_handlers(&self, ui: &MainWindow, anchor: String) -> Rc<Timer> {
        let writer = self.writer.clone();

        // ---- 拖拽 ----
        let cursor: Rc<std::cell::Cell<Option<(f32, f32)>>> = Rc::new(std::cell::Cell::new(None));
        {
            let reset = cursor.clone();
            ui.on_drag_begin(move || reset.set(None));
        }
        {
            let writer = writer.clone();
            let anchor = anchor.clone();
            let cursor = cursor.clone();
            let weak = ui.as_weak();
            ui.on_drag_end(move || {
                cursor.set(None);
                if let Some(ui) = weak.upgrade() {
                    report_layout(&ui, &anchor, &writer);
                }
            });
        }
        {
            let weak = ui.as_weak();
            let cursor = cursor.clone();
            ui.on_drag_move(move |x: f32, y: f32| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let window = ui.window();
                let origin = window.position();
                let offset = (ui.get_card_x(), ui.get_card_y());
                let now = screen_cursor(origin, window.scale_factor(), offset, x, y);
                if let Some(prev) = cursor.get() {
                    let (dx, dy) = drag_delta(prev, now);
                    window.set_position(PhysicalPosition::new(origin.x + dx, origin.y + dy));
                }
                cursor.set(Some(now));
            });
        }

        // ---- 缩放 ----
        let base = Rc::new(std::cell::Cell::new(0.0f32));
        let start: Rc<std::cell::Cell<Option<(f32, f32)>>> = Rc::new(std::cell::Cell::new(None));
        {
            let reset = start.clone();
            ui.on_resize_begin(move || reset.set(None));
        }
        {
            let writer = writer.clone();
            let anchor = anchor.clone();
            let start = start.clone();
            let weak = ui.as_weak();
            ui.on_resize_end(move || {
                start.set(None);
                if let Some(ui) = weak.upgrade() {
                    report_layout(&ui, &anchor, &writer);
                }
            });
        }
        {
            let weak = ui.as_weak();
            let start = start.clone();
            let base = base.clone();
            ui.on_resize_move(move |x: f32, y: f32| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if start.get().is_none() {
                    base.set(ui.get_card_side());
                }
                let from = start.get().unwrap_or((x, y));
                start.set(Some(from));
                let side = resized_side(base.get(), from, (x, y));
                ui.set_card_side(side);
                ui.window().set_size(slint::LogicalSize::new(side, side));
            });
        }

        // ---- 右键菜单外扩 ----
        {
            let weak = ui.as_weak();
            ui.on_menu_open_requested(move |px: f32| {
                if let Some(ui) = weak.upgrade() {
                    open_menu(&ui, px);
                }
            });
        }
        {
            let weak = ui.as_weak();
            ui.on_menu_closed(move || {
                if let Some(ui) = weak.upgrade() {
                    close_menu(&ui);
                }
            });
        }

        // ---- 窗口失焦：菜单跟着收起 ----
        //
        // 面板是自绘的，不是系统菜单，所以点桌面/别的程序时系统不会替我们关。这里挂 winit 的
        // 窗口事件过滤器：一失焦就把面板收掉，行为与系统菜单一致。
        {
            use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
            let weak = ui.as_weak();
            ui.window().on_winit_window_event(move |_, event| {
                if matches!(event, winit::event::WindowEvent::Focused(false))
                    && weak.upgrade().is_some_and(|ui| ui.get_menu_open())
                {
                    // 别在 winit 的事件分发里改窗口尺寸，推迟一轮事件循环再做。
                    let weak = weak.clone();
                    Timer::single_shot(Duration::ZERO, move || {
                        if let Some(ui) = weak.upgrade() {
                            dismiss_menu(&ui);
                        }
                    });
                }
                EventResult::Propagate
            });
        }

        // ---- 透明度：停手 300ms 再上报，避免每帧一次 ----
        let debounce = Rc::new(Timer::default());
        {
            let writer = writer.clone();
            let anchor = anchor.clone();
            let timer = debounce.clone();
            let weak = ui.as_weak();
            ui.on_transparency_adjusted(move || {
                let weak = weak.clone();
                let writer = writer.clone();
                let anchor = anchor.clone();
                timer.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(300),
                    move || {
                        if let Some(ui) = weak.upgrade() {
                            report_layout(&ui, &anchor, &writer);
                        }
                    },
                );
            });
        }

        // ---- 隐藏本窗口（只影响自己，不再杀进程）----
        {
            let writer = writer.clone();
            let anchor = anchor.clone();
            let weak = ui.as_weak();
            ui.on_close_requested(move || {
                // 回调是在 TouchArea 的事件处理里发的，那时 Slint 内部状态仍被借用，
                // 直接 hide() 会 panic（RefCell already borrowed），所以推迟一轮。
                let weak = weak.clone();
                let writer = writer.clone();
                let anchor = anchor.clone();
                Timer::single_shot(Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        let _ = ui.hide();
                    }
                    writer.send(&ChildMsg::Hidden {
                        v: PROTOCOL_VERSION,
                        anchor,
                    });
                });
            });
        }

        debounce
    }

    /// 演示模式：一个假单元 + 1s 心跳，用来单独跑 `cargo run -p voltnest-floating`。
    pub(crate) fn start_demo(&mut self) {
        let anchor = "demo:standalone".to_string();
        self.apply(HostMsg::Sync {
            v: PROTOCOL_VERSION,
            seq: 1,
            units: vec![demo_unit(&anchor)],
        });

        let Some(slot) = self.slots.get_mut(&anchor) else {
            return;
        };
        let weak = slot.ui.as_weak();
        let timer = Timer::default();
        let mut tick = 0u32;
        timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            tick = tick.wrapping_add(1);
            // 0.60 ~ 0.95 之间来回摆动，仅用于演示。
            let wave = (tick % 36) as f32;
            let level = 0.60 + if wave <= 18.0 { wave } else { 36.0 - wave } / 18.0 * 0.35;
            let percent = format!("{}%", (level * 100.0).round() as i32);
            ui.set_level(level);
            ui.set_device_battery(percent.as_str().into());
        });
        slot.demo = Some(timer);
    }
}

/// 把单元数据写进窗口，返回实际生效的卡片边长。
fn apply_unit(ui: &MainWindow, unit: &UnitWire) -> f32 {
    let side = f32::from(unit.side).clamp(MIN_SIDE, crate::geometry::MAX_SIDE);
    ui.set_card_side(side);
    ui.window().set_size(slint::LogicalSize::new(side, side));
    ui.set_transparency(f32::from(unit.transparency).clamp(0.0, 100.0));
    ui.set_device_name(unit.title.as_str().into());
    ui.set_device_type(unit.device_type.as_str().into());
    ui.set_device_battery(unit.battery.as_str().into());
    ui.set_device_status(unit.status.as_str().into());
    ui.set_level(unit.level.clamp(0.0, 1.0));
    side
}

/// 上报当前卡片的位置 / 尺寸 / 透明度（物理像素 + 逻辑边长）。
fn report_layout(ui: &MainWindow, anchor: &str, writer: &ChildWriter) {
    let window = ui.window();
    let scale = window.scale_factor();
    let pos = window.position();
    let x = pos.x as f32 + ui.get_card_x() * scale;
    let y = pos.y as f32 + ui.get_card_y() * scale;

    writer.send(&ChildMsg::Layout {
        v: PROTOCOL_VERSION,
        anchor: anchor.to_string(),
        x: x.round() as i32,
        y: y.round() as i32,
        side: ui.get_card_side(),
        scale,
        transparency: ui.get_transparency().round().clamp(0.0, 100.0) as u8,
    });
}

/// 当前窗口所在显示器的物理边界 `(left, top, right, bottom)`。
fn monitor_rect(ui: &MainWindow) -> Option<(i32, i32, i32, i32)> {
    use slint::winit_030::WinitWindowAccessor;

    ui.window().with_winit_window(|winit_window| {
        let monitor = winit_window.current_monitor()?;
        let pos = monitor.position();
        let size = monitor.size();
        Some((
            pos.x,
            pos.y,
            (pos.x + size.width as i32).max(pos.x + 1),
            (pos.y + size.height as i32).max(pos.y + 1),
        ))
    })?
}

/// 展开菜单：面板与卡片顶边对齐，优先摆在卡片右侧；右边放不下翻到左侧，
/// 两侧都放不下就贴着点击处，最后统一夹进显示器范围。
fn open_menu(ui: &MainWindow, px: f32) {
    let window = ui.window();
    let scale = window.scale_factor();
    let pos = window.position();
    let side = ui.get_card_side() * scale;
    let card_left = pos.x as f32 + ui.get_card_x() * scale;
    let card_top = pos.y as f32 + ui.get_card_y() * scale;

    let menu_w = MENU_W * scale;
    let menu_h = MENU_H * scale;
    let gap = MENU_GAP * scale;

    let mut menu_left = card_left + side + gap;
    let mut menu_top = card_top;
    if let Some((left, top, right, bottom)) = monitor_rect(ui) {
        let (left, top, right, bottom) = (left as f32, top as f32, right as f32, bottom as f32);
        if menu_left + menu_w > right {
            menu_left = card_left - gap - menu_w;
        }
        if menu_left < left {
            menu_left = card_left + px * scale;
        }
        menu_left = menu_left.clamp(left, (right - menu_w).max(left));
        menu_top = menu_top.clamp(top, (bottom - menu_h).max(top));
    }

    // 窗口 = 卡片 ∪ 面板；取整后所有偏移都相对同一个左上角，避免 1px 抖动。
    let win_left = card_left.min(menu_left).round();
    let win_top = card_top.min(menu_top).round();
    let win_right = (card_left + side).max(menu_left + menu_w).round();
    let win_bottom = (card_top + side).max(menu_top + menu_h).round();

    ui.set_card_x((card_left - win_left) / scale);
    ui.set_card_y((card_top - win_top) / scale);
    ui.set_menu_x((menu_left - win_left) / scale);
    ui.set_menu_y((menu_top - win_top) / scale);

    window.set_position(PhysicalPosition::new(win_left as i32, win_top as i32));
    window.set_size(slint::LogicalSize::new(
        ((win_right - win_left) / scale).max(ui.get_card_side()),
        ((win_bottom - win_top) / scale).max(ui.get_card_side()),
    ));
}

/// 从 Rust 侧收起菜单（失焦时用），与界面里点空白收起走同一套收尾。
pub(crate) fn dismiss_menu(ui: &MainWindow) {
    if !ui.get_menu_open() {
        return;
    }
    ui.set_menu_open(false);
    close_menu(ui);
}

/// 收起菜单：窗口缩回卡片大小，并挪到卡片当前所在的屏幕位置。
fn close_menu(ui: &MainWindow) {
    let window = ui.window();
    let scale = window.scale_factor();
    let pos = window.position();
    let card_left = pos.x as f32 + ui.get_card_x() * scale;
    let card_top = pos.y as f32 + ui.get_card_y() * scale;

    ui.set_card_x(0.0);
    ui.set_card_y(0.0);
    let side = ui.get_card_side();
    window.set_size(slint::LogicalSize::new(side, side));
    window.set_position(PhysicalPosition::new(
        card_left.round() as i32,
        card_top.round() as i32,
    ));
}

/// 演示单元：一笔静态数据，够把界面点亮。
fn demo_unit(anchor: &str) -> UnitWire {
    UnitWire {
        anchor: anchor.to_string(),
        title: "MCHOSE A5 Pro".into(),
        device_type: "mouse".into(),
        battery: "87%".into(),
        level: 0.87,
        status: "演示模式 · 2.4G".into(),
        offline: false,
        visible: true,
        x: None,
        y: None,
        side: 180,
        transparency: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, bool)]) -> BTreeMap<String, bool> {
        entries
            .iter()
            .map(|(anchor, visible)| ((*anchor).to_string(), *visible))
            .collect()
    }

    #[test]
    fn plan_creates_only_visible_units() {
        let actions = plan(&map(&[]), &map(&[("a", true), ("b", false)]));
        assert_eq!(actions, vec![("a".to_string(), Action::Create)]);
    }

    #[test]
    fn plan_distinguishes_show_hide_update_and_close() {
        let prev = map(&[("a", false), ("b", true), ("c", true)]);
        let next = map(&[("a", true), ("b", false), ("c", true)]);
        let actions = plan(&prev, &next);
        assert!(actions.contains(&("a".to_string(), Action::Show)));
        assert!(actions.contains(&("b".to_string(), Action::Hide)));
        assert!(actions.contains(&("c".to_string(), Action::Update)));
        assert!(!actions.iter().any(|(_, action)| *action == Action::Close));

        let closing = plan(&prev, &map(&[("a", false)]));
        assert!(closing.contains(&("b".to_string(), Action::Close)));
        assert!(closing.contains(&("c".to_string(), Action::Close)));
    }
}
