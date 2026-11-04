//! 桌面悬浮窗：始终置顶、无边框、圆角正方形的毛玻璃卡片，可拖拽移动、可拖右下角缩放。
//!
//! 运行：`cargo run -p voltnest-floating`。
//! 窗口的置顶 / 无边框 / 透明 / 玻璃外观 / 圆角命中判定 / 右键菜单全部由
//! `ui/floating-window.slint` 声明，本文件只负责拖拽与缩放的坐标换算、数据绑定。
//!
//! 关于「背景模糊」：Windows 11 的 DWM Acrylic（`DWMWA_SYSTEMBACKDROP_TYPE`）会铺满
//! 整个窗口矩形且不理会逐像素 alpha，圆角处会露出方块状的模糊底，与卡片的圆角外形
//! 冲突。因此这里改用渲染层实现的玻璃质感：半透明渐变 + 顶缘高光 + 1px 细边，
//! 不依赖系统版本，也不额外占用窗口边距。

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod logger;

use slint::{ComponentHandle, PhysicalPosition, Timer, TimerMode};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Slint 由 `.slint` 文件生成的全部类型（`MainWindow`、回调 setter 等）。
#[allow(missing_docs)]
mod ui {
    slint::include_modules!();
}

use ui::MainWindow;

fn main() {
    logger::init();
    let ui = MainWindow::new().expect("创建悬浮窗失败");
    ui.window()
        .set_size(slint::LogicalSize::new(DEFAULT_SIDE, DEFAULT_SIDE));

    ui.show().expect("显示悬浮窗失败");

    install_drag(&ui);
    install_resize(&ui);
    let _ticker = install_demo_data(&ui);

    log::info!("VoltNest 悬浮窗已启动");
    slint::run_event_loop().expect("事件循环异常退出");
}

/// 拖拽：以「指针在屏幕上的物理坐标」为不变量逐帧累加位移。
///
/// 不能用 `按下点 → 当前点` 的窗口内相对位移：窗口跟着鼠标移动时该相对坐标几乎不变，
/// 会把整段位移吃掉。这里每帧都用 `窗口位置 + 指针相对位置` 还原屏幕坐标，再与上一帧
/// 求差移动窗口，因此天然自纠正，不会漂移。
fn install_drag(ui: &MainWindow) {
    let last_cursor: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));

    let anchor_begin = last_cursor.clone();
    ui.on_drag_begin(move || anchor_begin.set(None));

    let anchor_end = last_cursor.clone();
    ui.on_drag_end(move || anchor_end.set(None));

    let anchor_move = last_cursor;
    let ui_weak = ui.as_weak();
    ui.on_drag_move(move |x: f32, y: f32| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let window = ui.window();
        let origin = window.position();
        let cursor = screen_cursor(origin, window.scale_factor(), x, y);

        if let Some(prev) = anchor_move.get() {
            let (dx, dy) = drag_delta(prev, cursor);
            window.set_position(PhysicalPosition::new(origin.x + dx, origin.y + dy));
        }
        anchor_move.set(Some(cursor));
    });

    // 菜单项的回调是在原生菜单的消息处理里触发的，那时 Slint 内部状态仍被借用，
    // 直接 hide / quit 会 panic（RefCell already borrowed），因此推迟到下一轮事件循环。
    ui.on_close_requested(|| {
        Timer::single_shot(Duration::ZERO, || {
            slint::quit_event_loop().expect("退出事件循环失败");
        });
    });
}

/// 把「指针在窗口内的逻辑坐标」还原成屏幕物理坐标。
///
/// Slint 上报的是逻辑像素，而 `set_position` 用物理像素，二者差一个缩放比。
fn screen_cursor(origin: PhysicalPosition, scale: f32, x: f32, y: f32) -> (f32, f32) {
    (origin.x as f32 + x * scale, origin.y as f32 + y * scale)
}

/// 卡片初始边长（逻辑像素）。
const DEFAULT_SIDE: f32 = 180.0;
/// 缩放边长下限 / 上限（逻辑像素）。下限放到 88，卡面只有图标，小尺寸仍看得清。
const MIN_SIDE: f32 = 88.0;
const MAX_SIDE: f32 = 520.0;

