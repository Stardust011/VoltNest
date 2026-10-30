//! 品牌注册表：`hid-vbas` 家族下的所有已知品牌。
//!
//! **新增品牌 = 新增一个模块 + 在 [`BRANDS`] 里加一项**，运行时只按传输类型分派：
//! 1. `src/brands/<brand>.rs` 声明 BrandSpec；2. `data/mappings/<brand>.json` 型号表；3. `BRANDS` 加一行（唯一要改的既有代码）。
//!
//! 型号表不再是编译期常量：注册表在首次枚举前建立一次（[`Registry::embedded`]），建立时
//! 从型号表目录**运行时加载**全部 `*.json`（定位规则见 [`crate::mappings`]），逐个跑
//! [`BrandSpec::validate`]，写错字段下标即 panic —— 早于任何设备 I/O。
//!
//! 型号表缺失 / 损坏按**品牌粒度跳过**：该品牌不参与匹配，但跳过项留在 [`Registry::skipped`]
//! 并在 [`Registry::try_embedded`] 里打一条诊断，其余品牌照常采集。只有「一个品牌都不可用」才
//! 升级为错误（[`RegistryError::NoUsableBrand`]）。

pub mod ajazz;
pub mod dareu;
pub mod mchose;
pub mod rapoo;

/// 厂商私有校验和（品牌模块专用，不对外暴露）。
pub(crate) mod checksum;

use crate::mappings::{MappingError, MappingSet};
use crate::spec::{BrandSpec, SpecError};
use crate::table::{DeviceTable, Entry};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use voltnest_provider::ProviderProfile;

/// 全部已知品牌（编译期常量数组）。顺序无关匹配结果：同一 VID:PID 被两品牌同时登记属配置错误，[`Registry::embedded`] 会 panic。
pub static BRANDS: &[&BrandSpec] = &[&ajazz::SPEC, &rapoo::SPEC, &dareu::SPEC, &mchose::SPEC];

/// 一个品牌：协议描述 + 已加载的型号表。
#[derive(Debug)]
pub struct Brand {
    /// 协议描述。
    pub spec: &'static BrandSpec,
    /// 型号表。
    pub table: DeviceTable,
}

/// 被跳过的品牌：型号表缺失 / 损坏，因而本次运行不参与匹配。
///
/// 跳过是**允许**的（一份坏表不该让其它品牌的设备全消失），但绝不静默：
/// 每一项都留在 [`Registry::skipped`] 里，由宿主日志 / 诊断页展示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedBrand {
    /// 品牌标识（[`BrandSpec::id`]）。
    pub brand: &'static str,
    /// 品牌声明的型号表文件名。
    pub file: &'static str,
    /// 跳过原因。
    pub error: MappingError,
}

impl fmt::Display for SkippedBrand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "品牌 {} 已跳过（型号表 {}）：{}",
            self.brand, self.file, self.error
        )
    }
}

/// 注册表建立失败：协议描述非法、型号表目录不可用，或**所有**品牌都被跳过。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// 品牌协议描述非法。
    Spec(SpecError),
    /// 型号表目录无法定位 / 读取。
    Mappings(MappingError),
    /// 每个品牌的型号表都不可用：一个能服务的品牌都没有，属部署事故而非局部损坏。
    NoUsableBrand {
        /// 被跳过的品牌（按声明顺序）。
        skipped: Vec<SkippedBrand>,
    },
}

impl RegistryError {
    /// 取出协议描述类错误（其它情况为 `None`）。
    pub fn spec(self) -> Option<SpecError> {
        match self {
            Self::Spec(e) => Some(e),
            Self::Mappings(_) | Self::NoUsableBrand { .. } => None,
        }
    }

    /// 取出型号表目录类错误（其它情况为 `None`）。
    pub fn mappings(self) -> Option<MappingError> {
        match self {
            Self::Mappings(e) => Some(e),
            Self::Spec(_) | Self::NoUsableBrand { .. } => None,
        }
    }

    /// 取出被跳过的品牌列表（仅 [`RegistryError::NoUsableBrand`] 有值）。
    pub fn skipped(self) -> Option<Vec<SkippedBrand>> {
        match self {
            Self::NoUsableBrand { skipped } => Some(skipped),
            Self::Spec(_) | Self::Mappings(_) => None,
        }
    }
}

impl From<SpecError> for RegistryError {
    fn from(err: SpecError) -> Self {
        Self::Spec(err)
    }
}

