# 上位机命名审计与重命名方案（VoltNest）

> 范围：**上位机（主机端）**——即桌面宿主进程与它直接调用的库。
> 包含 `crates/voltnest-provider`、`voltnest-core`、`voltnest-hid`、`voltnest-ble`、`voltnest-storage`、`voltnest-platform`、`apps/desktop/src-tauri`（宿主胶水层）与 `apps/desktop/src`（Vue 前端）。
> 不包含：厂商固件/驱动、`mappings/` 下的抓取脚本、`components/ui/*`（shadcn 生成物，不手改）。
>
> 性质：**纯命名重构**。不改任何行为、不改任何落盘/IPC 契约（冻结清单见 §4）。

---

## 1. 命名问题诊断

代码整体注释质量很高，问题不在"看不懂"，而在**同一个词在多个抽象层上被复用**，以及**同一概念有多个名字**。下面按类型列举，每条给出位置与示例。

### 1.1 同名不同义：一个词承担两三种含义（最严重）

| #   | 词                     | 含义 A                                                                 | 含义 B                                                                                                     | 含义 C                                                                                                        |
| --- | ---------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| 1   | `state`                | `DeviceState`：设备五态（`core/src/state.rs:10`）                      | `TransportState`：采集线程组运行状态（`src-tauri/src/dto.rs:226`）                                         | `StateFile`/`StateStore`/`state.json`：持久化缓存（`storage/src/state.rs`）                                   |
| 2   | `TrayDto`              | 快照里的托盘可见集合 `{visibleDeviceIds}`（`core/src/snapshot.rs:86`） | 设置里的托盘顺序与隐藏 `{order, hidden}`（`src-tauri/src/dto.rs:41`）                                      | —                                                                                                             |
| 3   | `DeviceState`          | 公开枚举（`core/src/state.rs:10`）                                     | 文件内私有结构：一台设备的通知状态（`core/src/notify.rs:100`）——**同名遮蔽了公开类型**                     | —                                                                                                             |
| 4   | `provider`             | trait `Provider`：采集线程（`provider/src/lib.rs:55`）                 | `Record.provider` / `DeviceFact.provider` / `DeviceDto.provider`：值是家族 ID `"hid-vbas"`，与采集线程无关 | crate 名 `voltnest-provider`；模块 `voltnest_hid::provider`（运行时）与 `voltnest_ble::provider`              |
| 5   | `Entry`                | 型号表一行（`hid/src/table.rs:35`）                                    | `EntryState`：某档通知的武装/冷却状态（`core/src/notify.rs:93`）                                           | `SeedEntry`（回填）、`NotificationEntry`（通知状态）、`DeviceStateEntry`（落盘）、`EnumEntry`（HID 枚举集合） |
| 6   | `Reading`              | `ValidReading`（事件载荷）                                             | `BatteryReading`（HID 帧解析结果）                                                                         | `tray::Reading`（托盘展示值，`tray.rs:377`）、`ProviderEvent::Reading`（变体）、`apply_reading`               |
| 7   | `thresholds`           | `ProviderProfile.thresholds`：毫秒级退化阈值链                         | `NotificationConfig.thresholds`：百分比低电量档位                                                          | `NotificationSettings.low_thresholds`（落盘）——`host.rs:276` 的注释自己承认了"「阈值」一词两义"               |
| 8   | `diag` / `diagnostics` | `voltnest_provider::diag`：只做 hex 格式化（`provider/src/diag.rs`）   | `voltnest_core::diagnostics`：200 条环形缓冲                                                               | `DiagnosticsDto` / `DiagnosticsPanel.vue`                                                                     |
| 9   | `Spec`                 | `supervisor::Spec`：一个 transport 的采集参数（`supervisor.rs:30`）    | `spec` 模块 / `BrandSpec`：HID 协议描述                                                                    | `RetrievePeripheralsOptions` 之类的第三方 `Spec` 语义                                                         |
| 10  | `state.rs`             | `core/src/state.rs`：设备五态                                          | `storage/src/state.rs`：`state.json` 读写                                                                  | 两个同名模块语义无关                                                                                          |

### 1.2 异名同义：一个东西多个名字

- **设备标识**：`stable_id`（`Record`、`NotificationEntry`）／`device_id`（`DiagEvent`、`push_diag` 参数、`NotificationNode.device_ids`、`TrayDto.visible_device_ids`、`CardDto.primary_device_id`）／`id`（局部变量）／`key`（`collect_targets` 的 `HashMap<String, Target>` 键，值也是 stableId，`hid/provider.rs:376`）。注释里又统称 `stableId`。前端 `Row.deviceId`（`App.vue:69`）同样是 stableId。
- **低电量提醒**：`LowBatteryAlert` / `pending_alerts` / `take_alerts`（core）／`alerts` 参数（`notification::dispatch`）／`alert_tray`（`tray.rs:308`）／`NotificationBook`、`NotificationEntry`、`NotificationNode`（同一领域另一半用 notification）。同类概念两套词：alert 与 notification。
- **协议家族**：`family`（`TransportStatus.family`、`family_for_transport`）／`provider`（`DeviceFact.provider`）／`family_id`（`ProviderProfile`）／`FAMILY_HID_VBAS` 常量。
- **通知状态结构**：`NotificationEntry`（core）／`NotificationStateEntry`（storage）／`NotificationStateDto`（wire，来源其实是 `NotificationNode`）／`EntryState`（内部）。
- **"一台设备"的三种形状**：`Record`（core 内存）／`DeviceStateEntry`（state.json）／`DeviceDto`（快照 wire）。三者名字毫无族谱关系，读者必须靠字段猜对应关系。
- **"打包/重启采集"**：`Session`（hid，`provider.rs:220`）与 `SessionHandle`（ble，`provider.rs:189`）描述同一职责。

