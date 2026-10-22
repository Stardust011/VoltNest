# VoltNest

Windows 上的 HID / BLE 设备电量监控应用。单进程 Rust（Tauri 2）+ Vue 3，托盘常驻，
聚合 2.4G 接收器与 BLE Battery Service 的电量、在线与充电状态。

- 实现基线：[`docs/VoltNest-v1-开发规格.md`](docs/VoltNest-v1-开发规格.md)
- 架构决策：[ADR-0008](docs/adr/0008-plugin-partitioning.md) ·
  [ADR-0009](docs/adr/0009-modular-single-process.md) ·
  [ADR-0010](docs/adr/0010-provider-contract.md)
- 规则到测试的映射：[`docs/test-matrix.md`](docs/test-matrix.md)

## 目录

```text
crates/
  voltnest-provider/   Provider 契约（纯类型，无 I/O）
  voltnest-core/       状态机、归一化、分组、通知策略、Snapshot（纯逻辑，无时钟无线程）
  voltnest-hid/        hid-nearlink-ajazz：枚举、读取、帧解析、型号表
  voltnest-ble/        ble-bas：发现、连接、通知、轮询、解析
  voltnest-platform/   Windows 适配（自启、单实例、AUMID）
  voltnest-storage/    settings.json / state.json 原子读写
apps/desktop/          Tauri 宿主 + Vue 前端
tools/                 取证探针（hid-probe-rs / ble-probe-rs），不被产品 crate 依赖
```

依赖方向只能向下：`core → provider`、`hid/ble → provider`、`src-tauri → 全部`。
禁止 `core → storage`、`core → platform`、`provider → core`。

## 开发

```bash
cargo test --workspace          # 全部单元测试
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cd apps/desktop && npm install && npm run build
cd apps/desktop && npm run tauri build
```

前端只做渲染：先订阅 `state://snapshot`，再 `get_snapshot()`，随后丢弃
`rev <= snapshot.rev` 的事件（W-7）。业务状态、命名、分组、排序全部由 Rust 决定。

## 已知限制（v1 有意接受）

1. 两只同型号 HID 接收器会被视为同一台设备（`stableId` 为 `hid:vid:pid`，Y-2/Y-3 会告警但只显示一台）。
2. 同一只鼠标切换 2.4G / 蓝牙时是两条独立 L1 记录，只在满足条件时于呈现层（L2）分组。
3. BLE 若使用可解析私有地址（RPA）的设备，重配对后设置会重置；本机设备为静态随机地址。
4. 仅跟踪 Windows 当前**已连接**的 BLE 设备；仅配对未连接的设备不主动拉起（O-1）。
5. v1 不做代码签名，Windows SmartScreen 可能显示「未知发布者」。v1 不做自动更新。
6. BLE 充电状态协议层不可得，界面显示 `Unknown`，不会显示「未充电」。
