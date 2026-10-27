//! Provider 能力声明：能力与节奏是编译期常量，不是 `settings.json`。
//! 型号表加一行不改这里，而阈值链与状态机不变量强耦合，写坏即「永不过期」。
//!
//! 可调数值（三档倍率、各家族阈值链基线）集中在 `data/profiles.json`，
//! 由 `build.rs` 生成本文件的常量；节奏与充电语义仍写死在下方声明里。

use crate::transport::Transport;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/profiles_generated.rs"));
}

use generated::{BLE_BAS_THRESHOLDS, HID_VBAS_THRESHOLDS, SENSITIVITY_RATIO};

/// 充电状态的可获得性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChargingSupport {
    /// 读数自带充电标志（HID：`payload[2]`，`0x01` = 充电中）。
    FromReading,
    /// 协议层不可得（BLE）→ UI 显示 `Unknown`，**不得**显示「未充电」。
    Unavailable,
}

/// 状态机退化阈值链（毫秒）。
///
/// 不变量 `stale < unavailable < offline` 由 [`ProviderProfile::validate`] 在构造期保证。
///
/// 与低电量百分比档位（`NotificationConfig::low_battery_levels`）无关：曾名 `Thresholds`，两义同词，故加 `Degradation` 前缀。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradationThresholds {
    /// 超过该时长未收到有效读数 → `Stale`。
    pub stale_ms: u64,
    /// 超过该时长未收到有效读数 → `Unavailable`。
    pub unavailable_ms: u64,
    /// 超过该时长未收到有效读数 → `Offline`。
    pub offline_ms: u64,
}

impl DegradationThresholds {
    /// 档位乘数作用于整条链，用整数 `(num, den)`：`(2,1)`/`(1,1)`/`(1,2)`。
    /// 全部阈值都是 500ms 的整数倍，`/2` 无损；若将来引入奇数毫秒值，须先把分母改为 1000 并向下取整，
    /// 不得引入浮点（会让边界测试出现 ±1ms 抖动）。
    pub fn scaled(self, sensitivity: Sensitivity) -> Self {
        let (num, den) = sensitivity.ratio();
        Self {
            stale_ms: self.stale_ms * num / den,
            unavailable_ms: self.unavailable_ms * num / den,
            offline_ms: self.offline_ms * num / den,
        }
    }
}

/// 用户可选的阈值档位。设置里只存档位，**不存毫秒**。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sensitivity {
    /// 敏感 ×0.5。
    Sensitive,
    /// 标准 ×1。
    #[default]
    Standard,
    /// 宽松 ×2。
    Loose,
}

impl Sensitivity {
    /// 整数乘数 `(num, den)`，取自 `data/profiles.json`。
    pub const fn ratio(self) -> (u64, u64) {
        let index = match self {
            Self::Sensitive => 0,
            Self::Standard => 1,
            Self::Loose => 2,
        };
        SENSITIVITY_RATIO[index]
    }

    /// 落盘字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sensitive => "sensitive",
            Self::Standard => "standard",
            Self::Loose => "loose",
        }
    }

    /// 解析落盘字符串（严格，未知值返回 `None`，调用方用默认档位）。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "sensitive" => Some(Self::Sensitive),
            "standard" => Some(Self::Standard),
            "loose" => Some(Self::Loose),
            _ => None,
        }
    }
}

/// 保底轮询区间。下限由 Provider 给出（它才知道一轮刷新的耗电代价），宿主不得放宽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PollPolicy {
    /// 用户可调下限。
    pub min_ms: u64,
    /// 用户可调上限。
    pub max_ms: u64,
    /// 默认值。
    pub default_ms: u64,
}

/// 采集节奏（**从宿主视角命名**：节奏由谁发起、本端做什么）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    /// 纯监听：宿主从不向设备发报文，读数节奏完全由设备决定（2.4G 实测：不发包也有数据）。
    ListenOnly {
        /// 实测期望间隔，仅用于诊断偏离告警。
        expected_interval_ms: u64,
    },
    /// 周期查询：宿主按 `interval_ms` 主动发查询报文，节奏由宿主决定。
    Polled {
        /// 查询周期。
        interval_ms: u64,
    },
    /// 订阅通知 + 兜底轮询（BLE 实测：两条路都要有）。
    NotifyWithPoll {
        /// 轮询区间。
        poll: PollPolicy,
    },
}

