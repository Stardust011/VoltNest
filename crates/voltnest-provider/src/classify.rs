//! 设备类型识别：纯函数，零 I/O、零时钟、零线程。输入是本轮实际取到的标准字段，输出 [`DeviceClassification`]。
//!
//! 三条不可协商的规则：① 证据不足 → `Unknown`，不做启发式猜测；② 每条结论都带证据，未命中也记录原因，便于追溯；
//! ③ 冲突不静默，两条可信证据指向不同类型时返回 `Unknown` 并写明冲突，绝不按优先级偷偷择一。
//!
//! 规则优先级（应用层约定，非 USB-IF / SIG 规定）：HID Usage → BLE Appearance → BLE 标准服务 → 可核验设备映射。
//! 名称类字符串（`local_name`、ProductString）不参与判定——厂商改名、系统缓存、串名都会让「名字像鼠标」变成不可靠判断。
//!
//! 复合集合：实测同一 `VID:PID`（本仓库 `7768:6426`）同时声明多个集合级功能声明（Mouse/Keyboard/Consumer/厂商页），
//! 它们说明「同时具备这些功能」而非「是哪种产品」，故标准侧不收敛到单一类型，交由产品级可核验映射裁决；无映射即 `Unknown`。
//!
//! 证据来源与可验证性：
//!
//! | 规则 | 来源 | 实测状态 |
//! |---|---|---|
//! | HID Usage `0x01/0x01,0x02,0x04..0x08` | USB-IF HID Usage Tables 1.4（Generic Desktop） | 单测覆盖；本机为厂商自定义 UsagePage，未命中 |
//! | BLE Appearance `0x03C1..0x03C4 / 0x0841..0x0844 / 0x0942,0x0943` | Bluetooth SIG Assigned Numbers，值 = `category << 6 \| subcategory` | 单测覆盖；本机 BLE 设备为 Unknown |
//! | BLE 服务 `0x180F / 0x1812 / 0x180A` | Battery Service / HOGP / DIS 规范 | 只作上下文证据，不单独定型 |
//! | 可核验设备映射 | 品牌型号表（官方驱动 `config.xml` 的 `deviceType`） | 仅 `deviceType` 恰为规范类型词时生效 |
//!
//! 未列入上表的 Usage / Appearance / 服务一律不映射：标准没定义或本项目未验证的组合，返回 `Unknown` 比猜一个类型更有价值。

use std::fmt;

/// 应用层设备类型。
///
/// **不是** USB / Bluetooth 标准枚举：USB-IF 只有 HID Usage，Bluetooth SIG 只有
/// Appearance / 服务，两者都描述「功能声明」，不描述完整产品。因此这里是应用层
/// 收敛后的分类，随时可以按业务增减（`Unknown` 必须保留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum DeviceType {
    /// 键盘。
    Keyboard,
    /// 鼠标。
    Mouse,
    /// 手柄（HID Game Pad / Appearance Gamepad）。
    Gamepad,
    /// 头戴式耳机（有麦克风，Appearance `Headset`）。
    Headset,
    /// 耳机（无麦克风语义，Appearance `Headphones`）。
    Headphones,
    /// 音箱（Appearance Audio Sink 的具体子类）。
    Speaker,
    /// 泛称的游戏控制器（HID Joystick 一类）。
    Controller,
    /// 有证据确认属于已知范围之外的类型。
    ///
    /// 仅在**确实取到**了标准里的类别、而本枚举没有对应项时使用；
    /// 「只确认是 HID 外设」不属于此列（那是 [`DeviceType::Unknown`]）。
    Other,
    /// 证据不足，无法判定。**默认值**。
    #[default]
    Unknown,
}

