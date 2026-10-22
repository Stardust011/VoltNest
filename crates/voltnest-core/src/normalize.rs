//! `modelKey` 归一化：宿主侧唯一实现点（ADR-0010 D4 / W-2）。
//!
//! 归一化规则一旦在 Provider 与宿主各写一遍，HID 与 BLE 的细节差异会让分组谓词
//! **静默失效**（不报错，只是永远不合并）。因此本函数是唯一实现，Provider 只上报
//! `model_name_raw`。
//!
//! 规则：去 NUL → 折叠连续空白 → 去首尾空白 → 小写。空串返回 `None`
//! （`None` 会让分组谓词自然失败 → 退回两条独立记录）。

/// 把设备原始型号名归一化为 `modelKey`。
///
/// ```text
/// "AJ159 NL\0\0"   -> Some("aj159 nl")
/// " aj159   nl "   -> Some("aj159 nl")
/// "\0\0"           -> None
/// ```
///
/// **禁止前缀匹配**：`6425`/`6426`、`NL`/`NL P` 只差一位，这里只做严格相等所需的归一化，
/// 不做任何模糊化处理。
pub fn normalize(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    for ch in raw.chars() {
        if ch == '\0' {
            continue;
        }
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        for lower in ch.to_lowercase() {
            out.push(lower);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_nul_and_padding() {
        // 实测 BLE GAP 0x2A00 为定长 32 字节，尾部补零。
        assert_eq!(normalize("AJ159 NL\0\0\0\0").as_deref(), Some("aj159 nl"));
    }

    #[test]
    fn collapses_whitespace_and_trims() {
        assert_eq!(normalize("  AJ159   NL  ").as_deref(), Some("aj159 nl"));
        assert_eq!(normalize("AJ159\tNL").as_deref(), Some("aj159 nl"));
        assert_eq!(normalize("AJ159\nNL").as_deref(), Some("aj159 nl"));
    }

    #[test]
    fn empty_or_blank_is_none() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("\0\0\0"), None);
        assert_eq!(normalize("   "), None);
    }

    #[test]
    fn is_case_insensitive_but_exact_otherwise() {
        assert_eq!(normalize("aj159 nl"), normalize("AJ159 NL"));
        // 只差一位的型号不得归一化到一起（禁止前缀匹配）。
        assert_ne!(normalize("AJ159 NL P"), normalize("AJ159 NL"));
        assert_eq!(normalize("AJ159 NL P").as_deref(), Some("aj159 nl p"));
    }

    #[test]
    fn handles_non_ascii_without_panicking() {
        assert_eq!(normalize("雷霆  鼠标").as_deref(), Some("雷霆 鼠标"));
    }
}