/// Provider 能力声明的完整形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderProfile {
    /// `<transport>-<protocol>` 形式的家族 ID。
    pub family_id: &'static str,
    /// 传输层。
    pub transport: Transport,
    /// 采集节奏。
    pub cadence: Cadence,
    /// 标准档阈值链。
    pub thresholds: DegradationThresholds,
    /// 充电状态可获得性。
    pub charging: ChargingSupport,
    /// 出现检测间隔；`None` = 复用轮询周期。仅允许 HID。
    pub presence_poll_ms: Option<u64>,
}

impl ProviderProfile {
    /// 取该档位下的阈值链。
    pub fn thresholds_for(&self, sensitivity: Sensitivity) -> DegradationThresholds {
        self.thresholds.scaled(sensitivity)
    }

    /// 充电语义收敛的唯一实现点：充电不可得时把任何 `Some(bool)` 压成 `None`。
    /// 宿主在把读数写进状态机前必须经过这里，否则 BLE 会被渲染成未充电。
    pub fn sanitize_charging(&self, raw: Option<bool>) -> Option<bool> {
        match self.charging {
            ChargingSupport::FromReading => raw,
            ChargingSupport::Unavailable => None,
        }
    }

    /// 构造期不变量，进程启动时跑一次（[`validate_registry`] 内部调用）。
    pub fn validate(&self) -> Result<(), ProfileError> {
        // 家族 ID 必须 `<transport>-<protocol>`，品牌名不得占据 transport 段或单独构成 ID。
        // 协议段允许带厂商标识，故不能简单黑名单整串；`hid-vbas` 已把品牌从 ID 移除，收敛为「协议 = 家族」。
        let prefix = self.transport.as_str();
        let mut segments = self.family_id.split('-');
        let first = segments.next().unwrap_or_default();
        if first != prefix {
            return Err(ProfileError::TransportPrefixMismatch {
                family_id: self.family_id,
                transport: self.transport,
            });
        }
        match segments.next() {
            Some(protocol) if !protocol.is_empty() => {}
            _ => {
                return Err(ProfileError::FamilyIdWithoutProtocol {
                    family_id: self.family_id,
                });
            }
        }
        for brand in BRAND_BLACKLIST {
            if self.family_id.eq_ignore_ascii_case(brand) || first.eq_ignore_ascii_case(brand) {
                return Err(ProfileError::BrandInFamilyId {
                    family_id: self.family_id,
                });
            }
        }

        // stale < unavailable < offline。
        let t = self.thresholds;
        if !(t.stale_ms < t.unavailable_ms && t.unavailable_ms < t.offline_ms) {
            return Err(ProfileError::ThresholdOrder {
                stale_ms: t.stale_ms,
                unavailable_ms: t.unavailable_ms,
                offline_ms: t.offline_ms,
            });
        }

        // 阈值必须显著大于采集周期，否则边界抖动。
        match self.cadence {
            Cadence::NotifyWithPoll { poll } => {
                // 先校验区间，再校验与周期之间的关系。
                if poll.min_ms < POLL_MIN_FLOOR_MS
                    || poll.max_ms > POLL_MAX_CEIL_MS
                    || !(poll.min_ms <= poll.default_ms && poll.default_ms <= poll.max_ms)
                {
                    return Err(ProfileError::PollRangeInvalid {
                        min_ms: poll.min_ms,
                        max_ms: poll.max_ms,
                        default_ms: poll.default_ms,
                    });
                }
                if t.stale_ms * 2 < poll.min_ms * 3 {
                    return Err(ProfileError::StaleBelowPollFloor {
                        stale_ms: t.stale_ms,
                        poll_min_ms: poll.min_ms,
                    });
                }
            }
            Cadence::ListenOnly {
                expected_interval_ms,
            }
            | Cadence::Polled {
                interval_ms: expected_interval_ms,
            } => {
                if t.stale_ms * 2 < expected_interval_ms * 3 {
                    return Err(ProfileError::StaleBelowExpectedInterval {
                        stale_ms: t.stale_ms,
                        expected_interval_ms,
                    });
                }
            }
        }

        // 出现检测间隔只允许出现在 HID。
        match (self.presence_poll_ms, self.transport) {
            (Some(0), _) => return Err(ProfileError::PresencePollZero),
            (Some(_), Transport::Ble) => {
                return Err(ProfileError::PresencePollNotAllowed {
                    transport: self.transport,
                });
            }
            _ => {}
        }

        Ok(())
    }
}

