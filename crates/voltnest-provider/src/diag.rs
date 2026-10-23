//! 诊断字符串构造。
//!
//! 坏帧必须留下**可复核的原始字节**：只有 `malformed_frame` 这一个代码时，
//! 诊断页只能回答「这帧被拒了」，回答不了「被哪一条校验拒的、拒的是什么数据」。
//! 本模块只做格式化，不含任何判定逻辑（判定在 Provider 内，见 ADR-0010 D9）。

/// 一条诊断里最多保留的原始字节数。
///
/// HID 报告最长 65 字节；24 字节足够覆盖「报告 ID + 命令字 + 长度字段 + payload
/// 开头」，又不会让 200 条环形缓冲被单条长 hex 撑爆。
pub const DIAG_BYTES: usize = 24;

/// 十六进制摘要：`06 FA 03 00 5A`，超过 `limit` 字节时截断并以 ` …` 结尾。
///
/// 输出恒为 ASCII，写入诊断页与日志后可直接 grep。空输入返回 `(空)`：
/// 长度为 0 本身就是最有价值的信息，不能渲染成一片空白。
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
