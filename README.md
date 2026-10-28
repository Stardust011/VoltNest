# VoltNest

Windows 与 Linux 上的 HID / BLE 设备电量监控应用。单进程 Rust（Tauri 2）+ Vue 3，托盘常驻，
聚合 2.4G 接收器与 BLE Battery Service 的电量、在线与充电状态。

架构上只有一条数据流：Provider 采集原始事实（设备出现、读数、结构化错误、链路状态），
核心状态机据此推导设备状态与快照，前端只渲染 Rust 给出的结论。

## 目录

```text
crates/
  voltnest-provider/   Provider 抽象层（纯类型，无 I/O）
  voltnest-core/       状态机、归一化、分组、通知策略、Snapshot（纯逻辑，无时钟无线程）
  voltnest-hid/        hid-vbas：枚举、读取、通用帧解析、品牌描述（brands/）与型号表
  voltnest-ble/        ble-bas：发现、连接、通知、轮询、解析
  voltnest-platform/   平台适配（自启、单实例；Windows 另含 AUMID 注册）
  voltnest-storage/    settings.json / state.json 原子读写
apps/desktop/          Tauri 宿主（src-tauri）+ Vue 前端（src）
```

`hid-vbas` 家族对品牌是开放的：只要设备走 HID 私有通道、用定长报告报电量，
就只补一份「字段定义 + 型号表」，不新增 crate、不改解析器与宿主：

```text
crates/voltnest-hid/
  src/spec.rs         BrandSpec / FrameLayout / BatteryFields / RequestTemplate（品牌无关）
  src/frame.rs        通用解码器：按 BrandSpec 取字段，唯一解析入口
  src/mappings.rs     型号表目录定位 + 运行时加载（不再编译期嵌入，安装包须带上该目录）
  src/brands/ajazz.rs 已验证品牌：UsagePage 0xFFB2 / cmd 0xFA / payload[1] 为电量
  data/mappings/*.json VID:PID → 型号（运行时按文件名加载，见 BrandSpec::device_table_file）
```

依赖方向只能向下：`core → provider`、`hid/ble → provider`、`src-tauri → 全部`。
禁止 `core → storage`、`core → platform`、`provider → core`。

## 开发

需要 Rust 稳定版（见 `rust-toolchain.toml`）与 Node 22+（前端用 npm，仓库带 `package-lock.json`）。
仓库根还有一份 `package.json`：装开发工具（husky / commitlint），同时是所有脚本的统一入口。
克隆后 `npm install` 挂上提交钩子，`npm --prefix apps/desktop ci` 装前端依赖，之后各命令都在根目录执行。
两个平台共用的门禁：

```bash
cargo test --workspace          # 全部单元测试（200+）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`cargo clippy` 是**门禁**而不只是建议：`clippy.toml` 把 `std::time::SystemTime`
列为禁用类型，全仓只有 `voltnest_provider::clock::now_utc_ms()` 能读时钟。

### 提交钩子

仓库根 `package.json` 装了 husky 与 commitlint，`npm install` 时 `prepare` 会自动挂上钩子
（`core.hooksPath` 指向 `.husky/_`），无需手动安装：

- `commit-msg`：`commitlint` 校验首行是否为 `type(scope): subject`，类型与长度见 `commitlint.config.mjs`。
- `pre-commit`：`npm run format:check`，即 rustfmt / prettier（前端 + 仓库根目录）/ ruff 三套检查，口径同 CI。

被拦下时执行 `npm run format` 修好再提交。

### Windows

另需 WebView2（Windows 11 自带）。

```bash
npm install                     # 仓库根：钩子工具（husky / commitlint）
npm --prefix apps/desktop ci    # 前端依赖

npm run test                    # Vitest，只测纯函数
npm run build                   # vue-tsc + vite build
npm run format                  # 全仓格式化：Rust + 仓库根目录文本 + 前端
npm run format:check            # 同上的只检查模式，CI 用
npm run dev                     # 只起 Vite
npm run tauri dev               # 开发调试（会先起 Vite）
npm run tauri build             # 出 NSIS 安装包
```

以上都在仓库根目录执行，根 `package.json` 会把带参数的脚本（`dev` / `build` / `test` / `tauri`）
连同参数一起转给 `apps/desktop`，不必 `cd` 进去。

### Linux

构建前置（Debian / Ubuntu；Fedora 对应 `webkit2gtk4.1-devel gtk3-devel libsoup3-devel
libayatana-appindicator-gtk3-devel systemd-devel dbus-devel`）：

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libayatana-appindicator3-dev libudev-dev libdbus-1-dev
```

- `libudev-dev`：hidapi 的 hidraw 后端在**构建期**用 pkg-config 探测 libudev（并把 C 源码静态编进来），
  运行期只依赖发行版自带的 `libudev.so.1`；缺它编译 `hidapi` 时直接报 `libudev.pc` / `libudev.h` 缺失。