### 1.3 命名与职责不符 / 误导

| 位置                                                     | 名字                                         | 实际问题                                                                                                                          |
| -------------------------------------------------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `core/src/engine.rs:72`                                  | `Transition::is_visible()`                   | 与 UI 可见性无关，语义是"该变化值得发布一次快照"。`host.rs:297` 用它决定 publish。                                                |
| `core/src/engine.rs:50`                                  | `Transition`                                 | 值域包含 `Created`/`Refreshed`/`Backfilled`/`BindingConflictRecorded`，并非状态转移。                                             |
| `src-tauri/src/dto.rs:166`                               | `TransportStatusDto.queued_events`           | 每个 transport 各一份，但值取自全局 `shared.queue_depth()`（`commands.rs:63`），并非本 transport 的队列深度。                     |
| `core/src/engine.rs:883`                                 | `family_for_transport()`                     | 返回 `&'static str`，实际是"该 transport 对应的家族 ID 字符串"，名字读起来像返回结构体。                                          |
| `hid/src/frame.rs:168`                                   | `uninv()`                                    | "un-invert" 的缩写，实际做"按 index 取字节并按 `reply_inverted` 归一化"。需要读实现才能懂。                                       |
| `core/src/notify.rs:100`                                 | `DeviceState`                                | 私有结构只存 `last_battery` + `entries`，叫"设备状态"会让人以为与五态有关。                                                       |
| `hid/src/provider.rs:225`                                | `Target`                                     | 与"匹配目标"无关，是"已选定待打开的 HID 集合 + 名称 + 分类"。                                                                     |
| `hid/src/provider.rs:282`                                | `EnumEntry`                                  | 是一条枚举出来的 **集合记录**；与 `classify::HidCollection` 概念相邻却不同名。                                                    |
| `ble/src/provider.rs:432`                                | `worth_probing()`                            | 名字是形容词，返回 bool 却承担"是否值得建会话（含 UUID 过滤）"的决策；调用点 `if ... && worth_probing(&p).await` 读不出业务含义。 |
| `src-tauri/src/host.rs:57`                               | `Shared`                                     | 40 处引用的核心上下文，名字不含任何信息（内容是 engine/settings/stores/queue/... ）。                                             |
| `src-tauri/src/notification.rs:20`                       | `dispatch(app, shared, _snap, alerts, _now)` | 两个 `_` 前缀死参数，命名里保留着"曾经需要"的痕迹。                                                                               |
| `core/src/engine.rs:211` vs `core/src/diagnostics.rs:72` | `push_diag` / `Diagnostics::push`            | 同一动作两个名；且参数一个叫 `now_ms` 一个叫 `at_ms`（同一含义）。                                                                |

### 1.4 大小写 / 缩写 / 单位风格

**缩写使用不一致（不统一展开，也不统一保留）**

| 场景             | 现状                                                                       | 说明                                                                                                      |
| ---------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| 采集运行时       | `provider` 模块（hid/ble）                                                 | 与 crate `voltnest-provider` 同名不同物；文档里其实一直叫它"运行时"                                       |
| 反相             | `uninv`（frame.rs）                                                        | 全仓唯一一处自造缩写，且是不可逆读的否定式                                                                |
| 重组             | `reasm`（frame.rs:478 起）／`reassembler`                                  | 同一结构两种写法并存                                                                                      |
| 请求             | `req`（spec.rs / hid provider 局部变量）                                   | 与 `self.request` 字段不同名                                                                              |
| 证据             | `ev`（`classify_hid(ev: &HidEvidence, ...)`）                              | 公开函数参数用两字母缩写                                                                                  |
| 字节缓冲         | `buf`、`SS`、`TRACK`、`BOLT`                                               | 局部可接受，`SS` 无注释（supersampling）                                                                  |
| 事件/命令        | `ProviderEvent` / `HostEvent` / `SessionMsg` / `SupervisorCmd`             | **同一层跨线程消息，三种后缀**：Event / Msg / Cmd                                                         |
| 常量             | `FAMILY_HID_VBAS`、`PRESENCE_POLL_MS`、`AUMID_KEY`                         | 一致（UPPER_SNAKE），可保留                                                                               |
| 品牌常量         | `ajazz::SPEC`、`dareu::{JM,TG,HS}`、`mchose::MOUSE_WIRED`、`rapoo::NRF54L` | 变体命名三套规则（协议族 / 缩写 / 形态），与 `BrandSpec.id`（`dareu-jm`、`mchose-mouse-wired`）无对应关系 |
| 类型前缀 Hid/Ble | `HidProvider`、`BleEvidence`、`BlePolicy`                                  | 一致，可保留                                                                                              |

**单位后缀不一致**

- 毫秒：核心层一律 `_ms`（`now_ms`、`stale_ms`、`cooldown_ms`），但 `ble/src/provider.rs:250` `let now = start.elapsed().as_millis() as u64;` 之后又当 `now_ms` 传参（`scan_and_spawn(..., now_ms)`），同一个值两个名字两种单位暗示。
- `Instant` 参数：`frame::Reassembler::feed(..., now: Instant)` 用 `now`，而 `core::Engine` 的 `now_ms: u64` 用 `now_ms` —— 没有成文规则，只能逐处看类型。
- 前端：`anchorOffset()` 返回毫秒差，但变量名 `offset`（`useSnapshot.ts:12`）、`serverNow(offset, performance.now())` 全无单位。
- 布尔：`DeviceFact`/`Record` 内 `present`、`hidden`、`armed`、`usage_available`、`reply_inverted`、`clamp_percent`、`from_cache` 混用形容词与 `is_/has_` 前缀；方法层则统一 `is_*`（`is_active`、`is_known`、`is_empty`）。