impl DeviceType {
    /// 稳定小写字符串：落进 `Snapshot.deviceType`，改动即破坏前端展示。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Gamepad => "gamepad",
            Self::Headset => "headset",
            Self::Headphones => "headphones",
            Self::Speaker => "speaker",
            Self::Controller => "controller",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    /// 是否已判定出具体类型（`Other` 也算已判定，`Unknown` 不算）。
    pub const fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 结论是由哪一类证据得出的。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassificationMethod {
    /// HID Usage Page + Usage（USB-IF HID Usage Tables）。
    HidUsage,
    /// BLE Appearance（GAP `0x2A01`，SIG Assigned Numbers）。
    BleAppearance,
    /// BLE 标准服务（`0x180F` / `0x1812` / `0x180A`）。
    ///
    /// 当前**没有**规则产出它：上表中的服务只能确认设备「提供某项功能」，
    /// 不足以单独确定产品类别（例如 `0x1812` 只说明是 HID 类设备）。
    /// 保留该取值是为了让将来的服务级规则有明确归类，而不是把服务证据
    /// 硬塞进 Appearance 或映射里。
    BleService,
    /// 可核验的设备映射（品牌型号表 / 官方驱动配置）。
    VerifiedDeviceMapping,
    /// 未得出类型。
    Unknown,
}

impl ClassificationMethod {
    /// 稳定小写字符串（进快照与诊断）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HidUsage => "hid_usage",
            Self::BleAppearance => "ble_appearance",
            Self::BleService => "ble_service",
            Self::VerifiedDeviceMapping => "verified_device_mapping",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ClassificationMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一次识别的完整结果。**结论必须可追溯**，所以证据随结论一起走。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceClassification {
    /// 判定结果。
    pub device_type: DeviceType,
    /// 得出该结果的方法（`Unknown` 时为 [`ClassificationMethod::Unknown`]）。
    pub method: ClassificationMethod,
    /// 逐条证据 / 未命中原因，按收集顺序。**只含参与判定（或解释未判定）的条目**。
    pub evidence: Vec<String>,
    /// 标准字段里**存在但本项目不支持**的组合，未参与判定。
    ///
    /// 与 [`Self::evidence`] 分开是为了让结论行保持可读：一台复合 HID 设备一次能
    /// 报出 5~7 个厂商/消费者集合，全都塞进结论行会把真正的证据淹掉。这些条目
    /// 仍然可追溯，但只进 **debug 级**诊断。
    pub skipped: Vec<String>,
}

impl DeviceClassification {
    /// 未判定（默认）。
    pub fn unknown() -> Self {
        Self {
            device_type: DeviceType::Unknown,
            method: ClassificationMethod::Unknown,
            evidence: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// 判定成功。
    pub fn new(
        device_type: DeviceType,
        method: ClassificationMethod,
        evidence: Vec<String>,
    ) -> Self {
        debug_assert!(
            device_type.is_known() == (method != ClassificationMethod::Unknown),
            "类型与方法必须一致：已知类型必须给出方法，Unknown 必须没有方法"
        );
        Self {
            device_type,
            method,
            evidence,
            skipped: Vec::new(),
        }
    }

    /// 是否仍未判定。
    pub fn is_unknown(&self) -> bool {
        !self.device_type.is_known()
    }

    /// 单行证据文本（诊断页展示用；逐条仍在 `evidence` 里）。
    pub fn evidence_text(&self) -> String {
        self.evidence.join("；")
    }

    /// 完整留痕文本：`evidence` + `skipped`。**仅供 debug 级诊断**。
    ///
    /// 结论行走的是「类型 + 方法」，证据与未支持清单一律走这里，避免把
    /// 一台复合设备 5~7 行的枚举细节塞进结论行。
    pub fn debug_text(&self) -> String {
        let mut parts = self.evidence.clone();
        parts.extend(self.skipped.iter().cloned());
        parts.join("；")
    }
}

impl Default for DeviceClassification {
    fn default() -> Self {
        Self::unknown()
    }
}

/// 未判定但保留证据（冲突 / 缺映射都用它）。`skipped` 由 [`resolve`] 统一回填。
fn unknown_with(evidence: Vec<String>) -> DeviceClassification {
    DeviceClassification {
        device_type: DeviceType::Unknown,
        method: ClassificationMethod::Unknown,
        evidence,
        skipped: Vec::new(),
    }
}

/// 一个 HID 集合（接口顶层集合）声明的 Usage。
///
/// 注意它描述的是**集合声明的功能**，不是完整物理产品：键盘带一个鼠标集合
/// 是常见形态，因此见 [`classify_hid`] 的冲突规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HidCollection {
    /// Usage Page。
    pub usage_page: u16,
    /// Usage。
    pub usage: u16,
}

/// HID 侧证据：同一 `VID:PID` 全部接口的 Usage 汇总。
///
/// 按 `VID:PID` 聚合而不是按接口聚合，是因为 `stableId` 也只到 `VID:PID`：
/// 一个接收器有多个接口，物理上仍是一台设备。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct HidEvidence {
    /// VID。
    pub vid: u16,
    /// PID。
    pub pid: u16,
    /// 已枚举到的全部集合（去重）。空 = 没有可用的 Usage 信息。
    pub collections: Vec<HidCollection>,
    /// `usage_page` / `usage` 字段在本平台是否真的可用。
    ///
    /// hidapi 官方文档：Linux libusb 后端不提供这两个字段（恒为 0），
    /// 因此**不得**把它们当成「合法的 0x0000 集合」参与匹配。
    pub usage_available: bool,
}

impl HidEvidence {
    /// 空证据。
    pub fn new(vid: u16, pid: u16) -> Self {
        Self {
            vid,
            pid,
            collections: Vec::new(),
            usage_available: false,
        }
    }