- `libwebkit2gtk-4.1-dev` / `libgtk-3-dev` / `libsoup-3.0-dev`：Tauri 的 wry / WebView 依赖。
- `libayatana-appindicator3-dev`：托盘（StatusNotifierItem）。
- `libdbus-1-dev`：bluez（BLE）与系统通知的 D-Bus 绑定。

命令与 Windows 完全相同，同样在仓库根目录执行（`npm run test` → `npm run build` → `npm run tauri dev|build`）。

格式化也从仓库根发起：`npm run format` 先跑 `cargo fmt --all`（借用 `--manifest-path ../../Cargo.toml`
覆盖整个 workspace，含 `src-tauri`），再跑两轮 `prettier --write`（仓库根目录文本 + 前端）；
`npm run format:check` 是同一套的只检查模式。

### 打包

| 平台    | 命令                  | 产物                                                                                           |
| ------- | --------------------- | ---------------------------------------------------------------------------------------------- |
| Windows | `npm run tauri build` | NSIS 安装包（`bundle.targets: ["nsis"]`，`currentUser` 安装模式）                              |
| Linux   | `npm run tauri build` | `deb` + `AppImage`（`tauri.linux.conf.json` 把 `bundle.targets` 覆盖为 `["deb", "appimage"]`） |

Tauri 2 会自动把同目录的 `tauri.linux.conf.json` 合并进 `tauri.conf.json`（对象递归合并、数组整体替换），
所以 Linux 侧不动基础配置、Windows 侧仍是 NSIS。deb / AppImage 的运行期依赖由 Tauri 依据链接到的库
自动生成，因此不在配置里手写 `depends`（手写会在 Ubuntu 24.04 这类 `t64` 重命名的发行版上写错）。

CI 侧：`release.yml` 在 master 上跑 release-please，它按 Conventional Commits 维护一个 release PR
（改 `CHANGELOG.md` 与 `Cargo.toml` / `tauri.conf.json` / `package.json` 里的版本号）；合并该 PR 即出
`vX.Y.Z` tag 与 Draft Release，同一次 run 接着双平台打包并把安装包挂上去（v1 不签名，需手动发布）。
发布说明用中文小标题（新功能 / 修复 / …）由 `release-please-config.json` 的 `changelog-sections` 定义；
`.release-please-manifest.json` 初始记 `0.0.0`（仓库还没有任何 tag），首个 release PR 会出 `v0.1.0`；
`Cargo.lock` 里 workspace 成员的版本不在 PR 内，下一次 cargo 构建会自动跟上。
`cargo-deny` 与工具链漂移扫描每周一跑（`.github/workflows/security.yml`）；
型号表每月 1 日抓取厂商网页驱动并开 PR
（`.github/workflows/mappings.yml`，脚本在 `mappings/scripts/`，只在 `devices` 内容有差异时才开 PR；
落盘前用 prettier 统一表格式，本地手工抓取后同样要跑 `npm run format:repo`）。

## Linux 运行期前提

- **HID（2.4G 接收器）**：读 `/dev/hidraw*` 需要当前用户有权限（发行版通常用 `plugdev` 组或 udev 规则放行）。
  本程序**不安装任何 udev 规则**；缺权限时该设备报 `access_denied` 诊断，不会静默丢读数。
- **BLE（`ble-bas`）**：需要 **`bluetoothd` 在运行**且系统 dbus 可用，当前用户需在 `bluetooth` 组
  或持有 polkit 授权。无 bluetoothd / 无适配器（容器、`rfkill` 关闭、WSL2 未直通）时，provider 上报一次
  结构化 `TransportDown`（detail 里直接写该查什么），诊断页把该 transport 标成「不可用」，
  HID 那条链路不受影响；连续 5 次失败后停止自动重启，可在诊断页手动重启。
- **托盘**：需要桌面环境提供 StatusNotifierItem 宿主（KDE 原生，GNOME 需 AppIndicator 扩展）。
- **系统通知**：走 `org.freedesktop.Notifications`，需要通知守护进程在跑；发不出去时诊断页记 `failed`。

## 运行时行为要点

- **Rust 是唯一状态来源**：前端先订阅 `state://snapshot`，再 `get_snapshot()`，
  随后丢弃 `rev <= localRev` 的事件；命名、分组、排序全部由 Rust 决定。
- **快照合并窗口 250ms**（≤4Hz），1s tick 驱动状态退化，只有真变化才发布。
- **关闭窗口只隐藏到托盘**，托盘菜单「退出」才结束进程；二次启动唤起已有窗口。
- **托盘图标跟着物理设备走**：每台可见设备一个电量环，右键菜单都是同一份
  「打开面板 / 退出 VoltNest」。颜色与面板 `batteryColor()` 一一对应（充电两侧同为
  绿色 `#10b981`：弧长照旧表达电量，闪电负责标注「在充电」）；刚启动
  还没有读数时是和面板一样的蓝环（有缓存读数就按缓存值画弧，悬停文字也报这个值并
  注明「上次读数」），一圈读数都没有时画一整圈压暗一档的蓝环。**一张设备图标都没有时
  占位的是应用图标本身**（和任务栏、开始菜单同一个 PNG）；有设备图标时它隐身让位，
  不会多出一个像第二台设备的环。