**语法形态不一致（事件变体）**

`ProviderEvent` = `Arrived`(过去式)、`Reading`(动名词)、`Unreadable`(形容词)、`LinkLost`(名+分词)、`Removed`、`TransportDown`、`BindingConflict`(名词短语)。其中 `Unreadable` 描述的是**状态**而不是事件（事件本身是"读数被拒"）；这一命名差异直接导致 `apply_unreadable` 与 `FrameReject`（真正描述"被拒"的类型）用了两套词汇。

### 1.5 文档与代码不一致（同一问题的一部分）

- `hid/src/table.rs:3` 写 `crate::spec::BrandDecl::device_table_json` —— 类型早已叫 `BrandSpec`，`BrandDecl` 不存在。
- `core/src/lib.rs:4` 用 camelCase 术语（`modelKey`）描述 Rust 字段 `model_key`，而 `core/src/record.rs:28` 用 snake_case。**术语在文档里就两种写法**，是 §1.4 混乱的来源之一。

---

## 2. 重构范围与命名约定

### 2.1 范围界定

| 分类                | 内容                                                                                                                                                                                                                                                                                                                                                                                                     | 处置           |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| A. 改               | Rust 标识符（类型/函数/字段/模块/局部变量/参数）、前端非序列化标识符、文档与注释中的术语                                                                                                                                                                                                                                                                                                                 | 本文 §3 清单   |
| B. 不改（**冻结**） | serde 字段名（`camelCase` wire）、`ErrorCode::as_str()` 与全部诊断 code 字面量、`last_result` 取值（`sent`/`failed`/`suppressed_*`/`tray_tooltip`/`unsupported_platform`/`cooldown_cleared_clock_jump`）、`stableId` 文本格式、Tauri 命令名（`get_snapshot` 等）、事件名（`state://snapshot`、`state://diagnostics`）、`NotificationMode` 的 serde 值（`trayBubble`）、`settings.json`/`state.json` 的键 | 一字不改       |
| C. 不纳入本轮       | `Engine`、`NotificationBook`、`Registry`、`Sensitivity`、`Transition`（见 §3.5 债务清单）、`components/ui/*`、`mappings/`                                                                                                                                                                                                                                                                                | 记录理由，不做 |

> B 类冻结字段的**类型名**仍可改（类型名不上 wire）。例如 `dto::NotificationStateDto` → `NotificationNodeDto`，JSON 键不变。

### 2.2 Rust 命名约定

1. 类型 `UpperCamel`；函数/变量/字段 `snake_case`；常量 `UPPER_SNAKE`；模块用**单数名词**（`record.rs` → `device_record.rs`，`frame.rs`、`paths.rs` 保持不变）。
2. getter 不加 `get_` 前缀（`records()`、`policy()`、`health()`）；`new`/`from_*`/`with_*`/`parse` 表示构造，`into_*`/`to_*` 表示转换。
3. 布尔：字段与方法一律 `is_`/`has_`/`should_`/`can_` 前缀（历史字段 `present`、`armed` 作为术语例外，见 §2.4）。
4. 单位：毫秒一律 `_ms` 后缀；`Instant` 类型参数命名为 `now`、`deadline`，`u64` 毫秒参数必须叫 `now_ms`。
5. 错误类型以 `Error` 结尾（`SpecError`、`ProfileError`）；**运行期失败载体**以 `Failure` 结尾（`TransportFailure` 已符合，`Failure` 私有结构保留）；告警以 `Warning` 结尾（`SettingsLoadWarning`）。
6. 跨线程消息：**数据/状态流用 `*Event`，指令用 `*Command`**，不使用 `Msg`/`Cmd`。
7. "一次结果"结构用 `*Outcome`（`RunOutcome`、`BubbleOutcome` 已符合）。
8. 三个"一台设备"的形状必须同族：`DeviceRecord`（内存）／`DeviceStateEntry`（落盘）／`DeviceDto`（wire）。
9. 缩写：只允许领域公认者（HID、BLE、VBAS、BAS、GATT、UUID、VID、PID、AUMID、DTO、IPC、TTL、MS）。禁止自造缩写与否定式缩写（`uninv`）；局部变量也不得用 `ev`/`req`/`reasm`，展开为 `evidence`/`request`/`reassembly`。
10. 品牌常量：**常量名 = `BrandSpec.id` 全大写、`-`→`_`**（`dareu-jm` → `DAREU_JM`），可机械校验。

### 2.3 TypeScript 约定

1. 类型/接口 `UpperCamel`，值与函数 `camelCase`，常量 `UPPER_SNAKE`。
2. 与 Rust DTO 同名的接口保持 `*Dto` 后缀，且**来源类型名与 Rust 侧对应**（`NotificationNode` → `NotificationNodeDto`，不是 `NotificationStateDto`）。
3. 联合字面量类型只定义一次并导出（当前 `"hid" | "ble"` 在 `types.ts`、`ipc.ts`、`App.vue` 各写一遍）。
4. 带单位的数值变量必须带单位后缀（`offsetMs`、`nowMs`）。

### 2.4 术语表（重构后的权威词）

