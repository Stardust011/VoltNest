//! `hid-vbas` 的通用协议描述（**品牌无关**）。
//!
//! 一个品牌接入 = 一份 [`BrandSpec`]：怎么匹配、帧长什么样、电量与充电标志落在第几个字节、是否要主动查询。解析器（[`crate::frame`]）与运行时（[`crate::provider`]）只读这些字段，无 `match brand_id` 分支，新增品牌不改任何既有代码。
//!
//! 两层：**家族**（`hid-vbas`，协议形状：HID 私有通道+定长报告+命令字，能力在 [`voltnest_provider::HID_VBAS`]）与**品牌**（[`BrandSpec`]，填充 VID/PID 表、UsagePage、字段下标、充电取值、查询报文）。差异只在数据。
//!
//! [`BrandSpec::validate`] 在首次枚举前跑一次，把「字段下标越界」等手写错误挡在读线程外：写错即启动失败，不会带病解析出错误电量。

use voltnest_provider::{Cadence, ChargingSupport, ProviderProfile};

/// 帧头布局：把一帧切成「头部 → payload」，其余字节是填充。
///
/// 头部只描述**位置**，不描述语义；语义在 [`BatteryFields`]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameLayout {
    /// 报告 ID（报告首字节）。`None` = 该协议不带报告 ID / 不校验。
    pub report_id: Option<u8>,
    /// 命令字所在下标（绝对下标）。
    pub cmd_index: usize,
    /// payload 长度字段的下标（绝对下标）。`None` = 长度固定为 `fixed_payload_len`。
    pub len_index: Option<usize>,
    /// payload 起始下标（绝对下标）。
    pub payload_offset: usize,
    /// `len_index` 为 `None` 时的 payload 长度。
    pub fixed_payload_len: usize,
}

/// 充电标志的定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChargingField {
    /// 标志在 payload 内的下标。
    pub index: usize,
    /// 表示「充电中」的取值。其余取值一律视为「未充电」，不猜测语义。
    pub true_value: u8,
}

/// 电量字段定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryFields {
    /// 命令字匹配值（`FrameLayout::cmd_index` 处的期望字节）。
    pub cmd: u8,
    /// payload 内的子标签：`(下标, 期望值)`。`None` = 该协议没有子标签。
    pub subtag: Option<(usize, u8)>,
    /// 电量百分比在 payload 内的下标（`0..=100`，越界即 `PercentOutOfRange`）。
    pub percent_index: usize,
    /// 充电标志。`None` = 协议不提供充电状态 → 读数恒为 `None`。
    pub charging: Option<ChargingField>,
}

/// 主动查询模板：向写通道发送一条固定报文，把电量「问」出来。
///
/// 这是与「被动监听推送帧」并列的另一条采集路径。是否启用由 [`BrandSpec`]
/// 决定；已验证设备走纯推送（`request: None`），不向设备发任何报文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestTemplate {
    /// 写通道的 UsagePage。
    pub usage_page: u16,
    /// 报文中携带的命令字（查询命令，可与推送帧的命令字不同）。
    pub cmd: u8,
    /// 报文总长（HID 输出报告长度，不足补 0）。
    pub report_len: usize,
    /// 命令字所在下标。
    pub cmd_index: usize,
    /// payload 起始下标。
    pub payload_offset: usize,
    /// payload 固定内容（不含命令字）。
    pub payload: &'static [u8],
    /// 周期发送间隔；`None` = 仅在会话建立时发一次。
    pub interval_ms: Option<u64>,
}

impl RequestTemplate {
    /// 按模板拼一帧可直接 `write` 的报文。
    ///
    /// 若协议带报告 ID，报告 ID 必须写在首字节 —— 这是 hidapi 的约定，
    /// 写错会被设备当成普通数据丢掉，表现为「查询永远没响应」。
    pub fn build(&self, report_id: Option<u8>) -> Vec<u8> {
        let mut buf = vec![0u8; self.report_len];
        if let Some(id) = report_id
            && let Some(first) = buf.first_mut()
        {
            *first = id;
        }
        if let Some(slot) = buf.get_mut(self.cmd_index) {
            *slot = self.cmd;
        }
        let end = self
            .payload_offset
            .saturating_add(self.payload.len())
            .min(self.report_len);
        if let Some(dst) = buf.get_mut(self.payload_offset..end) {
            let n = dst.len();
            dst.copy_from_slice(&self.payload[..n]);
        }
        buf
    }

