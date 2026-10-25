//! 品牌注册表：`hid-vbas` 家族下的所有已知品牌。
//!
//! **新增品牌 = 新增一个模块 + 在 [`BRANDS`] 里加一项**，运行时只按传输类型分派：
//! 1. `src/brands/<brand>.rs` 声明 BrandSpec；2. `data/vbas/<brand>.json` 型号表快照；3. `BRANDS` 加一行（唯一要改的既有代码）。
//!
//! 注册表在首次枚举前建立一次（[`Registry::embedded`]），建立时逐个跑
//! [`BrandSpec::validate`]，写错字段下标即 panic —— 早于任何设备 I/O。

pub mod ajazz;
pub mod dareu;
pub mod mchose;
pub mod rapoo;

use crate::spec::{BrandSpec, SpecError};
use crate::table::{DeviceTable, Entry};
use std::sync::OnceLock;
use voltnest_provider::ProviderProfile;

/// 全部已知品牌（编译期常量数组）。顺序无关匹配结果：同一 VID:PID 被两品牌同时登记属配置错误，[`Registry::embedded`] 会 panic。
pub static BRANDS: &[&BrandSpec] = &[
    &ajazz::SPEC,
    &rapoo::SPEC,
    &dareu::SPEC,
    &mchose::A7_SPEC,
    &mchose::M7_SPEC,
    &mchose::A5_SPEC,
];

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
    pub fn build(specs: &'static [&'static BrandSpec]) -> Result<Self, SpecError> {
        let mut brands = Vec::with_capacity(specs.len());
        for spec in specs {
            spec.validate()?;
            let table = DeviceTable::parse(spec.device_table_json).map_err(|detail| {
                SpecError::DeviceTable {
                    brand: spec.id,
                    detail,
                }
            })?;
            if brands.iter().any(|b: &Brand| b.spec.id == spec.id) {
                return Err(SpecError::DuplicateBrandId { brand: spec.id });
            }
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

    /// 全部品牌的节奏都必须落在家族声明的预算内。
    pub fn validate_against(&self, profile: &ProviderProfile) -> Result<(), SpecError> {
        for b in &self.brands {
            b.spec.validate_against(profile)?;
        }
        Ok(())
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
        assert_eq!(entry.name, "AJ159 NL");
        assert_eq!(entry.mode, "2.4G");
        assert_eq!(entry.interface_index(), Some(1));
        assert!(
            reg.lookup(0x249A, 0x5C2F).is_none(),
            "不同代际 VID 不得复用"
        );
        assert_eq!(reg.lookup(0x24AE, 0x1205).unwrap().0.spec.id, "rapoo");
        assert_eq!(reg.lookup(0x260D, 0x1114).unwrap().0.spec.id, "dareu");
        assert_eq!(reg.lookup(0x3837, 0x100B).unwrap().0.spec.id, "mchose-a7");
        assert_eq!(reg.lookup(0x5253, 0x1020).unwrap().0.spec.id, "mchose-m7");
        assert_eq!(reg.lookup(0x2023, 0xF013).unwrap().0.spec.id, "mchose-a5");
    }

    #[test]
    fn build_rejects_broken_spec() {
        static BROKEN: BrandSpec = BrandSpec {
            id: "broken",
            label: "Broken",
            usage_page: 0xFFB4,
            usage_page_write: None,
            transport: crate::spec::ReplyTransport::Push,
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
                cmd: 0x01,
                subtag: None,
                percent_index: 0,
                charging: None,
            },
            request: None,
            expected_interval_ms: 30_000,
            device_table_json: ajazz::SPEC.device_table_json,
        };
        static LIST: [&BrandSpec; 1] = [&BROKEN];
        assert!(Registry::build(&LIST).is_err());
    }
}