/// 轮询间隔下限：30s。
pub const POLL_MIN_FLOOR_MS: u64 = 30_000;
/// 轮询间隔上限：300s。
pub const POLL_MAX_CEIL_MS: u64 = 300_000;

/// 品牌黑名单：协议家族不能仅按品牌划分。
pub const BRAND_BLACKLIST: [&str; 2] = ["ajazz", "xctech"];

/// 能力声明的构造期错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProfileError {
    /// 阈值链顺序错误。
    ThresholdOrder {
        /// `staleAfter`。
        stale_ms: u64,
        /// `unavailableAfter`。
        unavailable_ms: u64,
        /// `offlineAfter`。
        offline_ms: u64,
    },
    /// `staleAfter` 相对轮询下限过小。
    StaleBelowPollFloor {
        /// `staleAfter`。
        stale_ms: u64,
        /// 轮询下限。
        poll_min_ms: u64,
    },
    /// `staleAfter` 相对期望上报间隔过小。
    StaleBelowExpectedInterval {
        /// `staleAfter`。
        stale_ms: u64,
        /// 期望间隔。
        expected_interval_ms: u64,
    },
    /// 家族 ID 使用了品牌名作为 transport 段。
    BrandInFamilyId {
        /// 家族 ID。
        family_id: &'static str,
    },
    /// 家族 ID 的前缀与 `transport` 不一致。
    TransportPrefixMismatch {
        /// 家族 ID。
        family_id: &'static str,
        /// 声明的 transport。
        transport: Transport,
    },
    /// 家族 ID 只有 transport，没有协议段。
    FamilyIdWithoutProtocol {
        /// 家族 ID。
        family_id: &'static str,
    },
    /// `presence_poll_ms` 出现在 BLE 上。
    PresencePollNotAllowed {
        /// 声明了出现检测的 transport。
        transport: Transport,
    },
    /// 出现检测间隔为 0。
    PresencePollZero,
    /// 轮询区间非法。
    PollRangeInvalid {
        /// 下限。
        min_ms: u64,
        /// 上限。
        max_ms: u64,
        /// 默认值。
        default_ms: u64,
    },
    /// 注册表出现重复家族 ID。
    DuplicateFamilyId {
        /// 重复的家族 ID。
        family_id: &'static str,
    },
    /// 注册表出现重复 transport。
    DuplicateTransport {
        /// 重复的 transport。
        transport: Transport,
    },
}

impl core::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ThresholdOrder {
                stale_ms,
                unavailable_ms,
                offline_ms,
            } => write!(
                f,
                "阈值链必须 stale<unavailable<offline，实际 {stale_ms}/{unavailable_ms}/{offline_ms}"
            ),
            Self::StaleBelowPollFloor {
                stale_ms,
                poll_min_ms,
            } => write!(
                f,
                "staleAfter={stale_ms}ms 必须 >= 1.5×轮询下限({poll_min_ms}ms)"
            ),
            Self::StaleBelowExpectedInterval {
                stale_ms,
                expected_interval_ms,
            } => write!(
                f,
                "staleAfter={stale_ms}ms 必须 >= 1.5×期望间隔({expected_interval_ms}ms)"
            ),
            Self::BrandInFamilyId { family_id } => {
                write!(f, "家族 ID {family_id} 不得以品牌名标识协议")
            }
            Self::TransportPrefixMismatch {
                family_id,
                transport,
            } => write!(
                f,
                "家族 ID {family_id} 必须以 \"{}-*\" 开头",
                transport.as_str()
            ),
            Self::FamilyIdWithoutProtocol { family_id } => {
                write!(f, "家族 ID {family_id} 缺少协议段")
            }
            Self::PresencePollNotAllowed { transport } => write!(
                f,
                "presence_poll_ms 仅允许 HID，实际 transport={}",
                transport.as_str()
            ),
            Self::PresencePollZero => write!(f, "presence_poll_ms 不得为 0"),
            Self::PollRangeInvalid {
                min_ms,
                max_ms,
                default_ms,
            } => write!(
                f,
                "轮询区间非法 min={min_ms} default={default_ms} max={max_ms}（要求 30000..=300000 且 min<=default<=max）"
            ),
            Self::DuplicateFamilyId { family_id } => {
                write!(f, "家族 ID {family_id} 重复")
            }
            Self::DuplicateTransport { transport } => {
                write!(f, "transport {} 被多个 Provider 占用", transport.as_str())
            }
        }
    }
}

