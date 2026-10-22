//! 全仓唯一时间源（D10 / Z-6）。
//!
//! 状态机退化判定、时间戳盖章、通知冷却一律使用 UTC 墙钟，且只允许在
//! **宿主**调用本函数；`voltnest-core` 通过参数接收 `now_ms`，自己从不读时钟。
//!
//! Provider 内部的巡检节奏与退避计时使用 [`std::time::Instant`]（单调），
//! 它不参与任何对外语义，因此不受本模块约束。

/// 当前 UTC 毫秒时间戳。
///
/// 系统时间早于 Unix 纪元（理论上不可能）时返回 0，而不是 panic ——
/// 启动期崩溃比时间戳为 0 更糟。
#[allow(clippy::disallowed_types)]
pub fn now_utc_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_plausible_epoch_millis() {
        let now = now_utc_ms();
        // 2020-01-01 之后、2100 年之前：只验证量级，避免测试依赖真实时间。
        assert!(now > 1_577_836_800_000, "时间戳应晚于 2020 年，实际 {now}");
        assert!(now < 4_102_444_800_000, "时间戳应早于 2100 年，实际 {now}");
    }

    #[test]
    fn is_monotonic_enough_for_stamping() {
        let a = now_utc_ms();
        let b = now_utc_ms();
        assert!(b >= a);
    }
}