    /// 记录一个集合。`usage_page == 0` 视为字段不可用（hidapi 在 Linux libusb
    /// 后端不填该字段），不写入集合列表。
    pub fn push_collection(&mut self, usage_page: u16, usage: u16) {
        if usage_page == 0 {
            return;
        }
        self.usage_available = true;
        let item = HidCollection { usage_page, usage };
        if !self.collections.contains(&item) {
            self.collections.push(item);
        }
    }
}

/// BLE 侧证据。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BleEvidence {
    /// 本轮服务发现**实际返回**的 16 位 SIG UUID（去重、升序）。
    ///
    /// 只含连接后 GATT 发现的结果，不含广播里出现的 UUID，也不代表设备全部能力
    /// （发现失败/被裁剪都会少列）。
    pub services: Vec<u16>,
    /// GAP `0x2A01` 的实际取值。`None` = 特征不存在、读取失败或长度非法。
    pub appearance: Option<u16>,
}

impl BleEvidence {
    /// 记录一个 SIG 16 位 UUID。
    pub fn push_service(&mut self, short: u16) {
        if !self.services.contains(&short) {
            self.services.push(short);
        }
        self.services.sort_unstable();
    }

    /// 本轮是否发现了该服务。
    pub fn has_service(&self, short: u16) -> bool {
        self.services.contains(&short)
    }
}

/// 有可核验来源的设备映射（VID/PID → 类型）。
///
/// `source` 是给人看的溯源信息（文件 + 版本），必须能对上实物或官方数据，
/// 否则不允许构造。**不得**把没有出处的映射塞进来。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerifiedMapping {
    /// 映射出的类型。
    pub device_type: DeviceType,
    /// 溯源：数据来源与版本。
    pub source: String,
}

impl VerifiedMapping {
    /// 构造。
    pub fn new(device_type: DeviceType, source: impl Into<String>) -> Self {
        Self {
            device_type,
            source: source.into(),
        }
    }
}

/// 一条候选结论。
struct Candidate {
    device_type: DeviceType,
    method: ClassificationMethod,
    detail: String,
}

/// 可核验映射的证据文本。
fn mapping_detail(v: &VerifiedMapping) -> String {
    format!("可核验映射 → {}（{}）", v.device_type, v.source)
}

