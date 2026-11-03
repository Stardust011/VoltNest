//! `hid-vbas` 的通用协议描述（**品牌无关**）。
//!
//! 一个品牌接入 = 一份 [`BrandSpec`]：怎么匹配、帧长什么样、电量与充电标志在第几个字节、是否主动
//! 查询、应答怎么确认。解析器与运行时只读这些字段，无 `match brand_id` 分支，新增品牌不改既有代码。
//!
//! 家族（`hid-vbas`）定协议形状（HID 私有通道 + 定长报告 + 命令字），品牌（[`BrandSpec`]）只填数据；
//! 一个品牌覆盖多套不兼容固件时，用 [`BrandSpec::route`] 把「按型号表字段选协议」交回品牌模块。
//! [`BrandSpec::validate`] 在首次枚举前校验字段越界等手写错误，写错即启动失败。

use crate::table::Entry;
use voltnest_provider::{Cadence, ChargingSupport, ProviderProfile};

/// 电量帧的收发方式（**从宿主视角命名**：本端要不要发报文、发了之后读哪里）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplyTransport {
    /// 纯监听：本端只读输入通道，不向设备发任何报文；何时上报由设备决定。
    Listen,
    /// 发送 output report 后等待输入报告。
    InterruptQuery,
    /// 发送 feature report 后读取 feature report 应答。
    FeatureQuery {
        /// 读取 feature report 时请求的总长度。
        reply_len: usize,
    },
    /// 周期读取 feature report，不发送查询报文。当前无品牌使用。
    FeaturePoll {
        /// 读取 feature report 时请求的总长度。
        reply_len: usize,
    },
}

impl ReplyTransport {
    /// 是否使用 feature report 传输。
    pub const fn is_feature(self) -> bool {
        matches!(self, Self::FeatureQuery { .. } | Self::FeaturePoll { .. })
    }

    /// feature report 的读取长度；非 feature 传输不需要该长度。
    pub const fn reply_len(self) -> Option<usize> {
        match self {
            Self::FeatureQuery { reply_len } | Self::FeaturePoll { reply_len } => Some(reply_len),
            Self::Listen | Self::InterruptQuery => None,
        }
    }
}

/// 帧头布局：把一帧切成「头部 → payload」，其余字节是填充。
///
/// 头部只描述**位置**，语义在 [`BatteryFields`]。
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

impl FrameLayout {
    /// 该帧声明的 payload 长度：有长度字段就读它，否则取固定长度。
    ///
    /// 返回 `None` 仅当声明了长度字段但该字节不在缓冲里（报告被读短）。
    pub fn payload_len(&self, report: &[u8]) -> Option<usize> {
        match self.len_index {
            Some(index) => report.get(index).map(|raw| *raw as usize),
            None => Some(self.fixed_payload_len),
        }
    }
}

/// 充电标志的定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChargingField {
    /// 标志在 payload 内的下标。
    pub index: usize,
    /// 参与比较的位。`0xFF` = 整字节比较（既有品牌的行为）。
    pub mask: u8,
    /// 表示「充电中」的取值集合（已与 `mask` 对齐）。其余取值一律视为「未充电」，不猜测语义。
    pub true_values: &'static [u8],
    /// 错误态起点：该字节 `>=` 此值表示设备自报异常，此时整帧不得当作有效读数。`None` = 不判错误态。
    pub error_from: Option<u8>,
}

impl ChargingField {
    /// 该字节是否表示「充电中」：先按 `mask` 取位，再比取值集合。
    pub fn is_charging(&self, byte: u8) -> bool {
        self.true_values.contains(&(byte & self.mask))
    }

    /// 该字节是否达到品牌声明的错误态起点（未声明错误态恒 `false`）。
    pub fn is_error(&self, byte: u8) -> bool {
        matches!(self.error_from, Some(from) if byte >= from)
    }
}

/// 应答确认位：`(byte & mask) == value` 才算设备已确认该命令。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AckField {
    /// 确认位所在下标（绝对下标）。
    pub index: usize,
    /// 参与比较的位。
    pub mask: u8,
    /// 期望值（已与 `mask` 对齐）。
    pub value: u8,
}

/// 电量字段定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatteryFields {
    /// 命令字匹配值（`FrameLayout::cmd_index` 处的期望字节）。
    ///
    /// `None` = 该协议不在应答里回显命令字，故不校验；此时 `cmd_index` 不参与匹配。
    pub cmd: Option<u8>,
    /// 命令字的高字节：`Some` = 该协议的命令字是**双字节小端**，低字节在 `cmd_index`、
    /// 高字节在 `cmd_index + 1`。
    pub cmd_high: Option<u8>,
    /// payload 内的子标签：`(下标, 期望值)`。`None` = 该协议没有子标签。
    pub subtag: Option<(usize, u8)>,
    /// 电量百分比在 payload 内的下标（`0..=100`，越界即 `PercentOutOfRange`）。
    pub percent_index: usize,
    /// 电量字节里承载百分比的位。`0xFF` = 整字节（既有品牌的行为）。
    pub percent_mask: u8,
    /// 充电标志。`None` = 协议不提供充电状态 → 读数恒为 `None`。
    pub charging: Option<ChargingField>,
}

impl BatteryFields {
    /// 从电量字节取百分比：先按 [`Self::percent_mask`] 取位，再交给 `Percent` 校验范围。
    pub const fn percent_of(&self, byte: u8) -> u8 {
        byte & self.percent_mask
    }

    /// 这组字段至少要占多少字节：子标签与电量里下标更靠后的那个。
    ///
    /// 推送帧与应答帧的字段是两份（[`NotifySpec`]），故按**当前正在解析的那一份**算。
    pub const fn min_payload_len(&self) -> usize {
        let percent = self.percent_index + 1;
        match self.subtag {
            Some((index, _)) => {
                let subtag = index + 1;
                if subtag > percent { subtag } else { percent }
            }
            None => percent,
        }
    }
}

/// 监听帧里状态字节的语义。
///
/// 两种形状互不相同，不能共用一个「低半字节」字段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenStatus {
    /// 高半字节 = 满电标志（非零即按 100% 上报），低半字节 = 充电状态（等于该值即充电中）。
    Nibbles {
        /// 低半字节等于该值表示「充电中」。
        charging_low_nibble: u8,
    },
    /// 整字节是充电标志，同时是电量可信度的门控。
    Gate {
        /// 该字节取这些值 → 充电中。
        charging: &'static [u8],
        /// 该字节取这些值时，本帧的电量才可信；其它取值整帧丢弃。
        usable: &'static [u8],
    },
}