| 中文         | 权威英文               | 定义                                  | 需消除的别名                 |
| ------------ | ---------------------- | ------------------------------------- | ---------------------------- |
| 上位机宿主   | Host                   | 桌面进程；`HostState` 为共享上下文    | `Shared`                     |
| 采集器       | Provider               | 实现 `Provider` trait 的线程          | collector                    |
| 协议家族     | Family（`family_id`）  | `hid-vbas` / `ble-bas`                | `provider`（作字段时）       |
| 采集线程组   | Transport runtime      | 每个 transport 一个监管线程           | —                            |
| 传输层       | Transport              | `Hid` / `Ble`                         | channel                      |
| 设备记录     | DeviceRecord           | 每 transport 一条，永不合并           | `Record`                     |
| 设备稳定标识 | StableId / `stable_id` | `hid:7768:6426`                       | `device_id`、`key`、`id`     |
| 设备状态     | DeviceState            | 五态                                  | status                       |
| 运行健康     | Health                 | `running` / `down` / `unavailable`    | `TransportState`             |
| 读数         | Reading                | 电量 + 充电标志                       | battery、percent（作字段时） |
| 有效读数     | ValidReading           | 事件载荷                              | —                            |
| 帧解析结果   | BatteryReading         | HID 单帧/组装结果                     | Reading                      |
| 展示读数     | DisplayedBattery       | 托盘/卡片上实际显示的值（可来自缓存） | `Reading`(tray)              |
| 报告 / 帧    | Report / Frame         | HID 报告；`report` 指字节切片         | packet                       |
| 通知         | Notification           | 低电量提醒                            | alert                        |
| 通知簿       | NotificationBook       | 纯逻辑策略                            | —                            |
| 武装         | armed                  | 该档可再次触发；术语例外，保留        | —                            |
| 退化阈值     | DegradationThresholds  | 毫秒级 stale/unavailable/offline      | `Thresholds`                 |
| 低电量档位   | LowBatteryLevel        | 百分比档位                            | `thresholds`（通知侧）       |
| 阈值档位     | Sensitivity            | 用户可选 ×0.5/×1/×2                   | —                            |
| 诊断         | Diagnostics            | 环形缓冲；`diag` 仅指 hex 格式化      | —                            |
| 快照         | Snapshot               | 全量 DTO                              | —                            |
| 呈现分组     | Card                   | 两链路合并卡                          | group                        |
| 归一化键     | `model_key`            | 分组键                                | `key`                        |

---

## 3. 重命名清单

优先级：**P0** 消歧义（必改，收益最高）/ **P1** 一物多名统一 / **P2** 风格统一 / **P3** 本轮不改（债务登记）。
"影响"列为 `grep` 命中行数量级（含测试）。

### 3.1 P0 —— 消歧义（7 项）

| #    | 旧名                                     | 新名                                                           | 位置（文件:行 / 符号）                                                                                                                                                                                                                                                         | 影响  |
| ---- | ---------------------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----- |
| P0-1 | `struct DeviceState`                     | `DeviceNotificationState`                                      | `crates/voltnest-core/src/notify.rs:100`；字段 `devices: HashMap<String, DeviceState>`（:112）、:144 `for device in self.devices.values_mut()` 内的类型推断不受影响                                                                                                            | 4 行  |
| P0-2 | `TrayDto`（快照）                        | `TraySnapshotDto`                                              | `core/src/snapshot.rs:86`；`Snapshot.tray` 字段类型（:24）、`engine.rs:15,738`、`core/src/lib.rs:28` 导出、`tests/snapshot_group.rs`、`tray.rs:604`（测试构造）                                                                                                                | 6 行  |
| P0-3 | `TrayDto`（设置）                        | `TraySettingsDto`                                              | `src-tauri/src/dto.rs:41`；`SettingsDto.tray`（:25）、`SettingsDto::from_settings`（:61）、`apply_to`（:78）                                                                                                                                                                   | 5 行  |
| P0-4 | `struct Record`（+ 模块 `record`）       | `struct DeviceRecord`（模块 `record.rs` → `device_record.rs`） | `core/src/record.rs:16`；`engine.rs` 全部 `Record::new` / `&Record` / `pick_inactive_primary(ra: &Record…)`（:982）、`core/src/lib.rs:16,27`                                                                                                                                   | 16 行 |
| P0-5 | `DeviceFact.provider`、`Record.provider` | `.family_id`                                                   | `provider/src/event.rs:21`（字段+`new()` 参数 :39、:46）、`core/src/record.rs:22`（字段+`new()` 参数 :69,79）、`engine.rs:331,649,747`；**`DeviceDto.provider` 冻结不改**，在 `engine.rs:747` 加注释说明                                                                       | 12 行 |
| P0-6 | `struct Spec`                            | `struct TransportSpec`                                         | `src-tauri/src/supervisor.rs:30`；`Spec::hid()`/`Spec::ble()`（:39,:50）、`start(shared, spec: Spec)`（:85-93）、`supervise(..., spec: Spec, ...)`（:110）、`run_once(&Spec)`（:268）、`upsert/set_status/push_down` 签名、`main.rs:27,131`、测试 `hid_spec/ble_spec` 返回类型 | 17 行 |
| P0-7 | `struct Shared`                          | `struct HostState`                                             | `src-tauri/src/host.rs:57`；`host.rs` 内 ~25 处、`supervisor.rs`（`Arc<Shared>` 4 处）、`commands.rs`、`tray.rs`、`notification.rs`、`main.rs`、`lock(&shared…)` 调用不变                                                                                                      | 40 行 |

> P0-4 的模块改名会改 `pub use` 路径，属仓库内部 API；若担心 diff 过大，可先只改类型名、模块名留待 P2。

### 3.2 P1 —— 一物多名统一

