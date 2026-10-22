# VoltNest

Windows 上的 HID / BLE 设备电量监控应用。单进程 Rust（Tauri 2）+ Vue 3，托盘常驻，
聚合 2.4G 接收器与 BLE Battery Service 的电量、在线与充电状态。

- 实现基线：[`docs/VoltNest-v1-开发规格.md`](docs/VoltNest-v1-开发规格.md)
- 架构决策：[ADR-0008](docs/adr/0008-plugin-partitioning.md) ·
  [ADR-0009](docs/adr/0009-modular-single-process.md) ·
  [ADR-0010](docs/adr/0010-provider-contract.md) ·
  [ADR-0011](docs/adr/0011-hid-vbas-brand-model.md)
- 新增品牌接入：[`docs/hid-vbas-brands.md`](docs/hid-vbas-brands.md)
- 规则到测试的映射：[`docs/test-matrix.md`](docs/test-matrix.md)

## 目录

```text
crates/
  voltnest-provider/   Provider 契约（纯类型，无 I/O）
  voltnest-core/       状态机、归一化、分组、通知策略、Snapshot（纯逻辑，无时钟无线程）
  voltnest-hid/        hid-vbas：枚举、读取、通用帧解析、品牌描述（brands/）与型号表
  voltnest-ble/        ble-bas：发现、连接、通知、轮询、解析
  voltnest-platform/   Windows 适配（自启、单实例、AUMID）
  voltnest-storage/    settings.json / state.json 原子读写
apps/desktop/          Tauri 宿主（src-tauri）+ Vue 前端（src）
tools/                 取证探针（hid-probe-rs / ble-probe-rs），不被产品 crate 依赖
```

`hid-vbas` 家族对品牌是开放的：只要设备走 HID 私有通道、用定长报告报电量，
就只补一份「字段定义 + 型号表」，不新增 crate、不改解析器与宿主：

```text
crates/voltnest-hid/
  src/spec.rs         BrandSpec / FrameLayout / BatteryFields / RequestTemplate（品牌无关）
  src/frame.rs        通用解码器：按 BrandSpec 取字段，唯一解析入口
  src/brands/ajazz.rs 已验证品牌：UsagePage 0xFFB2 / cmd 0xFA / payload[1] 为电量
  data/vbas/ajazz.json VID:PID → 型号（编译期嵌入）
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
- **托盘图标跟着物理设备走**：每台可见设备一个电量环，右键菜单都是同一份
  「打开面板 / 退出 VoltNest」。颜色与面板 `batteryColor()` 一一对应（充电除外：
  托盘画绿色弧 + 环心闪电，弧长照旧表达电量；面板仍是青色 + 数字旁的闪电）；刚启动
  还没有读数时是和面板一样的蓝环（有缓存读数就按缓存值画弧，悬停文字也报这个值并
  注明「上次读数」），一圈读数都没有时画一整圈压暗一档的蓝环。**一张设备图标都没有时
  占位的是应用图标本身**（和任务栏、开始菜单同一个 PNG）；有设备图标时它隐身让位，
  不会多出一个像第二台设备的环。
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
7. **托盘气泡模式**以「对应托盘的 tooltip 提示 6 秒」实现：Tauri 2 未暴露
   `Shell_NotifyIcon` 的气泡接口，v1 不为它引入自绘托盘窗口。提示挂在设备自己的
   图标上（没有则挂基础图标），6 秒后还原原 tooltip。选项仍是
   「系统通知 / 托盘气泡 / 关闭」三档，不自动降级。
8. 分支覆盖率（core ≥90%）暂由评审保证：`cargo llvm-cov` 的分支统计仍需 nightly。
9. **系统通知的「应用名 + 图标」由 AUMID 决定**：启动时写
   `HKCU\Software\Classes\AppUserModelId\VoltNest.Desktop`（`DisplayName` + `IconUri`，
   图标落在 `%APPDATA%\VoltNest\toast-icon.png`）。这两项缺一，Windows 会回落成
   `powershell.exe` 的蓝色图标和「Windows PowerShell」标题。Toast 由本程序直接走
   WinRT 发送（不经过 `tauri-plugin-notification`），开发运行同样显示 VoltNest。
