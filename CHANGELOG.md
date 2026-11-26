# Changelog

## [0.6.0](https://github.com/Stardust011/VoltNest/compare/v0.5.0...v0.6.0) (2026-11-26)


### ⚠ BREAKING CHANGES

* **hid:** 型号表改用厂商映射表
* **hid:** 家族统一为 hid-vbas
* **desktop-ui:** 迁移 shadcn-vue 与 Tailwind 令牌

### 新功能

* **app:** Tauri 宿主与采集线程监管 ([b45c355](https://github.com/Stardust011/VoltNest/commit/b45c355878cdea9ba6e87ab90448d8c0bde2161f))
* **ble:** 出现检测独立于读数轮询 ([fb9c51e](https://github.com/Stardust011/VoltNest/commit/fb9c51ea6be1ace9277f0f87852a10b17c029447))
* **ble:** 断链事件驱动 ([58de21a](https://github.com/Stardust011/VoltNest/commit/58de21a10d32524694c84ed76e085e81b47c48d2))
* **brand,titlebar:** 应用图标与标题栏行为 ([03dda22](https://github.com/Stardust011/VoltNest/commit/03dda22979250b622bc4dd25e6979cc0a16d1170))
* **classify:** 设备类型识别与坏帧诊断 ([f12a894](https://github.com/Stardust011/VoltNest/commit/f12a894c1c21078236f67d9082048bc05799aed6))
* **collectors:** HID 与 BLE 采集后端 ([aedb5d3](https://github.com/Stardust011/VoltNest/commit/aedb5d31624d030a6b44f14cb3fa553c32e7ae35))
* **core,storage:** 通知结果回填与设置告警 ([720cc13](https://github.com/Stardust011/VoltNest/commit/720cc132f1960dd4c2998474fdf0f0e43271d27a))
* **core:** Provider 契约、状态机与快照 ([5d05454](https://github.com/Stardust011/VoltNest/commit/5d0545410bff0d6cb6e4887eb3b3947b8ea5b50d))
* **desktop-ui:** 页面精简、自启管控与启动回填 ([e58a549](https://github.com/Stardust011/VoltNest/commit/e58a5498b245657f630849ae8e7cbcedd6f58d4f))
* **desktop,tray:** WebView2 回收与托盘离线收起 ([84ab0e4](https://github.com/Stardust011/VoltNest/commit/84ab0e4f98fbad3ea4e5a9edf3f7d8be62084696))
* **desktop:** 接入悬浮窗宿主进程与托盘入口 ([289f426](https://github.com/Stardust011/VoltNest/commit/289f42630ac0e35d4f0c84fb4d60cf20d9af8f2e))
* **desktop:** 设备卡片支持单独开关悬浮窗 ([2ca3a61](https://github.com/Stardust011/VoltNest/commit/2ca3a615fd9c3ed9c4f8eea6dc0a41f969d9869b))
* **devices:** 设备管理与合并卡呈现 ([d067bab](https://github.com/Stardust011/VoltNest/commit/d067babeabba0a59fd5c2e4c01738dbd58f51275))
* **diagnostics,onboarding:** 状态推送与引导页 ([58da0c4](https://github.com/Stardust011/VoltNest/commit/58da0c4d4dcccd661205a0b86be299ba76a5e48a))
* **floating:** 增加 anchor_id 锚点与悬浮窗布局设置 ([c1d41da](https://github.com/Stardust011/VoltNest/commit/c1d41da11089e85207e19fd3a998f55da98dcb71))
* **floating:** 拆成多窗口库，支持宿主托管与 NDJSON 协议 ([2427797](https://github.com/Stardust011/VoltNest/commit/2427797149e3feb78575e16d3a64fa99e40f7170))
* **floating:** 新增置顶无边框毛玻璃悬浮窗 ([78d6598](https://github.com/Stardust011/VoltNest/commit/78d6598f1f3ca2ea5092ced786c5f35b3a857a04))
* **hid,ble:** 多品牌电量支持与连接枚举修复 ([e3b7a1d](https://github.com/Stardust011/VoltNest/commit/e3b7a1d197f782b79d1b815a395b015c0c3bf981))
* **hid:** 设备名展示 ([829bc31](https://github.com/Stardust011/VoltNest/commit/829bc312514eccf24e12132dddbafbe1580390d2))
* **linux:** 平台适配、打包与测试隔离 ([4f24a20](https://github.com/Stardust011/VoltNest/commit/4f24a2048df5ffb231e1bbdcac1ce5a734d05cf4))
* **mappings:** 型号表运行时加载 ([17fdfca](https://github.com/Stardust011/VoltNest/commit/17fdfcaab9bdce2c672df8c95540b739643477dc))
* **mchose:** GLW 键盘电量采集 ([617963c](https://github.com/Stardust011/VoltNest/commit/617963c8c8111220718e46fcd95c3a4a86d1945d))
* **mchose:** 对齐官方驱动的键盘族分派 ([23214b9](https://github.com/Stardust011/VoltNest/commit/23214b90627488e2f717a2da4214773037c72f7e))
* **merge:** 设备合并支持多台与追加进已有组 ([72befa3](https://github.com/Stardust011/VoltNest/commit/72befa376d1b9efef2b6b7bdec7114881defb20f))
* **platform:** 自启动、单实例与 AUMID ([1014899](https://github.com/Stardust011/VoltNest/commit/10148991fa2525c4fdfd4a14b20d87bcb37dae7c))
* **probe:** 上报数据生成与卸载清理 ([bc7b09c](https://github.com/Stardust011/VoltNest/commit/bc7b09c06792022d978565302abeec5a2aa5f01b))
* **settings,notify:** 阈值档位与通知节点重建 ([67b4c9c](https://github.com/Stardust011/VoltNest/commit/67b4c9c03c3914533a0bbe3880856b6b22bd559e))
* **storage:** 设置与状态原子读写 ([823682a](https://github.com/Stardust011/VoltNest/commit/823682a35fb198005e2adc43745778a9b3c49d82))
* **tray,panel:** 托盘与面板读数呈现统一 ([bdca5f2](https://github.com/Stardust011/VoltNest/commit/bdca5f272f54471d3361a0e7bd56970f147a0061))
* **tray:** 托盘图标聚合与计数 ([ce4fd2b](https://github.com/Stardust011/VoltNest/commit/ce4fd2b78f9d200e8e0cc7f96603ee0e38d193d3))
* **ui:** Vue 面板壳与快照订阅 ([f8ccdb9](https://github.com/Stardust011/VoltNest/commit/f8ccdb94b07627082b0f973ec63fe052fe4b7d1a))


### 修复

* **release:** 修正打包产物路径并清理工作流冗余注释 ([87bc0c7](https://github.com/Stardust011/VoltNest/commit/87bc0c7d1fd54318330c5766d54a5ef222981eb1))


### 重构

* **desktop-ui:** 迁移 shadcn-vue 与 Tailwind 令牌 ([ed6e06f](https://github.com/Stardust011/VoltNest/commit/ed6e06f042ca588144b8fd1f7bef8250db77e30d))
* **hid:** 型号表改用厂商映射表 ([ea5d864](https://github.com/Stardust011/VoltNest/commit/ea5d8646a2cc2c67cffb3dec7f33fb6a6b804f1e))
* **hid:** 家族统一为 hid-vbas ([6874319](https://github.com/Stardust011/VoltNest/commit/6874319ccbc226da6a3d0849329088e655cc7a2d))
* **provider:** 阈值与采集节奏移入 profiles.json ([a80b33b](https://github.com/Stardust011/VoltNest/commit/a80b33bed89d05f56ae692b277ad5acc2dd48002))
* **ui:** 类型图标与调试开关 ([1ff9a71](https://github.com/Stardust011/VoltNest/commit/1ff9a71c7489907e86cb94685ee002ccdd05c31e))
* 命名统一、会话方法与全仓格式化 ([1217aa0](https://github.com/Stardust011/VoltNest/commit/1217aa0464da17c3c836683f2192fdb048044d30))


### 文档

* 同步 CI 与依赖策略说明 ([90cc20c](https://github.com/Stardust011/VoltNest/commit/90cc20cf3664afb0fcca4ca3e97133f43bc16c58))
* 同步 HID 家族命名说明 ([a54df7a](https://github.com/Stardust011/VoltNest/commit/a54df7ab9312da225138e65aff55df60f55f7213))
* 同步发布流程与设备名说明 ([741f7a5](https://github.com/Stardust011/VoltNest/commit/741f7a559e19113215430b25b26370712b2ccaa1))
* 同步格式化与测试命令 ([39b93d0](https://github.com/Stardust011/VoltNest/commit/39b93d0474d27b22f5022d2d01fc555dcbc4fad3))
* 同步桌面端组件与样式说明 ([9dc26b0](https://github.com/Stardust011/VoltNest/commit/9dc26b03bd7168c0d120c691f7141941debfec33))
* 精简 README 并重写架构说明 ([8b4710d](https://github.com/Stardust011/VoltNest/commit/8b4710de4827c87607e2881d46ad06959dd1229b))
* 补充 Linux 构建与打包说明 ([569563c](https://github.com/Stardust011/VoltNest/commit/569563c074c5e9004ed803192497187e57984fea))
* 补充发布与依赖策略说明 ([39fe070](https://github.com/Stardust011/VoltNest/commit/39fe070bb69799fca05ad73c80aadde70b8400bb))
* 补充型号表抓取说明 ([d04fbfa](https://github.com/Stardust011/VoltNest/commit/d04fbfa6a229ae31a55b1dba768fc862da7fecd4))
* 补充开发与构建说明 ([b50092a](https://github.com/Stardust011/VoltNest/commit/b50092a54cf18eab0f2136c02388509f7b1549e9))
* 补充悬浮窗说明 ([786f32b](https://github.com/Stardust011/VoltNest/commit/786f32b657400bac2c55decad7c8233b3e4dd98e))
* 补充托盘与面板读数说明 ([a8b416c](https://github.com/Stardust011/VoltNest/commit/a8b416c4aedc17bd534c2368a05e11e3b2f67d17))
* 补充本地开发与提交规范 ([aa99d74](https://github.com/Stardust011/VoltNest/commit/aa99d745f7455eee5fcc33899d69fe8555a9c5ab))
* 补充格式化命令说明 ([a3e342b](https://github.com/Stardust011/VoltNest/commit/a3e342b2d27b11078de402677b6c67b2c207f5eb))
* 补充设备识别说明 ([ad4b759](https://github.com/Stardust011/VoltNest/commit/ad4b7594ccbd655a49d6d2e355af88fcaddd38c7))


### 构建

* **deps:** bump 依赖组（[#13](https://github.com/Stardust011/VoltNest/issues/13)、[#14](https://github.com/Stardust011/VoltNest/issues/14)） ([b72749b](https://github.com/Stardust011/VoltNest/commit/b72749b30836fc127e1beb52298ff44bdac1386b))
* **deps:** bump 依赖组（[#15](https://github.com/Stardust011/VoltNest/issues/15)、[#16](https://github.com/Stardust011/VoltNest/issues/16)、[#19](https://github.com/Stardust011/VoltNest/issues/19)、[#20](https://github.com/Stardust011/VoltNest/issues/20)） ([8c43533](https://github.com/Stardust011/VoltNest/commit/8c43533ac139f5b9da1c236942655e3432424377))
* **deps:** update dependencies for cargo-deny compliance ([#17](https://github.com/Stardust011/VoltNest/issues/17)) ([565b0f7](https://github.com/Stardust011/VoltNest/commit/565b0f7697cfea568c133e4ae7853d0dbb160b66))
* 引入 cargo-deny 配置 ([e097f20](https://github.com/Stardust011/VoltNest/commit/e097f20d9f37d7f5cfa6307280a1d33827baf05d))
* 引入 pnpm workspace、husky 与 commitlint ([ebfd5ee](https://github.com/Stardust011/VoltNest/commit/ebfd5eee41bee492377e8b0c37fc133872875752))
* 引入 Prettier 配置与忽略规则 ([fd56b2a](https://github.com/Stardust011/VoltNest/commit/fd56b2a5b9b2f9eaf6ea60b9e2072c07474b84f2))
* 引入 release-please 清单与配置 ([8005c56](https://github.com/Stardust011/VoltNest/commit/8005c560a898a2838ce30d3d02f705850305d0fd))
* 收敛 cargo-deny 许可清单 ([f2e55af](https://github.com/Stardust011/VoltNest/commit/f2e55af38f406edd48ff3940d1a17c49ed05bc64))
* 收敛格式忽略规则与依赖约束 ([906bfa5](https://github.com/Stardust011/VoltNest/commit/906bfa5459f1412d977cc432f5e8c8db71d8318c))
* 新增 release-please 与锁文件同步脚本 ([34ca5d9](https://github.com/Stardust011/VoltNest/commit/34ca5d9cf43d63db4b5c2e6145571913f155c3c0))
* 统一 Prettier 与 Ruff 配置 ([d36ae15](https://github.com/Stardust011/VoltNest/commit/d36ae1557360154bcbb3044aedb837b8bdd674db))


### 持续集成

* **deps:** 禁止 dependabot 升级 typescript 主版本 ([e072f28](https://github.com/Stardust011/VoltNest/commit/e072f28c00feec331a69f8d8733829759eb92222))
* 合并检查流水线并增加变更检测 ([c918826](https://github.com/Stardust011/VoltNest/commit/c91882677a7d996bbb4e9faebed6c74a29fea9e4))
* 型号表流水线补充 PR 摘要 ([563c919](https://github.com/Stardust011/VoltNest/commit/563c91960c9b1c61ab2e96f57d0e33fa31cce0c3))
* 接入 release-please 并重构发布流水线 ([1e78574](https://github.com/Stardust011/VoltNest/commit/1e785740dd745d6d3be4152f73841078335f7e4f))
* 接入 ruff 格式门禁 ([9997ee9](https://github.com/Stardust011/VoltNest/commit/9997ee9cf5c9c56a45b4c68e5394a4d37553d1e6))
* 新增 Rust 与前端检查流水线 ([02b107f](https://github.com/Stardust011/VoltNest/commit/02b107f1d56530be583d75e4ca4e111ad25b59c2))
* 新增发布、依赖更新与安全扫描流水线 ([eda056c](https://github.com/Stardust011/VoltNest/commit/eda056c48a9ec6908440aa779240855b7e29dfde))
* 新增月度型号表抓取流水线 ([f207b8f](https://github.com/Stardust011/VoltNest/commit/f207b8f517823fb0c4937921730847b4a4819b08))
* 检查矩阵拆分 Windows 与 Linux ([8c691f7](https://github.com/Stardust011/VoltNest/commit/8c691f70f42ece59158115f1f8cfb3ec168501f7))
* 流水线改按 workspace 安装与构建 ([05b5e3b](https://github.com/Stardust011/VoltNest/commit/05b5e3ba43272143e4f8a6ce6085f1453f1c1a3c))
* 流水线移除重复步骤并接入格式检查 ([f51c4dd](https://github.com/Stardust011/VoltNest/commit/f51c4dde28b2e8a768e9383cc7859d660ed96ba4))
* 流水线纳入 Python 格式检查 ([47b2565](https://github.com/Stardust011/VoltNest/commit/47b2565b1cc3db54bd0b2290f69baf9b076410ab))
* 统一前端格式检查命令 ([56509c6](https://github.com/Stardust011/VoltNest/commit/56509c61cc9dcf8ef08c2df81339d1ec58207ab5))
* 调整型号表与发布流水线参数 ([29fb6ac](https://github.com/Stardust011/VoltNest/commit/29fb6ac1125d36bad72f5ce6926fd5ad782ed874))


### 维护

* **deps:** 升级前端工具链与依赖 ([c4e939b](https://github.com/Stardust011/VoltNest/commit/c4e939b9e64671c1992d6ecf0594687008162841))
* **mappings:** 修正 mchose 脚本过滤逻辑 ([3306cc3](https://github.com/Stardust011/VoltNest/commit/3306cc3fc8ef65b6483c9a86a163c04500d798e0))
* **mappings:** 修正 rapoo 型号表脚本 ([977db24](https://github.com/Stardust011/VoltNest/commit/977db24b578a82ba4021d9c6cae8a7333dccdc90))
* **mappings:** 同步 mchose 型号表抓取脚本 ([416c054](https://github.com/Stardust011/VoltNest/commit/416c0542641a4ff4fbba35cbc059f8a899fbea1a))
* **mappings:** 型号表 PR 摘要与依赖清理 ([00b8d31](https://github.com/Stardust011/VoltNest/commit/00b8d31a141ce73e9fd65355ee613c8f78a6b22b))
* **mappings:** 报告脚本输出 PR 摘要 ([cd17607](https://github.com/Stardust011/VoltNest/commit/cd176072d2491f5028f62d1d3841f7108812e348))
* **mappings:** 新增三家厂商型号表抓取脚本 ([a4e94cb](https://github.com/Stardust011/VoltNest/commit/a4e94cb47101651157e3a9e11fcc05ea4b8cb702))
* **mappings:** 新增月度抓取与差异报告脚本 ([df9c88d](https://github.com/Stardust011/VoltNest/commit/df9c88d4a78bdaccc242cd6e66bdfea5b9d37305))
* **mappings:** 移除旧的型号表抓取脚本 ([3517f96](https://github.com/Stardust011/VoltNest/commit/3517f9681eb24c471279046a2d213809ca8e98c6))
* **mappings:** 补充 GLW 型号条目 ([6785a04](https://github.com/Stardust011/VoltNest/commit/6785a04db26c717e9646936000fcf2024d110ed1))
* **master:** release 0.3.0 ([#9](https://github.com/Stardust011/VoltNest/issues/9)) ([ce3ff60](https://github.com/Stardust011/VoltNest/commit/ce3ff60e04dba65b00e31019a4936bc386bff234))
* **master:** release 0.3.1 ([#10](https://github.com/Stardust011/VoltNest/issues/10)) ([f2d5c2e](https://github.com/Stardust011/VoltNest/commit/f2d5c2eb5bba0884988913a1429cfc1f9fb840f3))
* **master:** release 0.4.0 ([#11](https://github.com/Stardust011/VoltNest/issues/11)) ([17546fa](https://github.com/Stardust011/VoltNest/commit/17546fadd87fef889c0b0c48738dd34e7ef4cdef))
* **master:** release 0.5.0 ([#12](https://github.com/Stardust011/VoltNest/issues/12)) ([35a3c6c](https://github.com/Stardust011/VoltNest/commit/35a3c6ceff8902d35dcc68eaa2d393b2b9e9814b))
* 初始化 workspace 骨架 ([07566a8](https://github.com/Stardust011/VoltNest/commit/07566a8b6074053a06a91bb3b8c46c4648ca7ae1))
* 固定型号表 JSON 的换行符 ([a4955ba](https://github.com/Stardust011/VoltNest/commit/a4955ba05322c9b6b44ae6d85686bd5f88f427e1))
* 忽略 Linux 构建产物 ([9ffcdfd](https://github.com/Stardust011/VoltNest/commit/9ffcdfd6cc514ecbfb12bf8aa164f7bc827dae47))
* 忽略构建产物与本地缓存 ([b031c78](https://github.com/Stardust011/VoltNest/commit/b031c7841b44f9d48b4a7ad81f8486f937665176))


### 样式

* **mappings:** 统一型号表脚本格式 ([f53b880](https://github.com/Stardust011/VoltNest/commit/f53b880c855de40656622bace704e4facb4a3064))
* 全仓应用 Prettier 格式化 ([aebd9e8](https://github.com/Stardust011/VoltNest/commit/aebd9e80e22541b1eabde978208323cef613f25c))
* 统一 Rust 与前端代码格式 ([a4af757](https://github.com/Stardust011/VoltNest/commit/a4af7572b0874fd4f9ccef06bdc9825c070cf702))

## [0.5.0](https://github.com/Stardust011/VoltNest/compare/v0.4.0...v0.5.0) (2026-11-10)


### 新功能

* **floating:** 新增置顶无边框毛玻璃悬浮窗 ([bc4fa35](https://github.com/Stardust011/VoltNest/commit/bc4fa35733e0bb0b4a7b8e1ae2c2d48edc52c706))
* **floating:** 增加 anchor_id 锚点与悬浮窗布局设置 ([2a4220e](https://github.com/Stardust011/VoltNest/commit/2a4220e58a51bfa93313a5457ea7dd64f68b420c))
* **floating:** 拆成多窗口库，支持宿主托管与 NDJSON 协议 ([164a7b2](https://github.com/Stardust011/VoltNest/commit/164a7b21a5c32b60cc27c656ce0d707ed261ee7a))
* **desktop:** 接入悬浮窗宿主进程与托盘入口 ([079ceba](https://github.com/Stardust011/VoltNest/commit/079ceba4320b644cfd3d5377f57408c1ecdb99f5))
* **desktop:** 设备卡片支持单独开关悬浮窗 ([23b3a5d](https://github.com/Stardust011/VoltNest/commit/23b3a5d1cbb3e19d14e5e8d20c0bd52ca91a968a))


### 文档

* 补充悬浮窗说明 ([62c6f2b](https://github.com/Stardust011/VoltNest/commit/62c6f2b806f3367169b98c0abeb649680a95783a))


### 构建

* **deps:** bump 依赖组（#13、#14） ([322d772](https://github.com/Stardust011/VoltNest/commit/322d772de5b89b1dab973da4e1addb779f1b1656))

## [0.4.0](https://github.com/Stardust011/VoltNest/compare/v0.3.1...v0.4.0) (2026-11-03)


### 新功能

* **desktop,tray:** WebView2 回收与托盘离线收起 ([f7ade5f](https://github.com/Stardust011/VoltNest/commit/f7ade5f28b740bfc4259b7e9056997d2a224235c))
* **devices:** 设备管理与合并卡呈现 ([7b8dd85](https://github.com/Stardust011/VoltNest/commit/7b8dd85eb4a5b508997f55443f3d43edd5659d3a))

## [0.3.1](https://github.com/Stardust011/VoltNest/compare/v0.3.0...v0.3.1) (2026-11-03)


### 修复

* **release:** 修正打包产物路径并清理工作流冗余注释 ([96888b8](https://github.com/Stardust011/VoltNest/commit/96888b820b92305825553df2ce5c17058f037caa))

## [0.3.0](https://github.com/Stardust011/VoltNest/compare/v0.2.4...v0.3.0) (2026-11-03)


### 新功能

* **mchose:** 对齐官方驱动的键盘族分派 ([a9d4a34](https://github.com/Stardust011/VoltNest/commit/a9d4a346ddecc600cc08321d3f90ab7abcfd1ae8))
* **probe:** 上报数据生成与卸载清理 ([a0a3ede](https://github.com/Stardust011/VoltNest/commit/a0a3ede5011a6eb9cb1297c81045362e4b783c1c))
* **mchose:** GLW 键盘电量采集 ([471d43d](https://github.com/Stardust011/VoltNest/commit/471d43dadceecf701d6a1e7dae647235553b964e))
* **hid:** 设备名展示 ([7d4d390](https://github.com/Stardust011/VoltNest/commit/7d4d390fa036443b712fee8c06dc5d7377a36e72))


### 文档

* 同步 CI 与依赖策略说明 ([b0f3047](https://github.com/Stardust011/VoltNest/commit/b0f3047cee0314771dd3cf9a2583ee3d2e633712))
* 同步发布流程与设备名说明 ([d929b59](https://github.com/Stardust011/VoltNest/commit/d929b59abd824e590f70b5d15d4533a36779f352))


### 构建

* 新增 release-please 与锁文件同步脚本 ([64cdf91](https://github.com/Stardust011/VoltNest/commit/64cdf9132589a66bdec8195bd6326ae7bddbace2))
* 引入 release-please 清单与配置 ([2783bfd](https://github.com/Stardust011/VoltNest/commit/2783bfd640f0e793859909c66c66ffd8e0cd8141))


### 持续集成

* 流水线纳入 Python 格式检查 ([db96f93](https://github.com/Stardust011/VoltNest/commit/db96f93adfa5d89f20c98fca0d08361052b5fde7))
* 型号表流水线补充 PR 摘要 ([18e9fa1](https://github.com/Stardust011/VoltNest/commit/18e9fa112cabe2b6d00ddd2a17f16d44575570b3))
* 接入 release-please 并重构发布流水线 ([3202f0e](https://github.com/Stardust011/VoltNest/commit/3202f0e4e5f9984de79c8786348e1987e07590d1))


### 维护

* **mappings:** 同步 mchose 型号表抓取脚本 ([d88ea40](https://github.com/Stardust011/VoltNest/commit/d88ea4095c44f3f9d6e7fb56314d064e8aa9892f))
* **mappings:** 修正 mchose 脚本过滤逻辑 ([a16850d](https://github.com/Stardust011/VoltNest/commit/a16850d928593eba05ac9a8c74a713b09a5afeed))
* **mappings:** 补充 GLW 型号条目 ([a72bdd9](https://github.com/Stardust011/VoltNest/commit/a72bdd965cd56b959d3f1c0760a2faab79afaedb))
* **mappings:** 型号表 PR 摘要与依赖清理 ([e2e53bd](https://github.com/Stardust011/VoltNest/commit/e2e53bd7112f00b18c843a049835bf983b334c34))
* **mappings:** 报告脚本输出 PR 摘要 ([be65d9e](https://github.com/Stardust011/VoltNest/commit/be65d9e2422f1d3afff3511ada1fd3d1961b6adc))
* **mappings:** 移除旧的型号表抓取脚本 ([602ca15](https://github.com/Stardust011/VoltNest/commit/602ca1546868d3a960c2a1e059322583e7753540))

## [0.2.4](https://github.com/Stardust011/VoltNest/compare/v0.2.3...v0.2.4) (2026-10-28)


### 新功能

* **ble:** 出现检测独立于读数轮询 ([fb9c51e](https://github.com/Stardust011/VoltNest/commit/fb9c51ea6be1ace9277f0f87852a10b17c029447))


### 文档

* 补充型号表抓取说明 ([d04fbfa](https://github.com/Stardust011/VoltNest/commit/d04fbfa6a229ae31a55b1dba768fc862da7fecd4))
* 补充格式化命令说明 ([a3e342b](https://github.com/Stardust011/VoltNest/commit/a3e342b2d27b11078de402677b6c67b2c207f5eb))
* 补充本地开发与提交规范 ([aa99d74](https://github.com/Stardust011/VoltNest/commit/aa99d745f7455eee5fcc33899d69fe8555a9c5ab))


### 构建

* 收敛 cargo-deny 许可清单 ([f2e55af](https://github.com/Stardust011/VoltNest/commit/f2e55af38f406edd48ff3940d1a17c49ed05bc64))
* 统一 Prettier 与 Ruff 配置 ([d36ae15](https://github.com/Stardust011/VoltNest/commit/d36ae1557360154bcbb3044aedb837b8bdd674db))
* 引入 pnpm workspace、husky 与 commitlint ([ebfd5ee](https://github.com/Stardust011/VoltNest/commit/ebfd5eee41bee492377e8b0c37fc133872875752))
* 收敛格式忽略规则与依赖约束 ([9d168a7](https://github.com/Stardust011/VoltNest/commit/9d168a7a380d3b384ed7ebdbffd621e01c29a60e))


### 持续集成

* 新增月度型号表抓取流水线 ([f207b8f](https://github.com/Stardust011/VoltNest/commit/f207b8f517823fb0c4937921730847b4a4819b08))
* **deps:** 禁止 dependabot 升级 typescript 主版本 ([e072f28](https://github.com/Stardust011/VoltNest/commit/e072f28c00feec331a69f8d8733829759eb92222))
* 调整型号表与发布流水线参数 ([29fb6ac](https://github.com/Stardust011/VoltNest/commit/29fb6ac1125d36bad72f5ce6926fd5ad782ed874))
* 接入 ruff 格式门禁 ([9997ee9](https://github.com/Stardust011/VoltNest/commit/9997ee9cf5c9c56a45b4c68e5394a4d37553d1e6))
* 流水线改按 workspace 安装与构建 ([05b5e3b](https://github.com/Stardust011/VoltNest/commit/05b5e3ba43272143e4f8a6ce6085f1453f1c1a3c))
* 合并检查流水线并增加变更检测 ([41412bc](https://github.com/Stardust011/VoltNest/commit/41412bc51c75aa383addd3aebfb8d7dffd6b663c))


### 维护

* **mappings:** 新增月度抓取与差异报告脚本 ([df9c88d](https://github.com/Stardust011/VoltNest/commit/df9c88d4a78bdaccc242cd6e66bdfea5b9d37305))
* 固定型号表 JSON 的换行符 ([a4955ba](https://github.com/Stardust011/VoltNest/commit/a4955ba05322c9b6b44ae6d85686bd5f88f427e1))
* **deps:** 升级前端工具链与依赖 ([c4e939b](https://github.com/Stardust011/VoltNest/commit/c4e939b9e64671c1992d6ecf0594687008162841))


### 样式

* 统一 Rust 与前端代码格式 ([a4af757](https://github.com/Stardust011/VoltNest/commit/a4af7572b0874fd4f9ccef06bdc9825c070cf702))
* **mappings:** 统一型号表脚本格式 ([f53b880](https://github.com/Stardust011/VoltNest/commit/f53b880c855de40656622bace704e4facb4a3064))

## [0.2.3](https://github.com/Stardust011/VoltNest/compare/v0.2.2...v0.2.3) (2026-10-27)


### 新功能

* **ble:** 断链事件驱动 ([58de21a](https://github.com/Stardust011/VoltNest/commit/58de21a10d32524694c84ed76e085e81b47c48d2))
* **mappings:** 型号表运行时加载 ([17fdfca](https://github.com/Stardust011/VoltNest/commit/17fdfcaab9bdce2c672df8c95540b739643477dc))


### 重构

* 命名统一、会话方法与全仓格式化 ([1217aa0](https://github.com/Stardust011/VoltNest/commit/1217aa0464da17c3c836683f2192fdb048044d30))
* **provider:** 阈值与采集节奏移入 profiles.json ([a80b33b](https://github.com/Stardust011/VoltNest/commit/a80b33bed89d05f56ae692b277ad5acc2dd48002))


### 文档

* 同步格式化与测试命令 ([39b93d0](https://github.com/Stardust011/VoltNest/commit/39b93d0474d27b22f5022d2d01fc555dcbc4fad3))
* 补充发布与依赖策略说明 ([39fe070](https://github.com/Stardust011/VoltNest/commit/39fe070bb69799fca05ad73c80aadde70b8400bb))


### 构建

* 引入 cargo-deny 配置 ([e097f20](https://github.com/Stardust011/VoltNest/commit/e097f20d9f37d7f5cfa6307280a1d33827baf05d))


### 持续集成

* 统一前端格式检查命令 ([56509c6](https://github.com/Stardust011/VoltNest/commit/56509c61cc9dcf8ef08c2df81339d1ec58207ab5))
* 新增发布、依赖更新与安全扫描流水线 ([eda056c](https://github.com/Stardust011/VoltNest/commit/eda056c48a9ec6908440aa779240855b7e29dfde))


### 维护

* **mappings:** 新增三家厂商型号表抓取脚本 ([a4e94cb](https://github.com/Stardust011/VoltNest/commit/a4e94cb47101651157e3a9e11fcc05ea4b8cb702))
* **mappings:** 修正 rapoo 型号表脚本 ([977db24](https://github.com/Stardust011/VoltNest/commit/977db24b578a82ba4021d9c6cae8a7333dccdc90))

## [0.2.2](https://github.com/Stardust011/VoltNest/compare/v0.2.1...v0.2.2) (2026-10-26)


### ⚠ BREAKING CHANGES

* **hid:** 型号表改用厂商映射表


### 新功能

* **linux:** 平台适配、打包与测试隔离 ([4f24a20](https://github.com/Stardust011/VoltNest/commit/4f24a2048df5ffb231e1bbdcac1ce5a734d05cf4))


### 重构

* **hid:** 型号表改用厂商映射表 ([ea5d864](https://github.com/Stardust011/VoltNest/commit/ea5d8646a2cc2c67cffb3dec7f33fb6a6b804f1e))


### 文档

* 补充 Linux 构建与打包说明 ([569563c](https://github.com/Stardust011/VoltNest/commit/569563c074c5e9004ed803192497187e57984fea))


### 持续集成

* 检查矩阵拆分 Windows 与 Linux ([8c691f7](https://github.com/Stardust011/VoltNest/commit/8c691f70f42ece59158115f1f8cfb3ec168501f7))


### 维护

* 忽略 Linux 构建产物 ([9ffcdfd](https://github.com/Stardust011/VoltNest/commit/9ffcdfd6cc514ecbfb12bf8aa164f7bc827dae47))

## [0.2.1](https://github.com/Stardust011/VoltNest/compare/v0.2.0...v0.2.1) (2026-10-25)


### 新功能

* **brand,titlebar:** 应用图标与标题栏行为 ([03dda22](https://github.com/Stardust011/VoltNest/commit/03dda22979250b622bc4dd25e6979cc0a16d1170))
* **hid,ble:** 多品牌电量支持与连接枚举修复 ([e3b7a1d](https://github.com/Stardust011/VoltNest/commit/e3b7a1d197f782b79d1b815a395b015c0c3bf981))


### 重构

* **ui:** 类型图标与调试开关 ([1ff9a71](https://github.com/Stardust011/VoltNest/commit/1ff9a71c7489907e86cb94685ee002ccdd05c31e))


### 文档

* 补充设备识别说明 ([ad4b759](https://github.com/Stardust011/VoltNest/commit/ad4b7594ccbd655a49d6d2e355af88fcaddd38c7))
* 精简 README 并重写架构说明 ([8b4710d](https://github.com/Stardust011/VoltNest/commit/8b4710de4827c87607e2881d46ad06959dd1229b))


### 构建

* 引入 Prettier 配置与忽略规则 ([fd56b2a](https://github.com/Stardust011/VoltNest/commit/fd56b2a5b9b2f9eaf6ea60b9e2072c07474b84f2))


### 持续集成

* 流水线移除重复步骤并接入格式检查 ([f51c4dd](https://github.com/Stardust011/VoltNest/commit/f51c4dde28b2e8a768e9383cc7859d660ed96ba4))


### 样式

* 全仓应用 Prettier 格式化 ([aebd9e8](https://github.com/Stardust011/VoltNest/commit/aebd9e80e22541b1eabde978208323cef613f25c))

## [0.2.0](https://github.com/Stardust011/VoltNest/compare/v0.1.1...v0.2.0) (2026-10-23)


### ⚠ BREAKING CHANGES

* **desktop-ui:** 迁移 shadcn-vue 与 Tailwind 令牌
* **hid:** 家族统一为 hid-vbas


### 新功能

* **tray:** 托盘图标聚合与计数 ([ce4fd2b](https://github.com/Stardust011/VoltNest/commit/ce4fd2b78f9d200e8e0cc7f96603ee0e38d193d3))
* **desktop-ui:** 页面精简、自启管控与启动回填 ([e58a549](https://github.com/Stardust011/VoltNest/commit/e58a5498b245657f630849ae8e7cbcedd6f58d4f))
* **tray,panel:** 托盘与面板读数呈现统一 ([bdca5f2](https://github.com/Stardust011/VoltNest/commit/bdca5f272f54471d3361a0e7bd56970f147a0061))
* **diagnostics,onboarding:** 状态推送与引导页 ([58da0c4](https://github.com/Stardust011/VoltNest/commit/58da0c4d4dcccd661205a0b86be299ba76a5e48a))
* **settings,notify:** 阈值档位与通知节点重建 ([67b4c9c](https://github.com/Stardust011/VoltNest/commit/67b4c9c03c3914533a0bbe3880856b6b22bd559e))
* **classify:** 设备类型识别与坏帧诊断 ([f12a894](https://github.com/Stardust011/VoltNest/commit/f12a894c1c21078236f67d9082048bc05799aed6))


### 重构

* **desktop-ui:** 迁移 shadcn-vue 与 Tailwind 令牌 ([ed6e06f](https://github.com/Stardust011/VoltNest/commit/ed6e06f042ca588144b8fd1f7bef8250db77e30d))
* **hid:** 家族统一为 hid-vbas ([6874319](https://github.com/Stardust011/VoltNest/commit/6874319ccbc226da6a3d0849329088e655cc7a2d))


### 文档

* 补充开发与构建说明 ([b50092a](https://github.com/Stardust011/VoltNest/commit/b50092a54cf18eab0f2136c02388509f7b1549e9))
* 同步桌面端组件与样式说明 ([9dc26b0](https://github.com/Stardust011/VoltNest/commit/9dc26b03bd7168c0d120c691f7141941debfec33))
* 同步 HID 家族命名说明 ([a54df7a](https://github.com/Stardust011/VoltNest/commit/a54df7ab9312da225138e65aff55df60f55f7213))
* 补充托盘与面板读数说明 ([a8b416c](https://github.com/Stardust011/VoltNest/commit/a8b416c4aedc17bd534c2368a05e11e3b2f67d17))


### 持续集成

* 新增 Rust 与前端检查流水线 ([02b107f](https://github.com/Stardust011/VoltNest/commit/02b107f1d56530be583d75e4ca4e111ad25b59c2))


### 维护

* 忽略构建产物与本地缓存 ([b031c78](https://github.com/Stardust011/VoltNest/commit/b031c7841b44f9d48b4a7ad81f8486f937665176))

## [0.1.1](https://github.com/Stardust011/VoltNest/compare/v0.1.0...v0.1.1) (2026-10-22)


### 新功能

* **core:** Provider 契约、状态机与快照 ([5d05454](https://github.com/Stardust011/VoltNest/commit/5d0545410bff0d6cb6e4887eb3b3947b8ea5b50d))
* **collectors:** HID 与 BLE 采集后端 ([aedb5d3](https://github.com/Stardust011/VoltNest/commit/aedb5d31624d030a6b44f14cb3fa553c32e7ae35))
* **storage:** 设置与状态原子读写 ([823682a](https://github.com/Stardust011/VoltNest/commit/823682a35fb198005e2adc43745778a9b3c49d82))
* **platform:** 自启动、单实例与 AUMID ([1014899](https://github.com/Stardust011/VoltNest/commit/10148991fa2525c4fdfd4a14b20d87bcb37dae7c))
* **ui:** Vue 面板壳与快照订阅 ([f8ccdb9](https://github.com/Stardust011/VoltNest/commit/f8ccdb94b07627082b0f973ec63fe052fe4b7d1a))
* **core,storage:** 通知结果回填与设置告警 ([720cc13](https://github.com/Stardust011/VoltNest/commit/720cc132f1960dd4c2998474fdf0f0e43271d27a))
* **app:** Tauri 宿主与采集线程监管 ([b45c355](https://github.com/Stardust011/VoltNest/commit/b45c355878cdea9ba6e87ab90448d8c0bde2161f))

## 0.1.0 (2026-10-22)


### 维护

* 初始化 workspace 骨架 ([07566a8](https://github.com/Stardust011/VoltNest/commit/07566a8b6074053a06a91bb3b8c46c4648ca7ae1))