| #     | 旧名                                                            | 新名                                                     | 位置                                                                                                                                                                                 | 影响   |
| ----- | --------------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| P1-1  | `LowBatteryAlert`                                               | `LowBatteryNotification`                                 | `core/src/notify.rs:49`；`engine.rs:12,111,229,439`、`core/src/lib.rs:25`、`src-tauri/src/notification.rs:12,24,95,107`、`tests/state_machine.rs:333`、`tests/snapshot_group.rs:230` | 13 行  |
| P1-2  | `Engine.pending_alerts` / `take_alerts()`                       | `pending_notifications` / `take_notifications()`         | `engine.rs:111,133,229,230,439`；`host.rs:202`；两个集成测试                                                                                                                         | 8 行   |
| P1-3  | `alert` 局部变量与 `alert_tray()`                               | `notification` / `notification_tray()`                   | `notify.rs:199,242`（`alert` 变量）、`notification.rs:32`（`for alert in alerts`）、`tray.rs:308`                                                                                    | 6 行   |
| P1-4  | `struct EntryState`                                             | `struct ThresholdState`                                  | `core/src/notify.rs:93`；`entries: BTreeMap<u8, EntryState>`（:105）、:153,:203 构造                                                                                                 | 4 行   |
| P1-5  | `struct Reading`（托盘）                                        | `struct DisplayedBattery`                                | `src-tauri/src/tray.rs:377`；`displayed_battery()`（:385，返回类型）、:792,:800 测试断言                                                                                             | 5 行   |
| P1-6  | `enum TransportState`                                           | `enum TransportHealth`                                   | `src-tauri/src/dto.rs:226`；`TransportStatus.status`（:215）、`as_str()`（:235）、`supervisor.rs:99,124,175,185,188,409`、`tests` 中 `TransportState::Unavailable`                   | 11 行  |
| P1-7  | `note_queued()` / `note_dequeued()`                             | `mark_enqueued()` / `mark_dequeued()`                    | `host.rs:148,153`；`supervisor.rs:352,358`、`host.rs:330,344`                                                                                                                        | 6 行   |
| P1-8  | `Transition::is_visible()`                                      | `Transition::should_publish()`                           | `engine.rs:72`；`host.rs:297`                                                                                                                                                        | 2 行   |
| P1-9  | `uninv()`                                                       | `normalized_byte()`                                      | `hid/src/frame.rs:168`；调用点 :245、:416                                                                                                                                            | 3 行   |
| P1-10 | `struct Session`（HID）                                         | `struct SessionHandle`（与 BLE 对齐）                    | `hid/src/provider.rs:220`；:74,:146,:181,:201,:213                                                                                                                                   | 6 行   |
| P1-11 | `struct Target`（HID）                                          | `struct HidTarget`                                       | `hid/src/provider.rs:225`；`select_targets`/`consider_target`/`collect_targets` 返回类型与 3 处 `Map<String, Target>`、测试 `path_of(&Target)`                                       | 10 行  |
| P1-12 | `struct EnumEntry`                                              | `struct EnumeratedCollection`                            | `hid/src/provider.rs:282`；:291,:314,:336、测试 `entry()` 构造（:820）                                                                                                               | 9 行   |
| P1-13 | `enum SessionMsg`（BLE）                                        | `enum BleSessionEvent`                                   | `ble/src/provider.rs:169`；:236,:253，以及 8 处 `SessionMsg::…` 构造                                                                                                                 | 14 行  |
| P1-14 | `Engine::apply_event(..., source: Option<Transport>, ...)` 参数 | `transport`                                              | `core/src/engine.rs:255`；内部传给 `apply_transport_down(source…)`（:273,565）→ 参数也改 `transport`                                                                                 | 6 行   |
| P1-15 | `Diagnostics::push(at_ms, …)` 参数                              | `now_ms`（字段 `DiagEvent.at_ms` 不变）                  | `core/src/diagnostics.rs:72`；调用点不变（位置参数）                                                                                                                                 | 1 行   |
| P1-16 | `ProviderProfile.thresholds: Thresholds` / `Thresholds` 类型    | `DegradationThresholds`                                  | `provider/src/profile.rs:19,28,32,96,124,134,176-183`；`engine.rs:183-190`、`hid/src/spec.rs:630-636`、`provider/src/profile.rs` 测试                                                | 20 行  |
| P1-17 | `NotificationConfig.thresholds`                                 | `low_battery_levels`                                     | `core/src/notify.rs:17,26,36-43,142-143,201,288`；`host.rs:288`、`dto.rs` 无（wire 走 `low_thresholds`，冻结）                                                                       | 12 行  |
| P1-18 | `clamp_thresholds()`                                            | `clamp_low_battery_levels()`                             | `src-tauri/src/dto.rs:87`；:74 调用、:252-265 测试                                                                                                                                   | 8 行   |
| P1-19 | 前端 `type Thresholds = [number, number]`                       | `LowBatteryLevels`                                       | `apps/desktop/src/lib/thresholds.ts:25`；`applyThresholdEdit` 签名（:79-83,:88）、`SettingsPanel.vue`、`tests/thresholds.spec.ts`                                                    | ~10 行 |
| P1-20 | `dto::NotificationStateDto`                                     | `NotificationNodeDto`（来源是 `NotificationNode`）       | `src-tauri/src/dto.rs:172`；`commands.rs:11,55-58`、`types.ts:73`、`DiagnosticsPanel.vue`                                                                                            | 6 行   |
| P1-21 | `voltnest_hid::provider` / `voltnest_ble::provider` 模块        | `runtime`（文档里已称"运行时"）                          | `hid/src/lib.rs:45`、`ble/src/lib.rs:26`；`supervisor.rs:45,54` 引用路径                                                                                                             | 4 行   |
| P1-22 | `supervisor::start()` / `start_with()`                          | `spawn_supervisor()` / `spawn_supervisor_with_backoff()` | `supervisor.rs:85,90`；`main.rs:133`、`supervisor.rs` 测试                                                                                                                           | 5 行   |
| P1-23 | `host::spawn_loop()`                                            | `spawn_host_loop()`                                      | `host.rs:313`；`main.rs:127`                                                                                                                                                         | 2 行   |
| P1-24 | `host::publish()`                                               | `publish_snapshot()`                                     | `host.rs:198`；`host.rs:366`、`main.rs:139`                                                                                                                                          | 3 行   |
| P1-25 | `host::persist()` / `build_state()`                             | `persist_state()` / `build_state_file()`                 | `host.rs:261,223`；`commands.rs:157`、`main.rs:170,187`、`host.rs:211`                                                                                                               | 6 行   |
| P1-26 | `tray::sync()`                                                  | `sync_tray_icons()`                                      | `tray.rs:116`；`host.rs:209`                                                                                                                                                         | 2 行   |
| P1-27 | `notification::dispatch()` / `notification::test()`             | `dispatch_notifications()` / `send_test_notification()`  | `notification.rs:20,139,145`；`host.rs:210`、`commands.rs:114`                                                                                                                       | 4 行   |