    /// payload 在报文中的结束下标。
    pub const fn data_end(&self) -> usize {
        self.payload_offset + self.payload.len()
    }
}

/// 一个品牌的完整接入描述。
#[derive(Clone, Copy, Debug)]
pub struct BrandSpec {
    /// 稳定标识（日志 / 诊断用，**不参与**设备匹配）。例如 `ajazz`。
    pub id: &'static str,
    /// 人类可读的品牌 / 代际名。例如 `AJAZZ（W1 / 星闪代）`。
    pub label: &'static str,
    /// 读取通道的 UsagePage。
    pub usage_page: u16,
    /// 只写命令通道的 UsagePage。`None` = 该品牌不主动发报文。
    pub usage_page_write: Option<u16>,
    /// 帧头布局。
    pub frame: FrameLayout,
    /// 电量字段。
    pub battery: BatteryFields,
    /// 主动查询模板。`None` = 纯推送（不向设备发任何报文）。
    pub request: Option<RequestTemplate>,
    /// 实测的推送期望间隔（毫秒），仅用于诊断与家族级节奏校验。
    pub expected_interval_ms: u64,
    /// 型号表快照（`VID:PID → 型号`），编译期嵌入。
    pub device_table_json: &'static str,
}

impl BrandSpec {
    /// 解析一帧所必需的最短 payload：必选字段（子标签、电量）都要落在里面。
    ///
    /// 充电标志**不计入**：它是可选字段，缺失表示「未知」，不得把缺一个字节
    /// 的旧固件判成坏帧（会直接把可用读数变成 `Unavailable`）。
    pub const fn min_payload_len(&self) -> usize {
        let percent = self.battery.percent_index + 1;
        match self.battery.subtag {
            Some((index, _)) => {
                let subtag = index + 1;
                if subtag > percent { subtag } else { percent }
            }
            None => percent,
        }
    }

    /// 一帧报告的最小长度。
    pub const fn min_report_len(&self) -> usize {
        self.frame.payload_offset + self.min_payload_len()
    }

    /// 该品牌期望的采集节奏（家族级 `ProviderProfile` 必须覆盖它）。
    pub const fn cadence(&self) -> Cadence {
        Cadence::PushOnly {
            expected_interval_ms: self.expected_interval_ms,
        }
    }