impl From<MappingError> for RegistryError {
    fn from(err: MappingError) -> Self {
        Self::Mappings(err)
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(err) => write!(f, "{err}"),
            Self::Mappings(err) => write!(f, "{err}"),
            Self::NoUsableBrand { skipped } => {
                write!(f, "没有任何可用品牌（{} 个品牌全部跳过：", skipped.len())?;
                for (i, item) in skipped.iter().enumerate() {
                    if i > 0 {
                        write!(f, "；")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "）")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// 品牌注册表（进程内唯一实例）。
#[derive(Debug)]
pub struct Registry {
    brands: Vec<Brand>,
    /// 实际加载型号表的目录（诊断页展示）。
    dir: PathBuf,
    /// 因型号表不可用而跳过的品牌（诊断页 / 日志展示）。
    skipped: Vec<SkippedBrand>,
}

impl Registry {
    /// 建立注册表：从型号表目录加载全部型号表，并校验每份品牌描述。
    ///
    /// 型号表缺失 / 损坏的品牌会被跳过并记入 [`Registry::skipped`]，不会拖垮其它品牌；
    /// panic 只发生在「目录不可用」或「一个品牌都不可用」这类部署事故上 —— 必须在第一次枚举前
    /// 暴露，而不是运行几小时后才表现为「一个设备都看不到」。需要优雅处理时用
    /// [`Registry::try_embedded`]。
    pub fn embedded() -> &'static Self {
        match Self::try_embedded() {
            Ok(registry) => registry,
            Err(err) => panic!("品牌注册表不可用：{err}"),
        }
    }

    /// [`Registry::embedded`] 的可失败版本：成功才缓存，失败可在下次调用时重试（例如目录稍后才就位）。
    ///
    /// 建立成功但**有品牌被跳过**时，跳过项在这里打一条诊断并保留在 [`Registry::skipped`]：
    /// 静默跳过会让「某品牌设备不见了」变成无从下手的玄学问题。
    pub fn try_embedded() -> Result<&'static Self, RegistryError> {
        static REGISTRY: OnceLock<Registry> = OnceLock::new();
        if let Some(registry) = REGISTRY.get() {
            return Ok(registry);
        }
        let registry = Self::build(BRANDS)?;
        for item in &registry.skipped {
            eprintln!("voltnest-hid: {item}");
        }
        if !registry.skipped.is_empty() {
            eprintln!(
                "voltnest-hid: 型号表目录 {} 共跳过 {} 个品牌，其余 {} 个品牌照常采集",
                registry.dir.display(),
                registry.skipped.len(),
                registry.brands.len()
            );
        }
        Ok(REGISTRY.get_or_init(|| registry))
    }

    /// 用给定品牌列表建立注册表：型号表从默认目录加载。
    ///
    /// 除顶层描述外，每个品牌的 [`BrandSpec::variants`] 也会逐份校验：变体只在 `route` 里用到，
    /// 少写一份单测就没人管它的下标越界 —— 而这类错误的表现是「读不出电量」，
    /// 比启动失败难查一个数量级。
    pub fn build(specs: &'static [&'static BrandSpec]) -> Result<Self, RegistryError> {
        Self::build_with(specs, &MappingSet::load()?)
    }

    /// 用给定品牌列表与**已加载**的型号表集合建立注册表（测试注入 / 复用同一次目录扫描）。
    pub fn build_with(
        specs: &'static [&'static BrandSpec],
        mappings: &MappingSet,
    ) -> Result<Self, RegistryError> {
        let mut brands = Vec::with_capacity(specs.len());
        let mut skipped: Vec<SkippedBrand> = Vec::new();
        let mut seen_ids: HashSet<&'static str> = HashSet::new();
        for spec in specs {
            spec.validate()?;
            if !seen_ids.insert(spec.id) {
                return Err(SpecError::DuplicateBrandId { brand: spec.id }.into());
            }
            for variant in spec.variants {
                variant.validate()?;
                // 品牌标识不参与匹配，但它是日志与诊断的唯一线索：重复即分不清是哪套协议出的问题。
                if !seen_ids.insert(variant.id) {
                    return Err(SpecError::DuplicateBrandId { brand: variant.id }.into());
                }
            }
            // 型号表不可用只跳过这个品牌：同一份表常被多套协议共用，缺一份不该殃及其它品牌。
            let table = match mappings.table(spec.device_table_file) {
                Ok(table) => table,
                Err(error) => {
                    skipped.push(SkippedBrand {
                        brand: spec.id,
                        file: spec.device_table_file,
                        error,
                    });
                    continue;
                }
            };
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
                        }
                        .into());
                    }
                }
            }
            brands.push(Brand { spec, table });
        }
        if brands.is_empty() {
            // 一个能服务的品牌都没有 = 装包 / 数据事故，必须显式失败，而不是「静默零品牌」。
            return Err(RegistryError::NoUsableBrand { skipped });
        }
        Ok(Self {
            brands,
            dir: mappings.dir().to_path_buf(),
            skipped,
        })
    }

    /// 全部品牌（诊断页 / 冲突提示用）。
    pub fn brands(&self) -> &[Brand] {
        &self.brands
    }

    /// 因型号表不可用而跳过的品牌（诊断页 / 日志展示；空表示全部品牌就位）。
    pub fn skipped(&self) -> &[SkippedBrand] {
        &self.skipped
    }

    /// 型号表目录（诊断页展示，便于核对「用的是哪一份表」）。
    pub fn mappings_dir(&self) -> &Path {
        &self.dir
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
                cmd_high: None,
                subtag: None,
                percent_index: 0,
                percent_mask: 0xFF,
                charging: None,
            },
            listen: None,
            notify: None,
            request: None,
            ack: None,
            fallback_report_id: None,
            expected_interval_ms: 30_000,
            block_offset: None,
            profile_probe: None,
            device_table_file: ajazz::SPEC.device_table_file,
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
            Registry::build(&LIST).err().and_then(RegistryError::spec),
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
            Registry::build(&LIST).err().and_then(RegistryError::spec),
            Some(SpecError::DuplicateBrandId { brand: "dup-brand" })
        );
    }

    /// 顶层 4 份 + 变体 14 份全部进注册表：新增变体必须登记，否则启动校验覆盖不到它。
    #[test]
    fn every_variant_is_registered_and_valid() {
        let reg = Registry::embedded();
        let total: usize = reg.brands().iter().map(|b| 1 + b.spec.variants.len()).sum();
        assert_eq!(total, 18, "4 顶层 + 3 达尔优 + 7 迈从 + 4 雷柏");
        for b in reg.brands() {
            for variant in b.spec.variants {
                assert!(variant.validate().is_ok(), "{} 非法", variant.id);
                assert_ne!(variant.id, b.spec.id, "变体不得与品牌标识重名");
            }
        }
        assert!(reg.validate_against(&HID_VBAS).is_ok());
    }

    /// 型号表来自文件系统：注册表必须报出实际目录，便于核对「用的是哪一份表」。
    #[test]
    fn registry_reports_mappings_dir() {
        let dir = Registry::embedded().mappings_dir();
        assert!(dir.ends_with("mappings"), "{dir:?}");
        assert!(dir.join("ajazz.json").is_file(), "{dir:?}");
    }

    /// 换表不换二进制：把一份改过的表放进指定目录，注册表必须读到改后的内容。
    #[test]
    fn registry_reads_tables_from_the_given_dir() {
        let dir =
            std::env::temp_dir().join(format!("voltnest-hid-registry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 键位换成一个内置表里不存在的 PID：若仍读编译期常量，这条必然查不到。
        let json = r#"{
            "config_version": "9.9.9",
            "source": { "file": "swap.xml" },
            "devices": { "7768:9999": { "name": "换表后的型号", "deviceType": "mouse", "mode": "2.4G" } }
        }"#;
        std::fs::write(dir.join("ajazz.json"), json).unwrap();

        let set = MappingSet::load_from(&dir).unwrap();
        static ONLY: [&BrandSpec; 1] = [&ajazz::SPEC];
        let reg = Registry::build_with(&ONLY, &set).unwrap();
        assert_eq!(reg.mappings_dir(), dir);
        let (_, entry) = reg.lookup(0x7768, 0x9999).expect("必须读到目录里的表");
        assert_eq!(entry.name.as_deref(), Some("换表后的型号"));
        assert!(reg.lookup(0x7768, 0x6426).is_none(), "内置条目不得残留");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 品牌声明的文件不存在：跳过该品牌并记录（缺哪份表、现有哪几份），其它品牌照常服务。
    #[test]
    fn missing_table_skips_brand_and_records() {
        static NO_FILE: BrandSpec = BrandSpec {
            id: "no-file",
            label: "缺表品牌",
            block_offset: None,
            profile_probe: None,
            device_table_file: "nope.json",
            ..ajazz::SPEC
        };
        static LIST: [&BrandSpec; 2] = [&NO_FILE, &ajazz::SPEC];
        let set = MappingSet::load().unwrap();
        let reg = Registry::build_with(&LIST, &set).expect("只跳过缺表品牌，不得整次失败");

        assert_eq!(reg.brands().len(), 1, "好的品牌必须留下");
        assert_eq!(reg.skipped().len(), 1, "跳过项必须留证");
        let skip = &reg.skipped()[0];
        assert_eq!(skip.brand, "no-file");
        assert_eq!(skip.file, "nope.json");
        assert!(
            matches!(&skip.error, MappingError::MissingTable { file, .. } if file == "nope.json"),
            "{skip:?}"
        );
        // 记录必须说清「缺哪份、现有哪几份」。
        assert!(
            skip.to_string().contains("ajazz.json"),
            "须列出实际存在的表：{skip}"
        );
        // 跳过的品牌不得凭空出现设备。
        assert!(reg.lookup(0x7768, 0x6426).is_some(), "ajazz 必须仍可查表");
    }

    /// 一个品牌都不剩就不能算「跳过」：静默零品牌会让「设备全丢了」无从排查。
    #[test]
    fn every_brand_skipped_is_an_error() {
        static NO_FILE: BrandSpec = BrandSpec {
            id: "no-file",
            label: "缺表品牌",
            block_offset: None,
            profile_probe: None,
            device_table_file: "nope.json",
            ..ajazz::SPEC
        };
        static LIST: [&BrandSpec; 1] = [&NO_FILE];
        let err = Registry::build(&LIST).expect_err("零可用品牌必须失败");
        let skipped = err.clone().skipped().expect("必须带出跳过清单");
        assert_eq!(skipped.len(), 1);
        assert!(err.to_string().contains("no-file"), "{err}");
    }

    /// 目录里某份表坏了：只有用它的品牌被跳过，其余品牌照常匹配。
    #[test]
    fn broken_table_skips_only_its_brand() {
        let dir = std::env::temp_dir().join(format!("voltnest-hid-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["ajazz.json", "mchose.json", "rapoo.json"] {
            std::fs::copy(manifest_mappings().join(file), dir.join(file)).unwrap();
        }
        std::fs::write(dir.join("dareu.json"), "{ not json").unwrap();

        let set = MappingSet::load_from(&dir).unwrap();
        assert_eq!(set.skipped().len(), 1, "坏表自身要留证");
        let reg = Registry::build_with(BRANDS, &set).expect("坏一份表不得整次失败");

        assert_eq!(reg.brands().len(), 3);
        assert_eq!(reg.skipped().len(), 1);
        assert_eq!(reg.skipped()[0].brand, "dareu");
        assert_eq!(reg.skipped()[0].file, "dareu.json");
        assert!(
            matches!(
                &reg.skipped()[0].error,
                MappingError::InvalidTable { path, .. } if path.ends_with("dareu.json")
            ),
            "必须报解析失败而不是「缺少文件」：{:?}",
            reg.skipped()[0]
        );
        assert!(reg.lookup(0x260D, 0x1117).is_none(), "达尔优已跳过");
        for (vid, pid, want) in [
            (0x7768, 0x6426, "ajazz"),
            (0x24AE, 0x1410, "rapoo"),
            (0x3837, 0x100B, "mchose"),
        ] {
            assert_eq!(reg.lookup(vid, pid).unwrap().0.spec.id, want);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 注册表失败与跳过项的诊断文本：宿主只靠这一行定位问题。
    #[test]
    fn registry_error_display_covers_every_variant() {
        let dir = PathBuf::from("/opt/voltnest/mappings");
        let skipped = vec![
            SkippedBrand {
                brand: "dareu",
                file: "dareu.json",
                error: MappingError::MissingTable {
                    dir: dir.clone(),
                    file: "dareu.json".to_string(),
                    available: Vec::new(),
                },
            },
            SkippedBrand {
                brand: "rapoo",
                file: "rapoo.json",
                error: MappingError::Empty { dir: dir.clone() },
            },
        ];

        let text = RegistryError::NoUsableBrand {
            skipped: skipped.clone(),
        }
        .to_string();
        assert!(text.contains("没有任何可用品牌"), "{text}");
        assert!(text.contains("dareu") && text.contains("rapoo"), "{text}");
        // 多个跳过项必须分隔，否则两个品牌会连成一串。
        assert!(text.contains('；'), "{text}");

        assert!(
            skipped[0].to_string().contains("dareu.json"),
            "{}",
            skipped[0]
        );
        assert!(
            RegistryError::Spec(SpecError::EmptyBrandId)
                .to_string()
                .contains("品牌标识")
        );
        assert!(
            RegistryError::Mappings(MappingError::Empty { dir: dir.clone() })
                .to_string()
                .contains("/opt/voltnest/mappings")
        );
    }

    /// 型号表目录（源码树里那份，测试用它复制真实表）。
    fn manifest_mappings() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("mappings")
    }
}