/// 设备**自发上报**（宿主未提问）的电量帧布局（与应答帧布局无关，两条通道各有一套）。
///
/// 只描述「怎么认出监听通道上的一帧电量上报、字段在哪」；认不出来的一律静默丢弃。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListenSpec {
    /// 帧首字节（命令字）必须等于该值。
    pub cmd: u8,
    /// 第二头字节。`None` = 单字节头（老族 `0x0A`）；`Some(0xFB)` = `FA FB` 双字节头。
    pub cmd2: Option<u8>,
    /// 子标签（`type`）所在下标。
    pub subtag_index: usize,
    /// 子标签必须等于该值（电量上报包的 `type`）。
    pub subtag_value: u8,
    /// 电量字节：`(下标, 掩码)`。`None` = 本帧只通报充电状态、不含电量（GLW 的 `FA FB FF`）。
    pub percent: Option<(usize, u8)>,
    /// 状态字节下标。
    pub status_index: usize,
    /// 状态字节语义。
    pub status: ListenStatus,
}

impl ListenSpec {
    /// 帧至少要有多少字节才装得下全部字段。
    pub const fn min_len(&self) -> usize {
        let mut max = self.subtag_index;
        if max < self.status_index {
            max = self.status_index;
        }
        if let Some((index, _)) = self.percent
            && max < index
        {
            max = index;
        }
        // 双字节头要求第二字节存在。
        if self.cmd2.is_some() && max < 1 {
            max = 1;
        }
        max + 1
    }
}

/// 设备**主动推送**的电量帧：与应答共用同一套帧头（[`BrandSpec::frame`]），但命令字与字段下标另有一套。
///
/// 与 [`ListenSpec`] 的区别在帧形状：`ListenSpec` 认「无帧头、首字节即命令字 + 子标签」的上报帧，
/// 本结构认「与应答共用头部、只是命令字不同」的推送帧。两者不复用 [`BatteryFields`]，因为同一台设备
/// 的推送帧与查询应答是两套下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotifySpec {
    /// 推送帧的命令字低字节（位于 [`BrandSpec::frame`] 的 `cmd_index`）。
    pub cmd: u8,
    /// 命令字高字节：`Some` = 双字节小端命令。
    pub cmd_high: Option<u8>,
    /// 电量百分比在 payload 内的下标。
    pub percent_index: usize,
    /// 电量字节里承载百分比的位。`0xFF` = 整字节。
    pub percent_mask: u8,
    /// 充电标志。`None` = 推送帧不提供充电状态（此时不得拿应答帧的充电字段去凑）。
    pub charging: Option<ChargingField>,
}

impl NotifySpec {
    /// 推送帧按 [`BatteryFields`] 的形状表达，供 [`crate::frame`] 复用同一套解析器。
    ///
    /// 不含子标签与确认位：这两项是**应答**的语义（「这条应答属于哪一问」）。
    pub const fn fields(self) -> BatteryFields {
        BatteryFields {
            cmd: Some(self.cmd),
            cmd_high: self.cmd_high,
            subtag: None,
            percent_index: self.percent_index,
            percent_mask: self.percent_mask,
            charging: self.charging,
        }
    }
}

/// 多包应答的重组约定：一条应答被设备拆成若干条输入报告发出。
///
/// 只描述**包**；组装结果里电量与状态在哪，仍由 [`BrandSpec::battery`] 说。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reassembly {
    /// 应答包的首字节命令字（与查询命令字不同，是设备回包的命令码）。
    pub cmd: u8,
    /// 包内总包数所在下标（相对数据窗口，窗口 `[0]` 即 `cmd`）。
    pub total_index: usize,
    /// 包内序号所在下标（从 `0` 开始，逐包加一）。
    pub seq_index: usize,
    /// 包内载荷起始下标。
    pub payload_offset: usize,
    /// 每包载荷尾部要丢掉的字节数（校验和）。`0` = 无尾部字段。
    pub trailer_len: usize,
    /// 一次应答最多等多久（毫秒）；超过则丢弃半包，等下一轮查询。
    pub timeout_ms: u64,
}

impl Reassembly {
    /// 一条应答包至少要有多长才装得下头部（命令字 + 总包数 + 序号）。
    pub const fn min_packet_len(&self) -> usize {
        let head = if self.seq_index > self.total_index {
            self.seq_index
        } else {
            self.total_index
        };
        head + 1
    }
}

/// 主动查询模板：向写通道发送一条固定报文，把电量「问」出来。
///
/// 与「纯监听」并列的另一条采集路径；纯监听设备的 [`BrandSpec::request`] 为 `None`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestTemplate {
    /// 写通道的 UsagePage。
    pub usage_page: u16,
    /// 报文中携带的命令字（查询命令，可与上报帧的命令字不同）。
    pub cmd: u8,
    /// 命令字的高字节：`Some` = 双字节小端命令，写在 `cmd_index + 1`。
    pub cmd_high: Option<u8>,
    /// 命令字之前的固定报文内容。
    pub prefix: &'static [u8],
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
    /// 是否对报告 ID 之后的报文内容逐字节取反。
    pub inverted: bool,
    /// 单轮查询失败后的重试次数（官方驱动的 `retryTime`）。`0` = 一次即定。
    ///
    /// 只对 feature 查询生效：纯监听 / output 查询没有「重问」这回事，读线程忽略它。
    pub retry: u8,
}

/// 查询报文里「数据块偏移」字段的位置：偏移随板载档位变化时用来重算报文。
///
/// 字段全在 payload 内，下标相对 payload 起点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockOffset {
    /// 偏移低字节的下标。
    pub lo_index: usize,
    /// 偏移高字节的下标。
    pub hi_index: usize,
    /// 折叠校验和的下标；`None` = 该协议头部没有校验和。
    pub checksum_index: Option<usize>,
    /// 参与校验和的长度字节下标（官方 `sum([size, lo, hi, 0])`）。`None` = 不计长度。
    pub size_index: Option<usize>,
    /// 每档板载的字节跨度。
    pub stride: u16,
}

/// 「先问一次设备当前板载档位」的探测描述。
///
/// base 应答给出当前档位与档位顺序表：顺序表是 `0..N` 的一个排列时真实档位是 `order[active]`，
/// 否则就是 `active` 本身。声明它之后，查询会先读一帧 base，再按解析出的档位构造后续查询。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileProbe {
    /// 读 base 的查询模板。
    pub request: RequestTemplate,
    /// payload 内「当前板载下标」的位置。
    pub active_index: usize,
    /// payload 内「档位数」的位置；`None` = 不校验。
    pub length_index: Option<usize>,
    /// payload 内档位顺序表的起点。
    pub order_offset: usize,
    /// 顺序表项数。
    pub order_len: usize,
}

impl ProfileProbe {
    /// 从 base 应答的 payload 解出当前档位；判定不成立时按 0 处理。
    pub fn resolve(&self, payload: &[u8]) -> u8 {
        let Some(active) = payload
            .get(self.active_index)
            .copied()
            .filter(|value| usize::from(*value) < self.order_len)
        else {
            return 0;
        };
        let Some(order) = payload.get(self.order_offset..self.order_offset + self.order_len) else {
            return 0;
        };
        let arranged = order.iter().enumerate().all(|(index, value)| {
            usize::from(*value) < self.order_len
                && order[..index].iter().all(|other| other != value)
        });
        if !arranged {
            return active;
        }
        let length_ok = self.length_index.is_none_or(|index| {
            payload
                .get(index)
                .is_some_and(|value| usize::from(*value) <= self.order_len)
        });
        if !length_ok {
            return 0;
        }
        order.get(usize::from(active)).copied().unwrap_or(0)
    }
}