- **数据目录**：Windows 为 `%APPDATA%\VoltNest\`（`settings.json` / `state.json` / `logs/`），
  安装目录只读，运行时不写入；Linux 为 `$XDG_DATA_HOME/VoltNest/`（缺省 `~/.local/share/VoltNest/`）。
- **采集异常隔离**：每个 transport 独立监管线程，`catch_unwind` 兜住 panic 后按
  5s / 15s / 60s 退避重启，连续 5 次失败转「不可用」，由诊断页手动重启。

## v1 指标

| 指标          | 目标         | 现状                                  |
| ------------- | ------------ | ------------------------------------- |
| HID 拔插发现  | ≤6s          | 枚举周期 5s，待真机复测               |
| BLE 掉线发现  | ≤2s          | 依赖 I/O 错误即时判定，待真机复测     |
| BLE 首次出现  | ≤60s         | 由系统性连接状态巡检覆盖              |
| 前端 bundle   | 计入程序目录 | JS ≈91KB / CSS ≈24KB（gzip 后 ≈40KB） |
| 安装包 / 内存 | 统计项       | 待出包后记录                          |

## 已知限制（v1 有意接受）

1. 两只同型号 HID 接收器会被视为同一台设备（`stableId` 为 `hid:vid:pid`，会记冲突告警但只显示一台）。
2. 同一只鼠标切换 2.4G / 蓝牙时是两条独立记录，只在型号与在线状态满足条件时于呈现层合并为一张卡片。
3. BLE 若使用可解析私有地址（RPA）的设备，重配对后设置会重置；本机设备为静态随机地址。
4. 仅跟踪 Windows 当前**已连接**的 BLE 设备；仅配对未连接的设备不主动拉起。
5. v1 不做代码签名，Windows SmartScreen 可能显示「未知发布者」。v1 不做自动更新。
6. BLE 充电状态协议层不可得，界面显示 `Unknown`，不会显示「未充电」。
7. **托盘气泡模式**以「对应托盘的 tooltip 提示 6 秒」实现：Tauri 2 未暴露
   `Shell_NotifyIcon` 的气泡接口，v1 不为它引入自绘托盘窗口。提示挂在设备自己的
   图标上（没有则挂基础图标），6 秒后还原原 tooltip。选项仍是
   「系统通知 / 托盘气泡 / 关闭」三档，不自动降级。**这条实现是 Windows 语义**，
   Linux 的托盘后端没有 tooltip，见第 10 条。
8. 分支覆盖率（core ≥90%）暂由评审保证：`cargo llvm-cov` 的分支统计仍需 nightly。
9. **系统通知的「应用名 + 图标」由 AUMID 决定**：启动时写
   `HKCU\Software\Classes\AppUserModelId\VoltNest.Desktop`（`DisplayName` + `IconUri`，
   图标落在 `%APPDATA%\VoltNest\toast-icon.png`）。这两项缺一，Windows 会回落成
   `powershell.exe` 的蓝色图标和「Windows PowerShell」标题。Toast 由本程序直接走
   WinRT 发送（不经过 `tauri-plugin-notification`），开发运行同样显示 VoltNest。AUMID / Toast 是
   Windows 概念：Linux 上没有这一步，系统通知由 `tauri-plugin-notification`（libnotify →
   `org.freedesktop.Notifications`）发送，应用名与图标由打包生成的 `.desktop` 文件决定。
10. **Linux 托盘不显示 tooltip，也不派发点击事件**：托盘走 libayatana-appindicator（StatusNotifierItem），
    Tauri 在该后端把 `set_tooltip` 实现成空操作（`tray-icon` 直接返回 `Ok`）、`TrayIconEvent` 根本不发。
    这个空实现同时废掉**设备 tooltip**（`型号 · 92%`）和**基础图标的汇总 tooltip**
    （`VoltNest · N 台设备`），读数以面板为准；左键点图标也开不了面板，打开面板走每张图标菜单里的
    「打开面板」。因此「托盘气泡」档在 Linux 上不可用，但**诊断页能分辨原因**：这条路径记
    `unsupported_platform`（与「托盘里真没有落脚图标」的 `failed` 分开，不是「等价为 tooltip」——
    那个后端没有 tooltip），日志另提示改用系统通知。
11. **Linux 侧只做过启动验收**（WSLg + 可写数据目录）：进程能起来、托盘初始化成功、HID 打「枚举无
    HID 设备」、BLE 按退避重启并保留 provider 的结构化原因（`需 bluetoothd…`）。仍**没有**在带
    蓝牙适配器的真机上跑过真实采集，也没有在真桌面会话里确认托盘的视觉呈现与通知投递。
