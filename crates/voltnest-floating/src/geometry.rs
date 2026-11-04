//! 窗口几何：拖拽 / 缩放 / 菜单外扩的坐标换算，以及各处共用的尺寸常量。
//!
//! 全是纯函数，不碰 Slint 的窗口对象，便于单测（CI 的 Linux 任务没有显示环境）。

use slint::PhysicalPosition;

/// 卡片边长下限（逻辑像素）。卡面只有图标，88 仍看得清。
pub(crate) const MIN_SIDE: f32 = 88.0;
/// 卡片边长上限（逻辑像素）。
pub(crate) const MAX_SIDE: f32 = 520.0;

/// 右键菜单面板宽度（逻辑像素），必须与 `ui/floating-window.slint` 里的 `menu.width` 一致。
pub(crate) const MENU_W: f32 = 196.0;
/// 右键菜单面板高度（逻辑像素），同样必须与 `.slint` 一致。
pub(crate) const MENU_H: f32 = 180.0;
/// 面板与卡片之间的间距（逻辑像素）。
pub(crate) const MENU_GAP: f32 = 8.0;

/// 把「指针在卡片内的逻辑坐标」还原成屏幕物理坐标。
///
/// Slint 上报的是逻辑像素，而 `set_position` 用物理像素，二者差一个缩放比；
/// 卡片在窗口内还可能被菜单挤开（`offset`），也要一起加上。
pub(crate) fn screen_cursor(
    origin: PhysicalPosition,
    scale: f32,
    offset: (f32, f32),
    x: f32,
    y: f32,
) -> (f32, f32) {
    (
        origin.x as f32 + (offset.0 + x) * scale,
        origin.y as f32 + (offset.1 + y) * scale,
    )
}

/// 本帧窗口需要移动的物理像素增量。
pub(crate) fn drag_delta(prev: (f32, f32), now: (f32, f32)) -> (i32, i32) {
    (
        (now.0 - prev.0).round() as i32,
        (now.1 - prev.1).round() as i32,
    )
}

/// 缩放后的边长：按下点到当前点的位移取横纵较大者，再夹在上下限内。
pub(crate) fn resized_side(base: f32, start: (f32, f32), now: (f32, f32)) -> f32 {
    (base + (now.0 - start.0).max(now.1 - start.1)).clamp(MIN_SIDE, MAX_SIDE)
}

/// 级联默认位：显示器右下角往左上排，第 `index` 个窗口依次错开。
///
/// 只在配置里没有存过坐标时用；交给宿主算会更准，这里是子进程拿不到显示器信息时的兜底。
pub(crate) fn cascade_position(
    screen: (i32, i32, i32, i32),
    side_px: i32,
    index: i32,
) -> (i32, i32) {
    let (_, _, right, bottom) = screen;
    let step = 28 * index.max(0);
    (
        (right - side_px - 24 - step).max(screen.0),
        (bottom - side_px - 24 - step).max(screen.1),
    )
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
        let anchored = screen_cursor(origin, scale, (0.0, 0.0), 20.0, 20.0);

        assert_eq!(
            drag_delta(
                anchored,
                screen_cursor(origin, scale, (0.0, 0.0), 20.0, 20.0)
            ),
            (0, 0)
        );
    }

    /// 指针位移跨缩放比时，窗口按物理像素等比跟随。
    #[test]
    fn delta_scales_with_dpi() {
        let scale = 1.25;
        let origin = PhysicalPosition::new(10, 20);
        let prev = screen_cursor(origin, scale, (0.0, 0.0), 5.0, 5.0);
        let now = screen_cursor(origin, scale, (0.0, 0.0), 45.0, 25.0);

        assert_eq!(drag_delta(prev, now), (50, 25));
    }

    /// 连续多帧累加应等于指针总位移，不随帧数衰减。
    #[test]
    fn accumulated_delta_matches_total_travel() {
        let scale = 1.0;
        let mut origin = PhysicalPosition::new(0, 0);
        let mut prev = screen_cursor(origin, scale, (0.0, 0.0), 10.0, 10.0);
        let mut total = (0, 0);

        for _ in 0..10 {
            let now = screen_cursor(origin, scale, (0.0, 0.0), 14.0, 10.0);
            let (dx, dy) = drag_delta(prev, now);
            origin = PhysicalPosition::new(origin.x + dx, origin.y + dy);
            total = (total.0 + dx, total.1 + dy);
            prev = now;
        }

        assert_eq!(total, (40, 0));
    }

    /// 级联位从右下角往左上错开，且不会跑到屏幕外。
    #[test]
    fn cascade_steps_up_left_and_stays_inside() {
        let screen = (0, 0, 1920, 1080);
        let (x0, y0) = cascade_position(screen, 180, 0);
        let (x1, y1) = cascade_position(screen, 180, 1);
        assert_eq!((x0, y0), (1716, 876));
        assert_eq!((x1, y1), (1688, 848));

        let (x, y) = cascade_position(screen, 180, 100);
        assert!(x >= 0 && y >= 0, "不能跑到屏幕左上角之外");
    }
}
