//! HID 报文的十六进制文本表示：只做「字节 ↔ 文本」，不含任何 HID 语义。
//!
//! 生成上报数据时，用户可能需要在界面上手填一条查询报文（例如官方驱动里的
//! `06 FB 00 00 00 03`）。这里负责把这种手写文本解析成字节，并把收到的原始报告
//! 渲染成 issue 里可直接对照的十六进制串。两种写法必须**互逆**：
//! `parse(format(bytes)) == bytes`，否则用户复制出去再贴回来就对不上了。

/// 一条报文允许的最大字节数（HID 报告远小于此，纯粹防呆）。
pub const MAX_FRAME_BYTES: usize = 1024;

const DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// 字节序列 → 大写、空格分隔的十六进制串。空序列得到空串。
///
/// 用大写是与 VID/PID 及各家协议文档里的报文字节写法保持一致；
/// 分隔符统一为单个空格，便于在等宽字体下按列对齐。
pub fn format(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(3));
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0F) as usize] as char);
    }
    out
}

/// 解析失败的原因。全部渲染成中文，直接进界面。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HexError {
    /// 输入里没有任何数字。
    Empty,
    /// 十六进制位数为奇数，无法凑成整字节。
    OddDigits,
    /// 出现了非十六进制字符（去掉分隔符与 `0x` 前缀后）。
    InvalidChar(char),
    /// 超过 [`MAX_FRAME_BYTES`]。
    TooLong(usize),
}

impl core::fmt::Display for HexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "报文为空"),
            Self::OddDigits => write!(f, "十六进制位数为奇数，无法凑成整字节"),
            Self::InvalidChar(c) => write!(f, "含有非十六进制字符：{c}"),
            Self::TooLong(len) => write!(f, "报文过长（{len} 字节，上限 {MAX_FRAME_BYTES}）"),
        }
    }
}

impl std::error::Error for HexError {}

/// 手写报文 → 字节。
///
/// 宽容的分隔符（空格、制表、换行、逗号、冒号、连字符）与可选的 `0x` / `0X` 前缀，
/// 其余字符必须是十六进制数字。**不宽容奇偶性**：`0x5` 这种「半个字节」直接拒绝 ——
/// 猜成 `0x05` 会让发出去的报文与用户看到的不是同一条。
pub fn parse(text: &str) -> Result<Vec<u8>, HexError> {
    let mut digits: Vec<u8> = Vec::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        // `0x` 前缀：跳过这两个字符，字节值本身照常按位收集。
        if (c == '0') && matches!(chars.get(index + 1), Some('x' | 'X')) {
            index += 2;
            continue;
        }
        if c.is_whitespace() || matches!(c, ',' | ':' | '-' | ';' | '_') {
            index += 1;
            continue;
        }
        let Some(value) = c.to_digit(16) else {
            return Err(HexError::InvalidChar(c));
        };
        digits.push(value as u8);
        index += 1;
    }
    if digits.is_empty() {
        return Err(HexError::Empty);
    }
    if !digits.len().is_multiple_of(2) {
        return Err(HexError::OddDigits);
    }
    let len = digits.len() / 2;
    if len > MAX_FRAME_BYTES {
        return Err(HexError::TooLong(len));
    }
    Ok(digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (pair[0] << 4) | pair[1])
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_uppercase_space_separated() {
        assert_eq!(format(&[]), "");
        assert_eq!(format(&[0x00]), "00");
        assert_eq!(format(&[0x06, 0xFB, 0x0A]), "06 FB 0A");
        assert_eq!(format(&[0xFF, 0x80]), "FF 80");
    }

    #[test]
    fn parses_common_separators_and_prefixes() {
        let want = vec![0x06, 0xFB, 0x0A];
        for text in [
            "06 FB 0A",
            "06fb0a",
            "06fb 0a",
            "06,FB,0A",
            "0x06 0xFB 0x0A",
            "0X06,0xFB,0x0A",
            "06-FB-0A",
            " 06\tFB\n0A ",
            "06:FB:0A",
        ] {
            assert_eq!(parse(text).as_ref(), Ok(&want), "输入 {text:?}");
        }
    }

    #[test]
    fn format_and_parse_are_inverse() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        assert_eq!(parse(&format(&bytes)).as_deref(), Ok(bytes.as_slice()));
    }

    #[test]
    fn rejects_half_bytes_instead_of_guessing() {
        assert_eq!(parse("0x5"), Err(HexError::OddDigits));
        assert_eq!(parse("5"), Err(HexError::OddDigits));
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(parse(""), Err(HexError::Empty));
        assert_eq!(parse("   \n"), Err(HexError::Empty));
        assert_eq!(parse("06 ZZ"), Err(HexError::InvalidChar('Z')));
        // 中文/全角字符同样要指出来，不能静默吞掉。
        assert!(matches!(parse("06 号"), Err(HexError::InvalidChar(_))));
    }

    #[test]
    fn rejects_oversized_frames() {
        let text = "00 ".repeat(MAX_FRAME_BYTES + 1);
        assert_eq!(parse(&text), Err(HexError::TooLong(MAX_FRAME_BYTES + 1)));
    }

    /// 诊断文本进界面：每种失败都要说清是什么问题。
    #[test]
    fn error_display_names_the_problem() {
        assert!(HexError::Empty.to_string().contains("为空"));
        assert!(HexError::OddDigits.to_string().contains("奇数"));
        assert!(HexError::InvalidChar('Z').to_string().contains('Z'));
        assert!(HexError::TooLong(9).to_string().contains('9'));
    }
}
