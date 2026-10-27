//! 品牌注册表：`hid-vbas` 家族下的所有已知品牌。
//!
//! **新增品牌 = 新增一个模块 + 在 [`BRANDS`] 里加一项**，运行时只按传输类型分派：
//! 1. `src/brands/<brand>.rs` 声明 BrandSpec；2. `data/mappings/<brand>.json` 型号表快照；3. `BRANDS` 加一行（唯一要改的既有代码）。
//!
//! 注册表在首次枚举前建立一次（[`Registry::embedded`]），建立时逐个跑
//! [`BrandSpec::validate`]，写错字段下标即 panic —— 早于任何设备 I/O。

pub mod ajazz;
pub mod dareu;
pub mod mchose;
pub mod rapoo;

/// 厂商私有校验和（品牌模块专用，不对外暴露）。
pub(crate) mod checksum;

use crate::spec::{BrandSpec, SpecError};
use crate::table::{DeviceTable, Entry};
use std::collections::HashSet;
use std::sync::OnceLock;
use voltnest_provider::ProviderProfile;

/// 全部已知品牌（编译期常量数组）。顺序无关匹配结果：同一 VID:PID 被两品牌同时登记属配置错误，[`Registry::embedded`] 会 panic。
pub static BRANDS: &[&BrandSpec] = &[&ajazz::SPEC, &rapoo::SPEC, &dareu::SPEC, &mchose::SPEC];

/// 一个品牌：协议描述 + 已解析的型号表。
#[derive(Debug)]
pub struct Brand {
    /// 协议描述。
    pub spec: &'static BrandSpec,
    /// 型号表。
    pub table: DeviceTable,
}

/// 品牌注册表（进程内唯一实例）。
#[derive(Debug)]
pub struct Registry {
    brands: Vec<Brand>,
}

impl Registry {
    /// 建立注册表：校验每份品牌描述与型号表。panic 可接受——这些是编译期常量，失败属发版事故，须在第一枚举前暴露，而非运行几小时后解析出错电量。
    pub fn embedded() -> &'static Self {
        static REGISTRY: OnceLock<Registry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            Self::build(BRANDS).expect("内置品牌注册表非法（编译期数据，失败即发版事故）")
        })
    }

    /// 用给定品牌列表建立注册表（测试注入自定义品牌时使用）。
    ///
    /// 除顶层描述外，每个品牌的 [`BrandSpec::variants`] 也会逐份校验：变体只在 `route` 里用到，
    /// 少写一份单测就没人管它的下标越界 —— 而这类错误的表现是「读不出电量」，
    /// 比启动失败难查一个数量级。
    pub fn build(specs: &'static [&'static BrandSpec]) -> Result<Self, SpecError> {
        let mut brands = Vec::with_capacity(specs.len());
        let mut seen_ids: HashSet<&'static str> = HashSet::new();
        for spec in specs {
            spec.validate()?;
            if !seen_ids.insert(spec.id) {
                return Err(SpecError::DuplicateBrandId { brand: spec.id });
            }
            for variant in spec.variants {
                variant.validate()?;
                // 品牌标识不参与匹配，但它是日志与诊断的唯一线索：重复即分不清是哪套协议出的问题。
                if !seen_ids.insert(variant.id) {
                    return Err(SpecError::DuplicateBrandId { brand: variant.id });
                }
            }
            let table = DeviceTable::parse(spec.device_table_json).map_err(|detail| {
                SpecError::DeviceTable {
                    brand: spec.id,
                    detail,
                }
            })?;
            // 精确匹配必须唯一：同一 VID:PID 落到两品牌时型号名与字段位置会随枚举顺序漂移，属配置错误，不能拖到运行期。
            for other in &brands {
                let other: &Brand = other;
                for ((vid, pid), _) in other.table.entries() {
                    if table.lookup(vid, pid).is_some() {
                        return Err(SpecError::DuplicateDeviceKey {
                            vid,
                            pid,
                            first: other.spec.id,
                            second: spec.id,
                        });
                    }
                }
            }
            brands.push(Brand { spec, table });
        }
        Ok(Self { brands })
    }

    /// 全部品牌（诊断页 / 冲突提示用）。
    pub fn brands(&self) -> &[Brand] {
        &self.brands
    }

    /// 跨品牌精确查表。
    pub fn lookup(&self, vid: u16, pid: u16) -> Option<(&Brand, &Entry)> {
        self.brands
            .iter()
            .find_map(|b| b.table.lookup(vid, pid).map(|e| (b, e)))
    }

    /// 全部品牌的节奏都必须落在家族声明的预算内（含每个品牌的变体协议）。
    pub fn validate_against(&self, profile: &ProviderProfile) -> Result<(), SpecError> {
        for b in &self.brands {
            b.spec.validate_against(profile)?;
            for variant in b.spec.variants {
                variant.validate_against(profile)?;
            }
        }
        Ok(())
    }
}

/// 品牌模块单测的共享构造器（`route` 分派只读型号表附加字段，故只需造出这些字段）。
#[cfg(test)]
pub(crate) mod test_support {
    use crate::table::Entry;
    use std::collections::HashMap;