/// 标准证据的裁决（本文件唯一实现点）：同类型合并，复合集合或与映射冲突时按显式规则返回。
///
/// | 标准证据 | 有可核验映射 | 结果 |
/// |---|---|---|
/// | 无 | 有 | 用映射 |
/// | 无 | 无 | `Unknown` |
/// | 单一类型 | 同类型 | 用该类型（方法取标准的），映射作补充证据保留 |
/// | 单一类型 | 不同类型 | `Unknown` + 冲突（两条独立可信来源矛盾，不裁决） |
/// | 多类型（复合） | 有 | 用映射 + 冲突记录（标准自身无法收敛到单一产品类型） |
/// | 多类型（复合） | 无 | `Unknown` + 冲突 |
///
/// 「多类型」是实测常态：游戏鼠标常同时声明 `0x01/0x0002`（鼠标）与 `0x01/0x0006`（键盘/宏键）；
/// 集合声明的是功能而非完整产品，故标准侧不下结论，交给产品级映射，无映射即 `Unknown`。
/// `skipped` 是字段存在但本项目不支持、未参与判定的条目，带进 [`DeviceClassification::skipped`]（debug 级），不进 `evidence`。
fn resolve(
    standard: Vec<Candidate>,
    notes: Vec<String>,
    skipped: Vec<String>,
    mapping: Option<&VerifiedMapping>,
) -> DeviceClassification {
    let mut groups: Vec<(DeviceType, ClassificationMethod, Vec<String>)> = Vec::new();
    for c in standard {
        match groups.iter_mut().find(|(t, _, _)| *t == c.device_type) {
            Some(slot) => slot.2.push(c.detail),
            None => groups.push((c.device_type, c.method, vec![c.detail])),
        }
    }

    let mut evidence = notes;
    let mapping_text = mapping.map(mapping_detail);

    let mut result = match groups.len() {
        0 => match (mapping, mapping_text) {
            (Some(v), Some(text)) => {
                evidence.push(text);
                DeviceClassification::new(
                    v.device_type,
                    ClassificationMethod::VerifiedDeviceMapping,
                    evidence,
                )
            }
            _ => unknown_with(evidence),
        },
        1 => {
            let (device_type, method, details) = groups.remove(0);
            evidence.extend(details);
            match (mapping, mapping_text) {
                (Some(v), Some(text)) if v.device_type == device_type => {
                    evidence.push(text);
                    DeviceClassification::new(device_type, method, evidence)
                }
                (Some(v), Some(text)) => {
                    evidence.push(text);
                    evidence.push(format!(
                        "证据冲突：{device_type}（标准）vs {}（映射）",
                        v.device_type
                    ));
                    DeviceClassification {
                        device_type: DeviceType::Unknown,
                        method: ClassificationMethod::Unknown,
                        evidence,
                        skipped: Vec::new(),
                    }
                }
                _ => DeviceClassification::new(device_type, method, evidence),
            }
        }
        _ => {
            let types: Vec<&str> = groups.iter().map(|(t, _, _)| t.as_str()).collect();
            for (_, _, details) in groups {
                evidence.extend(details);
            }
            evidence.push(format!("复合集合：{}", types.join(" + ")));
            match (mapping, mapping_text) {
                (Some(v), Some(text)) => {
                    evidence.push(text);
                    DeviceClassification::new(
                        v.device_type,
                        ClassificationMethod::VerifiedDeviceMapping,
                        evidence,
                    )
                }
                _ => {
                    evidence.push("缺少可核验映射，无法收敛".to_string());
                    unknown_with(evidence)
                }
            }
        }
    };
    result.skipped = skipped;
    result
}

/// HID Generic Desktop 页的标准 Usage → 类型。
///
/// 收录 HID Usage Tables 1.4 在 `0x0001` 页定义的**全部**设备类应用集合
/// （`0x01` Pointer … `0x08` Multi-axis Controller）。`0x03` 在标准里是
/// Reserved，没有设备含义，因此不映射；`0x80` System Control 不是设备类别，
/// 同样不映射。除此之外的组合一律 `None`。
///
/// 三处映射是**项目粒度上的收敛**，不是标准里的等价关系：
///
/// - `0x01` Pointer：通用指针集合（鼠标 / 触控板 / 指点杆都可能声明它）。
///   本项目只有「鼠标」这一个指针槽位，故归入 `Mouse`，证据保留 `Pointer` 原文。
/// - `0x07` Keypad：独立数字键盘，属键盘族，归入 `Keyboard`。
/// - `0x04` Joystick / `0x08` Multi-axis Controller：标准里都是游戏控制器，
///   本项目只有粗粒度的 `Controller`。
///
/// 这样收敛的好处是同一台设备的多个相关集合不会互相冲突：指针类
/// （`0x01`+`0x02`）、键盘类（`0x06`+`0x07`）、控制器类（`0x04`+`0x08`）
/// 各自合并为一种候选，而不是被判成「复合冲突」。
pub fn usage_type(usage_page: u16, usage: u16) -> Option<(DeviceType, &'static str)> {
    const PAGE_GENERIC_DESKTOP: u16 = 0x0001;
    match (usage_page, usage) {
        (PAGE_GENERIC_DESKTOP, 0x01) => Some((DeviceType::Mouse, "Pointer")),
        (PAGE_GENERIC_DESKTOP, 0x02) => Some((DeviceType::Mouse, "Mouse")),
        (PAGE_GENERIC_DESKTOP, 0x04) => Some((DeviceType::Controller, "Joystick")),
        (PAGE_GENERIC_DESKTOP, 0x05) => Some((DeviceType::Gamepad, "Game Pad")),
        (PAGE_GENERIC_DESKTOP, 0x06) => Some((DeviceType::Keyboard, "Keyboard")),
        (PAGE_GENERIC_DESKTOP, 0x07) => Some((DeviceType::Keyboard, "Keypad")),
        (PAGE_GENERIC_DESKTOP, 0x08) => Some((DeviceType::Controller, "Multi-axis Controller")),
        _ => None,
    }
}