impl RequestTemplate {
    /// 按模板拼一帧可直接 `write` 的报文（数据块偏移取 0）。
    ///
    /// 协议带报告 ID 时写在首字节（hidapi 约定）。
    pub fn build(&self, report_id: Option<u8>) -> Vec<u8> {
        self.build_at(report_id, 0, None)
    }

    /// 按板载档位 `profile` 拼报文：偏移与折叠校验和随档位重算。
    ///
    /// 校验和是 `size + lo + hi + 0` 的**求和**（不是异或）。
    /// `block` 为 `None` 时 `profile` 不参与，与 [`Self::build`] 等价。
    pub fn build_at(
        &self,
        report_id: Option<u8>,
        profile: u8,
        block: Option<BlockOffset>,
    ) -> Vec<u8> {
        let mut buf = self.build_raw(report_id);
        let Some(offset) = block else {
            return buf;
        };
        let raw = u16::from(profile) * offset.stride;
        let base = usize::from(report_id.is_some()) + self.payload_offset;
        if let Some(slot) = buf.get_mut(base + offset.lo_index) {
            *slot = (raw & 0xFF) as u8;
        }
        if let Some(slot) = buf.get_mut(base + offset.hi_index) {
            *slot = (raw >> 8) as u8;
        }
        if let Some(index) = offset.checksum_index {
            let size = offset
                .size_index
                .and_then(|index| buf.get(base + index).copied())
                .unwrap_or(0);
            let lo = (raw & 0xFF) as u8;
            let hi = (raw >> 8) as u8;
            if let Some(slot) = buf.get_mut(base + index) {
                *slot = size.wrapping_add(lo).wrapping_add(hi);
            }
        }
        buf
    }

    /// 只按静态 payload 拼报文，不做板载档位替换。
    fn build_raw(&self, report_id: Option<u8>) -> Vec<u8> {
        let mut buf = vec![0u8; self.report_len];
        if let Some(id) = report_id
            && let Some(first) = buf.first_mut()
        {
            *first = id;
        }
        let prefix_start = usize::from(report_id.is_some());
        let prefix_end = self.cmd_index.min(self.report_len);
        if prefix_start < prefix_end
            && let Some(dst) = buf.get_mut(prefix_start..prefix_end)
        {
            let n = dst.len().min(self.prefix.len());
            dst[..n].copy_from_slice(&self.prefix[..n]);
        }
        if let Some(slot) = buf.get_mut(self.cmd_index) {
            *slot = self.cmd;
        }
        if let Some(high) = self.cmd_high
            && let Some(slot) = buf.get_mut(self.cmd_index + 1)
        {
            *slot = high;
        }
        let end = self
            .payload_offset
            .saturating_add(self.payload.len())
            .min(self.report_len);
        if let Some(dst) = buf.get_mut(self.payload_offset..end) {
            let n = dst.len();
            dst.copy_from_slice(&self.payload[..n]);
        }
        if self.inverted {
            let start = usize::from(report_id.is_some());
            for byte in buf.iter_mut().skip(start) {
                *byte ^= 0xFF;
            }
        }
        buf
    }

    /// payload 在报文中的结束下标。
    pub const fn data_end(&self) -> usize {
        self.payload_offset + self.payload.len()
    }

    /// 一轮采集的总尝试次数（首发 + 重试）。永远 ≥ 1。
    pub const fn attempts(&self) -> usize {
        self.retry as usize + 1
    }
}

/// 按型号表条目选协议的钩子。
///
/// 只有需要「同品牌多套协议」的品牌才声明它；返回 `None` 表示该设备没有已证实的协议，
/// 此时不得退回默认协议去猜。
pub type RouteFn = fn(&Entry) -> Option<&'static BrandSpec>;

/// 一套协议（一个固件家族）的完整接入描述。
#[derive(Clone, Copy, Debug)]
pub struct BrandSpec {
    /// 稳定标识（日志 / 诊断用，**不参与**设备匹配）。例如 `ajazz`。
    pub id: &'static str,
    /// 人类可读的品牌 / 代际名。例如 `AJAZZ（W1 / 星闪代）`。
    pub label: &'static str,
    /// 展示名是否优先用设备自报的产品名（ProductString），而不是型号表型号。
    /// 两者都缺失时仍回落到 `VID:PID`。
    pub name_from_product: bool,
    /// 读取通道的 UsagePage。`0` = **通配**：该 VID:PID 的任意集合都接受。
    pub usage_page: u16,
    /// 备用读取 UsagePage：只有当该 VID:PID **没有任何集合**落在 `usage_page` 上时才启用。
    ///
    /// 主页面优先：主页面零命中才降级，因此「两页都在」的设备不会变成两只设备。
    pub usage_page_alt: Option<u16>,
    /// 读取通道的 Usage。`None` = 该 UsagePage 下任意集合都接受（既有品牌的历史行为）。
    pub usage: Option<u16>,
    /// 只写命令通道的 UsagePage。`None` = 该品牌不主动发报文。
    ///
    /// **仅参与构造期校验**：运行时只用读句柄收发。声明了查询模板时写页必须等于读页
    /// （读页为 `0` 通配时除外，见 [`BrandSpec::validate`]）。
    pub usage_page_write: Option<u16>,
    /// 读取电量帧所使用的 HID 传输方式。
    pub transport: ReplyTransport,
    /// 是否对报告 ID 之后的应答内容逐字节取反。
    pub reply_inverted: bool,
    /// 电量超过 100 时是否按厂商实现截断到 100。当前无品牌置 `true`（越界一律按读失败处理）。
    pub clamp_percent: bool,
    /// 帧头布局。
    pub frame: FrameLayout,
    /// 电量字段。
    pub battery: BatteryFields,
    /// 监听通道上「设备自发上报的电量帧」的识别模板。可声明多条（同一设备用不同帧分别报电量与
    /// 充电状态）。`None` = 该协议没有这条旁路（读数只从查询应答取）。
    pub listen: Option<&'static [ListenSpec]>,
    /// 设备**主动推送**的电量帧（与应答共用帧头、命令字不同）。与 `listen` 是两条互斥的旁路。
    pub notify: Option<NotifySpec>,
    /// 主动查询模板。`None` = 纯监听（不向设备发任何报文）。
    pub request: Option<RequestTemplate>,
    /// 查询报文里的「数据块偏移」字段：声明后查询按当前板载档位构造（见 [`BlockOffset`]）。
    pub block_offset: Option<BlockOffset>,
    /// 连接后先读一次「当前板载档位」的探测描述（见 [`ProfileProbe`]）。`None` = 不需要探测。
    pub profile_probe: Option<ProfileProbe>,
    /// 多包应答的重组约定。`None` = 一次查询由单条报告应答（绝大多数品牌）。
    ///
    /// 声明后 `frame` / `battery` 描述的是**组装结果**的布局，不是单包布局。
    pub reassembly: Option<Reassembly>,
    /// 应答确认位。`None` = 该协议无确认约定，不做该校验。
    pub ack: Option<AckField>,
    /// 备用报告 ID。运行时按「主 → 备」顺序尝试，主 ID 成功即锁定。
    pub fallback_report_id: Option<u8>,
    /// 实测的读数期望间隔（毫秒）：纯监听品牌是设备自发上报的周期，查询品牌是宿主的查询周期。
    pub expected_interval_ms: u64,
    /// 型号表文件名（相对型号表目录，如 `ajazz.json`），运行时由 [`crate::mappings`] 加载。
    ///
    /// 多个协议共用一份表时各写同一个文件名。
    pub device_table_file: &'static str,
    /// 本品牌下**其它**已接入的协议描述（不含自身）。`&[]` = 单协议品牌。
    ///
    /// 只给注册表用：启动时逐份 [`BrandSpec::validate`]，匹配路径不读它。
    pub variants: &'static [&'static BrandSpec],
    /// 按型号表条目选协议的钩子。`None` = 本协议适用于该品牌全部设备。
    pub route: Option<RouteFn>,
}