/// 右下角缩放：窗口左上角固定，因此「指针相对窗口左上角的位移」直接等于尺寸增量。
///
/// 卡片是圆角正方形，缩放时保持长宽相等：取横纵位移中较大的一边作为边长增量，
/// 拖右、拖下或斜拖都能按直觉放大。
fn install_resize(ui: &MainWindow) {
    let base = Rc::new(Cell::new(MIN_SIDE));
    let anchor: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));

    let anchor_begin = anchor.clone();
    ui.on_resize_begin(move || anchor_begin.set(None));

    let anchor_end = anchor.clone();
    ui.on_resize_end(move || anchor_end.set(None));

    let anchor_move = anchor;
    let base_move = base.clone();
    let ui_weak = ui.as_weak();
    ui.on_resize_move(move |x: f32, y: f32| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let window = ui.window();
        let size = window.size();
        let scale = window.scale_factor();

        // 第一次移动时记下基准尺寸与按下点；之后都以它们为原点计算总增量。
        if anchor_move.get().is_none() {
            base_move.set(size.width as f32 / scale);
        }
        let start = anchor_move.get().unwrap_or((x, y));
        anchor_move.set(Some(start));

        let side = resized_side(base_move.get(), start, (x, y));
        window.set_size(slint::LogicalSize::new(side, side));
    });
}

/// 缩放后的边长：按下点到当前点的位移取横纵较大者，再夹在上下限内。
fn resized_side(base: f32, start: (f32, f32), now: (f32, f32)) -> f32 {
    (base + (now.0 - start.0).max(now.1 - start.1)).clamp(MIN_SIDE, MAX_SIDE)
}

/// 本帧窗口需要移动的物理像素增量。
fn drag_delta(prev: (f32, f32), now: (f32, f32)) -> (i32, i32) {
    (
        (now.0 - prev.0).round() as i32,
        (now.1 - prev.1).round() as i32,
    )
}

/// 演示数据：接入 `voltnest-core` 前先用一组静态值把界面点亮，
/// 顺带用 1s 心跳刷新进度环与右键菜单里的电量，验证属性绑定是活的。
///
/// 返回的 `Timer` 必须由调用方持有到事件循环结束，否则定时器会被立刻析构。
fn install_demo_data(ui: &MainWindow) -> Timer {
    let timer = Timer::default();
    let ui_weak = ui.as_weak();
    let mut tick = 0u32;

    timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        let Some(ui) = ui_weak.upgrade() else {
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

    timer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缩放按横纵位移中较大的一边计算边长，按住右下角斜拖最直观。
    #[test]
    fn resize_takes_the_larger_axis() {
        assert_eq!(resized_side(180.0, (170.0, 170.0), (250.0, 240.0)), 260.0);
        assert_eq!(resized_side(180.0, (170.0, 170.0), (200.0, 300.0)), 310.0);
    }

    /// 边长被夹在上下限之间，向左上拖不会小于下限。
    #[test]
    fn resize_is_clamped() {
        assert_eq!(resized_side(180.0, (170.0, 170.0), (0.0, 0.0)), MIN_SIDE);
        assert_eq!(resized_side(500.0, (0.0, 0.0), (900.0, 900.0)), MAX_SIDE);
    }

    /// 窗口已经跟到指针位置时，相对坐标不变，增量必须为 0（否则会抖或漂移）。
    #[test]
    fn window_already_following_yields_no_movement() {
        let scale = 1.25;
        let origin = PhysicalPosition::new(100, 100);
        let anchored = screen_cursor(origin, scale, 20.0, 20.0);

        assert_eq!(
            drag_delta(anchored, screen_cursor(origin, scale, 20.0, 20.0)),
            (0, 0)
        );
    }

    /// 指针位移跨缩放比时，窗口按物理像素等比跟随。
    #[test]
    fn delta_scales_with_dpi() {
        let scale = 1.25;
        let origin = PhysicalPosition::new(10, 20);
        let prev = screen_cursor(origin, scale, 5.0, 5.0);
        let now = screen_cursor(origin, scale, 45.0, 25.0);

        assert_eq!(drag_delta(prev, now), (50, 25));
    }

    /// 连续多帧累加应等于指针总位移，不随帧数衰减。
    ///
    /// 模拟真实情形：指针每帧右移 4px，窗口跟上后其相对坐标稳定在 14（按下点 10 + 首帧 4）。
    #[test]
    fn accumulated_delta_matches_total_travel() {
        let scale = 1.0;
        let mut origin = PhysicalPosition::new(0, 0);
        let mut prev = screen_cursor(origin, scale, 10.0, 10.0);
        let mut total = (0, 0);

        for _ in 0..10 {
            let now = screen_cursor(origin, scale, 14.0, 10.0);
            let (dx, dy) = drag_delta(prev, now);
            origin = PhysicalPosition::new(origin.x + dx, origin.y + dy);
            total = (total.0 + dx, total.1 + dy);
            prev = now;
        }

        assert_eq!(total, (40, 0));
    }
}
