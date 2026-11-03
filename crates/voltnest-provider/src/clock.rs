//! 全仓唯一时间源：UTC 墙钟毫秒，只由宿主调用；`voltnest-core` 通过参数接收 `now_ms`，从不读时钟。

/// 当前 UTC 毫秒。系统时间早于 Unix 纪元时返回 0 而非 panic。
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
