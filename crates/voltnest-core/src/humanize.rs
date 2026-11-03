//! 时长文案。展示措辞归宿主：前端只渲染 Rust 给的字符串，不自行推算。

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;

/// 把「距今多久」渲染成用户文案：`12_000` → "刚刚"，`12` 分钟 → "12 分钟前"，`3` 小时 → "3 小时前"，`2` 天 → "2 天前"。
///
/// 入参须为宿主同一时钟算出的 UTC 差值，前端不得用 `Date.now()` 自行相减。
pub fn humanize_ago(elapsed_ms: u64) -> String {
    if elapsed_ms < MINUTE_MS {
        return "刚刚".to_string();
    }
    if elapsed_ms < HOUR_MS {
        return format!("{} 分钟前", elapsed_ms / MINUTE_MS);
    }
    if elapsed_ms < DAY_MS {
        return format!("{} 小时前", elapsed_ms / HOUR_MS);
    }
    format!("{} 天前", elapsed_ms / DAY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_one_minute_is_just_now() {
        assert_eq!(humanize_ago(0), "刚刚");
        assert_eq!(humanize_ago(59_999), "刚刚");
    }

    #[test]
    fn minutes_hours_days() {
        assert_eq!(humanize_ago(60_000), "1 分钟前");
        assert_eq!(humanize_ago(12 * MINUTE_MS), "12 分钟前");
        assert_eq!(humanize_ago(59 * MINUTE_MS), "59 分钟前");
        assert_eq!(humanize_ago(HOUR_MS), "1 小时前");
        assert_eq!(humanize_ago(3 * HOUR_MS + 59 * MINUTE_MS), "3 小时前");
        assert_eq!(humanize_ago(DAY_MS), "1 天前");
        assert_eq!(humanize_ago(5 * DAY_MS + 7 * HOUR_MS), "5 天前");
    }

    #[test]
    fn saturating_large_values_do_not_overflow() {
        let text = humanize_ago(u64::MAX);
        assert!(text.ends_with("天前"), "{text}");
    }
}