    /// 构造期不变量。进程内首次枚举前跑一次。
    pub fn validate(&self) -> Result<(), SpecError> {
        // 标识与展示名不得为空。
        if self.id.is_empty() {
            return Err(SpecError::EmptyBrandId);
        }
        if self.label.is_empty() {
            return Err(SpecError::EmptyLabel { brand: self.id });
        }

        // 命令字必须在 payload 之前 —— 否则「头」和「体」重叠，
        // 同一字节会被当成两种语义。
        if self.frame.cmd_index >= self.frame.payload_offset {
            return Err(SpecError::HeaderOverlap {
                brand: self.id,
                cmd_index: self.frame.cmd_index,
                payload_offset: self.frame.payload_offset,
            });
        }

        // 报告 ID 占首字节，此时 payload 不能从 0 开始。
        if self.frame.report_id.is_some() && self.frame.payload_offset == 0 {
            return Err(SpecError::PayloadStartsAtReportId { brand: self.id });
        }

        // 长度字段必须在头部。
        if let Some(index) = self.frame.len_index
            && (index < self.frame.cmd_index || index >= self.frame.payload_offset)
        {
            return Err(SpecError::LengthFieldOutsideHeader {
                brand: self.id,
                len_index: index,
                cmd_index: self.frame.cmd_index,
                payload_offset: self.frame.payload_offset,
            });
        }

        // 固定长度模式必须给出非零长度，否则所有帧都会被判坏帧。
        if self.frame.len_index.is_none() && self.frame.fixed_payload_len < self.min_payload_len() {
            return Err(SpecError::FixedPayloadTooShort {
                brand: self.id,
                fixed_payload_len: self.frame.fixed_payload_len,
                need: self.min_payload_len(),
            });
        }

        // 子标签不得与电量字段抢同一字节。
        if let Some((index, _)) = self.battery.subtag
            && index == self.battery.percent_index
        {
            return Err(SpecError::SubtagOverlapsPercent { brand: self.id });
        }

        // 充电标志不得与电量字段抢同一字节。
        if let Some(charging) = self.battery.charging
            && charging.index == self.battery.percent_index
        {
            return Err(SpecError::ChargingOverlapsPercent { brand: self.id });
        }

        // 查询模板必须自洽，且声明了写通道。
        if let Some(req) = self.request {
            if self.usage_page_write.is_none() {
                return Err(SpecError::RequestWithoutWritePage { brand: self.id });
            }
            if req.data_end() > req.report_len
                || req.cmd_index >= req.payload_offset
                || req.report_len < self.frame.payload_offset
            {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
            if self.frame.report_id.is_some() && req.report_len == 0 {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
        }

        // 节奏必须为正，否则「期望间隔」失去意义。
        if self.expected_interval_ms == 0 {
            return Err(SpecError::ZeroInterval { brand: self.id });
        }

        Ok(())
    }

    /// 品牌节奏是否落在家族能力声明的预算内。
    ///
    /// 家族只有一个 `stale` 阈值链；若某品牌的推送间隔慢到
    /// `1.5 × 间隔 > stale_ms`，它的正常上报会被判过期 —— 这类品牌必须先
    /// 改家族声明的阈值链，不能靠加载顺序或特例绕开。
    pub fn validate_against(&self, profile: &ProviderProfile) -> Result<(), SpecError> {
        self.validate()?;
        if profile.transport != voltnest_provider::Transport::Hid {
            return Err(SpecError::ProfileTransportMismatch { brand: self.id });
        }
        if profile.charging == ChargingSupport::Unavailable && self.battery.charging.is_some() {
            return Err(SpecError::ProfileChargingMismatch { brand: self.id });
        }
        let budget = profile.thresholds.stale_ms;
        if budget * 2 < self.expected_interval_ms * 3 {
            return Err(SpecError::CadenceExceedsProfile {
                brand: self.id,
                expected_interval_ms: self.expected_interval_ms,
                stale_ms: budget,
            });
        }
        Ok(())
    }
}

/// 品牌描述的结构性错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SpecError {
    /// 品牌标识为空。
    EmptyBrandId,
    /// 展示名为空。
    EmptyLabel {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 命令字落在 payload 内（头体重叠）。
    HeaderOverlap {
        /// 品牌标识。
        brand: &'static str,
        /// 命令字下标。
        cmd_index: usize,
        /// payload 起始下标。
        payload_offset: usize,
    },
    /// 带报告 ID，但 payload 从 0 开始。
    PayloadStartsAtReportId {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 长度字段不在头部。
    LengthFieldOutsideHeader {
        /// 品牌标识。
        brand: &'static str,
        /// 长度字段下标。
        len_index: usize,
        /// 命令字下标。
        cmd_index: usize,
        /// payload 起始下标。
        payload_offset: usize,
    },
    /// 固定 payload 长度不足以容纳必选字段。
    FixedPayloadTooShort {
        /// 品牌标识。
        brand: &'static str,
        /// 声明的固定长度。
        fixed_payload_len: usize,
        /// 需要的最小长度。
        need: usize,
    },
    /// 子标签与电量字段重叠。
    SubtagOverlapsPercent {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 充电标志与电量字段重叠。
    ChargingOverlapsPercent {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 声明了查询模板，却没有写通道 UsagePage。
    RequestWithoutWritePage {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 查询模板越界。
    RequestLayoutInvalid {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 期望间隔为 0。
    ZeroInterval {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 品牌节奏超出家族 `stale` 预算。
    CadenceExceedsProfile {
        /// 品牌标识。
        brand: &'static str,
        /// 品牌期望间隔。
        expected_interval_ms: u64,
        /// 家族声明的 `stale_ms`。
        stale_ms: u64,
    },
    /// 家族不是 HID。
    ProfileTransportMismatch {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 品牌提供充电标志，但家族声明「不可得」。
    ProfileChargingMismatch {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 品牌标识重复登记。
    DuplicateBrandId {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 同一 VID:PID 被两个品牌重复登记。
    DuplicateDeviceKey {
        /// 视频 ID。
        vid: u16,
        /// 产品 ID。
        pid: u16,
        /// 先前登记的品牌。
        first: &'static str,
        /// 后登记的品牌。
        second: &'static str,
    },
    /// 型号表无法解析。
    DeviceTable {
        /// 品牌标识。
        brand: &'static str,
        /// 解析器给出的原因。
        detail: String,
    },
}

impl core::fmt::Display for SpecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyBrandId => write!(f, "品牌标识不得为空"),
            Self::EmptyLabel { brand } => write!(f, "品牌 {brand} 的展示名为空"),
            Self::HeaderOverlap {
                brand,
                cmd_index,
                payload_offset,
            } => write!(
                f,
                "品牌 {brand} 的 cmd_index={cmd_index} 必须 < payload_offset={payload_offset}"
            ),
            Self::PayloadStartsAtReportId { brand } => {
                write!(f, "品牌 {brand} 带报告 ID，payload 不得从 0 开始")
            }
            Self::LengthFieldOutsideHeader {
                brand,
                len_index,
                cmd_index,
                payload_offset,
            } => write!(
                f,
                "品牌 {brand} 的长度字段下标 {len_index} 不在头部 [{cmd_index}, {payload_offset})"
            ),
            Self::FixedPayloadTooShort {
                brand,
                fixed_payload_len,
                need,
            } => write!(
                f,
                "品牌 {brand} 固定 payload 长度 {fixed_payload_len} < 必需 {need}"
            ),
            Self::SubtagOverlapsPercent { brand } => {
                write!(f, "品牌 {brand} 的子标签与电量字段重叠")
            }
            Self::ChargingOverlapsPercent { brand } => {
                write!(f, "品牌 {brand} 的充电标志与电量字段重叠")
            }
            Self::RequestWithoutWritePage { brand } => write!(
                f,
                "品牌 {brand} 声明了查询模板，必须同时给出 usage_page_write"
            ),
            Self::RequestLayoutInvalid { brand } => {
                write!(f, "品牌 {brand} 的查询模板越界")
            }
            Self::ZeroInterval { brand } => {
                write!(f, "品牌 {brand} 的 expected_interval_ms 不得为 0")
            }
            Self::CadenceExceedsProfile {
                brand,
                expected_interval_ms,
                stale_ms,
            } => write!(
                f,
                "品牌 {brand} 期望间隔 {expected_interval_ms}ms 超出家族 stale={stale_ms}ms 预算（需 stale >= 1.5×间隔）"
            ),
            Self::ProfileTransportMismatch { brand } => {
                write!(f, "品牌 {brand} 只能挂在 HID 家族上")
            }
            Self::ProfileChargingMismatch { brand } => {
                write!(f, "品牌 {brand} 提供充电标志，但家族声明充电不可得")
            }
            Self::DuplicateBrandId { brand } => {
                write!(f, "品牌标识 {brand} 重复登记")
            }
            Self::DuplicateDeviceKey {
                vid,
                pid,
                first,
                second,
            } => write!(
                f,
                "VID:PID {vid:04X}:{pid:04X} 被品牌 {first} 与 {second} 同时登记（精确匹配必须唯一）"
            ),
            Self::DeviceTable { brand, detail } => {
                write!(f, "品牌 {brand} 的型号表无法解析：{detail}")
            }
        }
    }
}

impl std::error::Error for SpecError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brands::ajazz;

    fn base() -> BrandSpec {
        ajazz::SPEC
    }

    #[test]
    fn real_brand_spec_is_valid() {
        assert!(base().validate().is_ok());
    }

    #[test]
    fn min_payload_ignores_optional_charging() {
        // 子标签 + 电量 = 必需；充电标志可选。
        assert_eq!(base().min_payload_len(), 2);
        assert_eq!(base().min_report_len(), 7);
    }

    #[test]
    fn bv2_rejects_header_overlap() {
        let mut spec = base();
        spec.frame.cmd_index = spec.frame.payload_offset;
        assert!(matches!(
            spec.validate(),
            Err(SpecError::HeaderOverlap { .. })
        ));
    }

    #[test]
    fn bv4_rejects_length_field_after_payload() {
        let mut spec = base();
        spec.frame.len_index = Some(spec.frame.payload_offset);
        assert!(matches!(
            spec.validate(),
            Err(SpecError::LengthFieldOutsideHeader { .. })
        ));
    }

    #[test]
    fn bv5_rejects_too_short_fixed_payload() {
        let mut spec = base();
        spec.frame.len_index = None;
        spec.frame.fixed_payload_len = 1;
        assert!(matches!(
            spec.validate(),
            Err(SpecError::FixedPayloadTooShort { .. })
        ));
    }

    #[test]
    fn bv6_and_bv7_reject_field_overlap() {
        let mut spec = base();
        spec.battery.subtag = Some((spec.battery.percent_index, 0x03));
        assert_eq!(
            spec.validate(),
            Err(SpecError::SubtagOverlapsPercent { brand: spec.id })
        );

        let mut spec = base();
        spec.battery.charging = Some(ChargingField {
            index: spec.battery.percent_index,
            true_value: 1,
        });
        assert_eq!(
            spec.validate(),
            Err(SpecError::ChargingOverlapsPercent { brand: spec.id })
        );
    }

    #[test]
    fn bv8_rejects_request_without_write_page() {
        let mut spec = base();
        spec.usage_page_write = None;
        spec.request = Some(RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03],
            interval_ms: Some(10_000),
        });
        assert_eq!(
            spec.validate(),
            Err(SpecError::RequestWithoutWritePage { brand: spec.id })
        );
    }

    #[test]
    fn bv8_rejects_request_out_of_bounds() {
        let mut spec = base();
        spec.request = Some(RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            report_len: 6,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x00, 0x00],
            interval_ms: None,
        });
        assert_eq!(
            spec.validate(),
            Err(SpecError::RequestLayoutInvalid { brand: spec.id })
        );
    }

    #[test]
    fn bv9_rejects_zero_interval() {
        let mut spec = base();
        spec.expected_interval_ms = 0;
        assert_eq!(
            spec.validate(),
            Err(SpecError::ZeroInterval { brand: spec.id })
        );
    }

    #[test]
    fn bv10_accepts_family_budget() {
        assert!(
            base()
                .validate_against(&voltnest_provider::HID_VBAS)
                .is_ok()
        );
    }

    #[test]
    fn bv10_rejects_slow_brand() {
        let mut spec = base();
        spec.expected_interval_ms = 40_000; // 1.5×40s = 60s > stale 45s
        assert!(matches!(
            spec.validate_against(&voltnest_provider::HID_VBAS),
            Err(SpecError::CadenceExceedsProfile { .. })
        ));
    }

    #[test]
    fn bv10_rejects_non_hid_family() {
        assert!(matches!(
            base().validate_against(&voltnest_provider::BLE_BAS),
            Err(SpecError::ProfileTransportMismatch { .. })
        ));
    }

    #[test]
    fn request_template_builds_full_report() {
        let req = RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x01],
            interval_ms: Some(10_000),
        };
        assert_eq!(
            req.build(Some(0x06)),
            vec![0x06, 0xFB, 0x00, 0x00, 0x00, 0x03, 0x01, 0x00]
        );
        // 无报告 ID 时首字节保持 0，命令字位置不变。
        assert_eq!(
            req.build(None),
            vec![0x00, 0xFB, 0x00, 0x00, 0x00, 0x03, 0x01, 0x00]
        );
        assert_eq!(req.data_end(), 7);
    }

    #[test]
    fn request_template_never_panics_on_tight_buffers() {
        let req = RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            report_len: 6,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x01, 0x02],
            interval_ms: None,
        };
        // 声明长度不足时只填能填的部分，不越界 panic（模板本身会被 validate 拒绝）。
        assert_eq!(req.build(Some(0x06)).len(), 6);
    }
}