/// BLE Appearance → 类型。
///
/// 取值来自 SIG Assigned Numbers（Appearance Values），16 位值的构成为
/// `category << 6 | subcategory`。只收录**具体子类**：
///
/// - HID 类（category `0x0F`）：Keyboard `0x03C1`、Mouse `0x03C2`、
///   Joystick `0x03C3`、Gamepad `0x03C4`；
/// - Audio Sink（category `0x21`）的具体音箱子类 `0x0841..=0x0844`；
/// - Wearable Audio Device（category `0x25`）：Headset `0x0942`、Headphones `0x0943`。
///
/// 通用类别（`0x03C0` Generic HID、`0x0840` Generic Audio Sink、`0x0940`
/// Generic Wearable Audio Device）以及未收录子类（Earbud、Touchpad 等）
/// **不映射**：它们只说明设备属于某一大类，猜具体产品形态就是过度推断。
pub fn appearance_type(value: u16) -> Option<(DeviceType, &'static str)> {
    match value {
        0x03C1 => Some((DeviceType::Keyboard, "HID Keyboard")),
        0x03C2 => Some((DeviceType::Mouse, "HID Mouse")),
        0x03C3 => Some((DeviceType::Controller, "HID Joystick")),
        0x03C4 => Some((DeviceType::Gamepad, "HID Gamepad")),
        0x0841 => Some((DeviceType::Speaker, "Audio Sink / Standalone Speaker")),
        0x0842 => Some((DeviceType::Speaker, "Audio Sink / Soundbar")),
        0x0843 => Some((DeviceType::Speaker, "Audio Sink / Bookshelf Speaker")),
        0x0844 => Some((DeviceType::Speaker, "Audio Sink / Standmounted Speaker")),
        0x0942 => Some((DeviceType::Headset, "Wearable Audio / Headset")),
        0x0943 => Some((DeviceType::Headphones, "Wearable Audio / Headphones")),
        _ => None,
    }
}

/// BLE 标准服务的「能说明什么 / 不能说明什么」，仅作为证据文本。
///
/// 这三个服务都**不产出候选类型**：`0x180F` 与产品类别无关，`0x180A` 只是设备
/// 信息服务，`0x1812` 只能确认「HID 类设备」而不能区分键盘/鼠标/手柄。
pub fn service_note(short: u16) -> Option<&'static str> {
    match short {
        0x180F => Some("0x180F Battery Service（与类别无关）"),
        0x1812 => Some("0x1812 HID Service（只说明 HID 类）"),
        0x180A => Some("0x180A Device Information Service"),
        _ => None,
    }
}

/// HID 设备分类。
///
/// 顺序：先按 Usage 收集候选，再叠加可核验映射；最后统一做冲突裁决。
///
/// **只解析已支持的组合**：窗口里没收录的 Usage Page / Usage 一律不映射，也不写进
/// 结论证据（那是每台复合设备 5~7 行的噪音），而是收进
/// [`DeviceClassification::skipped`]，只出现在 debug 级诊断里。
pub fn classify_hid(ev: &HidEvidence, verified: Option<&VerifiedMapping>) -> DeviceClassification {
    let mut candidates = Vec::new();
    let mut notes = Vec::new();
    let mut skipped = Vec::new();

    if ev.usage_available {
        for c in &ev.collections {
            match usage_type(c.usage_page, c.usage) {
                Some((device_type, name)) => candidates.push(Candidate {
                    device_type,
                    method: ClassificationMethod::HidUsage,
                    detail: format!("HID Usage {:#06x}/{:#06x} = {name}", c.usage_page, c.usage),
                }),
                None => skipped.push(format!(
                    "HID Usage {:#06x}/{:#06x} 未支持",
                    c.usage_page, c.usage
                )),
            }
        }
        if ev.collections.is_empty() {
            notes.push("未枚举到任何集合".to_string());
        }
    } else {
        notes.push("Usage 字段不可用（该后端不提供），跳过 Usage 规则".to_string());
    }

    resolve(candidates, notes, skipped, verified)
}

