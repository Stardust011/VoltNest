//! 品牌：雷柏 RAPOO（VID `0x24AE`，网咖/OEM 线 `0x2E3C`）。
//!
//! 型号表的 `driver` 字段是官方的家族码，各家族的通道、报告 ID、字段下标都不同，由 `route`
//! 在品牌内部完成分派：
//!
//! | `driver` | 采集方式 | 通道 / 报告 | 电量字段 |
//! |---|---|---|---|
//! | `VT_nrf54L` | 纯监听（功能态快照） | `0xFF00`/`0x0002`，报告 ID `0x07` | `data[6]`=状态、`data[7]`=百分比 |
//! | `VT_CH584` | 纯监听 | `0xFF00`/`0x000F`，报告 ID `0x08` | `data[7]`=百分比 |
//! | `LM20RT` | 纯监听 | `0xFF00`/`0x0002`，报告 ID `0x07` | `data[7]`=百分比 |
//! | `Telink3228C_Mouse` | 主动查询（维盛 AP 协议） | `0xFF1C`/`0x0092`，报告 ID `0x04` | `DATA[0]`=百分比、`DATA[1]`=充电 |
//!
//! 未接入（`route` 返回 `None`）：`TelinkKeyboard`、`CH585M`、`GCOME_AP`、`OHID`。
//!
//! 充电状态取值：`1` = 未充电、`2` = 充电中、`0` = 未上报（按未充电处理）。老布局
//! （`VT_CH584` / `LM20RT`）的 `batStatus` 在 16 位状态字的 bit14-15 里，故只读电量。

use crate::brands::checksum;
use crate::spec::{
    BatteryFields, BrandSpec, ChargingField, FrameLayout, ReplyTransport, RequestTemplate,
};
use crate::table::Entry;

/// VT_nrf54L（主力鼠标）：监听功能态快照，报告 ID `0x07`，`data[6]` 充电状态、`data[7]` 电量。
///
/// 无主动查询报文，打开设备后由设备按自身节奏上报。
pub const NRF54L: BrandSpec = BrandSpec {
    id: "rapoo-nrf54l",
    label: "雷柏 · VT_nrf54L 固件",
    name_from_product: false,
    usage_page: 0xFF00,
    usage_page_alt: None,
    usage: Some(0x0002),
    usage_page_write: None,
    reassembly: None,
    transport: ReplyTransport::Listen,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x07),
        cmd_index: 0x06,
        len_index: None,
        payload_offset: 0x07,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: None,
        cmd_high: None,
        subtag: None,
        percent_index: 1,
        percent_mask: 0xFF,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 0,
            true_values: &[0x02],
            error_from: None,
        }),
    },
    listen: None,
    notify: None,
    request: None,
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    block_offset: None,
    profile_probe: None,
    device_table_file: "rapoo.json",
    variants: &[],
    route: None,
};

/// LM20RT：报告 ID 与字段位置同 [`NRF54L`]，充电状态在状态字位域里 → 不提供。
pub const LM20RT: BrandSpec = BrandSpec {
    id: "rapoo-lm20rt",
    label: "雷柏 · LM20RT 固件",
    name_from_product: false,
    battery: BatteryFields {
        percent_mask: 0xFF,
        charging: None,
        ..NRF54L.battery
    },
    ..NRF54L
};

/// VT_CH584：同套用户参数布局，报告 ID `0x08`、Usage `0x0F`，不提供充电状态。
pub const CH584: BrandSpec = BrandSpec {
    id: "rapoo-ch584",
    label: "雷柏 · VT_CH584 固件",
    name_from_product: false,
    usage: Some(0x000F),
    frame: FrameLayout {
        report_id: Some(0x08),
        cmd_index: 0x06,
        len_index: None,
        payload_offset: 0x07,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        percent_mask: 0xFF,
        charging: None,
        ..NRF54L.battery
    },
    ..NRF54L
};

/// 维盛 AP 电量查询报文的校验和（`[csum_lo, csum_hi]`）：`0xAD + 0x06` 的 u16 小端和。
const AP_QUERY_CHECKSUM: [u8; 2] = checksum::sum16_le(&[0xAD, 0x06]);