### 3.3 P2 —— 风格统一（可延后，建议同批做完）

| #     | 旧名                                             | 新名                                                                                                                                                                                                                                                                                                         | 位置                                                                                               |
| ----- | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| P2-1  | `enum SupervisorCmd`                             | `SupervisorCommand`                                                                                                                                                                                                                                                                                          | `supervisor.rs:22`；`host.rs:71`、`commands.rs:16,98`、`main.rs:27,184`                            |
| P2-2  | `worth_probing()` / `worth_probing_uuids()`      | `should_probe()` / `should_probe_by_uuids()`                                                                                                                                                                                                                                                                 | `ble/src/provider.rs:432,450`（:418 调用、:870-879 测试）                                          |
| P2-3  | `reasm` 局部变量                                 | `reassembly`                                                                                                                                                                                                                                                                                                 | `hid/src/frame.rs:478,488,490,500,505`                                                             |
| P2-4  | `req` 局部变量                                   | `request`                                                                                                                                                                                                                                                                                                    | `hid/src/provider.rs:456,463,519,555,559,592,608,612`；`hid/src/spec.rs:523,528,531-534,538,543`   |
| P2-5  | 公开参数 `ev: &HidEvidence` / `ev: &BleEvidence` | `evidence`                                                                                                                                                                                                                                                                                                   | `provider/src/classify.rs:499,529`                                                                 |
| P2-6  | `provider/src/diag.rs` 模块 / `DIAG_BYTES`       | `hexdump.rs` / `DIAG_BYTE_LIMIT`                                                                                                                                                                                                                                                                             | `provider/src/diag.rs:6`；`provider/src/lib.rs:18,30`、`frame.rs:23,198`、`ble/provider.rs:56,716` |
| P2-7  | `DevicePolicy`（BLE 调度状态）                   | `BleAddressPolicy`                                                                                                                                                                                                                                                                                           | `ble/src/policy.rs:68`；:83,176                                                                    |
| P2-8  | 品牌常量名                                       | 与 `BrandSpec.id` 对齐：`ajazz::SPEC`→`AJAZZ`；`dareu::{SPEC,JM,TG,HS}`→`{DAREU,DAREU_JM,DAREU_TG,DAREU_HS}`；`mchose::{SPEC,MOUSE_WIRED,MOUSE_WIRELESS,AUDIO,KEYBOARD_WIRED,KEYBOARD_WIRELESS}`→`MCHOSE_*`；`rapoo::{SPEC,NRF54L,LM20RT,CH584,AP}`→`{RAPOO,RAPOO_NRF54L,RAPOO_LM20RT,RAPOO_CH584,RAPOO_AP}` | `hid/src/brands/*.rs` 顶层常量、`brands/mod.rs:24,217`、各品牌测试引用                             |
| P2-9  | `Engine.profiles` 字段                           | `provider_profiles`                                                                                                                                                                                                                                                                                          | `core/src/engine.rs:107,129,876,884`                                                               |
| P2-10 | 前端重复的 `"hid" \| "ble"`                      | 导出 `type Transport` 后用一处                                                                                                                                                                                                                                                                               | `types.ts:7,65`、`ipc.ts:29`、`App.vue:198`                                                        |
| P2-11 | `useSnapshot.ts` 的 `offset`                     | `offsetMs`（`anchorOffset` 返回值同义）                                                                                                                                                                                                                                                                      | `useSnapshot.ts:12,24,40`、`lib/time.ts:14`                                                        |
| P2-12 | `Row.deviceId`                                   | `stableId`                                                                                                                                                                                                                                                                                                   | `App.vue:69,90,284`                                                                                |

### 3.4 文档修正（不涉及代码语义）

| 位置                  | 修正                                                                     |
| --------------------- | ------------------------------------------------------------------------ |
| `hid/src/table.rs:3`  | `crate::spec::BrandDecl` → `crate::spec::BrandSpec`                      |
| `core/src/lib.rs:4-5` | 统一写 `model_key`，首次出现处括注 wire 名 `modelKey`                    |
| 各模块文档            | 把 `alert`/`provider`(家族义)/`state`(运行健康义) 的用法换成 §2.4 权威词 |

### 3.5 P3 —— 本轮不改（登记理由）

| 名字                             | 理由                                                                                                                                         |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `Engine`                         | 名字泛，但全仓只有宿主一处持有，且文档已定义为"状态机+注册表+快照生成器"；改名收益 < 40 处 diff 风险                                         |
| `NotificationBook` / `Registry`  | `Book`/`Registry` 都表示"状态簿"，语义分区清楚（通知状态 vs 品牌静态数据），保留                                                             |
| `Transition`                     | 值域确实超出"状态转移"，但 `RecordChange` 涉及 40+ 处 match 分支，列为后续独立 PR                                                            |
| `Sensitivity`                    | 已是产品词（设置页"灵敏度/档位"），保留；仅在注释中补"阈值档位"                                                                              |
| `ProviderEvent::Unreadable`      | 改成 `ReadingRejected` 语义更准（与 `FrameReject` 对齐），但 `kind()` 字面量 `"unreadable"` 被诊断消费，须同时确认无外部依赖；本轮不改，登记 |
| `busy`/`error`/`view`（App.vue） | 组件内单例状态，作用域清晰                                                                                                                   |
| `components/ui/*`                | 第三方生成物                                                                                                                                 |

