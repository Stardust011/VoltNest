//! 读 `data/profiles.json`，生成阈值链与三档倍率常量。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 本脚本认识的 JSON 结构版本。
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// 宿主必须能拿到的家族，缺一即构建失败。
const REQUIRED_FAMILIES: [&str; 2] = ["hid-vbas", "ble-bas"];

/// 阈值必须按此粒度对齐：`scaled` 用整数除法，非对齐值会出现毫秒级漂移。
const ALIGNMENT_MS: u64 = 500;

#[derive(Deserialize)]
struct Profiles {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    sensitivity: SensitivityTable,
    families: BTreeMap<String, Family>,
}

#[derive(Deserialize)]
struct SensitivityTable {
    sensitive: Ratio,
    standard: Ratio,
    loose: Ratio,
}

#[derive(Deserialize, Clone, Copy)]
struct Ratio {
    numerator: u64,
    denominator: u64,
}

#[derive(Deserialize)]
struct Family {
    thresholds: Thresholds,
}

#[derive(Deserialize, Clone, Copy)]
struct Thresholds {
    #[serde(rename = "staleMs")]
    stale_ms: u64,
    #[serde(rename = "unavailableMs")]
    unavailable_ms: u64,
    #[serde(rename = "offlineMs")]
    offline_ms: u64,
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest.join("data/profiles.json");
    println!("cargo:rerun-if-changed={}", source.display());

    let raw = std::fs::read_to_string(&source).expect("读取 data/profiles.json 失败");
    let profiles: Profiles = serde_json::from_str(&raw).expect("data/profiles.json 解析失败");

    let code = generate(&profiles).unwrap_or_else(|e| panic!("data/profiles.json 非法：{e}"));

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("profiles_generated.rs");
    std::fs::write(out, code).expect("写入 profiles_generated.rs 失败");
}

fn generate(profiles: &Profiles) -> Result<String, String> {
    if profiles.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "schemaVersion 只支持 {SUPPORTED_SCHEMA_VERSION}，实际 {}",
            profiles.schema_version
        ));
    }

    let levels = [
        ("sensitive", "快", profiles.sensitivity.sensitive),
        ("standard", "标准", profiles.sensitivity.standard),
        ("loose", "慢", profiles.sensitivity.loose),
    ];
    for (name, _, ratio) in levels {
        if ratio.numerator == 0 || ratio.denominator == 0 {
            return Err(format!("倍率 {name} 的分子分母必须为正"));
        }
    }

    for family in REQUIRED_FAMILIES {
        if !profiles.families.contains_key(family) {
            return Err(format!("缺少必需家族 {family}"));
        }
    }

    for (id, family) in &profiles.families {
        let t = family.thresholds;
        if !(t.stale_ms < t.unavailable_ms && t.unavailable_ms < t.offline_ms) {
            return Err(format!(
                "家族 {id} 阈值链必须 stale<unavailable<offline，实际 {}/{}/{}",
                t.stale_ms, t.unavailable_ms, t.offline_ms
            ));
        }
        for value in [t.stale_ms, t.unavailable_ms, t.offline_ms] {
            if value % ALIGNMENT_MS != 0 {
                return Err(format!(
                    "家族 {id} 阈值 {value}ms 必须是 {ALIGNMENT_MS}ms 的整数倍"
                ));
            }
        }
        for (name, _, ratio) in levels {
            for value in [t.stale_ms, t.unavailable_ms, t.offline_ms] {
                if value * ratio.numerator % ratio.denominator != 0 {
                    return Err(format!(
                        "家族 {id} 阈值 {value}ms 在 {name} 档无法整除，会出现毫秒漂移"
                    ));
                }
            }
        }
    }

    let mut code = String::from(
        "// 由 build.rs 依据 data/profiles.json 生成，勿手改。\n\
         \n\
         /// 三档倍率，次序与 `Sensitivity` 判别式一致：快、标准、慢。\n",
    );
    writeln!(
        code,
        "pub(crate) const SENSITIVITY_RATIO: [(u64, u64); 3] = [({}, {}), ({}, {}), ({}, {})];",
        levels[0].2.numerator,
        levels[0].2.denominator,
        levels[1].2.numerator,
        levels[1].2.denominator,
        levels[2].2.numerator,
        levels[2].2.denominator,
    )
    .expect("写入字符串失败");

    for (id, family) in &profiles.families {
        let t = family.thresholds;
        writeln!(
            code,
            "\n/// 家族 `{id}` 的标准档阈值链。\n\
             pub(crate) const {}: super::DegradationThresholds = super::DegradationThresholds {{\n\
             \x20   stale_ms: {},\n\
             \x20   unavailable_ms: {},\n\
             \x20   offline_ms: {},\n\
             }};",
            const_name(id),
            grouped(t.stale_ms),
            grouped(t.unavailable_ms),
            grouped(t.offline_ms),
        )
        .expect("写入字符串失败");
    }

    Ok(code)
}

/// `hid-vbas` → `HID_VBAS_THRESHOLDS`。
fn const_name(family: &str) -> String {
    format!(
        "{}_THRESHOLDS",
        family.replace('-', "_").to_ascii_uppercase()
    )
}

/// 生成千位带下划线的字面量，产出与 `rustfmt` 风格一致。
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}
