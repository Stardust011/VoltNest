# Changelog

## [0.1.1](https://github.com/Stardust011/VoltNest/compare/v0.1.0...v0.1.1) (2026-10-28)


### 修复

* **release:** 更新文件路径以完成构建输出 ([24a7fc3](https://github.com/Stardust011/VoltNest/commit/24a7fc30345cf13dc28e3c3980f765ec4060d88e))


### 持续集成

* **workflows:** skip release PRs and remove release check ([1926e04](https://github.com/Stardust011/VoltNest/commit/1926e041634ed1f85d335176a24ff4976fb8ca69))

## 0.1.0 (2026-10-28)


### ⚠ BREAKING CHANGES

* **hid:** 型号表改用厂商映射表并统一到 data/mappings
* **hid:** `Snapshot.provider`、日志前缀与诊断分组键的家族 ID 由 `hid-nearlink-ajazz` 改为 `hid-vbas`，不保留旧值兼容读取（v1 未发版）。
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌

### 新功能

* **app:** Tauri 宿主、宿主事件循环与采集线程监管 ([7677961](https://github.com/Stardust011/VoltNest/commit/76779617365afd7f015aa9cd1bf937e1b378f63f))
* **autostart:** 自启与手动启动统一受「启动时最小化到托盘」管控 ([30cf057](https://github.com/Stardust011/VoltNest/commit/30cf057db3982fa743fbfcfd2d8cc94e30eb8a92))
* **ble:** 出现检测独立于读数轮询，连上蓝牙即被发现 ([74f8e2f](https://github.com/Stardust011/VoltNest/commit/74f8e2fd68a17d8e4f57998cc96de83a46eb4a43))
* **ble:** 实现 ble-bas 解析与协调/会话模型 ([52d5247](https://github.com/Stardust011/VoltNest/commit/52d52477c7100ebda916521b04ab96583d2bc3a4))
* **ble:** 支持 Linux bluez 后端 ([7848c0a](https://github.com/Stardust011/VoltNest/commit/7848c0ae6b15802dd24aaf771c73c3f811042264))
* **ble:** 断链改为事件驱动，巡检降为保底 ([eb1670c](https://github.com/Stardust011/VoltNest/commit/eb1670c9bf9cb1bb5a3801fe74fa64042bfff5fd))
* **brand:** 应用图标改为「V + 闪电」标识 ([e7719ff](https://github.com/Stardust011/VoltNest/commit/e7719ff92c00939448673c6c260433b600d88837))
* **classify:** 增加 HID 与 BLE 设备类型识别，证据不足返回 Unknown ([a5a305e](https://github.com/Stardust011/VoltNest/commit/a5a305ed6f4aafa5dcaeef7bb30cf9dbb0134a4f))
* **classify:** 补齐 Generic Desktop 页 HID 设备类 Usage ([690f9ba](https://github.com/Stardust011/VoltNest/commit/690f9bab3403f40d44baf37882e10b8da5866256))
* **core:** 回填通知投递的真实结果 ([4f993c7](https://github.com/Stardust011/VoltNest/commit/4f993c7d4a6a36ca671f09fff9add041c9755858))
* **core:** 状态机、归一化、L2 分组与快照 ([50a0bb4](https://github.com/Stardust011/VoltNest/commit/50a0bb4dec8a0720d0152038a5f2d397639fe047))
* **desktop-ui:** 启动回填时用缓存电量填充主读数 ([1dfaff7](https://github.com/Stardust011/VoltNest/commit/1dfaff73bca865900b46b8a842ed75842bdcd4ee))
* **desktop:** Linux 平台适配与采集失败原因透传 ([ee4b40f](https://github.com/Stardust011/VoltNest/commit/ee4b40f3bb5e503627ed3061bf1f2ef2616bc7c3))
* **diagnostics:** 坏帧记录成因与原始字节，按种类去重 ([ef9a6c1](https://github.com/Stardust011/VoltNest/commit/ef9a6c16835a2c10577fea75ef6cfd8e0faa2e43))
* **diagnostics:** 底部加红框调试区，一键重现首次引导 ([1d9645d](https://github.com/Stardust011/VoltNest/commit/1d9645d7def8d1fd1b24112d65176a1eea74733e))
* **diagnostics:** 未支持组合降为 debug 级，结论行只留参与判定的证据 ([e856ce1](https://github.com/Stardust011/VoltNest/commit/e856ce1ba4c87cda2db6a3e8d59cee2a8d759407))
* **hid:** add multi-brand battery support ([a881a5e](https://github.com/Stardust011/VoltNest/commit/a881a5e81bb5fb0d7ad0e3d1d6f76a5307c6b70e))
* **hid:** 型号表改为运行时加载 ([afd7f75](https://github.com/Stardust011/VoltNest/commit/afd7f750138139772e5eff02bf527521452914c8))
* **hid:** 实现 hid-nearlink-ajazz 解析器与读取线程组 ([14c0142](https://github.com/Stardust011/VoltNest/commit/14c0142a5784849e8b8e806efe511961511d473a))
* **hid:** 支持 Linux hidraw 后端 ([ec25c4b](https://github.com/Stardust011/VoltNest/commit/ec25c4b1f875f3bc23aff5f371139986d0e1e45f))
* **mappings:** restrict rapoo device map to vendor id 24AE ([f7ba98e](https://github.com/Stardust011/VoltNest/commit/f7ba98ebe818130ef3a37c790eeebf084aea8476))
* **mchose:** 共享接收器 PID 输出歧义候选与官方消歧规则 ([458a205](https://github.com/Stardust011/VoltNest/commit/458a2058a4e6f76ebca4fe112df0895425663f5e))
* **mchose:** 新增设备映射表抓取脚本与转换结果 ([737a397](https://github.com/Stardust011/VoltNest/commit/737a397c083d03ca30110a1547b3f6cfe0f051ea))
* **onboarding:** 引导页两步结构重做，补齐进度、示意面板与无障碍关联 ([c0b0587](https://github.com/Stardust011/VoltNest/commit/c0b058769b9ac0fdafcb1777c69acc15571b3a12))
* **panel:** 充电读数改绿色，与托盘环同色 ([08e8861](https://github.com/Stardust011/VoltNest/commit/08e88615ce9230ca5afa7efe9dc489060581e1ac))
* **platform:** Linux 上实现自启、单实例与 AUMID 适配 ([646c4d3](https://github.com/Stardust011/VoltNest/commit/646c4d388779905d6e7b6370461207b8b9329460))
* **platform:** 自启动注册、单实例与 AUMID 声明 ([1742aa8](https://github.com/Stardust011/VoltNest/commit/1742aa81753da43d7c66d83459adfe5836a58799))
* **provider:** 落地 Provider 契约与 PC-1..PC-7 不变量 ([d9655af](https://github.com/Stardust011/VoltNest/commit/d9655af3ba317c1ef11f9e2b872476964a10cc74))
* **provider:** 采集节奏与出现检测间隔移入 data/profiles.json ([c4c1354](https://github.com/Stardust011/VoltNest/commit/c4c1354626a72f54e71a2bcfe3445929537610e3))
* **rapoo-mapping:** add web manifest scraper and generated device map ([b16c051](https://github.com/Stardust011/VoltNest/commit/b16c051896289f8ef06de40be5fe2e99f865e645))
* **settings:** 低电量档位拆成两行列表，输入实时标红并分离临时输入 ([a99654c](https://github.com/Stardust011/VoltNest/commit/a99654cb7434671a8b5587c69ebd39f538f1d7a1))
* **settings:** 阈值输入改用数字框并放开到 0～95，失焦不再自动调整 ([0f1bdfe](https://github.com/Stardust011/VoltNest/commit/0f1bdfef2d4d72e0d40c64b780380a77bf27447e))
* **storage:** settings.json / state.json 原子读写与损坏恢复 ([8f27981](https://github.com/Stardust011/VoltNest/commit/8f27981a8a389fe27266eaeec69fefc531551161))
* **storage:** 数据目录改按 XDG Base Directory 解析 ([ba66fd3](https://github.com/Stardust011/VoltNest/commit/ba66fd300996db146234fa3fc20763f25233fc3e))
* **storage:** 设置加载告警提供可读文案 ([9eea31a](https://github.com/Stardust011/VoltNest/commit/9eea31a44a034a0eeffb702ba3dc7f560df4cebb))
* **titlebar:** 标题栏改为最小化到托盘与退出程序 ([7289b5a](https://github.com/Stardust011/VoltNest/commit/7289b5ae082fa6b9dc7be49396023b477d2c286e))
* **tray:** 基础图标聚合「还有 N 台」 ([f425701](https://github.com/Stardust011/VoltNest/commit/f425701d2e5b8cd35243a2557b75d2c403ede8f3))
* **tray:** 没有设备时托盘用应用图标占位 ([16f1c14](https://github.com/Stardust011/VoltNest/commit/16f1c14e1ed60328c81dfed2c910043d644c7b16))
* **ui:** Vue 面板壳、快照订阅与纯函数测试 ([bdf48ed](https://github.com/Stardust011/VoltNest/commit/bdf48edbe5863530036a5ad716daa134616b7672))
* **ui:** 设备卡最左侧显示类型图标，去掉类型文字标签 ([ef3c724](https://github.com/Stardust011/VoltNest/commit/ef3c724cb26ca698767f10fafb1a7199ba11d03c))


### 修复

* **ble:** 已连接设备改用 retrieve_peripherals 枚举，修复取不到电量 ([2bf6f81](https://github.com/Stardust011/VoltNest/commit/2bf6f81d804a994f3bc5f26b34c255d1c9a038c8))
* **ci:** 修正缓存作用域与冷缓存写入 ([2e45b39](https://github.com/Stardust011/VoltNest/commit/2e45b39338ff275e52d22982ba182e5c1138823f))
* **ci:** 拆分 Windows clippy 与测试的缓存键 ([5f4a92b](https://github.com/Stardust011/VoltNest/commit/5f4a92b4c660f49086faaa22a4cd04bd8c12f3af))
* **desktop-tray:** 冷启动无读数时托盘画蓝环，与面板「等待读数」同色 ([71bf05f](https://github.com/Stardust011/VoltNest/commit/71bf05fd3c5ed79b32b4ffcb8589b10534a5a4bf))
* **desktop-tray:** 托盘每张图标右键可用，有设备时隐藏基础灰环 ([b8167d0](https://github.com/Stardust011/VoltNest/commit/b8167d0e55455481b404865ce0aaa57596e504d4))
* **desktop-tray:** 等待读数的蓝环也把电量写进 tooltip ([4b8d43c](https://github.com/Stardust011/VoltNest/commit/4b8d43cec24d019fc3130bb2484671ade271ebeb))
* **diagnostics:** 线程组状态变化主动推送，重启次数即时刷新 ([b2440a8](https://github.com/Stardust011/VoltNest/commit/b2440a8296fa69bea8c45275d5b9bee2cd44d37b))
* **diagnostics:** 通知触发节点改按阈值从高到低排列 ([7609222](https://github.com/Stardust011/VoltNest/commit/76092221fda9fc7cd9537c5ac0ba1e40f5aa5a77))
* **grouping:** 合并设备在启动时只呈现一台 ([b02b2cd](https://github.com/Stardust011/VoltNest/commit/b02b2cd5ae866ae703d5838babfa8d9a06c99ff2))
* **hid:** 补齐 Linux 安装形态的型号表目录探测 ([2019788](https://github.com/Stardust011/VoltNest/commit/20197886e35e0da7e46bb722acde7dc121ce6da6))
* **notify:** Toast 显式使用 VoltNest 的 AUMID 与图标 ([e60a2e0](https://github.com/Stardust011/VoltNest/commit/e60a2e0d2358f85b0f12f9f8039bf60a60935864))
* **notify:** 两档反转给出方向提示，改阈值后清理旧触发节点 ([b2dc873](https://github.com/Stardust011/VoltNest/commit/b2dc8730469b2f1ce7e4f146341a95dc3f6c0984))
* **notify:** 改阈值后立即重建两档节点，并推送诊断刷新 ([ae61de9](https://github.com/Stardust011/VoltNest/commit/ae61de95b450eaa31e395a02b5063e0f03467360))
* **panel:** 充电闪电与电量数字改垂直居中，行高钉死 1em ([360db26](https://github.com/Stardust011/VoltNest/commit/360db269b0a162b6ceadacf0029e282ee27a0184))
* **tray,panel:** 充电绿环保留电量弧 + 闪电与数字按基线对齐 ([7962c65](https://github.com/Stardust011/VoltNest/commit/7962c65ea5a080a2b07509626c72b4e589b26a1d))


### 重构

* **brand:** 界面徽标改用打包位图，便于整体替换 ([366456d](https://github.com/Stardust011/VoltNest/commit/366456ddb01d5b22553901a72a58401c3ca6c0fd))
* **desktop-ui:** 精简设置页文案并改用下拉框选择过期速度 ([81ad420](https://github.com/Stardust011/VoltNest/commit/81ad4205c8336e96129e2efd300f1a19882993ba))
* **desktop-ui:** 精简诊断页字段与文案 ([842dac8](https://github.com/Stardust011/VoltNest/commit/842dac86481f4454ff4c23df0807d8649878f3d4))
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌 ([d973501](https://github.com/Stardust011/VoltNest/commit/d9735011a7a92aba0b4d1ebd4c934b9ea7ee8c02))
* **diagnostics:** 结论行只留类型与方法，全部证据降为 debug ([28ba5ca](https://github.com/Stardust011/VoltNest/commit/28ba5ca34b3c61afa85c67b3b2557e69dd861601))
* **hid:** 型号表 source 仅作展示，不再按结构解析 ([b08c958](https://github.com/Stardust011/VoltNest/commit/b08c958f4c5c128f285beabdba560f99950164eb))
* **hid:** 型号表改用厂商映射表并统一到 data/mappings ([47adfdf](https://github.com/Stardust011/VoltNest/commit/47adfdf68b1739ebd06dcaa7ab032ce84f772838))
* **hid:** 家族统一为 hid-vbas 并抽象品牌可扩展协议描述 ([2557320](https://github.com/Stardust011/VoltNest/commit/255732024322ab7187e0091053fc6cdd291ce045))
* **hid:** 读取线程改为按 ReplyTransport 一一对应的会话方法 ([9110fe1](https://github.com/Stardust011/VoltNest/commit/9110fe159e862f23195f8acb5d07ca93593ade45))
* **mappings:** config_version 改为抓取时刻 ([c621a0c](https://github.com/Stardust011/VoltNest/commit/c621a0cfbd90cf0cf3338a6f13aa5150feedcc3e))
* **mappings:** make rapoo vendor filter a list ([e35bec8](https://github.com/Stardust011/VoltNest/commit/e35bec8bd79afa89331a6f40be7c2ed85e3f300a))
* **naming:** 统一上位机侧命名并收敛一词多义 ([0beb456](https://github.com/Stardust011/VoltNest/commit/0beb45658400ccc2e5afc5f5d77e0093e371a482))
* **provider:** 三档阈值改为逐档秒值，去掉倍率换算 ([61adb77](https://github.com/Stardust011/VoltNest/commit/61adb77dd5f0371b37c902e7ae8fb2746a951889))
* **provider:** 阈值链基线与三档倍率移入 data/profiles.json ([e59371d](https://github.com/Stardust011/VoltNest/commit/e59371d01c8e36f5f1e79139b60d5d05defd83da))
* **rapoo-mapping:** uppercase hex keys, drop base devices and redundant fields ([d793b82](https://github.com/Stardust011/VoltNest/commit/d793b82561a2ccd6feb50f5037a85228d518092e))
* **rapoo-mapping:** use uppercase USB mode value ([7cef811](https://github.com/Stardust011/VoltNest/commit/7cef811b0927888c557178244f72391c0ef37f1e))
* **settings:** 「数据过期速度」改为「刷新速率」 ([c88807d](https://github.com/Stardust011/VoltNest/commit/c88807dbf0605b0d16bfa2c06c55ace5d2e95f7c))
* **ui:** 显示调试改为开关并挪进调试区 ([dbb8bba](https://github.com/Stardust011/VoltNest/commit/dbb8bba6efc2befc13140a83fd200f258c8aa8a7))
* 精简全仓注释与面向用户文案 ([2e59158](https://github.com/Stardust011/VoltNest/commit/2e5915893a11be8503d2a3a61d3a0f5116a1acf5))


### 文档

* **naming:** 新增上位机命名审计与重命名方案 ([54ae758](https://github.com/Stardust011/VoltNest/commit/54ae758b7e7b0eb1db2f387b9ed5990d33fc3f30))
* README 补充 Linux 构建、运行与已知限制 ([3167170](https://github.com/Stardust011/VoltNest/commit/3167170021d6bbd416a96aa293a7b9b2b3a622ea))
* **readme:** 补充开发流程、指标与已知限制 ([1de45b6](https://github.com/Stardust011/VoltNest/commit/1de45b6a34143ab90a5e47b440a6f868759e2273))
* **test-matrix:** 把规则映射到真实测试函数 ([1567c2d](https://github.com/Stardust011/VoltNest/commit/1567c2d8a506cebcff33b37786409b06a2b2e680))
* **ui:** 补充 shadcn-vue 迁移清单、差异与验证记录 ([eea34b9](https://github.com/Stardust011/VoltNest/commit/eea34b9fa8d8af7fed52d3c0e91a9250a4e0b6be))
* 补齐实现规格、ADR 与协议取证报告 ([0009387](https://github.com/Stardust011/VoltNest/commit/0009387e5bc7be2668ddf29b969bbab3f45d17bf))


### 测试

* **app:** 采集线程 panic 隔离与退避序列 ([9260e6f](https://github.com/Stardust011/VoltNest/commit/9260e6fa0e48b737c8e92d3b1e7a6d919c0364de))
* **core:** 按阈值定位通知条目而不是取下标 ([d8777a3](https://github.com/Stardust011/VoltNest/commit/d8777a3bf4398f8e795df57366fcc040f7d3a896))
* **hid:** 补齐命令字过滤、成因文本与布局校验分支的覆盖 ([51d586f](https://github.com/Stardust011/VoltNest/commit/51d586f8bd9eab31e7234a42bcf06a0c16e2b35f))
* **hid:** 补齐诊断文本与解码分支的覆盖 ([65163ab](https://github.com/Stardust011/VoltNest/commit/65163ab775bc847de5674728c0418042d546ba61))
* **storage:** 隔离 Linux 路径语义测试避免 Windows 上误报失败 ([5e477b1](https://github.com/Stardust011/VoltNest/commit/5e477b1304c88aa2f0380ace9dd724005880c1cf))


### 构建

* **deps:** 升级 commitlint 到 21.2.4 ([f9fdeb0](https://github.com/Stardust011/VoltNest/commit/f9fdeb09a5dd43a66a4ea7a0196496839c744cd7))
* **desktop:** 按平台拆分打包目标，Linux 出 deb 与 AppImage ([85c5c79](https://github.com/Stardust011/VoltNest/commit/85c5c79b27ec0c868d9efd44de3a1f3535b4ec83))
* **pnpm:** 前端脚本内部串联改用 pnpm ([78f3b7c](https://github.com/Stardust011/VoltNest/commit/78f3b7cb13800e3f0fa200558bd685a4cffa367e))
* **pnpm:** 包管理从 npm 迁移到 pnpm workspace ([3b6ff00](https://github.com/Stardust011/VoltNest/commit/3b6ff009b93e1ecdb4233601394c611f48754a93))
* **scripts:** 前端脚本统一从仓库根目录运行 ([7ec1c23](https://github.com/Stardust011/VoltNest/commit/7ec1c2300e53e63c37566ddc3b538a4f4ad7b205))


### 持续集成

* **coverage:** 补充 dbus 开发包以构建 voltnest-ble ([2ae31bc](https://github.com/Stardust011/VoltNest/commit/2ae31bca96dded46d08cf1140d4969abdffd71d2))
* **dependabot:** 为前端依赖添加更新冷却期 ([0649c4d](https://github.com/Stardust011/VoltNest/commit/0649c4dfe76f5cb302bebe960930315baad0734a))
* **deps:** cargo 生态补上依赖冷却期 ([50ddc26](https://github.com/Stardust011/VoltNest/commit/50ddc26c413816dd4d2ce06d3e7563c6e325aed1))
* **deps:** typescript 只拦 7.0.0 及以上 ([41bed70](https://github.com/Stardust011/VoltNest/commit/41bed70cc53a1ed3430c6b73424b9296ddf67c75))
* **deps:** 禁止 dependabot 升级 typescript 主版本 ([0c0a310](https://github.com/Stardust011/VoltNest/commit/0c0a310e4d69061825da5dd94aa9b5055ecee5ca))
* fmt、clippy、测试、覆盖率、前端与矩阵校验 ([a32dec9](https://github.com/Stardust011/VoltNest/commit/a32dec976d3329b45a5fe663c86af45c865156be))
* **format:** 补齐根目录文本与 Python 脚本的格式门禁 ([736cbf5](https://github.com/Stardust011/VoltNest/commit/736cbf52a86cd5f8e4296ee2efec46bac9829b2d))
* linux 缓存键改名为 linux-test ([a484f87](https://github.com/Stardust011/VoltNest/commit/a484f8745659d8b7702206156210cfb30cc2fee7))
* **mappings:** 仅在 devices 有差异时开 PR ([7c53aaa](https://github.com/Stardust011/VoltNest/commit/7c53aaa5c6e878dd7a94fe0c0bb73d6df667be0d))
* **mappings:** 抓取后统一型号表排版再落盘 ([ed627e4](https://github.com/Stardust011/VoltNest/commit/ed627e4595a5004a44ff232f43d81f3465b81600))
* **mappings:** 每月自动抓取厂商型号表 ([8472d0a](https://github.com/Stardust011/VoltNest/commit/8472d0a801c85c6b11cb8eec1e8df4b4a7117546))
* PR 上不再按路径裁剪，保证汇总检查始终上报 ([87d6d1d](https://github.com/Stardust011/VoltNest/commit/87d6d1d333df98a5178885929b0b473d461a1424))
* release 构建只留在 release.yml ([ddf47f3](https://github.com/Stardust011/VoltNest/commit/ddf47f33dcbca4f0844cc7e7ec3396e764641d44))
* **release:** 恢复 master 触发，补齐 manifest 并支持按 tag 重打包 ([f592050](https://github.com/Stardust011/VoltNest/commit/f59205059b71f49ec113ba9e6f551c73f293b9a8))
* **release:** 打包前增加 release 模式编译检查 ([8f91059](https://github.com/Stardust011/VoltNest/commit/8f9105956e10783921d44e3149f5db8daf8744c5))
* **release:** 改由 release-please 生成版本与更新日志 ([08ee959](https://github.com/Stardust011/VoltNest/commit/08ee9595e41da34dce2d286085851243338f206b))
* 优化ci流程 ([1b24348](https://github.com/Stardust011/VoltNest/commit/1b24348947bf6df9f190bc7977692afdaef8c9fe))
* 修正触发分支并补 release 构建冒烟 ([d7947c2](https://github.com/Stardust011/VoltNest/commit/d7947c24c3f9ac2d04a48391cf3c47af591dfa8e))
* 升级 actions 到 Node 24 版本 ([d3ea2f1](https://github.com/Stardust011/VoltNest/commit/d3ea2f107032cfdd8d0700287235947c916cf4e4))
* 升级 pnpm/action-setup 到 v6 ([12a9100](https://github.com/Stardust011/VoltNest/commit/12a910043f66fe29b88a67b99a550724dfb12c8e))
* 升级 tauri-action 1 与 create-pull-request 8 ([0acb786](https://github.com/Stardust011/VoltNest/commit/0acb786d4711a1a7c42461a55302fe001f07dc26))
* 合并 Windows 测试矩阵与格式检查以压缩计费分钟 ([71759f3](https://github.com/Stardust011/VoltNest/commit/71759f36faa62b815ecfdcbdfe4c38b112fd44aa))
* 增加 Linux 构建与测试任务 ([a7b7dc8](https://github.com/Stardust011/VoltNest/commit/a7b7dc8cd69b9d3ed6f3c21877693a0216782ea2))
* 按改动区域门控并收窄 Windows 矩阵 ([89cd429](https://github.com/Stardust011/VoltNest/commit/89cd4293e93593342a997785a41d44abca386dc1))
* 新增 tag 发布、依赖扫描与每周漂移检测 ([e68c8c3](https://github.com/Stardust011/VoltNest/commit/e68c8c339cd7880d0570d301e5e2b99deca21260))
* 移除覆盖率门禁 ([7c1f2a3](https://github.com/Stardust011/VoltNest/commit/7c1f2a3889f8533808ad8fcde26b802b889fbe00))
* 统一 Node 运行时为 24 ([8891b67](https://github.com/Stardust011/VoltNest/commit/8891b67ac243af5aa6fb2f04d4d76b3871c3bcb1))
* 迁移到 pnpm/setup 并放开 btleplug 更新 ([fba490b](https://github.com/Stardust011/VoltNest/commit/fba490beb3ea021a1505ff0bf2a3ccfdd2fc80a4))


### 维护

* **ci:** mappings 的 pnpm/setup 升到 v3 ([1cfa69e](https://github.com/Stardust011/VoltNest/commit/1cfa69ea3b45395b45448b2bf43bc49ac99d0a9c))
* **comments:** 精简 hid crate Linux 后端选型注释 ([e330381](https://github.com/Stardust011/VoltNest/commit/e330381b4c09d5a29ceb4b0e0d2923b4f182738f))
* **config:** 关闭 WebView 调试模式 ([e6d734b](https://github.com/Stardust011/VoltNest/commit/e6d734bed263bbe15f63544344a45f59314f11cb))
* **deps:** 升级 uuid 1.29 与 winreg 0.56 ([0ffc54f](https://github.com/Stardust011/VoltNest/commit/0ffc54f609ef971bd88dc7dff51ab2cffbf758ec))
* **deps:** 取消版本上限并按最新版本解析 ([8df9a03](https://github.com/Stardust011/VoltNest/commit/8df9a03ab1ab3821daee855c7803b534a8eb5ef2))
* **format:** npm run format 改为全仓格式化 ([cf23f23](https://github.com/Stardust011/VoltNest/commit/cf23f23993f9edc0aec2dbf2edf96d521961a7a1))
* **frontend:** 引入 Prettier 统一前端排版 ([d1d87c2](https://github.com/Stardust011/VoltNest/commit/d1d87c2426c447fc250ce1902ed039f8e368e72c))
* **hooks:** 增加提交信息与代码格式的 Git 钩子 ([065024a](https://github.com/Stardust011/VoltNest/commit/065024a36595a710b1161103eeb26f9d16f047d7))
* **hooks:** 提交钩子改用 husky 与 commitlint ([74ed335](https://github.com/Stardust011/VoltNest/commit/74ed33565e911b1858be0e6b148da7c72e19cbfc))
* **hooks:** 标记钩子文件可执行 ([e2721ac](https://github.com/Stardust011/VoltNest/commit/e2721ac867670e1e929670ee030e4a01251d33a2))
* **lint:** 收敛新工具链下的 clippy 与 rustfmt 告警 ([36724e5](https://github.com/Stardust011/VoltNest/commit/36724e5093a2b6175ba4f4be8382cae0e35c3d9a))
* **mappings:** 同步迈从型号表 ([c7f89ab](https://github.com/Stardust011/VoltNest/commit/c7f89abe33c5e389978e6878ef2171a2f8417564))
* **mappings:** 型号表统一为 LF ([3a944ae](https://github.com/Stardust011/VoltNest/commit/3a944ae9388715130edacce96f26aeeeee9b6fbe))
* **repo:** 忽略 .voltnest 兜底目录 ([34bf08c](https://github.com/Stardust011/VoltNest/commit/34bf08c7563732c682d1b97dcd812a9adbc73369))
* **repo:** 清理内部规格文档与探针工具，统一注释表述 ([e451dbe](https://github.com/Stardust011/VoltNest/commit/e451dbedea514558d4cf76691016b6909a0c343e))
* **tauri:** 允许 MSVC 链接器提示 ([b403f13](https://github.com/Stardust011/VoltNest/commit/b403f13ec29795a29c24fffdd674758e70737e8a))
* **tools:** 纳入取证探针与原始抓包脚本 ([21c7926](https://github.com/Stardust011/VoltNest/commit/21c792645969daf5f8f917c6aae10e56b2efc7cf))
* **web:** 升级前端工具链至 vite 8 / vitest 5 / TS 6 ([3367e29](https://github.com/Stardust011/VoltNest/commit/3367e29d90f31409d3106e2209b1466cc4ba4794))
* 初始化 workspace 骨架 ([b64ff8d](https://github.com/Stardust011/VoltNest/commit/b64ff8d1ed798a54ac04ceb0163e132937e31e8a))


### 样式

* **ble:** 修正 runtime 超长行的 rustfmt 差异 ([b56cb13](https://github.com/Stardust011/VoltNest/commit/b56cb13de5704affce384c0e14f6288d26692991))
* **core:** 按 rustfmt 归一 notify.rs ([49f602a](https://github.com/Stardust011/VoltNest/commit/49f602a9a18e5450580c3a31d4b8715fbf519a30))
* **hid:** 按 rustfmt 归一 frame.rs 与 mchose.rs ([a1fc26f](https://github.com/Stardust011/VoltNest/commit/a1fc26fbb04303686d7bf1855d9356e155022043))
* **mappings:** 型号表纳入 prettier 统一排版 ([2383eca](https://github.com/Stardust011/VoltNest/commit/2383eca48813ca1801d57921e9c32292c9b97c12))
* **panel:** 重排设备卡片布局，收紧徽章与错误文本尺寸 ([ead1485](https://github.com/Stardust011/VoltNest/commit/ead14859cbe3b4a7eaa846bdbfab663375f0ab1e))
* **platform:** 应用 rustfmt ([7ed23a3](https://github.com/Stardust011/VoltNest/commit/7ed23a3059e4e7c476afb164916ff00c3219a2c9))
* **provider:** 统一 rustfmt 格式并整理多行构造 ([647fd6c](https://github.com/Stardust011/VoltNest/commit/647fd6c01fd545c0257e76f27e0672984705fb58))
* 格式化 Markdown 表格、JSON 与 Python 抓取脚本 ([86d3423](https://github.com/Stardust011/VoltNest/commit/86d342319542eb34a7e696e709426158893c6785))
* 统一 rustfmt 与 prettier 格式 ([5c4e879](https://github.com/Stardust011/VoltNest/commit/5c4e8791b1ad41454c74fef9515255532aeaa8bc))
