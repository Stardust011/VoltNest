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
apps/desktop/          Tauri 宿主（src-tauri）+ Vue 前端（src）
tools/                 取证探针（hid-probe-rs / ble-probe-rs），不被产品 crate 依赖
```

依赖方向只能向下：`core → provider`、`hid/ble → provider`、`src-tauri → 全部`。
禁止 `core → storage`、`core → platform`、`provider → core`。

## 开发

需要 Rust 稳定版（见 `rust-toolchain.toml`）、Node 22、WebView2（Windows 11 自带）。

```bash
cargo test --workspace          # 全部单元测试（200+）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check

cd apps/desktop
npm install
npm run test                    # Vitest，只测纯函数
npm run build                   # vue-tsc + vite build
npm run tauri dev               # 开发调试（会先起 Vite）
npm run tauri build             # 出 NSIS 安装包
```

`cargo clippy` 是**门禁**而不只是建议：`clippy.toml` 把 `std::time::SystemTime`
列为禁用类型（Z-6），全仓只有 `voltnest_provider::clock::now_utc_ms()` 能读时钟。

测试矩阵与代码的一致性由脚本校验（CI 同款）：

```powershell
pwsh scripts/check-test-matrix.ps1
```

## 运行时行为要点

- **Rust 是唯一状态来源**：前端先订阅 `state://snapshot`，再 `get_snapshot()`，
  随后丢弃 `rev <= localRev` 的事件；命名、分组、排序全部由 Rust 决定。
- **快照合并窗口 250ms**（≤4Hz），1s tick 驱动状态退化，只有真变化才发布。
- **关闭窗口只隐藏到托盘**，托盘菜单「退出」才结束进程；二次启动唤起已有窗口。
- **数据目录**：`%APPDATA%\VoltNest\`（`settings.json` / `state.json` / `logs/`），
  安装目录只读，运行时不写入。
- **采集异常隔离**：每个 transport 独立监管线程，`catch_unwind` 兜住 panic 后按
  5s / 15s / 60s 退避重启，连续 5 次失败转「不可用」，由诊断页手动重启。

## v1 指标（§14.4）

| 指标 | 目标 | 现状 |
|---|---|---|
| HID 拔插发现 | ≤6s | 枚举周期 5s，待真机复测 |
| BLE 掉线发现 | ≤2s | 依赖 I/O 错误即时判定，待真机复测 |
| BLE 首次出现 | ≤60s | 由系统性连接状态巡检覆盖 |
| 前端 bundle | 计入程序目录 | JS ≈91KB / CSS ≈24KB（gzip 后 ≈40KB） |
| 安装包 / 内存 | 统计项 | 待出包后记录 |

## 已知限制（v1 有意接受）

1. 两只同型号 HID 接收器会被视为同一台设备（`stableId` 为 `hid:vid:pid`，Y-2/Y-3 会告警但只显示一台）。
2. 同一只鼠标切换 2.4G / 蓝牙时是两条独立 L1 记录，只在满足条件时于呈现层（L2）分组。
3. BLE 若使用可解析私有地址（RPA）的设备，重配对后设置会重置；本机设备为静态随机地址。
4. 仅跟踪 Windows 当前**已连接**的 BLE 设备；仅配对未连接的设备不主动拉起（O-1）。
5. v1 不做代码签名，Windows SmartScreen 可能显示「未知发布者」。v1 不做自动更新。
6. BLE 充电状态协议层不可得，界面显示 `Unknown`，不会显示「未充电」。
7. **托盘气泡模式**以「托盘 tooltip 提示 6 秒」实现：Tauri 2 未暴露
   `Shell_NotifyIcon` 的气泡接口，v1 不为它引入自绘托盘窗口。选项仍是
   「系统通知 / 托盘气泡 / 关闭」三档，不自动降级。
8. 分支覆盖率（core ≥90%）暂由评审保证：`cargo llvm-cov` 的分支统计仍需 nightly。
