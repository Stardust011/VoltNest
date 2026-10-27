//! 读 `data/profiles.json`，生成三档阈值链常量。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 本脚本认识的 JSON 结构版本。
const SUPPORTED_SCHEMA_VERSION: u32 = 2;

/// 宿主必须能拿到的家族，缺一即构建失败。
const REQUIRED_FAMILIES: [&str; 2] = ["hid-vbas", "ble-bas"];

#[derive(Deserialize)]
struct Profiles {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    families: BTreeMap<String, Family>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Family {
    thresholds: Levels,
}

/// 三档阈值链，键名即 `Sensitivity` 的落盘字符串。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Levels {
    sensitive: Level,
    standard: Level,
    loose: Level,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct Level {
    #[serde(rename = "staleSeconds")]
    stale_seconds: f64,
    #[serde(rename = "unavailableSeconds")]
    unavailable_seconds: f64,
    #[serde(rename = "offlineSeconds")]
    offline_seconds: f64,
}

/// 转成毫秒后的阈值链。
struct MsLevel {
    stale_ms: u64,
    unavailable_ms: u64,
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

    for family in REQUIRED_FAMILIES {
        if !profiles.families.contains_key(family) {
            return Err(format!("缺少必需家族 {family}"));
        }
    }

    let mut code = String::from("// 由 build.rs 依据 data/profiles.json 生成，勿手改。\n");

    for (id, family) in &profiles.families {
        let levels = [
            ("sensitive", to_level(&family.thresholds.sensitive)?),
            ("standard", to_level(&family.thresholds.standard)?),
            ("loose", to_level(&family.thresholds.loose)?),
        ];
        for (name, level) in &levels {
            if !(level.stale_ms < level.unavailable_ms && level.unavailable_ms < level.offline_ms) {
                return Err(format!(
                    "家族 {id} 的 {name} 档阈值链必须 stale<unavailable<offline，实际 {}/{}/{}",
                    level.stale_ms, level.unavailable_ms, level.offline_ms
                ));
            }
        }
        if levels[0].1.stale_ms > levels[1].1.stale_ms
            || levels[1].1.stale_ms > levels[2].1.stale_ms
        {
            return Err(format!(
                "家族 {id} 的 staleSeconds 必须随档位放宽（快 ≤ 标准 ≤ 慢）"
            ));
        }

        let mut table = String::new();
        for (_, level) in &levels {
            writeln!(
                table,
                "    super::DegradationThresholds {{\n\
                 \x20       stale_ms: {},\n\
                 \x20       unavailable_ms: {},\n\
                 \x20       offline_ms: {},\n\
                 \x20   }},",
                grouped(level.stale_ms),
                grouped(level.unavailable_ms),
                grouped(level.offline_ms),
            )
            .expect("写入字符串失败");
        }
        writeln!(
            code,
            "\n/// 家族 `{id}` 的三档阈值链，次序与 `Sensitivity` 判别式一致：快、标准、慢。\n\
             pub(crate) const {}: [super::DegradationThresholds; 3] = [\n{table}];",
            const_name(id, "THRESHOLDS"),
        )
        .expect("写入字符串失败");
    }

    Ok(code)
}

fn to_level(level: &Level) -> Result<MsLevel, String> {
    Ok(MsLevel {
        stale_ms: to_ms(level.stale_seconds, "staleSeconds")?,
        unavailable_ms: to_ms(level.unavailable_seconds, "unavailableSeconds")?,
        offline_ms: to_ms(level.offline_seconds, "offlineSeconds")?,
    })
}

/// 秒 → 毫秒。要求换算结果为整数毫秒，避免静默丢精度。
fn to_ms(seconds: f64, what: &str) -> Result<u64, String> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(format!("{what} 必须为正数，实际 {seconds}"));
    }
    let ms = seconds * 1000.0;
    let rounded = ms.round();
    if (ms - rounded).abs() > 1e-6 {
        return Err(format!("{what}={seconds}s 不是整数毫秒，最多三位小数"));
    }
    if rounded <= 0.0 {
        return Err(format!("{what}={seconds}s 换算后为 0ms"));
    }
    Ok(rounded as u64)
}

/// `hid-vbas` + `THRESHOLDS` → `HID_VBAS_THRESHOLDS`。
fn const_name(family: &str, suffix: &str) -> String {
    format!("{}_{suffix}", family.replace('-', "_").to_ascii_uppercase())
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
