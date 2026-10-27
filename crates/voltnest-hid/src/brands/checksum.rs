//! 厂商报文校验和：**品牌私有算法，只供品牌模块使用**。
//!
//! 通用层（[`crate::spec`] / [`crate::frame`] / [`crate::runtime`]）不认识这些算法 —— 校验和在
//! `const` 上下文里算进 [`crate::spec::RequestTemplate::payload`]，运行时只当作普通字节发出去。
//! 这样既把「手算十六进制」从协议描述里清掉（载荷一改，校验和自动跟随），又不让通用层长出厂商形状。
//!
//! 三家的算法确实互不通用，故这里是三个具名函数而不是一个带参数的「通用校验和」：
//!
//! | 品牌 | 官方实现 | 覆盖范围 | 落点 |
//! |---|---|---|---|
//! | 匠盟（达尔优） | `0x55 - Σ` | 含报告 ID 与命令字 | 报文末字节 |
//! | 迈从（键盘 2.4G） | `Σ & 0xFF` | 含报告 ID 与命令字 | 报文末字节 |
//! | 维盛 AP（雷柏） | `Σ`（u16 回卷） | 命令字与 LEN，不含报告 ID | 报告 ID 之后两字节（小端） |

/// 逐字节求和取低 8 位（迈从键盘 2.4G 的 `tJe`；**入参必须含报告 ID**，官方就是这么算的）。
pub(crate) const fn sum8(bytes: &[u8]) -> u8 {
    let mut sum = 0u8;
    let mut i = 0;
    while i < bytes.len() {
        sum = sum.wrapping_add(bytes[i]);
        i += 1;
    }
    sum
}

/// `0x55 - Σ(bytes)` 的低 8 位（匠盟 `CheckSum`；入参含报告 ID + 命令字）。
pub(crate) const fn neg_sum8(bytes: &[u8]) -> u8 {
    0x55u8.wrapping_sub(sum8(bytes))
}

/// 逐字节 u16 累加（回卷），返回小端两字节（维盛 AP `WL`；入参**不含**报告 ID 与校验和本身）。
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
        // 匠盟：0x55 - (0x08 报告 ID + 0x04 命令) = 0x49（`jmdevice.js:1449`）。
        assert_eq!(neg_sum8(&[0x08, 0x04]), 0x49);
        // 迈从键盘 2.4G：0x13 + 0x4A + 0x01（`tJe` 含报告 ID）。
        assert_eq!(sum8(&[0x13, 0x4A, 0x01]), 0x5E);
        // 维盛 AP：0xAD 命令 + LEN 6 = 0x00B3，小端落成 `B3 00`。
        assert_eq!(sum16_le(&[0xAD, 0x06]), [0xB3, 0x00]);
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