impl std::error::Error for ProfileError {}

/// `hid-vbas` 的能力声明。阈值链基线在 `data/profiles.json`，节奏在此固定，有测试断言防止漂移。
/// 家族级能力声明只有一份：品牌再多也不新增家族；品牌各自的上报节奏由
/// `voltnest-hid` 的品牌描述声明，并在启动期与本声明做兼容校验。
///
/// 家族内既有**纯监听**品牌（AJAZZ、雷柏纯监听族：宿主从不发包，节奏由设备决定），
/// 也有**周期查询**品牌（达尔优、迈从、雷柏 AP：宿主每 30s 问一次）。家族级只声明
/// 同一个 30s 预算（stale 阈值链按它校验），谁发起由各品牌的
/// `BrandSpec::cadence()` 说明。
pub static HID_VBAS: ProviderProfile = ProviderProfile {
    family_id: crate::FAMILY_HID_VBAS,
    transport: Transport::Hid,
    cadence: Cadence::Polled {
        interval_ms: 30_000,
    },
    thresholds: HID_VBAS_THRESHOLDS,
    charging: ChargingSupport::FromReading,
    // 内部常量，不进 settings.json。
    presence_poll_ms: Some(5_000),
};

/// `ble-bas` 的能力声明。阈值链基线在 `data/profiles.json`。
pub static BLE_BAS: ProviderProfile = ProviderProfile {
    family_id: crate::FAMILY_BLE_BAS,
    transport: Transport::Ble,
    cadence: Cadence::NotifyWithPoll {
        poll: PollPolicy {
            min_ms: 60_000,
            max_ms: 300_000,
            default_ms: 60_000,
        },
    },
    thresholds: BLE_BAS_THRESHOLDS,
    // 0x2BF0 / 0x2BEA 未实现，0x2BED 失真（BLE 充电状态不可用）。
    charging: ChargingSupport::Unavailable,
    // 出现由 60s 巡检天然覆盖。
    presence_poll_ms: None,
};