/// BLE 设备分类。
pub fn classify_ble(ev: &BleEvidence, verified: Option<&VerifiedMapping>) -> DeviceClassification {
    let mut candidates = Vec::new();
    let mut notes = Vec::new();

    match ev.appearance {
        Some(value) => match appearance_type(value) {
            Some((device_type, name)) => candidates.push(Candidate {
                device_type,
                method: ClassificationMethod::BleAppearance,
                detail: format!("GAP Appearance 0x{value:04X} = {name}"),
            }),
            None => notes.push(format!("GAP Appearance 0x{value:04X} 无类型映射")),
        },
        None => notes.push("未取得 GAP Appearance（缺失/读取失败）".to_string()),
    }

    for short in &ev.services {
        if let Some(note) = service_note(*short) {
            notes.push(note.to_string());
        }
    }
    if ev.services.is_empty() {
        notes.push("服务发现无 SIG 已知服务".to_string());
    }

    resolve(candidates, notes, Vec::new(), verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hid_mouse() -> HidEvidence {
        let mut ev = HidEvidence::new(0x046D, 0xC077);
        ev.push_collection(0x01, 0x02);
        ev
    }

    // ---------- HID Usage ----------

    #[test]
    fn hid_usage_hits_standard_types() {
        for (usage, want, name) in [
            (0x01u16, DeviceType::Mouse, "Pointer"),
            (0x02, DeviceType::Mouse, "Mouse"),
            (0x04, DeviceType::Controller, "Joystick"),
            (0x05, DeviceType::Gamepad, "Game Pad"),
            (0x06, DeviceType::Keyboard, "Keyboard"),
            (0x07, DeviceType::Keyboard, "Keypad"),
            (0x08, DeviceType::Controller, "Multi-axis Controller"),
        ] {
            let mut ev = HidEvidence::new(0x1234, 0x5678);
            ev.push_collection(0x01, usage);
            let c = classify_hid(&ev, None);
            assert_eq!(c.device_type, want, "usage {usage:#04x}");
            assert_eq!(c.method, ClassificationMethod::HidUsage);
            assert!(
                c.evidence.iter().any(|e| e.contains(name)),
                "usage {usage:#04x} 的证据必须写明标准名称 {name}：{:?}",
                c.evidence
            );
        }
    }

    #[test]
    fn pointer_and_mouse_share_one_slot() {
        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x01);
        ev.push_collection(0x01, 0x02);
        let c = classify_hid(&ev, None);
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::HidUsage);
        assert!(!c.evidence.iter().any(|e| e.contains("复合集合")));
    }

    #[test]
    fn keyboard_family_collections_do_not_conflict() {
        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x06);
        ev.push_collection(0x01, 0x07);
        let c = classify_hid(&ev, None);
        assert_eq!(c.device_type, DeviceType::Keyboard);

        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x04);
        ev.push_collection(0x01, 0x08);
        assert_eq!(classify_hid(&ev, None).device_type, DeviceType::Controller);
    }

    #[test]
    fn unknown_usage_is_unknown_not_other() {
        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x0080); // System Control：不是设备类别
        ev.push_collection(0xFFB2, 0x0001); // 厂商自定义页
        let c = classify_hid(&ev, None);
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert_eq!(c.method, ClassificationMethod::Unknown);
        assert!(
            c.evidence.iter().all(|e| !e.contains("未支持")),
            "未支持的 Usage 不得出现在结论证据里：{:?}",
            c.evidence
        );
        assert_eq!(c.skipped.len(), 2, "两条未收录 Usage 都要留痕");
        assert_eq!(c.debug_text().matches("未支持").count(), 2);
    }

    #[test]
    fn unavailable_usage_falls_back_to_unknown() {
        let mut ev = HidEvidence::new(0x046D, 0xC077);
        ev.push_collection(0, 0);
        assert!(!ev.usage_available);
        assert!(ev.collections.is_empty());
        let c = classify_hid(&ev, None);
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert!(c.evidence.iter().any(|e| e.contains("Usage 字段不可用")));
    }

    #[test]
    fn usage_page_zero_never_matches_desktop_usage() {
        let ev = HidEvidence {
            vid: 1,
            pid: 2,
            collections: vec![HidCollection {
                usage_page: 0,
                usage: 0x02,
            }],
            usage_available: true,
        };
        assert_eq!(classify_hid(&ev, None).device_type, DeviceType::Unknown);
    }

    #[test]
    fn composite_hid_with_two_types_is_conflict() {
        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x06); // 键盘集合
        ev.push_collection(0x01, 0x02); // 鼠标集合（复合设备常见）
        let c = classify_hid(&ev, None);
        assert_eq!(c.device_type, DeviceType::Unknown, "复合集合不得静默择一");
        assert!(c.evidence.iter().any(|e| e.contains("复合集合")));
        assert!(c.evidence.iter().any(|e| e.contains("keyboard")));
        assert!(c.evidence.iter().any(|e| e.contains("mouse")));
        assert!(c.evidence.iter().any(|e| e.contains("缺少可核验映射")));
    }

    /// 实测回归：本仓库 2.4G 接收器 `7768:6426` 的真实集合清单（本机枚举记录）。
    ///
    /// 它同时声明鼠标与键盘集合，所以标准侧只能判成「复合」；结论来自
    /// 官方驱动配置的 `deviceType=mouse`。这条用例保证真实设备不会因为
    /// 复合集合而被降级成 Unknown。
    #[test]
    fn real_2_4g_receiver_is_resolved_by_mapping() {
        let mut ev = HidEvidence::new(0x7768, 0x6426);
        for (page, usage) in [
            (0x000Cu16, 0x0001u16),
            (0x0001, 0x0080),
            (0xFFB1, 0x0001),
            (0x0001, 0x0006),
            (0xFFB2, 0x0001),
            (0xFFB3, 0x0001),
            (0x0001, 0x0002),
        ] {
            ev.push_collection(page, usage);
        }
        assert_eq!(ev.collections.len(), 7);

        let c = classify_hid(&ev, None);
        assert_eq!(
            c.device_type,
            DeviceType::Unknown,
            "只有标准证据时必须承认无法收敛"
        );

        let mapping = VerifiedMapping::new(DeviceType::Mouse, "config.xml 1.0.7.6");
        let c = classify_hid(&ev, Some(&mapping));
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);
        assert!(c.evidence.iter().any(|e| e.contains("复合集合")));
        assert!(c.evidence.iter().any(|e| e.contains("config.xml 1.0.7.6")));
        assert_eq!(c.skipped.len(), 5, "未收录的 Usage 逐条留痕但不进 evidence");
        assert!(
            c.evidence.iter().all(|e| !e.contains("未支持")),
            "evidence 不得被未支持组合淹没：{:?}",
            c.evidence
        );
    }

    #[test]
    fn duplicate_collections_do_not_conflict() {
        let mut ev = HidEvidence::new(0x1234, 0x5678);
        ev.push_collection(0x01, 0x02);
        ev.push_collection(0x01, 0x02); // 同一集合重复声明
        let c = classify_hid(&ev, None);
        assert_eq!(ev.collections.len(), 1, "集合必须去重");
        assert_eq!(c.device_type, DeviceType::Mouse);
    }

    #[test]
    fn verified_mapping_alone_classifies() {
        let ev = HidEvidence::new(0x7768, 0x6426);
        let m = VerifiedMapping::new(DeviceType::Mouse, "config.xml 1.0.7.6");
        let c = classify_hid(&ev, Some(&m));
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(c.method, ClassificationMethod::VerifiedDeviceMapping);
        assert!(c.evidence.iter().any(|e| e.contains("config.xml")));
    }

    #[test]
    fn usage_beats_verified_mapping_when_they_agree() {
        let ev = hid_mouse();
        let m = VerifiedMapping::new(DeviceType::Mouse, "config.xml");
        let c = classify_hid(&ev, Some(&m));
        assert_eq!(c.device_type, DeviceType::Mouse);
        assert_eq!(
            c.method,
            ClassificationMethod::HidUsage,
            "同类型时保留更高优先级规则的方法"
        );
        assert_eq!(
            c.evidence
                .iter()
                .filter(|e| e.contains("HID Usage"))
                .count(),
            1
        );
        assert!(
            c.evidence.iter().any(|e| e.contains("可核验映射")),
            "证据不得丢弃"
        );
    }

    #[test]
    fn usage_conflicting_with_mapping_is_unknown() {
        let ev = hid_mouse();
        let m = VerifiedMapping::new(DeviceType::Keyboard, "config.xml");
        let c = classify_hid(&ev, Some(&m));
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert!(c.evidence.iter().any(|e| e.contains("证据冲突")));
    }

    #[test]
    fn hid_without_any_evidence_is_unknown() {
        let ev = HidEvidence::new(0x7768, 0x6426);
        let c = classify_hid(&ev, None);
        assert!(c.is_unknown());
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert_eq!(c.method, ClassificationMethod::Unknown);
        assert_eq!(c.evidence.len(), 1, "证据：{:?}", c.evidence);
        assert!(c.evidence[0].contains("Usage 字段不可用"));
        assert!(c.evidence.iter().all(|e| !e.contains("7768")));
    }

    // ---------- BLE Appearance ----------

    #[test]
    fn ble_appearance_hits_standard_types() {
        for (value, want) in [
            (0x03C1u16, DeviceType::Keyboard),
            (0x03C2, DeviceType::Mouse),
            (0x03C3, DeviceType::Controller),
            (0x03C4, DeviceType::Gamepad),
            (0x0841, DeviceType::Speaker),
            (0x0842, DeviceType::Speaker),
            (0x0843, DeviceType::Speaker),
            (0x0844, DeviceType::Speaker),
            (0x0942, DeviceType::Headset),
            (0x0943, DeviceType::Headphones),
        ] {
            let ev = BleEvidence {
                services: vec![0x180F],
                appearance: Some(value),
            };
            let c = classify_ble(&ev, None);
            assert_eq!(c.device_type, want, "appearance {value:#06x}");
            assert_eq!(c.method, ClassificationMethod::BleAppearance);
        }
    }

    #[test]
    fn generic_appearance_is_not_a_type() {
        for value in [0x0000u16, 0x03C0, 0x0840, 0x0940, 0x0941, 0x0845] {
            let ev = BleEvidence {
                services: vec![],
                appearance: Some(value),
            };
            let c = classify_ble(&ev, None);
            assert_eq!(c.device_type, DeviceType::Unknown, "value {value:#06x}");
            assert!(c.evidence.iter().any(|e| e.contains("无类型映射")));
        }
    }

    #[test]
    fn missing_appearance_does_not_panic_and_falls_back() {
        let ev = BleEvidence {
            services: vec![0x180F],
            appearance: None,
        };
        let c = classify_ble(&ev, None);
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert!(
            c.evidence
                .iter()
                .any(|e| e.contains("未取得 GAP Appearance")),
            "必须记录缺失原因，不得伪装成默认有效值"
        );
    }

    #[test]
    fn battery_service_alone_is_not_a_product_type() {
        let ev = BleEvidence {
            services: vec![0x180F, 0x180A],
            appearance: None,
        };
        let c = classify_ble(&ev, None);
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert!(c.evidence.iter().any(|e| e.contains("0x180F")));
        assert!(c.evidence.iter().any(|e| e.contains("0x180A")));
    }

    #[test]
    fn hid_service_alone_is_not_a_specific_type() {
        let ev = BleEvidence {
            services: vec![0x180F, 0x1812],
            appearance: None,
        };
        let c = classify_ble(&ev, None);
        assert_eq!(
            c.device_type,
            DeviceType::Unknown,
            "0x1812 只能确认 HID 类设备，不能给具体类型"
        );
        assert_eq!(c.method, ClassificationMethod::Unknown);
        assert!(c.evidence.iter().any(|e| e.contains("0x1812")));
    }

    #[test]
    fn appearance_survives_missing_services() {
        let ev = BleEvidence {
            services: vec![],
            appearance: Some(0x03C2),
        };
        assert_eq!(classify_ble(&ev, None).device_type, DeviceType::Mouse);
    }

    #[test]
    fn ble_mapping_conflict_is_reported() {
        let ev = BleEvidence {
            services: vec![0x180F],
            appearance: Some(0x03C1),
        };
        let m = VerifiedMapping::new(DeviceType::Mouse, "vendor sheet");
        let c = classify_ble(&ev, Some(&m));
        assert_eq!(c.device_type, DeviceType::Unknown);
        assert!(c.evidence.iter().any(|e| e.contains("证据冲突")));
    }

    // ---------- 字符串稳定性 ----------

    #[test]
    fn type_strings_are_stable() {
        assert_eq!(DeviceType::Mouse.as_str(), "mouse");
        assert_eq!(DeviceType::Headphones.as_str(), "headphones");
        assert_eq!(DeviceType::Unknown.as_str(), "unknown");
        assert_eq!(
            ClassificationMethod::BleAppearance.as_str(),
            "ble_appearance"
        );
        assert_eq!(ClassificationMethod::Unknown.as_str(), "unknown");
        assert!(DeviceType::Other.is_known());
        assert!(!DeviceType::Unknown.is_known());
        assert_eq!(DeviceType::default(), DeviceType::Unknown);
    }

    #[test]
    fn evidence_text_is_single_line() {
        let c = classify_hid(&hid_mouse(), None);
        let text = c.evidence_text();
        assert!(text.contains("HID Usage"));
        assert!(!text.contains('\n'));
    }
}
