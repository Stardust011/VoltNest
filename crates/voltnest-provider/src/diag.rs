//! 诊断字符串构造：把坏帧原始字节渲染为可 grep 的十六进制摘要。只做格式化，不含判定逻辑。

/// 一条诊断最多保留的原始字节数，足以覆盖报告 ID、命令字与长度字段。
pub const DIAG_BYTES: usize = 24;

/// 十六进制摘要：`06 FA 03 00 5A`，超过 `limit` 字节截断并以 ` …` 结尾；空输入返回 `(空)`。
pub fn hex_bytes(bytes: &[u8], limit: usize) -> String {
    if bytes.is_empty() {
        return "(空)".to_string();
    }
    let mut out = String::with_capacity(bytes.len().min(limit) * 3 + 4);
    for (i, b) in bytes.iter().take(limit).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{b:02X}"));
    }
    if bytes.len() > limit {
        out.push_str(" …");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_uppercase_and_space_separated() {
        assert_eq!(hex_bytes(&[0x06, 0xFA, 0x00], DIAG_BYTES), "06 FA 00");
    }

    #[test]
    fn hex_truncates_with_ellipsis() {
        let bytes: Vec<u8> = (0..40).collect();
        let text = hex_bytes(&bytes, 4);
        assert_eq!(text, "00 01 02 03 …");
    }

    #[test]
    fn empty_is_explicit() {
        assert_eq!(hex_bytes(&[], DIAG_BYTES), "(空)");
    }
}