/// 启动期自检，断言 `(family_id, transport)` 唯一。违反时宿主不启动该 Provider，
/// 诊断页红条 + 稳定错误码 `PROFILE_CONFLICT`，宿主照常运行；v1 不做运行期优先级仲裁。
pub fn validate_registry(profiles: &[&'static ProviderProfile]) -> Result<(), ProfileError> {
    for p in profiles {
        p.validate()?;
    }
    for (i, p) in profiles.iter().enumerate() {
        for other in profiles.iter().skip(i + 1) {
            if p.family_id == other.family_id {
                return Err(ProfileError::DuplicateFamilyId {
                    family_id: p.family_id,
                });
            }
            if p.transport == other.transport {
                return Err(ProfileError::DuplicateTransport {
                    transport: p.transport,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_hid() -> ProviderProfile {
        HID_VBAS
    }

    fn base_ble() -> ProviderProfile {
        BLE_BAS
    }

    // ---------- 阈值链顺序 ----------

    #[test]
    fn pc1_accepts_canonical_chain() {
        assert!(base_hid().validate().is_ok());
        assert!(base_ble().validate().is_ok());
    }

    #[test]
    fn pc1_rejects_equal_or_inverted_chain() {
        let mut p = base_hid();
        p.thresholds.unavailable_ms = p.thresholds.stale_ms;
        assert!(matches!(
            p.validate(),
            Err(ProfileError::ThresholdOrder { .. })
        ));

        let mut p = base_hid();
        p.thresholds.offline_ms = p.thresholds.unavailable_ms;
        assert!(matches!(
            p.validate(),
            Err(ProfileError::ThresholdOrder { .. })
        ));

        let mut p = base_hid();
        p.thresholds.stale_ms = 200_000; // 大于 offline
        assert!(matches!(
            p.validate(),
            Err(ProfileError::ThresholdOrder { .. })
        ));
    }

    // ---------- stale 与轮询下限 ----------

    /// 该家族的 `stale` 恰好 1.5× 采集周期的毫秒数：`validate` 的通过边界。
    fn stale_floor(p: &ProviderProfile) -> u64 {
        let interval = match p.cadence {
            Cadence::NotifyWithPoll { poll } => poll.min_ms,
            Cadence::ListenOnly {
                expected_interval_ms,
            }
            | Cadence::Polled {
                interval_ms: expected_interval_ms,
            } => expected_interval_ms,
        };
        interval * 3 / 2
    }

    #[test]
    fn pc2_ble_boundary_is_exact() {
        // 恰好等于 1.5× 轮询下限时通过，差 1ms 即拒绝。
        let boundary = stale_floor(&base_ble());
        let mut p = base_ble();
        p.thresholds.stale_ms = boundary;
        assert!(p.validate().is_ok());

        p.thresholds.stale_ms = boundary - 1;
        assert!(matches!(
            p.validate(),
            Err(ProfileError::StaleBelowPollFloor { .. })
        ));
    }

    // ---------- stale 与期望间隔 ----------

    #[test]
    fn pc3_hid_boundary_is_exact() {
        // 恰好等于 1.5× 期望间隔时通过，差 1ms 即拒绝。
        let boundary = stale_floor(&base_hid());
        let mut p = base_hid();
        p.thresholds.stale_ms = boundary;
        p.thresholds.unavailable_ms = boundary + 1;
        p.validate().unwrap();

        p.thresholds.stale_ms = boundary - 1;
        assert!(matches!(
            p.validate(),
            Err(ProfileError::StaleBelowExpectedInterval { .. })
        ));
    }

    // ---------- 家族 ID ----------

    #[test]
    fn pc4_rejects_brand_as_transport() {
        let mut p = base_hid();
        p.family_id = "ajazz-hid";
        assert!(matches!(
            p.validate(),
            Err(ProfileError::TransportPrefixMismatch { .. })
        ));

        let mut p = base_hid();
        p.family_id = "ajazz";
        assert!(matches!(
            p.validate(),
            Err(ProfileError::TransportPrefixMismatch { .. })
        ));

        let mut p = base_hid();
        p.family_id = "xctech-lightspeed";
        assert!(matches!(
            p.validate(),
            Err(ProfileError::TransportPrefixMismatch { .. })
        ));
    }

    #[test]
    fn pc4_allows_vendor_as_protocol_segment() {
        // D1：crate 名与目录名不用品牌，但家族 ID 的协议段允许带厂商标识以区分实现。
        // `hid-vbas` 本身不需要它；这里验证「允许」而非「要求」。
        let mut p = base_hid();
        p.family_id = "hid-vbas-ajazz";
        assert!(p.validate().is_ok());
    }

    #[test]
    fn pc4_requires_protocol_segment() {
        let mut p = base_hid();
        p.family_id = "hid-";
        assert!(matches!(
            p.validate(),
            Err(ProfileError::FamilyIdWithoutProtocol { .. })
        ));

        let mut p = base_hid();
        p.family_id = "hid";
        assert!(matches!(
            p.validate(),
            Err(ProfileError::FamilyIdWithoutProtocol { .. })
        ));
    }

    // ---------- 充电语义 ----------

    #[test]
    fn pc5_charging_unavailable_forces_none() {
        // 正例：BLE 的充电语义不可得，任何输入都必须是 None。
        assert_eq!(base_ble().sanitize_charging(Some(true)), None);
        assert_eq!(base_ble().sanitize_charging(Some(false)), None);
        assert_eq!(base_ble().sanitize_charging(None), None);
        assert_eq!(base_ble().charging, ChargingSupport::Unavailable);
    }

    #[test]
    fn pc5_charging_from_reading_is_preserved() {
        assert_eq!(base_hid().sanitize_charging(Some(true)), Some(true));
        assert_eq!(base_hid().sanitize_charging(Some(false)), Some(false));
        assert_eq!(base_hid().sanitize_charging(None), None);
    }

    // ---------- 出现检测间隔 ----------

    #[test]
    fn pc6_presence_poll_only_for_hid() {
        assert_eq!(base_hid().presence_poll_ms, Some(5_000));
        assert!(base_hid().validate().is_ok());
        assert_eq!(base_ble().presence_poll_ms, None);
        assert!(base_ble().validate().is_ok());

        let mut p = base_ble();
        p.presence_poll_ms = Some(5_000);
        assert!(matches!(
            p.validate(),
            Err(ProfileError::PresencePollNotAllowed { .. })
        ));
    }

    #[test]
    fn pc6_rejects_zero_presence_poll() {
        let mut p = base_hid();
        p.presence_poll_ms = Some(0);
        assert_eq!(p.validate(), Err(ProfileError::PresencePollZero));
    }

    // ---------- 轮询区间 ----------

    #[test]
    fn pc7_enforces_range_and_order() {
        let mut p = base_ble();
        p.cadence = Cadence::NotifyWithPoll {
            poll: PollPolicy {
                min_ms: 20_000,
                max_ms: 300_000,
                default_ms: 60_000,
            },
        };
        assert!(matches!(
            p.validate(),
            Err(ProfileError::PollRangeInvalid { .. })
        ));

        let mut p = base_ble();
        p.cadence = Cadence::NotifyWithPoll {
            poll: PollPolicy {
                min_ms: 60_000,
                max_ms: 400_000,
                default_ms: 60_000,
            },
        };
        assert!(matches!(
            p.validate(),
            Err(ProfileError::PollRangeInvalid { .. })
        ));

        let mut p = base_ble();
        p.cadence = Cadence::NotifyWithPoll {
            poll: PollPolicy {
                min_ms: 60_000,
                max_ms: 120_000,
                default_ms: 50_000,
            },
        };
        assert!(matches!(
            p.validate(),
            Err(ProfileError::PollRangeInvalid { .. })
        ));

        let mut p = base_ble();
        p.cadence = Cadence::NotifyWithPoll {
            poll: PollPolicy {
                min_ms: 60_000,
                max_ms: 120_000,
                default_ms: 130_000,
            },
        };
        assert!(matches!(
            p.validate(),
            Err(ProfileError::PollRangeInvalid { .. })
        ));
    }

    #[test]
    fn pc7_accepts_full_legal_range() {
        let mut p = base_ble();
        p.cadence = Cadence::NotifyWithPoll {
            poll: PollPolicy {
                min_ms: 30_000,
                max_ms: 300_000,
                default_ms: 30_000,
            },
        };
        // poll.min=30s → 要求 stale >= 45s，BLE 是 90s，通过。
        assert!(p.validate().is_ok());
    }

    // ---------- 数值不变量（具体毫秒以 `data/profiles.json` 为唯一来源）----------

    #[test]
    fn baseline_thresholds_are_half_second_aligned() {
        // 整数除法缩放要求 500ms 对齐，否则会出现毫秒级漂移。
        for p in [HID_VBAS, BLE_BAS] {
            for value in [
                p.thresholds.stale_ms,
                p.thresholds.unavailable_ms,
                p.thresholds.offline_ms,
            ] {
                assert_eq!(value % 500, 0, "{}: {value}ms 未对齐 500ms", p.family_id);
            }
        }
    }

    #[test]
    fn sensitivity_scales_whole_chain() {
        // 三值同比缩放，不能只改 staleAfter。
        for p in [HID_VBAS, BLE_BAS] {
            let base = p.thresholds_for(Sensitivity::Standard);
            for (level, num, den) in [
                (Sensitivity::Sensitive, 1, 2),
                (Sensitivity::Standard, 1, 1),
                (Sensitivity::Loose, 2, 1),
            ] {
                let (actual_num, actual_den) = level.ratio();
                assert_eq!((actual_num, actual_den), (num, den), "{level:?}");
                let t = p.thresholds_for(level);
                assert_eq!(t.stale_ms, base.stale_ms * num / den, "{level:?}");
                assert_eq!(
                    t.unavailable_ms,
                    base.unavailable_ms * num / den,
                    "{level:?}"
                );
                assert_eq!(t.offline_ms, base.offline_ms * num / den, "{level:?}");
            }
        }
    }

    #[test]
    fn scaling_keeps_chain_order_for_every_level() {
        // D3 的核心收益：无需 clamp、无特例分支。
        for p in [HID_VBAS, BLE_BAS] {
            for s in [
                Sensitivity::Sensitive,
                Sensitivity::Standard,
                Sensitivity::Loose,
            ] {
                let t = p.thresholds_for(s);
                assert!(t.stale_ms < t.unavailable_ms, "{:?} {:?}", p.family_id, s);
                assert!(t.unavailable_ms < t.offline_ms, "{:?} {:?}", p.family_id, s);
            }
        }
    }

    #[test]
    fn cadence_matches_spec() {
        assert_eq!(
            HID_VBAS.cadence,
            Cadence::Polled {
                interval_ms: 30_000
            }
        );
        assert_eq!(
            BLE_BAS.cadence,
            Cadence::NotifyWithPoll {
                poll: PollPolicy {
                    min_ms: 60_000,
                    max_ms: 300_000,
                    default_ms: 60_000
                }
            }
        );
    }

    #[test]
    fn sensitivity_strings_round_trip() {
        for s in [
            Sensitivity::Sensitive,
            Sensitivity::Standard,
            Sensitivity::Loose,
        ] {
            assert_eq!(Sensitivity::parse(s.as_str()), Some(s));
        }
        assert_eq!(Sensitivity::parse("Loose"), None);
        assert_eq!(Sensitivity::parse(""), None);
        assert_eq!(Sensitivity::default(), Sensitivity::Standard);
    }

    // ---------- 注册表自检 ----------

    #[test]
    fn registry_accepts_v1_providers() {
        assert_eq!(validate_registry(&[&HID_VBAS, &BLE_BAS]), Ok(()));
    }

    #[test]
    fn registry_rejects_duplicate_family() {
        static DUP: ProviderProfile = ProviderProfile {
            family_id: "ble-bas",
            transport: Transport::Ble,
            cadence: Cadence::NotifyWithPoll {
                poll: PollPolicy {
                    min_ms: 60_000,
                    max_ms: 300_000,
                    default_ms: 60_000,
                },
            },
            thresholds: DegradationThresholds {
                stale_ms: 90_000,
                unavailable_ms: 120_000,
                offline_ms: 360_000,
            },
            charging: ChargingSupport::Unavailable,
            presence_poll_ms: None,
        };
        assert_eq!(
            validate_registry(&[&BLE_BAS, &DUP]),
            Err(ProfileError::DuplicateFamilyId {
                family_id: "ble-bas"
            })
        );
    }

    #[test]
    fn registry_rejects_duplicate_transport() {
        static DUP: ProviderProfile = ProviderProfile {
            family_id: "hid-vbas-other",
            transport: Transport::Hid,
            cadence: Cadence::Polled {
                interval_ms: 30_000,
            },
            thresholds: DegradationThresholds {
                stale_ms: 45_000,
                unavailable_ms: 60_000,
                offline_ms: 180_000,
            },
            charging: ChargingSupport::FromReading,
            presence_poll_ms: Some(5_000),
        };
        assert_eq!(
            validate_registry(&[&HID_VBAS, &DUP]),
            Err(ProfileError::DuplicateTransport {
                transport: Transport::Hid
            })
        );
    }

    #[test]
    fn registry_validates_members_first() {
        static BROKEN: ProviderProfile = ProviderProfile {
            family_id: "hid-vbas-broken",
            transport: Transport::Hid,
            cadence: Cadence::Polled {
                interval_ms: 30_000,
            },
            thresholds: DegradationThresholds {
                stale_ms: 10_000,
                unavailable_ms: 60_000,
                offline_ms: 180_000,
            },
            charging: ChargingSupport::FromReading,
            presence_poll_ms: None,
        };
        assert!(matches!(
            validate_registry(&[&BROKEN]),
            Err(ProfileError::StaleBelowExpectedInterval { .. })
        ));
    }

    #[test]
    fn error_display_is_human_readable() {
        let msg = ProfileError::ThresholdOrder {
            stale_ms: 1,
            unavailable_ms: 2,
            offline_ms: 3,
        }
        .to_string();
        assert!(msg.contains("阈值链"));
        assert!(msg.contains("1/2/3"));
    }
}