    /// 造一条只带 `key = value` 附加字段的型号表条目；`value` 为 `None` 时字段缺失。
    pub(crate) fn entry_with(key: &str, value: Option<serde_json::Value>) -> Entry {
        let mut extra = HashMap::new();
        if let Some(value) = value {
            extra.insert(key.to_string(), value);
        }
        Entry {
            name: None,
            vendor_device_id: None,
            device_type: String::new(),
            mode: String::new(),
            extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnest_provider::{BLE_BAS, HID_VBAS};

    #[test]
    fn embedded_registry_is_consistent() {
        let reg = Registry::embedded();
        assert_eq!(reg.brands().len(), BRANDS.len());
        for b in reg.brands() {
            assert!(b.spec.validate().is_ok(), "{}", b.spec.id);
            assert!(!b.table.is_empty());
        }
    }

    #[test]
    fn every_brand_fits_the_family_profile() {
        assert!(Registry::embedded().validate_against(&HID_VBAS).is_ok());
        assert!(Registry::embedded().validate_against(&BLE_BAS).is_err());
    }

    #[test]
    fn lookup_spans_brands() {
        let reg = Registry::embedded();
        let (brand, entry) = reg.lookup(0x7768, 0x6426).expect("已验证设备必须在表内");
        assert_eq!(brand.spec.id, "ajazz");
        assert_eq!(entry.name.as_deref(), Some("AJ159 NL"));
        assert_eq!(entry.mode, "2.4G");
        assert_eq!(entry.vendor_device_id, Some(102));
        assert_eq!(entry.device_type, "mouse");
        assert!(
            reg.lookup(0x249A, 0x5C2F).is_none(),
            "不同代际 VID 不得复用"
        );
        assert_eq!(reg.lookup(0x24AE, 0x1410).unwrap().0.spec.id, "rapoo");
        assert_eq!(reg.lookup(0x260D, 0x1117).unwrap().0.spec.id, "dareu");
        assert_eq!(reg.lookup(0x3837, 0x100B).unwrap().0.spec.id, "mchose");
        assert_eq!(reg.lookup(0x5253, 0x1020).unwrap().0.spec.id, "mchose");
        assert!(
            reg.lookup(0x2023, 0xF013).is_none(),
            "旧 A5 段已从厂商表移除"
        );
    }

    #[test]
    fn build_rejects_broken_spec() {
        static BROKEN: BrandSpec = BrandSpec {
            id: "broken",
            label: "Broken",
            usage_page: 0xFFB4,
            usage_page_alt: None,
            usage: None,
            usage_page_write: None,
            reassembly: None,
            transport: crate::spec::ReplyTransport::Listen,
            reply_inverted: false,
            clamp_percent: false,
            frame: crate::spec::FrameLayout {
                report_id: None,
                cmd_index: 1,
                len_index: None,
                // 头体重叠：cmd_index 落在 payload 内。
                payload_offset: 1,
                fixed_payload_len: 4,
            },
            battery: crate::spec::BatteryFields {
                cmd: Some(0x01),
                subtag: None,
                percent_index: 0,
                charging: None,
            },
            listen: None,
            request: None,
            ack: None,
            fallback_report_id: None,
            expected_interval_ms: 30_000,
            device_table_json: ajazz::SPEC.device_table_json,
            variants: &[],
            route: None,
        };
        static LIST: [&BrandSpec; 1] = [&BROKEN];
        assert!(Registry::build(&LIST).is_err());
    }

    /// 变体协议也必须在启动前逐份校验：越界下标若不在这里拦，就会变成「读不出电量」。
    #[test]
    fn build_rejects_broken_variant() {
        static BAD_VARIANT: BrandSpec = BrandSpec {
            id: "bad-variant",
            label: "坏变体",
            // 头体重叠：命令字落在 payload 内（payload 从 cmd_index 处开始）。
            frame: crate::spec::FrameLayout {
                payload_offset: 1,
                ..ajazz::SPEC.frame
            },
            ..ajazz::SPEC
        };
        static WITH_BAD: BrandSpec = BrandSpec {
            id: "with-bad-variant",
            label: "带坏变体的品牌",
            variants: &[&BAD_VARIANT],
            ..ajazz::SPEC
        };
        static LIST: [&BrandSpec; 1] = [&WITH_BAD];
        assert_eq!(
            Registry::build(&LIST).err(),
            Some(SpecError::HeaderOverlap {
                brand: "bad-variant",
                cmd_index: 1,
                payload_offset: 1,
            })
        );
    }

    /// 变体 id 与品牌标识共用一个命名空间：重名等于日志里分不清哪套协议出事。
    #[test]
    fn build_rejects_duplicate_variant_id() {
        // 变体 id 与自己的品牌标识重名。
        static CLONE: BrandSpec = BrandSpec {
            id: "dup-brand",
            label: "同名变体",
            ..ajazz::SPEC
        };
        static WITH_DUP: BrandSpec = BrandSpec {
            id: "dup-brand",
            label: "重名品牌",
            variants: &[&CLONE],
            ..ajazz::SPEC
        };
        static LIST: [&BrandSpec; 1] = [&WITH_DUP];
        assert_eq!(
            Registry::build(&LIST).err(),
            Some(SpecError::DuplicateBrandId { brand: "dup-brand" })
        );
    }

    /// 顶层 4 份 + 变体 12 份全部进注册表：新增变体必须登记，否则启动校验覆盖不到它。
    #[test]
    fn every_variant_is_registered_and_valid() {
        let reg = Registry::embedded();
        let total: usize = reg.brands().iter().map(|b| 1 + b.spec.variants.len()).sum();
        assert_eq!(total, 16, "4 顶层 + 3 达尔优 + 5 迈从 + 4 雷柏");
        for b in reg.brands() {
            for variant in b.spec.variants {
                assert!(variant.validate().is_ok(), "{} 非法", variant.id);
                assert_ne!(variant.id, b.spec.id, "变体不得与品牌标识重名");
            }
        }
        assert!(reg.validate_against(&HID_VBAS).is_ok());
    }
}