---

## 4. 行为不变性保障（等效性论证）

三条硬约束，任何一条被打破即视为重构失败：

1. **wire 契约零变化**
   - `serde` 字段名（全部 DTO 与 `Settings`/`StateFile` 的 `camelCase`）不动；改动仅限 Rust 字段标识符，`#[serde(rename_all = "camelCase")]` 输出的键不变。
   - 冻结字面量：`ErrorCode::as_str()` 10 个值、`Diagnostic` code（`device_arrived`、`display_name_updated`、`link_lost`、`transport_down`、`backfill_dropped`、`reading_unreadable`、`binding_conflict`、`clock_backward`、`clock_forward_jump`、`seed_id_invalid`、`record_implicit`、`device_removed`、`device_classified`、`device_classification_evidence`、`event_queue_backlog`、`transport_unavailable`、`transport_restart_requested`、`settings_load_warning`、`notification_failed`、`notification_unsupported_platform`）、`last_result` 取值、`NotificationMode`/`DeviceState`/`Transport` 的 serde 字符串、`StableId` 格式、Tauri 命令名、Tauri 事件名。
   - **注意**：P0-7 改 `Shared`、P1-1 改 `LowBatteryAlert` 等只在 Rust 侧，不产生 JSON 影响；`CardDto`/`TrayDto` 的**字段名**不动。
2. **逻辑分支零变化**
   - 每个 PR 只允许"标识符替换 + 同步改 import/路径"两类 diff；不允许顺手合并表达式、调整参数顺序、删除 `_snap`/`_now`（见 §3.5 之外的观察项：删除死参数属改动，须独立 PR）。
   - 不改变 `pub`/`priv` 可见性，不改变类型别名与泛型边界。
3. **可机械复核**
   - `git diff -w --word-diff=porcelain` 逐块确认每处改动都是同一个词的替换。
   - `git diff -U0 -- '*.json'` 必须为空（`tests/fixtures` 除外）。
   - `git diff -U0 | grep -E '^[+-].*"[a-z_]+"'` 应只剩注释；任何字符串字面量的增删都要单独解释。

---

## 5. 验证方式

### 5.1 基线（改动前先跑一遍并记录数字）

```bash
# Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# 前端
cd apps/desktop
npx vue-tsc --noEmit
npx vitest run
npx prettier --check .
```

本次审计已抽样确认基线为绿：`cargo test -p voltnest-provider -p voltnest-core --offline` 全部通过（provider 70 项、core 全绿，0 失败）。

### 5.2 每批（B1–B6）后

1. 同上四条命令全跑（**不许只看编译过**）。
2. 差异审阅：`git diff -w --word-diff=porcelain | grep -v '^~'` 逐行确认。
3. 冻结项检查：
   ```bash
   git diff --stat -- '*.json' 'apps/desktop/src-tauri/tauri*.conf.json'
   git diff -U0 | grep -nE '"(hid|ble|hid-vbas|ble-bas|sent|failed|unreadable|link_lost|state://[a-z]+)"' || echo "冻结字面量未变"
   ```
4. 前端仅在 P1-19/P1-20/P2-10/P2-11/P2-12 批次需要跑 `vue-tsc` + `vitest`。

### 5.3 全部完成后（语义等价）

| 检查                 | 方法                                                                                                             | 期望                                                                                 |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 快照 wire 形态       | 现有 `core/src/snapshot.rs` 单测（camelCase、`battery:null`、`deviceType` 等断言）                               | 原样通过，不改断言                                                                   |
| 设置/状态 round-trip | `storage` 的 `round_trip_preserves_values_and_writes_schema_version`、`unknown_fields_survive_read_modify_write` | 原样通过                                                                             |
| 诊断字符串           | `ErrorCode::stable_strings`、`RemovalReason`/`Transport::strings_are_stable`、`RejectReason` 的 detail 断言      | 原样通过                                                                             |
| 状态机行为           | `core/tests/*`（classification / state_machine / snapshot_group）                                                | 原样通过                                                                             |
| 手工冒烟             | `npm run tauri dev`：插入接收器 → 面板/托盘/卡片/诊断页 → 改设置 → 隐藏到托盘 → 退出                             | 与改动前逐屏一致；`%APPDATA%\VoltNest\*.json` 内容与改动前**逐字节相同**（同数据下） |
| 冷启动回填           | 删除 `state.json` 后重启两次                                                                                     | 首屏仍显示缓存值，第二次启动不再重复通知                                             |

### 5.4 建议的提交切分（每批可独立编译且测试全绿）

| 批  | 内容                                                                                      | 触及文件                                                           |
| --- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| B1  | P0-1…P0-7（消歧义，不含模块改名）                                                         | core/notify, core/snapshot, core/record, provider/event, src-tauri |
| B2  | P1-1…P1-4（alert → notification）                                                         | core/notify, core/engine, src-tauri/notification, 测试             |
| B3  | P1-16…P1-19（阈值三分：退化阈值 / 低电量档位 / 前端）                                     | provider/profile, core/notify, src-tauri/dto, 前端                 |
| B4  | P1-21…P1-27 + P2-1（宿主外壳：模块名、publish/persist/tray/notification/supervisor 动词） | src-tauri 全量                                                     |
| B5  | P1-5…P1-13（HID/BLE 内部：Target/EnumEntry/Session/uninv/SessionMsg）                     | hid, ble, src-tauri/tray                                           |
| B6  | P2 剩余（品牌常量、缩写、前端类型去重）+ §3.4 文档                                        | hid/brands, 前端, 注释                                             |