/// Telink3228C_Mouse（如 VT2 ULTRA V2）：维盛 AP 协议，唯一需要主动查询的雷柏家族。
///
/// 发 `getBatteryLevel(0xAD)`（`LEN = 6`、`ADDR = 0`），等同一报告 ID 的输入报告；
/// 应答 `DATA[0]` = 电量（>100 为错）、`DATA[1]` = 充电（只认 `0` / `1`，其余为错）。
pub const AP: BrandSpec = BrandSpec {
    id: "rapoo-ap",
    label: "雷柏 · 维盛 AP 固件（Telink3228C）",
    name_from_product: false,
    usage_page: 0xFF1C,
    usage_page_alt: None,
    usage: Some(0x0092),
    usage_page_write: Some(0xFF1C),
    reassembly: None,
    transport: ReplyTransport::InterruptQuery,
    reply_inverted: false,
    clamp_percent: false,
    frame: FrameLayout {
        report_id: Some(0x04),
        cmd_index: 0x03,
        len_index: Some(0x04),
        payload_offset: 0x08,
        fixed_payload_len: 2,
    },
    battery: BatteryFields {
        cmd: Some(0xAD),
        cmd_high: None,
        subtag: None,
        percent_index: 0,
        percent_mask: 0xFF,
        charging: Some(ChargingField {
            mask: 0xFF,
            index: 1,
            true_values: &[0x01],
            error_from: Some(0x02),
        }),
    },
    listen: None,
    notify: None,
    request: Some(RequestTemplate {
        usage_page: 0xFF1C,
        cmd: 0xAD,
        cmd_high: None,
        prefix: &AP_QUERY_CHECKSUM,
        report_len: 64,
        cmd_index: 0x03,
        payload_offset: 0x04,
        payload: &[0x06, 0x00, 0x00],
        interval_ms: Some(30_000),
        inverted: false,
        retry: 0,
    }),
    ack: None,
    fallback_report_id: None,
    expected_interval_ms: 30_000,
    block_offset: None,
    profile_probe: None,
    device_table_file: "rapoo.json",
    variants: &[],
    route: None,
};

/// 雷柏自己的 `driver → 协议` 分派；未验证家族返回 `None`。
fn route(entry: &Entry) -> Option<&'static BrandSpec> {
    match entry.declared_driver() {
        Some("VT_nrf54L") => Some(&NRF54L),
        Some("VT_CH584") => Some(&CH584),
        Some("LM20RT") => Some(&LM20RT),
        Some("Telink3228C_Mouse") => Some(&AP),
        _ => None,
    }
}