impl BrandSpec {
    /// 该设备实际生效的协议描述。
    ///
    /// `route: None` 的品牌直接返回自己；声明了钩子的品牌交给品牌自己的模块决定。
    pub fn for_device(&'static self, entry: &Entry) -> Option<&'static BrandSpec> {
        match self.route {
            Some(route) => route(entry),
            None => Some(self),
        }
    }

    /// 解析一帧所必需的最短 payload：必选字段（子标签、电量）都要落在里面。
    ///
    /// 充电标志**不计入**：它是可选字段，缺失表示「未知」。
    pub const fn min_payload_len(&self) -> usize {
        self.battery.min_payload_len()
    }

    /// 一帧报告的最小长度。
    pub const fn min_report_len(&self) -> usize {
        self.frame.payload_offset + self.min_payload_len()
    }

    /// 该品牌期望的采集节奏（家族级 `ProviderProfile` 必须覆盖它）。
    ///
    /// 有查询模板 = 节奏由宿主决定（[`Cadence::Polled`]）；否则 = 纯监听，节奏由设备决定
    /// （[`Cadence::ListenOnly`]）。
    pub const fn cadence(&self) -> Cadence {
        match self.request {
            Some(_) => Cadence::Polled {
                interval_ms: self.expected_interval_ms,
            },
            None => Cadence::ListenOnly {
                expected_interval_ms: self.expected_interval_ms,
            },
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

        if self.frame.cmd_index >= self.frame.payload_offset {
            return Err(SpecError::HeaderOverlap {
                brand: self.id,
                cmd_index: self.frame.cmd_index,
                payload_offset: self.frame.payload_offset,
            });
        }

        if self.battery.cmd_high.is_some() && self.frame.cmd_index + 1 >= self.frame.payload_offset
        {
            return Err(SpecError::CmdHighOutsideHeader {
                brand: self.id,
                cmd_index: self.frame.cmd_index,
            });
        }

        if self.battery.cmd_high.is_some() && self.battery.cmd.is_none() {
            return Err(SpecError::CmdHighWithoutCmd { brand: self.id });
        }

        if self.frame.report_id.is_some() && self.frame.payload_offset == 0 {
            return Err(SpecError::PayloadStartsAtReportId { brand: self.id });
        }

        if let Some(index) = self.frame.len_index
            && (index >= self.frame.payload_offset
                || index == self.frame.cmd_index
                || (self.battery.cmd_high.is_some() && index == self.frame.cmd_index + 1))
        {
            return Err(SpecError::LengthFieldOutsideHeader {
                brand: self.id,
                len_index: index,
                cmd_index: self.frame.cmd_index,
                payload_offset: self.frame.payload_offset,
            });
        }

        if self.frame.len_index.is_none() && self.frame.fixed_payload_len < self.min_payload_len() {
            return Err(SpecError::FixedPayloadTooShort {
                brand: self.id,
                fixed_payload_len: self.frame.fixed_payload_len,
                need: self.min_payload_len(),
            });
        }

        if self.battery.percent_mask == 0 {
            return Err(SpecError::ZeroPercentMask { brand: self.id });
        }

        // 子标签不得与电量字段抢同一字节。
        if let Some((index, _)) = self.battery.subtag
            && index == self.battery.percent_index
        {
            return Err(SpecError::SubtagOverlapsPercent { brand: self.id });
        }

        if let Some(charging) = self.battery.charging
            && charging.index == self.battery.percent_index
            && (charging.mask & self.battery.percent_mask) != 0
        {
            return Err(SpecError::ChargingOverlapsPercent { brand: self.id });
        }

        if let Some(listen) = self.listen {
            for frame in listen {
                let mut fields = Vec::with_capacity(3);
                fields.push(frame.subtag_index);
                fields.push(frame.status_index);
                if let Some((index, _)) = frame.percent {
                    fields.push(index);
                }
                let overlap = fields
                    .iter()
                    .enumerate()
                    .any(|(index, field)| fields[index + 1..].contains(field));
                let header = if frame.cmd2.is_some() { 2 } else { 1 };
                if overlap || frame.subtag_index < header {
                    return Err(SpecError::ListenLayoutInvalid { brand: self.id });
                }
            }
        }

        if let Some(notify) = self.notify {
            if notify.percent_mask == 0 {
                return Err(SpecError::ZeroPercentMask { brand: self.id });
            }
            if self.battery.cmd == Some(notify.cmd) && self.battery.cmd_high == notify.cmd_high {
                return Err(SpecError::NotifyCollidesWithReply { brand: self.id });
            }
            if let Some(charging) = notify.charging
                && charging.index == notify.percent_index
                && (charging.mask & notify.percent_mask) != 0
            {
                return Err(SpecError::ChargingOverlapsPercent { brand: self.id });
            }
        }

        // 查询模板必须自洽，且声明了写通道。
        if let Some(req) = self.request {
            if self.usage_page_write.is_none() {
                return Err(SpecError::RequestWithoutWritePage { brand: self.id });
            }
            if Some(req.usage_page) != self.usage_page_write {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
            if req.data_end() > req.report_len
                || req.cmd_index >= req.payload_offset
                || (req.cmd_high.is_some() && req.cmd_index + 1 >= req.payload_offset)
                || req.report_len < self.frame.payload_offset
                || req.prefix.len() > req.cmd_index
            {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
            if self.frame.report_id.is_some() && req.report_len == 0 {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
            if req.interval_ms.is_none() && self.listen.is_none() && self.notify.is_none() {
                return Err(SpecError::PassiveWithoutPushSource { brand: self.id });
            }
            if self.usage_page != 0 && req.usage_page != self.usage_page {
                return Err(SpecError::RequestPageDiffersFromRead { brand: self.id });
            }
        }

        if let Some(block) = self.block_offset {
            let Some(req) = self.request else {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            };
            let max = [
                Some(block.lo_index),
                Some(block.hi_index),
                block.checksum_index,
                block.size_index,
            ]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or_default();
            if block.stride == 0 || block.lo_index == block.hi_index || max >= req.payload.len() {
                return Err(SpecError::RequestLayoutInvalid { brand: self.id });
            }
        }

        if let Some(probe) = self.profile_probe
            && (probe.order_len == 0
                || probe.active_index == probe.order_offset
                || probe.request.payload.len() < probe.order_offset + probe.order_len)
        {
            return Err(SpecError::RequestLayoutInvalid { brand: self.id });
        }

        match self.transport {
            ReplyTransport::Listen | ReplyTransport::InterruptQuery => {}
            ReplyTransport::FeatureQuery { reply_len }
            | ReplyTransport::FeaturePoll { reply_len } => {
                if self.frame.report_id.is_none() || reply_len < self.min_report_len() {
                    return Err(SpecError::FeatureLayoutInvalid { brand: self.id });
                }
            }
        }
        if matches!(self.transport, ReplyTransport::FeaturePoll { .. }) && self.request.is_some() {
            return Err(SpecError::FeaturePollHasRequest { brand: self.id });
        }
        if matches!(self.transport, ReplyTransport::FeatureQuery { .. }) && self.request.is_none() {
            return Err(SpecError::FeatureQueryWithoutRequest { brand: self.id });
        }

        if let Some(reasm) = self.reassembly {
            if !matches!(self.transport, ReplyTransport::InterruptQuery) || self.request.is_none() {
                return Err(SpecError::ReassemblyWithoutQuery { brand: self.id });
            }
            if reasm.payload_offset <= reasm.seq_index
                || reasm.payload_offset <= reasm.total_index
                || reasm.seq_index == reasm.total_index
                || reasm.timeout_ms == 0
            {
                return Err(SpecError::ReassemblyLayoutInvalid { brand: self.id });
            }
        }

        if let Some(alt) = self.usage_page_alt
            && (alt == self.usage_page || self.usage_page == 0)
        {
            return Err(SpecError::AltUsagePageInvalid { brand: self.id });
        }

        if let Some(ack) = self.ack
            && ack.index >= self.min_report_len()
        {
            return Err(SpecError::AckOutsideReport {
                brand: self.id,
                index: ack.index,
                min_report_len: self.min_report_len(),
            });
        }

        if let Some(alt) = self.fallback_report_id
            && (!matches!(self.transport, ReplyTransport::FeatureQuery { .. })
                || self.frame.report_id.is_none()
                || self.frame.report_id == Some(alt))
        {
            return Err(SpecError::FallbackReportIdInvalid { brand: self.id });
        }

        if self.expected_interval_ms == 0 {
            return Err(SpecError::ZeroInterval { brand: self.id });
        }

        Ok(())
    }

    /// 品牌节奏是否落在家族能力声明的预算内（内部先跑 [`Self::validate`]）。
    ///
    /// 家族只有一组三档阈值链；间隔慢到正常上报会被判过期的品牌，必须先改家族的阈值声明。
    pub fn validate_against(&self, profile: &ProviderProfile) -> Result<(), SpecError> {
        self.validate()?;
        if profile.transport != voltnest_provider::Transport::Hid {
            return Err(SpecError::ProfileTransportMismatch { brand: self.id });
        }
        if profile.charging == ChargingSupport::Unavailable && self.battery.charging.is_some() {
            return Err(SpecError::ProfileChargingMismatch { brand: self.id });
        }
        let budget = profile
            .thresholds_for(voltnest_provider::Sensitivity::Standard)
            .stale_ms;
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
    /// 双字节命令字的高字节越过 payload 起点。
    CmdHighOutsideHeader {
        /// 品牌标识。
        brand: &'static str,
        /// 命令字低字节下标。
        cmd_index: usize,
    },
    /// 声明了命令字高字节，却没有声明命令字低字节。
    CmdHighWithoutCmd {
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
    /// 电量掩码为 0（取位后恒为 0%）。
    ZeroPercentMask {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 充电标志与电量字段重叠。
    ChargingOverlapsPercent {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 监听帧字段互相重叠。
    ListenLayoutInvalid {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 推送帧的命令字与应答帧的命令字相同（两条通道无法区分）。
    NotifyCollidesWithReply {
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
    /// 查询模板不周期发送，但该协议没有任何推送源（`listen` / `notify` 全空）。
    PassiveWithoutPushSource {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 查询模板的写通道与读通道不同页（运行时只有一个读句柄，无法兑现）。
    RequestPageDiffersFromRead {
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
    /// Feature report 布局缺少报告 ID或长度不足。
    FeatureLayoutInvalid {
        /// 品牌标识。
        brand: &'static str,
    },
    /// Feature 轮询不应同时声明查询报文。
    FeaturePollHasRequest {
        /// 品牌标识。
        brand: &'static str,
    },
    /// Feature 查询必须声明查询报文。
    FeatureQueryWithoutRequest {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 声明了多包重组，但通道不是 output 查询，或没有查询报文。
    ReassemblyWithoutQuery {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 多包重组的包内下标自相矛盾。
    ReassemblyLayoutInvalid {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 备用 UsagePage 与主页面相同，或主页面是通配。
    AltUsagePageInvalid {
        /// 品牌标识。
        brand: &'static str,
    },
    /// 确认位下标落在一帧之外。
    AckOutsideReport {
        /// 品牌标识。
        brand: &'static str,
        /// 确认位下标。
        index: usize,
        /// 一帧的最小长度。
        min_report_len: usize,
    },
    /// 备用报告 ID 的声明不成立（非 feature 查询 / 无主 ID / 与主 ID 相同）。
    FallbackReportIdInvalid {
        /// 品牌标识。
        brand: &'static str,
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
    /// 型号表无法解析（内置注册表现按品牌粒度跳过坏表，此变体保留供自定义装配方使用）。
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
            Self::CmdHighOutsideHeader { brand, cmd_index } => write!(
                f,
                "品牌 {brand} 的双字节命令字（低字节 @{cmd_index}）越过了 payload 起点"
            ),
            Self::CmdHighWithoutCmd { brand } => {
                write!(f, "品牌 {brand} 声明了命令字高字节，却没有命令字低字节")
            }
            Self::LengthFieldOutsideHeader {
                brand,
                len_index,
                cmd_index,
                payload_offset,
            } => write!(
                f,
                "品牌 {brand} 的长度字段下标 {len_index} 不在头部（须 < payload_offset={payload_offset} 且不等于命令字下标 {cmd_index}）"
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
            Self::ZeroPercentMask { brand } => {
                write!(f, "品牌 {brand} 的电量掩码为 0，取位后恒为 0%")
            }
            Self::ChargingOverlapsPercent { brand } => {
                write!(f, "品牌 {brand} 的充电标志与电量字段重叠")
            }
            Self::ListenLayoutInvalid { brand } => {
                write!(f, "品牌 {brand} 的监听帧字段与其他字段重叠")
            }
            Self::NotifyCollidesWithReply { brand } => {
                write!(f, "品牌 {brand} 的推送帧命令字与应答帧相同，两者无法区分")
            }
            Self::PassiveWithoutPushSource { brand } => write!(
                f,
                "品牌 {brand} 声明了不周期查询，却没有 listen / notify 任何推送源"
            ),
            Self::RequestWithoutWritePage { brand } => write!(
                f,
                "品牌 {brand} 声明了查询模板，必须同时给出 usage_page_write"
            ),
            Self::RequestLayoutInvalid { brand } => {
                write!(f, "品牌 {brand} 的查询模板越界")
            }
            Self::RequestPageDiffersFromRead { brand } => write!(
                f,
                "品牌 {brand} 的查询写通道与读通道不同页，运行时只有一个读句柄，无法兑现"
            ),
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
            Self::FeatureLayoutInvalid { brand } => {
                write!(f, "品牌 {brand} 的 feature report 布局非法")
            }
            Self::FeaturePollHasRequest { brand } => {
                write!(f, "品牌 {brand} 的 feature 轮询不得声明查询报文")
            }
            Self::FeatureQueryWithoutRequest { brand } => {
                write!(f, "品牌 {brand} 的 feature 查询缺少查询报文")
            }
            Self::ReassemblyWithoutQuery { brand } => {
                write!(
                    f,
                    "品牌 {brand} 声明了多包重组，但通道不是带查询报文的 output 查询"
                )
            }
            Self::ReassemblyLayoutInvalid { brand } => {
                write!(
                    f,
                    "品牌 {brand} 的多包重组下标自相矛盾（头部越界或电量与状态抢同一字节）"
                )
            }
            Self::AltUsagePageInvalid { brand } => {
                write!(
                    f,
                    "品牌 {brand} 的备用 UsagePage 与主页面相同，或主页面是通配"
                )
            }
            Self::AckOutsideReport {
                brand,
                index,
                min_report_len,
            } => write!(
                f,
                "品牌 {brand} 的确认位下标 {index} 超出一帧最小长度 {min_report_len}"
            ),
            Self::FallbackReportIdInvalid { brand } => write!(
                f,
                "品牌 {brand} 的备用报告 ID 只允许与主 ID 不同的 feature 查询"
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

    /// AJAZZ 读页上的合法查询模板：写页、读页与模板三者一致。
    fn valid_request() -> RequestTemplate {
        RequestTemplate {
            usage_page: 0xFFB2,
            cmd: 0xFB,
            cmd_high: None,
            prefix: &[],
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03],
            interval_ms: Some(10_000),
            inverted: false,
            retry: 0,
        }
    }

    /// 带合法查询模板的 AJAZZ 描述（写页迁到读页上）。
    fn with_request(mut spec: BrandSpec) -> BrandSpec {
        spec.usage_page_write = Some(0xFFB2);
        spec.request = Some(valid_request());
        spec
    }

    const SYNTHETIC_REASSEMBLY: Reassembly = Reassembly {
        cmd: 0x55,
        total_index: 1,
        seq_index: 2,
        payload_offset: 4,
        trailer_len: 1,
        timeout_ms: 200,
    };

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
            mask: 0xFF,
            index: spec.battery.percent_index,
            true_values: &[1],
            error_from: None,
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
            cmd_high: None,
            prefix: &[],
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03],
            interval_ms: Some(10_000),
            inverted: false,
            retry: 0,
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
            cmd_high: None,
            prefix: &[],
            report_len: 6,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x00, 0x00],
            interval_ms: None,
            inverted: false,
            retry: 0,
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
            cmd_high: None,
            prefix: &[],
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x01],
            interval_ms: Some(10_000),
            inverted: false,
            retry: 0,
        };
        assert_eq!(
            req.build(Some(0x06)),
            vec![0x06, 0xFB, 0x00, 0x00, 0x00, 0x03, 0x01, 0x00]
        );
        assert_eq!(
            req.build(None),
            vec![0x00, 0xFB, 0x00, 0x00, 0x00, 0x03, 0x01, 0x00]
        );
        assert_eq!(req.data_end(), 7);
    }

    #[test]
    fn attempts_counts_first_try_plus_retries() {
        let mut req = RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            cmd_high: None,
            prefix: &[],
            report_len: 8,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03],
            interval_ms: Some(10_000),
            inverted: false,
            retry: 0,
        };
        assert_eq!(req.attempts(), 1, "retry=0 时一轮只有一次尝试");
        req.retry = 2;
        assert_eq!(req.attempts(), 3, "迈从官方 retryTime=2：首发 + 两次重试");
    }

    #[test]
    fn request_template_never_panics_on_tight_buffers() {
        let req = RequestTemplate {
            usage_page: 0xFFB1,
            cmd: 0xFB,
            cmd_high: None,
            prefix: &[],
            report_len: 6,
            cmd_index: 1,
            payload_offset: 5,
            payload: &[0x03, 0x01, 0x02],
            interval_ms: None,
            inverted: false,
            retry: 0,
        };
        assert_eq!(req.build(Some(0x06)).len(), 6);
    }

    /// 诊断文本是对外契约：每个变体都要渲染出一条点名品牌的完整信息。
    #[test]
    fn spec_error_display_covers_every_variant() {
        const B: &str = "diag-brand";
        let cases = vec![
            SpecError::EmptyLabel { brand: B },
            SpecError::HeaderOverlap {
                brand: B,
                cmd_index: 1,
                payload_offset: 2,
            },
            SpecError::PayloadStartsAtReportId { brand: B },
            SpecError::LengthFieldOutsideHeader {
                brand: B,
                len_index: 1,
                cmd_index: 2,
                payload_offset: 3,
            },
            SpecError::FixedPayloadTooShort {
                brand: B,
                fixed_payload_len: 1,
                need: 2,
            },
            SpecError::SubtagOverlapsPercent { brand: B },
            SpecError::ZeroPercentMask { brand: B },
            SpecError::ChargingOverlapsPercent { brand: B },
            SpecError::ListenLayoutInvalid { brand: B },
            SpecError::NotifyCollidesWithReply { brand: B },
            SpecError::CmdHighOutsideHeader {
                brand: B,
                cmd_index: 1,
            },
            SpecError::CmdHighWithoutCmd { brand: B },
            SpecError::PassiveWithoutPushSource { brand: B },
            SpecError::RequestWithoutWritePage { brand: B },
            SpecError::RequestLayoutInvalid { brand: B },
            SpecError::RequestPageDiffersFromRead { brand: B },
            SpecError::ZeroInterval { brand: B },
            SpecError::CadenceExceedsProfile {
                brand: B,
                expected_interval_ms: 1000,
                stale_ms: 500,
            },
            SpecError::FeatureLayoutInvalid { brand: B },
            SpecError::FeaturePollHasRequest { brand: B },
            SpecError::FeatureQueryWithoutRequest { brand: B },
            SpecError::ReassemblyWithoutQuery { brand: B },
            SpecError::ReassemblyLayoutInvalid { brand: B },
            SpecError::AltUsagePageInvalid { brand: B },
            SpecError::AckOutsideReport {
                brand: B,
                index: 9,
                min_report_len: 4,
            },
            SpecError::FallbackReportIdInvalid { brand: B },
            SpecError::ProfileTransportMismatch { brand: B },
            SpecError::ProfileChargingMismatch { brand: B },
            SpecError::DuplicateBrandId { brand: B },
            SpecError::DeviceTable {
                brand: B,
                detail: "坏的 JSON".to_string(),
            },
        ];
        for err in &cases {
            let text = err.to_string();
            assert!(text.contains(B), "{err:?} 的诊断文本未点名品牌：{text}");
        }
        assert_eq!(SpecError::EmptyBrandId.to_string(), "品牌标识不得为空");
        assert_eq!(
            SpecError::DuplicateDeviceKey {
                vid: 0x260D,
                pid: 0x0030,
                first: "dareu",
                second: "rapoo",
            }
            .to_string(),
            "VID:PID 260D:0030 被品牌 dareu 与 rapoo 同时登记（精确匹配必须唯一）"
        );
        let boxed: Box<dyn std::error::Error> = Box::new(SpecError::ZeroInterval { brand: B });
        assert!(boxed.to_string().contains(B));
    }

    /// 充电标志先按 mask 取位再比取值集合；错误态起点未声明时恒不报错。
    #[test]
    fn charging_field_masks_before_comparing() {
        let whole = ChargingField {
            index: 0,
            mask: 0xFF,
            true_values: &[0x01],
            error_from: None,
        };
        assert!(whole.is_charging(0x01));
        assert!(!whole.is_charging(0x11), "整字节比较不认半字节");
        let nibble = ChargingField {
            index: 0,
            mask: 0x0F,
            true_values: &[0x01],
            error_from: Some(0x03),
        };
        assert!(nibble.is_charging(0x11), "取位后应判为充电");
        assert!(!nibble.is_charging(0x12));

        assert!(!whole.is_error(0xFF), "未声明错误态时不得报错");
        assert!(!nibble.is_error(0x02));
        assert!(nibble.is_error(0x03));
        assert!(nibble.is_error(0xFF), "超过起点同样算错误态");
    }

    /// 声明了 route 的品牌由品牌自己分派；没声明的直接返回自己。
    #[test]
    fn for_device_returns_self_when_no_route_declared() {
        static SPEC: BrandSpec = ajazz::SPEC;
        let entry = crate::table::Entry {
            name: None,
            vendor_device_id: None,
            device_type: String::new(),
            mode: String::new(),
            extra: Default::default(),
        };
        let resolved = SPEC.for_device(&entry).expect("无 route 的品牌必须回自己");
        assert_eq!(resolved.id, SPEC.id);

        let listen = ListenSpec {
            cmd: 0xFA,
            cmd2: None,
            subtag_index: 5,
            subtag_value: 0x03,
            percent: Some((2, 0xFF)),
            status_index: 7,
            status: ListenStatus::Nibbles {
                charging_low_nibble: 0,
            },
        };
        assert_eq!(listen.min_len(), 8);
        let reasm = Reassembly {
            cmd: 0x55,
            total_index: 3,
            seq_index: 1,
            payload_offset: 4,
            trailer_len: 1,
            timeout_ms: 200,
        };
        assert_eq!(reasm.min_packet_len(), 4);
    }

    /// feature 通道要带读取长度，其余两条通道没有这个字段。
    #[test]
    fn transport_reports_feature_shape() {
        assert!(
            ReplyTransport::FeatureQuery { reply_len: 32 }.is_feature(),
            "feature 查询属于 feature 通道"
        );
        assert!(ReplyTransport::FeaturePoll { reply_len: 8 }.is_feature());
        assert!(!ReplyTransport::Listen.is_feature());
        assert!(!ReplyTransport::InterruptQuery.is_feature());

        assert_eq!(
            ReplyTransport::FeatureQuery { reply_len: 32 }.reply_len(),
            Some(32)
        );
        assert_eq!(
            ReplyTransport::FeaturePoll { reply_len: 8 }.reply_len(),
            Some(8)
        );
        assert_eq!(ReplyTransport::Listen.reply_len(), None);
        assert_eq!(ReplyTransport::InterruptQuery.reply_len(), None);
    }

    /// 有查询模板 = 节奏由宿主决定；纯监听 = 节奏由设备决定，两者共用同一间隔预算。
    #[test]
    fn cadence_follows_request_template() {
        assert_eq!(
            base().cadence(),
            Cadence::ListenOnly {
                expected_interval_ms: base().expected_interval_ms
            }
        );
        let polled = with_request(base()).cadence();
        assert_eq!(
            polled,
            Cadence::Polled {
                interval_ms: base().expected_interval_ms
            }
        );
    }

    /// 取反只作用于报告 ID 之后的字节：ID 本身被取反的话设备会直接丢掉整帧。
    #[test]
    fn inverted_template_flips_bytes_after_report_id() {
        let mut req = valid_request();
        req.inverted = true;
        assert_eq!(
            req.build(Some(0x06)),
            vec![0x06, 0x04, 0xFF, 0xFF, 0xFF, 0xFC, 0xFF, 0xFF]
        );
        assert_eq!(
            req.build(None),
            vec![0xFF, 0x04, 0xFF, 0xFF, 0xFF, 0xFC, 0xFF, 0xFF]
        );
    }

    /// 布局错误逐条可辨：每条只违反一条不变量，且拒绝原因不得互相冒充。
    #[test]
    fn validate_rejects_layout_mistakes() {
        fn rejects(mutate: impl FnOnce(&mut BrandSpec), want: fn(&SpecError) -> bool, label: &str) {
            let mut spec = base();
            mutate(&mut spec);
            match spec.validate() {
                Err(err) if want(&err) => {}
                other => panic!("{label}：期望被拒绝，实际 {other:?}"),
            }
        }

        rejects(
            |s| s.id = "",
            |e| matches!(e, SpecError::EmptyBrandId),
            "空品牌标识",
        );
        rejects(
            |s| s.label = "",
            |e| matches!(e, SpecError::EmptyLabel { .. }),
            "空展示名",
        );
        // 监听帧：子标签与电量抢同一字节。
        rejects(
            |s| {
                s.listen = Some(&[ListenSpec {
                    cmd: 0xFA,
                    cmd2: None,
                    subtag_index: 1,
                    subtag_value: 0x03,
                    percent: Some((1, 0xFF)),
                    status_index: 2,
                    status: ListenStatus::Nibbles {
                        charging_low_nibble: 0,
                    },
                }])
            },
            |e| matches!(e, SpecError::ListenLayoutInvalid { .. }),
            "监听帧字段重叠",
        );
        rejects(
            |s| {
                s.usage_page_write = Some(0xFFB1);
                s.request = Some(RequestTemplate {
                    usage_page: 0xFFB1,
                    ..valid_request()
                });
            },
            |e| matches!(e, SpecError::RequestPageDiffersFromRead { .. }),
            "写页与读页不一致",
        );
        // feature 通道读不出满足最小帧长的应答。
        rejects(
            |s| {
                *s = with_request(*s);
                s.transport = ReplyTransport::FeatureQuery { reply_len: 1 };
            },
            |e| matches!(e, SpecError::FeatureLayoutInvalid { .. }),
            "feature 读取长度不足",
        );
        rejects(
            |s| {
                *s = with_request(*s);
                s.transport = ReplyTransport::FeaturePoll { reply_len: 64 };
            },
            |e| matches!(e, SpecError::FeaturePollHasRequest { .. }),
            "轮询通道不得带查询模板",
        );
        rejects(
            |s| s.transport = ReplyTransport::FeatureQuery { reply_len: 64 },
            |e| matches!(e, SpecError::FeatureQueryWithoutRequest { .. }),
            "查询通道必须有查询模板",
        );
        rejects(
            |s| {
                *s = with_request(*s);
                s.reassembly = Some(SYNTHETIC_REASSEMBLY);
            },
            |e| matches!(e, SpecError::ReassemblyWithoutQuery { .. }),
            "多包重组只能挂在 output 查询上",
        );
        rejects(
            |s| {
                *s = with_request(*s);
                s.transport = ReplyTransport::InterruptQuery;
                s.reassembly = Some(Reassembly {
                    seq_index: 1,
                    ..SYNTHETIC_REASSEMBLY
                });
            },
            |e| matches!(e, SpecError::ReassemblyLayoutInvalid { .. }),
            "总包数与序号抢同一字节",
        );
        rejects(
            |s| s.usage_page_alt = Some(s.usage_page),
            |e| matches!(e, SpecError::AltUsagePageInvalid { .. }),
            "备用页与主页相同",
        );
        rejects(
            |s| {
                s.ack = Some(crate::spec::AckField {
                    index: 99,
                    mask: 0x0F,
                    value: 0x02,
                })
            },
            |e| matches!(e, SpecError::AckOutsideReport { .. }),
            "确认位落在一帧之外",
        );
        rejects(
            |s| s.fallback_report_id = Some(0x06),
            |e| matches!(e, SpecError::FallbackReportIdInvalid { .. }),
            "监听通道没有备用报告 ID",
        );
        rejects(
            |s| s.battery.percent_mask = 0,
            |e| matches!(e, SpecError::ZeroPercentMask { .. }),
            "电量掩码为 0",
        );
        rejects(
            |s| {
                s.battery.cmd = None;
                s.battery.cmd_high = Some(0x09);
            },
            |e| matches!(e, SpecError::CmdHighWithoutCmd { .. }),
            "只有命令字高字节",
        );
        // 双字节命令的高字节不能压到 payload 上。
        rejects(
            |s| {
                s.frame.cmd_index = 1;
                s.frame.payload_offset = 2;
                s.battery.cmd_high = Some(0x09);
            },
            |e| matches!(e, SpecError::CmdHighOutsideHeader { .. }),
            "命令字高字节越过 payload",
        );
        rejects(
            |s| {
                s.frame.len_index = Some(2);
                s.frame.payload_offset = 2;
            },
            |e| matches!(e, SpecError::LengthFieldOutsideHeader { .. }),
            "长度字段落在 payload 里",
        );
        rejects(
            |s| {
                s.frame.len_index = Some(1);
                s.frame.cmd_index = 1;
            },
            |e| matches!(e, SpecError::LengthFieldOutsideHeader { .. }),
            "长度字段与命令字抢同一字节",
        );
        rejects(
            |s| {
                s.notify = Some(NotifySpec {
                    cmd: 0xFA,
                    cmd_high: None,
                    percent_index: 0,
                    percent_mask: 0xFF,
                    charging: None,
                });
            },
            |e| matches!(e, SpecError::NotifyCollidesWithReply { .. }),
            "推送帧与应答命令字相同",
        );
        rejects(
            |s| {
                *s = with_request(*s);
                s.request = s.request.map(|mut req| {
                    req.interval_ms = None;
                    req
                });
                s.listen = None;
                s.notify = None;
            },
            |e| matches!(e, SpecError::PassiveWithoutPushSource { .. }),
            "不轮询又没有推送源",
        );
        rejects(
            |s| {
                s.notify = Some(NotifySpec {
                    cmd: 0x01,
                    cmd_high: None,
                    percent_index: 0,
                    percent_mask: 0,
                    charging: None,
                });
            },
            |e| matches!(e, SpecError::ZeroPercentMask { .. }),
            "推送帧电量掩码为 0",
        );
    }

    /// 推送帧是另一套下标：`NotifySpec::fields()` 必须原样交给解析器，
    /// 不能被 [`BrandSpec::battery`] 的值悄悄顶掉。
    #[test]
    fn notify_fields_are_independent_of_reply_fields() {
        let mut spec = base();
        spec.notify = Some(NotifySpec {
            cmd: 0x00,
            cmd_high: Some(0x07),
            percent_index: 3,
            percent_mask: 0xFF,
            charging: Some(ChargingField {
                mask: 0xFF,
                index: 2,
                true_values: &[0x01],
                error_from: None,
            }),
        });
        assert!(spec.validate().is_ok(), "命令不同的推送帧应当合法");

        let fields = spec.notify.expect("刚设置过").fields();
        assert_eq!(fields.cmd, Some(0x00));
        assert_eq!(fields.cmd_high, Some(0x07));
        assert_eq!(fields.percent_index, 3);
        assert_ne!(
            fields.percent_index, spec.battery.percent_index,
            "推送帧与应答帧的下标是两份"
        );
        assert_eq!(spec.battery.percent_index, base().battery.percent_index);
    }

    /// 一个字节装两个字段：电量与充电**可以**同下标，只要掩码互不重叠。
    ///
    /// 迈从 GLW 键盘就是这样（`payload[32]`：bit0..6 电量、bit7 充电）。反过来，掩码重叠时
    /// 必须照旧拒绝 —— 同一批位既是电量又是充电标志，解析结果一定是错的。
    #[test]
    fn charging_may_share_byte_only_when_masks_are_disjoint() {
        let mut spec = base();
        spec.battery.percent_index = 1;
        spec.battery.percent_mask = 0x7F;
        spec.battery.charging = Some(ChargingField {
            index: 1,
            mask: 0x80,
            true_values: &[0x80],
            error_from: None,
        });
        assert!(
            spec.validate().is_ok(),
            "电量 bit0..6 + 充电 bit7 是合法布局：{:?}",
            spec.validate()
        );
        assert_eq!(spec.battery.percent_of(0x80 | 42), 42, "取位后只剩电量");

        spec.battery.percent_mask = 0xFF;
        assert_eq!(
            spec.validate(),
            Err(SpecError::ChargingOverlapsPercent { brand: spec.id })
        );
    }
}
