//! 厂商报文校验和：品牌私有算法，只供品牌模块使用。
//!
//! 校验和都在 `const` 上下文里算进 [`crate::spec::RequestTemplate::payload`]，运行时按普通字节发出，
//! 通用层不认识这些算法。各家算法互不通用，故按品牌各给一个具名函数：
//!
//! | 品牌 | 算法 | 覆盖范围 | 落点 |
//! |---|---|---|---|
//! | 匠盟（达尔优） | `0x55 - Σ` | 含报告 ID 与命令字 | 报文末字节 |
//! | 迈从（键盘 2.4G） | `Σ & 0xFF` | 含报告 ID 与命令字 | 报文末字节 |
//! | 维盛 AP（雷柏） | `Σ`（u16 回卷） | 命令字与 LEN，不含报告 ID | 报告 ID 之后两字节（小端） |
//! | 迈从（键盘 CZ/QHW） | `⊕` | 从 `flags` 起（不含报告 ID 与 `version`） | 报文末字节 |

/// 逐字节求和取低 8 位（迈从键盘 2.4G；入参须含报告 ID）。
pub(crate) const fn sum8(bytes: &[u8]) -> u8 {
    let mut sum = 0u8;
    let mut i = 0;
    while i < bytes.len() {
        sum = sum.wrapping_add(bytes[i]);
        i += 1;
    }
    sum
}

/// `0x55 - Σ(bytes)` 的低 8 位（匠盟；入参含报告 ID 与命令字）。
pub(crate) const fn neg_sum8(bytes: &[u8]) -> u8 {
    0x55u8.wrapping_sub(sum8(bytes))
}

/// 逐字节异或（迈从 CZ/QHW 族帧尾校验；入参从 `flags` 起，不含报告 ID 与 `version`）。
pub(crate) const fn xor8(bytes: &[u8]) -> u8 {
    let mut acc = 0u8;
    let mut i = 0;
    while i < bytes.len() {
        acc ^= bytes[i];
        i += 1;
    }
    acc
}

/// 逐字节 u16 累加（回卷），返回小端两字节（维盛 AP；入参不含报告 ID 与校验和本身）。
pub(crate) const fn sum16_le(bytes: &[u8]) -> [u8; 2] {
    let mut sum = 0u16;
    let mut i = 0;
    while i < bytes.len() {
        sum = sum.wrapping_add(bytes[i] as u16);
        i += 1;
    }
    sum.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_the_values_the_official_drivers_send() {
        assert_eq!(neg_sum8(&[0x08, 0x04]), 0x49);
        assert_eq!(sum8(&[0x13, 0x4A, 0x01]), 0x5E);
        assert_eq!(sum16_le(&[0xAD, 0x06]), [0xB3, 0x00]);
        assert_eq!(xor8(&[0x03, 0x00, 0x04, 0x09, 0x00, 0x00]), 0x0E);
    }

    #[test]
    fn wraps_instead_of_panicking() {
        assert_eq!(sum8(&[0xFF, 0x02]), 0x01);
        assert_eq!(neg_sum8(&[0x56]), 0xFF);
        assert_eq!(
            sum16_le(&[0xFF, 0xFF, 0x01]),
            [0xFF, 0x01],
            "0x01FF 的低字节在前"
        );
        assert_eq!(sum8(&[]), 0);
    }
}