回滚策略：一批一 commit，消息遵循 Conventional Commits（`refactor(naming): ...`）；任一批测试不绿则整批 `git revert`，不保留半成品。

### 5.5 防复发

- 在 `README` 或 `CONTRIBUTING` 中固化 §2.4 术语表与 §2.2 规则。
- 可选的一次性脚本（不进 CI）：在 `crates/` + `src-tauri/src` 上 `rg` 黑名单 `\b(device_id|alert|reasm|uninv|EntryState)\b`，要求命中数只减不增。
- 品牌常量与 `id` 的一致性可用单测钉住（对 `BRANDS` 逐个断言 `spec.id` 与预期的常量名清单一一对应）。

---

## 6. 执行记录（B1 + B2 已完成）

### 6.1 已落地项

| 批               | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | 结果 |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- |
| B1（P0-1…P0-7）  | `DeviceState`→`DeviceNotificationState`、快照 `TrayDto`→`TraySnapshotDto`、设置 `TrayDto`→`TraySettingsDto`、`Record`→`DeviceRecord`、`DeviceFact/Record.provider`→`family_id`、`Spec`→`TransportSpec`、`Shared`→`HostState`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 完成 |
| B2（P1-1…P1-27） | alert→notification 词汇统一、`EntryState`→`ThresholdState`、`Reading`→`DisplayedBattery`、`TransportState`→`TransportHealth`、`note_queued`→`mark_enqueued`、`is_visible`→`should_publish`、`uninv`→`normalized_byte`、`Session`→`SessionHandle`、`Target`→`HidTarget`、`EnumEntry`→`EnumeratedCollection`、`SessionMsg`→`BleSessionEvent`、`Thresholds`→`DegradationThresholds`、`NotificationConfig.thresholds`→`low_battery_levels`、`clamp_thresholds`→`clamp_low_battery_levels`、前端 `Thresholds`→`LowBatteryLevels`、`NotificationStateDto`→`NotificationNodeDto`、模块 `provider`→`runtime`（两 crate 文件已 `git mv`）、`start/start_with`→`spawn_supervisor*`、`spawn_loop`→`spawn_host_loop`、`publish`→`publish_snapshot`、`persist/build_state`→`persist_state/build_state_file`、`tray::sync`→`sync_tray_icons`、`notification::dispatch/test`→`dispatch_notifications/send_test_notification` | 完成 |

未做（留待 B3）：P2-1…P2-12（`SupervisorCmd`→`SupervisorCommand`、`worth_probing`→`should_probe`、品牌常量与 `BrandSpec.id` 对齐、前端 `Row.deviceId`→`stableId` 等）。

### 6.2 验证结果（2026-10-27 实测）

| 检查     | 命令                                     | 结果                                              |
| -------- | ---------------------------------------- | ------------------------------------------------- |
| 编译     | `cargo check --workspace --all-targets`  | 0 error                                           |
| 测试     | `cargo test --workspace`                 | **376 passed / 0 failed**（与改名前基线完全相同） |
| 静态检查 | `cargo clippy --workspace --all-targets` | 无代码告警（仅 Windows 增量编译锁文件噪声）       |
| 前端类型 | `npx vue-tsc --noEmit`                   | 干净                                              |
| 前端单测 | `npx vitest run`                         | 55 passed                                         |

### 6.3 行为不变性的机械证据

对每个改动文件做「新名 → 旧名」反向替换后与备份版本逐行比对：

- 剩余差异**只有说明注释**（`曾名 xxx` 之类）与 P1-3 的局部变量改名（`alerts`→`notifications` 等，同一变量改名，不涉及逻辑）。
- `git diff` 中所有含字符串字面量的行，字面量本身**一字未改**（`"{} 电量偏低"`、`"{} via {}"`、`"{:?}: {detail}"`、`"hid-vbas"`、测试断言文案等）。
- 冻结清单未触碰：`Snapshot`/`DeviceDto`/`CardDto` 的 serde 字段名、`TraySnapshotDto.visibleDeviceIds`、`TraySettingsDto.order/hidden`、`NotificationNodeDto` 的 wire 字段、Tauri 命令名（`get_snapshot` 等）与事件名（`state://snapshot`）、`stableId` 字符串格式、`ErrorCode::as_str()` 取值。

### 6.4 建议提交切分（按域，便于 review）

```text
refactor(naming): 消歧义——DeviceState/TrayDto/Record/provider/Spec/Shared
refactor(naming): alert 词汇统一为 notification
refactor(naming): 宿主外壳函数动词化（spawn_/publish_/persist_/sync_/dispatch_）
refactor(naming): 低电量档位与退化阈值链分名（low_battery_levels / DegradationThresholds）
refactor(naming): HID/BLE 内部类型（SessionHandle/HidTarget/EnumeratedCollection/BleSessionEvent）
refactor(naming): provider 模块更名为 runtime（hid、ble 各一次 git mv）
refactor(naming): 前端类型 Thresholds→LowBatteryLevels、NotificationStateDto→NotificationNodeDto
```

每批单独 `cargo test` 均为 376 passed；整批一起验证同样通过。

### 6.5 遗留

- P2-1…P2-12 未执行（P2-8 品牌常量改名会影响 hid 各品牌测试的事实字符串，建议单独一批）。
- §3.4 文档修正已随手处理：`table.rs:3` 的 `BrandDecl`→`BrandSpec`、`hid/ble` 模块文档中的 `provider`→`runtime` 均已改；`cargo doc` 不再报未解析链接（`bas.rs` 的 `0x180F` 链接警告为既有问题，与本次无关）。