/// 注册表登记的雷柏协议描述（nrf54L 为字段默认项，实际由 `route` 按 `driver` 分派）。
///
/// `id` 保持品牌级 `rapoo`；单套协议的标识在 [`NRF54L`] / [`CH584`] / [`LM20RT`] / [`AP`] 上。
pub const SPEC: BrandSpec = BrandSpec {
    id: "rapoo",
    label: "雷柏 RAPOO（按 driver 分派）",
    name_from_product: false,
    variants: &[&NRF54L, &CH584, &LM20RT, &AP],
    route: Some(route),
    ..NRF54L
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brands::Registry;
    use crate::frame::{RejectReason, decode_battery, decode_battery_detailed};
    use voltnest_provider::{ErrorCode, HID_VBAS};

    /// 构造一条只带 `driver` 的型号表条目（分派只读这一个字段）。
    fn entry_with_driver(driver: Option<&str>) -> Entry {
        super::super::test_support::entry_with("driver", driver.map(|v| serde_json::json!(v)))
    }

    /// nrf54L 功能态快照：报告 ID `0x07`，`data[6]`=状态、`data[7]`=电量。
    fn nrf54l_report(status: u8, percent: u8) -> [u8; 64] {
        let mut report = [0u8; 64];
        report[0] = 0x07;
        report[7] = status; // data[6]
        report[8] = percent; // data[7]
        report
    }

    #[test]
    fn nrf54l_parses_status_and_percent() {
        let reading = decode_battery(&NRF54L, &nrf54l_report(0x02, 86)).unwrap();
        assert_eq!(reading.percent.get(), 86);
        assert_eq!(reading.charging, Some(true), "状态 2 = 充电中");

        let reading = decode_battery(&NRF54L, &nrf54l_report(0x01, 40)).unwrap();
        assert_eq!(reading.percent.get(), 40);
        assert_eq!(reading.charging, Some(false), "状态 1 = 未充电");
    }

    /// 官方对 `batStatus` 的语义只有「是否等于 1」；≥3 的未定义值按未充电处理，不丢整帧。
    #[test]
    fn nrf54l_undefined_status_is_not_charging() {
        let reading = decode_battery(&NRF54L, &nrf54l_report(0x03, 55)).unwrap();
        assert_eq!(reading.percent.get(), 55);
        assert_eq!(reading.charging, Some(false));
    }

    /// 只有报告 ID `0x07` 承载功能态：别的 ID（如 `0x06` 输出报告、其他族报告）不得当电量。
    #[test]
    fn nrf54l_rejects_other_report_ids() {
        let mut report = nrf54l_report(0x02, 86);
        report[0] = 0x06;
        assert_eq!(
            decode_battery(&NRF54L, &report),
            Err(ErrorCode::MalformedFrame)
        );
    }

    /// 报告短于 payload（`data[7]` 缺失）是坏帧，不是 0%。
    #[test]
    fn nrf54l_short_report_is_malformed() {
        let report = [0x07u8, 0, 0, 0, 0, 0, 0, 0x02];
        assert_eq!(
            decode_battery(&NRF54L, &report),
            Err(ErrorCode::MalformedFrame)
        );
    }

    /// 老布局（CH584 / LM20RT）只读电量，充电状态留空而不是猜。
    #[test]
    fn legacy_user_data_reports_percent_only() {
        let mut report = [0u8; 64];
        report[0] = 0x08;
        report[8] = 73;
        let reading = decode_battery(&CH584, &report).unwrap();
        assert_eq!(reading.percent.get(), 73);
        assert_eq!(reading.charging, None, "位域状态不得当成整字节充电标志");

        let mut report = [0u8; 64];
        report[0] = 0x07;
        report[8] = 12;
        assert_eq!(decode_battery(&LM20RT, &report).unwrap().percent.get(), 12);

        let mut wrong = [0u8; 64];
        wrong[0] = 0x07;
        wrong[8] = 73;
        assert_eq!(
            decode_battery(&CH584, &wrong),
            Err(ErrorCode::MalformedFrame)
        );
    }

    /// 维盛 AP 查询帧：报告 ID `0x04` + 校验和 `0xB3 0x00` + `CMD 0xAD` + `LEN 6` + `ADDR 0`。
    #[test]
    fn ap_query_frame_matches_official_layout() {
        let query = AP.request.expect("AP 必须声明查询报文").build(Some(0x04));
        assert_eq!(query.len(), 64);
        assert_eq!(&query[..7], &[0x04, 0xB3, 0x00, 0xAD, 0x06, 0x00, 0x00]);
        assert!(
            query[7..].iter().all(|b| *b == 0),
            "DATA 区必须全零：{:02X?}",
            &query[7..]
        );
        assert_eq!(0xAD + 0x06, 0x00B3);
    }

    /// AP 应答：`data[2]` 回显命令、`data[3]`=LEN、`data[7..]`=DATA（电量、充电）。
    fn ap_report(level: u8, charging: u8) -> [u8; 64] {
        let mut report = [0u8; 64];
        report[..5].copy_from_slice(&[0x04, 0xB3, 0x00, 0xAD, 0x06]);
        report[8] = level; // DATA[0]
        report[9] = charging; // DATA[1]
        report
    }

    #[test]
    fn ap_parses_level_and_charging() {
        let reading = decode_battery(&AP, &ap_report(64, 0x01)).unwrap();
        assert_eq!(reading.percent.get(), 64);
        assert_eq!(reading.charging, Some(true));

        let reading = decode_battery(&AP, &ap_report(64, 0x00)).unwrap();
        assert_eq!(reading.charging, Some(false));
    }

    /// 官方把「>100 的电量」与「≥0x02 的充电值」都当读失败：整帧不可用，不得降级成 0%。
    #[test]
    fn ap_rejects_out_of_range_values() {
        assert_eq!(
            decode_battery(&AP, &ap_report(101, 0x00)),
            Err(ErrorCode::PercentOutOfRange)
        );
        let err = decode_battery_detailed(&AP, &ap_report(64, 0x02)).unwrap_err();
        assert_eq!(err.code, ErrorCode::MalformedFrame);
        assert_eq!(err.reason, RejectReason::StatusError { raw: 0x02 });
    }

    /// 命令回显不是 `0xAD` 的应答（例如别的 AP 命令的回复）不得当电量。
    #[test]
    fn ap_requires_command_echo() {
        let mut report = ap_report(64, 0x00);
        report[3] = 0xA4; // getFunctionArea 之类的应答
        assert_eq!(decode_battery(&AP, &report), Err(ErrorCode::MalformedFrame));
    }

    /// 分派只认雷柏自己的 `driver`；未实测家族返回 `None`（不采集，不猜）。
    #[test]
    fn route_dispatches_by_driver() {
        assert_eq!(
            route(&entry_with_driver(Some("VT_nrf54L"))).map(|s| s.id),
            Some("rapoo-nrf54l")
        );
        assert_eq!(
            route(&entry_with_driver(Some("VT_CH584"))).map(|s| s.id),
            Some("rapoo-ch584")
        );
        assert_eq!(
            route(&entry_with_driver(Some("LM20RT"))).map(|s| s.id),
            Some("rapoo-lm20rt")
        );
        assert_eq!(
            route(&entry_with_driver(Some("Telink3228C_Mouse"))).map(|s| s.id),
            Some("rapoo-ap")
        );
        assert!(
            route(&entry_with_driver(Some("TelinkKeyboard"))).is_none(),
            "键盘族布局未实测"
        );
        assert!(route(&entry_with_driver(Some("CH585M"))).is_none());
        assert!(route(&entry_with_driver(Some("GCOME_AP"))).is_none());
        assert!(
            route(&entry_with_driver(None)).is_none(),
            "型号表缺 driver 时不猜协议"
        );
    }

    /// 四套协议都必须通过构造期校验，且落在家族能力预算内。
    #[test]
    fn all_rapoo_specs_validate() {
        for spec in [&NRF54L, &CH584, &LM20RT, &AP, &SPEC] {
            assert!(spec.validate().is_ok(), "{} 非法", spec.id);
        }
        assert!(SPEC.validate_against(&HID_VBAS).is_ok());
        assert_eq!(
            SPEC.for_device(&entry_with_driver(Some("VT_nrf54L")))
                .map(|s| s.id),
            Some("rapoo-nrf54l")
        );
    }

    /// 型号表里每条都必须带官方 `driver`，否则设备会静默落到「无协议」。
    #[test]
    fn table_every_entry_declares_driver() {
        let reg = Registry::embedded();
        let brand = reg
            .brands()
            .iter()
            .find(|b| b.spec.id == "rapoo")
            .expect("雷柏必须在注册表内");
        let mut missing = Vec::new();
        for ((vid, pid), entry) in brand.table.entries() {
            if entry.declared_driver().is_none() {
                missing.push(format!("{vid:04X}:{pid:04X}"));
            }
        }
        assert!(missing.is_empty(), "缺 driver 的条目：{missing:?}");
    }

    /// 真实设备：VT0（nrf54L）、VT2 ULTRA V2（AP）、VT7 SE（CH584）、VT2 RT V2（LM20RT）。
    #[test]
    fn table_routes_real_models() {
        let reg = Registry::embedded();
        for ((vid, pid), want) in [
            ((0x24AE, 0x1410), "rapoo-nrf54l"),
            ((0x24AE, 0x1472), "rapoo-ap"),
            ((0x24AE, 0x1A00), "rapoo-ch584"),
            ((0x24AE, 0x1B20), "rapoo-lm20rt"),
        ] {
            let (brand, entry) = reg.lookup(vid, pid).expect("型号必须在表内");
            assert_eq!(
                brand.spec.for_device(entry).map(|s| s.id),
                Some(want),
                "{vid:04X}:{pid:04X}"
            );
        }
    }
}
