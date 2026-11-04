//! 宿主 ↔ 悬浮窗子进程的 NDJSON 协议。
//!
//! 一行一条 UTF-8 JSON，双向都用内部 tag 区分类型（`"t"`）。所有消息带 `v`（协议版本）；
//! 两侧版本不一致时子进程直接退出（`bye{reason:"protocol_mismatch"}`），宿主置降级状态，
//! 主功能不受影响。

use serde::{Deserialize, Serialize};

/// 协议版本。改动字段语义时必须加一。
pub const PROTOCOL_VERSION: u32 = 1;

/// 单行上限。超过视为非法数据，直接丢弃（防止异常输入撑爆内存）。
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// 宿主下发的一个呈现单元（一台设备或一个合并组）。
///
/// 展示用的文案（`battery` / `status` / `level`）由宿主算好，子进程只负责渲染，
/// 避免业务规则出现第二份实现。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitWire {
    /// 单元锚点（某个成员的 `stableId`），窗口身份的唯一键。
    pub anchor: String,
    /// 卡片标题。
    pub title: String,
    /// 设备类型串，与 `voltnest-provider` 的 `DeviceType::as_str()` 一致。
    pub device_type: String,
    /// 电量文案，例如 `87%` / `—`。
    pub battery: String,
    /// 圆环进度 0.0~1.0；无读数时取最近一次已知值，没有则 0。
    pub level: f32,
    /// 状态文案，例如 `已连接 · 2.4G` / `离线`。
    pub status: String,
    /// 设备当前不可用（窗口仍显示，只是文案标离线）。
    pub offline: bool,
    /// 是否应显示窗口。
    pub visible: bool,
    /// 窗口左上角物理像素；`None` 表示用级联默认位。
    pub x: Option<i32>,
    /// 同 [`UnitWire::x`]。
    pub y: Option<i32>,
    /// 卡片边长（逻辑像素）。
    pub side: u16,
    /// 透明度 0~100。
    pub transparency: u8,
}

/// 宿主 → 子进程。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HostMsg {
    /// 全量同步：`units` 里没有的锚点会被关窗并销毁。
    Sync {
        /// 协议版本。
        v: u32,
        /// 单调递增序号，子进程丢弃 `seq <= 已处理` 的旧包。
        seq: u64,
        /// 全部单元（含 `visible: false` 的，便于保留实例）。
        units: Vec<UnitWire>,
    },
    /// 让子进程退出。
    Shutdown {
        /// 协议版本。
        v: u32,
    },
}

/// 子进程 → 宿主。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ChildMsg {
    /// 会话就绪（事件循环已跑起来）。
    Ready {
        /// 协议版本。
        v: u32,
        /// 子进程 pid。
        pid: u32,
    },
    /// 用户改了某个窗口的位置 / 尺寸 / 透明度。
    Layout {
        /// 协议版本。
        v: u32,
        /// 单元锚点。
        anchor: String,
        /// 卡片左上角物理像素 X。
        x: i32,
        /// 卡片左上角物理像素 Y。
        y: i32,
        /// 卡片边长（逻辑像素）。
        side: f32,
        /// 当时的缩放比，仅供诊断，不落盘。
        scale: f32,
        /// 透明度 0~100。
        transparency: u8,
    },
    /// 用户在悬浮窗菜单里点了「隐藏悬浮窗」。
    Hidden {
        /// 协议版本。
        v: u32,
        /// 单元锚点。
        anchor: String,
    },
    /// 子进程内部日志，由宿主带前缀写进统一日志文件。
    Log {
        /// 协议版本。
        v: u32,
        /// 级别（`error` / `warn` / `info` / `debug`）。
        level: String,
        /// 文本。
        message: String,
    },
    /// 子进程即将退出。
    Bye {
        /// 协议版本。
        v: u32,
        /// 原因，例如 `stdin_eof` / `panic: ...`。
        reason: String,
    },
}

/// 编码成单行 JSON（serde 会把字符串里的换行转义掉，所以天然安全）。
pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(msg).unwrap_or_else(|err| {
        format!(r#"{{"t":"log","v":0,"level":"error","message":"encode failed: {err}"}}"#)
    })
}

/// 解析宿主发来的一行。
pub fn decode_host(line: &str) -> Result<HostMsg, ProtoError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtoError::TooLong(line.len()));
    }
    serde_json::from_str(line).map_err(|err| ProtoError::Json(err.to_string()))
}

/// 解析子进程上报的一行。
pub fn decode_child(line: &str) -> Result<ChildMsg, ProtoError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtoError::TooLong(line.len()));
    }
    serde_json::from_str(line).map_err(|err| ProtoError::Json(err.to_string()))
}

/// 协议解析错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtoError {
    /// 单行超过 [`MAX_LINE_BYTES`]。
    TooLong(usize),
    /// JSON 结构不匹配（字段缺失、未知 `t` 等）。
    Json(String),
}

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong(len) => write!(f, "消息过长（{len} 字节）"),
            Self::Json(err) => write!(f, "消息解析失败：{err}"),
        }
    }
}

impl std::error::Error for ProtoError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> UnitWire {
        UnitWire {
            anchor: "hid:7768:6426".into(),
            title: "MCHOSE A5 Pro".into(),
            device_type: "mouse".into(),
            battery: "87%".into(),
            level: 0.87,
            status: "已连接 · 2.4G".into(),
            offline: false,
            visible: true,
            x: Some(1200),
            y: Some(640),
            side: 180,
            transparency: 40,
        }
    }

    #[test]
    fn host_msg_round_trips() {
        let msg = HostMsg::Sync {
            v: PROTOCOL_VERSION,
            seq: 7,
            units: vec![unit()],
        };
        let line = encode(&msg);
        assert!(!line.contains('\n'));
        assert_eq!(decode_host(&line), Ok(msg));
    }

    #[test]
    fn child_msg_round_trips() {
        let msg = ChildMsg::Layout {
            v: PROTOCOL_VERSION,
            anchor: "hid:7768:6426".into(),
            x: 10,
            y: 20,
            side: 180.0,
            scale: 1.25,
            transparency: 30,
        };
        let line = encode(&msg);
        assert_eq!(decode_child(&line), Ok(msg));
    }

    #[test]
    fn unknown_type_is_an_error_not_a_panic() {
        assert!(decode_host(r#"{"t":"nope","v":1}"#).is_err());
        assert!(decode_host("not json").is_err());
    }

    #[test]
    fn oversized_line_is_rejected() {
        let line = "x".repeat(MAX_LINE_BYTES + 1);
        assert_eq!(
            decode_host(&line),
            Err(ProtoError::TooLong(MAX_LINE_BYTES + 1))
        );
    }

    #[test]
    fn sync_without_units_still_decodes() {
        let msg = decode_host(r#"{"t":"sync","v":1,"seq":1,"units":[]}"#).unwrap();
        assert!(matches!(msg, HostMsg::Sync { units, .. } if units.is_empty()));
    }
}
