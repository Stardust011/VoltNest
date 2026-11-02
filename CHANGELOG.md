# Changelog

## [0.2.4](https://github.com/Stardust011/VoltNest/compare/v0.2.4...v0.2.4) (2026-11-02)


### ⚠ BREAKING CHANGES

* **hid:** 型号表改用厂商映射表并统一到 data/mappings
* **hid:** `Snapshot.provider`、日志前缀与诊断分组键的家族 ID 由 `hid-nearlink-ajazz` 改为 `hid-vbas`，不保留旧值兼容读取（v1 未发版）。
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌

### 新功能

* **app:** Tauri 宿主、宿主事件循环与采集线程监管 ([7bd4f81](https://github.com/Stardust011/VoltNest/commit/7bd4f81e938a9cd3ea197626697bceb120b5dd9b))
* **autostart:** 自启与手动启动统一受「启动时最小化到托盘」管控 ([a0fec4b](https://github.com/Stardust011/VoltNest/commit/a0fec4bc7ddff13f2903d237b99330c87151a674))
* **ble:** 出现检测独立于读数轮询，连上蓝牙即被发现 ([03c45c6](https://github.com/Stardust011/VoltNest/commit/03c45c6bc6f0aed1b6b5045ac47e877945c2d110))
* **ble:** 实现 ble-bas 解析与协调/会话模型 ([713ff36](https://github.com/Stardust011/VoltNest/commit/713ff36dd357cff44e1438ad62985e6cae78dd94))
* **ble:** 支持 Linux bluez 后端 ([8125054](https://github.com/Stardust011/VoltNest/commit/8125054a43dc50f5fa5d74ae3eb6c948c08c1787))
* **ble:** 断链改为事件驱动，巡检降为保底 ([2746b24](https://github.com/Stardust011/VoltNest/commit/2746b24262add68839d2ef9ab88f35b0e481162d))
* **brand:** 应用图标改为「V + 闪电」标识 ([e0f6e53](https://github.com/Stardust011/VoltNest/commit/e0f6e536c6ceec0198dc011198833b8797fd762f))
* **classify:** 增加 HID 与 BLE 设备类型识别，证据不足返回 Unknown ([14c9055](https://github.com/Stardust011/VoltNest/commit/14c905519e7b07eda45f6dae406688acb5280bd5))
* **classify:** 补齐 Generic Desktop 页 HID 设备类 Usage ([300ee51](https://github.com/Stardust011/VoltNest/commit/300ee5138ba75743708324a046ee4868eb93a7a6))
* **core:** 回填通知投递的真实结果 ([013baed](https://github.com/Stardust011/VoltNest/commit/013baed4eeed1603285a9f671787605f504dd4e3))
* **core:** 状态机、归一化、L2 分组与快照 ([50a0bb4](https://github.com/Stardust011/VoltNest/commit/50a0bb4dec8a0720d0152038a5f2d397639fe047))
* **desktop-ui:** 启动回填时用缓存电量填充主读数 ([8c9fd1b](https://github.com/Stardust011/VoltNest/commit/8c9fd1b3363bd69cb691499eaf699acf0cb7ef96))
* **desktop:** Linux 平台适配与采集失败原因透传 ([f329097](https://github.com/Stardust011/VoltNest/commit/f329097ffb7499e59f7c20838a553a71f5b2717b))
* **diagnostics:** 坏帧记录成因与原始字节，按种类去重 ([3851f01](https://github.com/Stardust011/VoltNest/commit/3851f019e44ceeff5d09f89aba8822f9c322b7bc))
* **diagnostics:** 底部加红框调试区，一键重现首次引导 ([5049304](https://github.com/Stardust011/VoltNest/commit/5049304b067513e70db80a81c94375a1f1d5f8a3))
* **diagnostics:** 未支持组合降为 debug 级，结论行只留参与判定的证据 ([802c946](https://github.com/Stardust011/VoltNest/commit/802c946a6dd28f3826f2119af94d07a25b9e7daa))
* **hid:** add multi-brand battery support ([63649b8](https://github.com/Stardust011/VoltNest/commit/63649b834877ae9f0480babc0aa5660780fa13c3))
* **hid:** 型号表改为运行时加载 ([2c571ee](https://github.com/Stardust011/VoltNest/commit/2c571ee704bb7dbf9a79ad84d63bb4d981f8c879))
* **hid:** 实现 hid-nearlink-ajazz 解析器与读取线程组 ([f0d551e](https://github.com/Stardust011/VoltNest/commit/f0d551eded79d0952447dad78eb0ef9715dc56ba))
* **hid:** 支持 Linux hidraw 后端 ([3bd8f31](https://github.com/Stardust011/VoltNest/commit/3bd8f31a82e0af153d81fedcb1adef9bb938fde5))
* **mappings:** restrict rapoo device map to vendor id 24AE ([c092e5e](https://github.com/Stardust011/VoltNest/commit/c092e5ef042cd9e46c785072773a559906292df2))
* **mchose:** qhw 族改走设备主动推送，查询只在读数过期前探活 ([d24db37](https://github.com/Stardust011/VoltNest/commit/d24db37df6b611703ed3fe81a09582c64aedadd7))
* **mchose:** 对齐官方驱动的 GLW 电量采集 ([9b706e7](https://github.com/Stardust011/VoltNest/commit/9b706e75a2bbc544d5f9188d4f1fd87ffc45a7d1))
* **mchose:** 接入 CZ/QHW 族键盘（G98 V3 / K87 V3） ([f84a108](https://github.com/Stardust011/VoltNest/commit/f84a108789b51e32cf176e4cb651e7cb04a2c1cf))
* **onboarding:** 引导页两步结构重做，补齐进度、示意面板与无障碍关联 ([aa27ecf](https://github.com/Stardust011/VoltNest/commit/aa27ecfc88d61a5305d6624828d1a367bcaaaa77))
* **panel:** 充电读数改绿色，与托盘环同色 ([1f68cc4](https://github.com/Stardust011/VoltNest/commit/1f68cc43ff33db93324ab1be55dfda76d57ea030))
* **platform:** Linux 上实现自启、单实例与 AUMID 适配 ([0adc9ec](https://github.com/Stardust011/VoltNest/commit/0adc9ecfb3ee99173c7a7d986a055eba17ab18c1))
* **platform:** 自启动注册、单实例与 AUMID 声明 ([93cb9d8](https://github.com/Stardust011/VoltNest/commit/93cb9d83a5c33bc9c4f76471c39509d147249535))
* **probe:** 新增「生成上报数据」，未识别设备可抓包产出 issue 报告 ([16d66a8](https://github.com/Stardust011/VoltNest/commit/16d66a82be74a7085615c2d595868925c79117eb))
* **probe:** 移除确认改用 AlertDialog，删记录一并删报告文件，预览不再落盘 ([f416d10](https://github.com/Stardust011/VoltNest/commit/f416d109911e7ad542adc81e8512094b066c8d6c))
* **provider:** 落地 Provider 契约与 PC-1..PC-7 不变量 ([d9655af](https://github.com/Stardust011/VoltNest/commit/d9655af3ba317c1ef11f9e2b872476964a10cc74))
* **provider:** 采集节奏与出现检测间隔移入 data/profiles.json ([91b88c9](https://github.com/Stardust011/VoltNest/commit/91b88c99ff2ab952d6f93f18c6659ffb98922353))
* **settings:** 低电量档位拆成两行列表，输入实时标红并分离临时输入 ([941ddfb](https://github.com/Stardust011/VoltNest/commit/941ddfbcd077776dd3d4929e5e9c813c05b0c8b4))
* **settings:** 阈值输入改用数字框并放开到 0～95，失焦不再自动调整 ([8e0049b](https://github.com/Stardust011/VoltNest/commit/8e0049b93bc3b7d92fb06572ce8ab5f1624e47a4))
* **storage:** settings.json / state.json 原子读写与损坏恢复 ([37ac706](https://github.com/Stardust011/VoltNest/commit/37ac70625e7d413b3da02925e0f0201c62616eae))
* **storage:** 数据目录改按 XDG Base Directory 解析 ([f34a239](https://github.com/Stardust011/VoltNest/commit/f34a2398deb78afac9db6ad0d97249b58c874548))
* **storage:** 设置加载告警提供可读文案 ([3c67375](https://github.com/Stardust011/VoltNest/commit/3c67375755be9bf53a92fb40744d4746dc155f7a))
* **titlebar:** 标题栏改为最小化到托盘与退出程序 ([1afe3ba](https://github.com/Stardust011/VoltNest/commit/1afe3ba428dfd14613d14f710bca748bc2b62e9a))
* **tray:** 全部设备离线时托盘改用应用图标 ([064d25b](https://github.com/Stardust011/VoltNest/commit/064d25b253f5477f1dc6fc3a863c4978e519a627))
* **tray:** 基础图标聚合「还有 N 台」 ([b050d81](https://github.com/Stardust011/VoltNest/commit/b050d819fc011032560dfb1de41039b7352be900))
* **tray:** 没有设备时托盘用应用图标占位 ([510ab50](https://github.com/Stardust011/VoltNest/commit/510ab50c124bd75aee4123876d177c728afa4e35))
* **ui:** Vue 面板壳、快照订阅与纯函数测试 ([e45437c](https://github.com/Stardust011/VoltNest/commit/e45437cae88300e01ca78a577299bdd662e24cf1))
* **ui:** 设备卡最左侧显示类型图标，去掉类型文字标签 ([a76e38e](https://github.com/Stardust011/VoltNest/commit/a76e38e657fa743cbf50d4b71740941b2ac6976e))


### 修复

* **ble:** 已连接设备改用 retrieve_peripherals 枚举，修复取不到电量 ([c745384](https://github.com/Stardust011/VoltNest/commit/c74538453f61ecf187f2b4da8e842963f57ce930))
* **ci:** 修复CI错误 ([acf0f6c](https://github.com/Stardust011/VoltNest/commit/acf0f6c4bb41acb3aa6a96c03cdf492a64dc9e99))
* **ci:** 修正缓存作用域与冷缓存写入 ([d11965a](https://github.com/Stardust011/VoltNest/commit/d11965a58d6fa9ed8ad42998402b3945e392382a))
* **ci:** 授权 detect changes 读取 PR 改动文件 ([27f7a05](https://github.com/Stardust011/VoltNest/commit/27f7a057de5fe79663a0a7fc4834dca058d04590))
* **ci:** 更新 CI 配置以排除 changelog 变更时的构建步骤 ([604f9bd](https://github.com/Stardust011/VoltNest/commit/604f9bd0bcef93411cf7cfd37a5eef46a5087567))
* **desktop-tray:** 冷启动无读数时托盘画蓝环，与面板「等待读数」同色 ([1d9fc48](https://github.com/Stardust011/VoltNest/commit/1d9fc489bbde057b70d63e63a0deef32e203c4e6))
* **desktop-tray:** 托盘每张图标右键可用，有设备时隐藏基础灰环 ([76b3c95](https://github.com/Stardust011/VoltNest/commit/76b3c95d17a49f2613b00f888c9bb5f52d5d4cc7))
* **desktop-tray:** 等待读数的蓝环也把电量写进 tooltip ([640f0d6](https://github.com/Stardust011/VoltNest/commit/640f0d6aa3f7f0d2b09c203cd15f2bd6e135cfd7))
* **diagnostics:** 线程组状态变化主动推送，重启次数即时刷新 ([8d4f5de](https://github.com/Stardust011/VoltNest/commit/8d4f5decf5bc66084e5703f282b221d7d13cf88f))
* **diagnostics:** 通知触发节点改按阈值从高到低排列 ([dfb8fb5](https://github.com/Stardust011/VoltNest/commit/dfb8fb57f091f6e686acb058929e6b5777d966c0))
* **grouping:** 合并设备在启动时只呈现一台 ([5f1374e](https://github.com/Stardust011/VoltNest/commit/5f1374e43a8dfee7e8e3fba4ff32a8e152dcd65e))
* **hid:** 补齐 Linux 安装形态的型号表目录探测 ([ce914b5](https://github.com/Stardust011/VoltNest/commit/ce914b5f6c9226dbf20c256d84bdd8afe87f95a1))
* **hid:** 钉死 MCHOSE GLW 键盘集合的 usage ([a09bf26](https://github.com/Stardust011/VoltNest/commit/a09bf26c04874e7177b5932277ba9d4fcdf8d3f4))
* **mchose:** 按官方驱动族分派键盘电量，修复键盘全部枚举不到 ([f94d467](https://github.com/Stardust011/VoltNest/commit/f94d467e9e1e349d123229a2e8aad62f528334b6))
* **mchose:** 鼠标不再周期查询，改为连接时读一次 + 靠实时帧刷新 ([31ade6b](https://github.com/Stardust011/VoltNest/commit/31ade6bc345851fb4f76955384a587a6dde0dbb1))
* **notify:** Toast 显式使用 VoltNest 的 AUMID 与图标 ([ab01ebb](https://github.com/Stardust011/VoltNest/commit/ab01ebb798cf7e6ea46e8b3a24c5ad905228bd82))
* **notify:** 两档反转给出方向提示，改阈值后清理旧触发节点 ([734f81b](https://github.com/Stardust011/VoltNest/commit/734f81befb379531e75beec0ad9e8e9d16f2ae82))
* **notify:** 改阈值后立即重建两档节点，并推送诊断刷新 ([c8d390c](https://github.com/Stardust011/VoltNest/commit/c8d390c8c035930998b1daac84ceeebd3c405080))
* **panel:** 充电闪电与电量数字改垂直居中，行高钉死 1em ([de8d78f](https://github.com/Stardust011/VoltNest/commit/de8d78f7d61ad631b4c6f243a12fc9e126d15076))
* **release:** Locate signtool and add to PATH for Windows builds ([ba927c0](https://github.com/Stardust011/VoltNest/commit/ba927c0200c777004d9fb6a9a7113b7ed2ad0533))
* **release:** release PR 只保留一个 GPG 签名提交 ([d8f7ef7](https://github.com/Stardust011/VoltNest/commit/d8f7ef73b65fb291df95f56e9e141eb2ad238b71))
* **release:** 修正 Windows 签名配置写入方式 ([720f517](https://github.com/Stardust011/VoltNest/commit/720f517ee6088c1c15e55c4bddfa024522e0d2ca))
* **release:** 改用等值匹配更新 Cargo.lock，避免正则被 jsonpath 解析失败 ([3318e2b](https://github.com/Stardust011/VoltNest/commit/3318e2bc8281283bb7a81f69d2e25ba2ea5eb22b))
* **release:** 改由工作流同步 Cargo.lock 版本 ([b526acc](https://github.com/Stardust011/VoltNest/commit/b526acc3d61e644dbb7c76ac75dc0ff5b87e3f27))
* **release:** 更新文件路径以完成构建输出 ([293a6b9](https://github.com/Stardust011/VoltNest/commit/293a6b97fd4804a0ddc310df0bd97991ec231bec))
* **release:** 用 Git Data API 以 github-actions[bot] 身份提交 Cargo.lock ([9ae0fd0](https://github.com/Stardust011/VoltNest/commit/9ae0fd0a2aba376409e2f9c54a71a8a97242df49))
* **release:** 补回 0.1.1 升级时漏掉的 Cargo.lock ([7561428](https://github.com/Stardust011/VoltNest/commit/7561428f905e5b3fd3ac980b3a41ee3827865add))
* **tray,panel:** 充电绿环保留电量弧 + 闪电与数字按基线对齐 ([52c6631](https://github.com/Stardust011/VoltNest/commit/52c6631e9d5d08cbda56c4e8a86d97b63c22bdf5))


### 重构

* **brand:** 界面徽标改用打包位图，便于整体替换 ([fb1ecb7](https://github.com/Stardust011/VoltNest/commit/fb1ecb750356c1694cdce8b86cbfb76eee45049a))
* **desktop-ui:** 精简设置页文案并改用下拉框选择过期速度 ([f332475](https://github.com/Stardust011/VoltNest/commit/f332475cdf9af7b8c309ba1139650ef7d1429c89))
* **desktop-ui:** 精简诊断页字段与文案 ([23a5f88](https://github.com/Stardust011/VoltNest/commit/23a5f88c7f5221bf94e3911139869a92027e0d57))
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌 ([2180575](https://github.com/Stardust011/VoltNest/commit/21805757976a1766b959d0112a35db956285e465))
* **diagnostics:** 结论行只留类型与方法，全部证据降为 debug ([d187690](https://github.com/Stardust011/VoltNest/commit/d187690c921653b29103d0809f8e8509c2f92064))
* **hid:** 型号表 source 仅作展示，不再按结构解析 ([e3e8669](https://github.com/Stardust011/VoltNest/commit/e3e8669d94f06ab9afb5c8140ea50c97a48d71a2))
* **hid:** 型号表改用厂商映射表并统一到 data/mappings ([91fab0d](https://github.com/Stardust011/VoltNest/commit/91fab0d80000424b12c502380d2628a73be9e2f1))
* **hid:** 家族统一为 hid-vbas 并抽象品牌可扩展协议描述 ([1fde86a](https://github.com/Stardust011/VoltNest/commit/1fde86a0713a286101fe9e3277fad79511daa293))
* **hid:** 读取线程改为按 ReplyTransport 一一对应的会话方法 ([6368358](https://github.com/Stardust011/VoltNest/commit/6368358828517a6a170869b60599fa6768f7d3ad))
* **mappings:** config_version 改为抓取时刻 ([8877380](https://github.com/Stardust011/VoltNest/commit/8877380139def83bfef5d65739fba470bf14cda6))
* **mappings:** make rapoo vendor filter a list ([94aec80](https://github.com/Stardust011/VoltNest/commit/94aec80afda760ffd92747852cbb9a34077f1561))
* **naming:** 统一上位机侧命名并收敛一词多义 ([7798f0c](https://github.com/Stardust011/VoltNest/commit/7798f0cdbfbc049b20ca6b6175ddf7fe1a8f7679))
* **provider:** 三档阈值改为逐档秒值，去掉倍率换算 ([32b0e72](https://github.com/Stardust011/VoltNest/commit/32b0e72ffb51d38d1b2a7925ead7ba992d1043e7))
* **provider:** 阈值链基线与三档倍率移入 data/profiles.json ([164e880](https://github.com/Stardust011/VoltNest/commit/164e8802e09060eac8cf12daf094c7346a1c52e1))
* **settings:** 「数据过期速度」改为「刷新速率」 ([2ad14a8](https://github.com/Stardust011/VoltNest/commit/2ad14a8585bb7f07000498d5dbbc78a217934da1))
* **ui:** 显示调试改为开关并挪进调试区 ([1ba10ad](https://github.com/Stardust011/VoltNest/commit/1ba10ad2fc5de7268c1beec8584468f50777a07d))
* 精简全仓注释与面向用户文案 ([decf80c](https://github.com/Stardust011/VoltNest/commit/decf80cef87f5899470822180d0573d9005975da))


### 文档

* **hid:** 精简 GLW 集合钉死的注释 ([661f162](https://github.com/Stardust011/VoltNest/commit/661f16287af5c66f15223310fbc408208d36c66a))
* README 补充 Linux 构建、运行与已知限制 ([772ccff](https://github.com/Stardust011/VoltNest/commit/772ccffe88b5cc6f134914861bbf76285c0e83d3))
* **readme:** 补充开发流程、指标与已知限制 ([d3f087a](https://github.com/Stardust011/VoltNest/commit/d3f087a9d4affc595db97b2db331cee93cc7135f))


### 测试

* **app:** 采集线程 panic 隔离与退避序列 ([696f30b](https://github.com/Stardust011/VoltNest/commit/696f30b03c7c3a4fbb63564b26f6079356ee311b))
* **core:** 按阈值定位通知条目而不是取下标 ([dd2f12b](https://github.com/Stardust011/VoltNest/commit/dd2f12bedf021c6ab6b02770ce042fd437fd7075))
* **hid:** 补齐命令字过滤、成因文本与布局校验分支的覆盖 ([68286e2](https://github.com/Stardust011/VoltNest/commit/68286e21d99001d06eff7fd371baa4b96e2cadbb))
* **hid:** 补齐诊断文本与解码分支的覆盖 ([035a049](https://github.com/Stardust011/VoltNest/commit/035a049399b94dc7ccf98d90478bb9ee003b687a))
* **storage:** 隔离 Linux 路径语义测试避免 Windows 上误报失败 ([a9b5d2b](https://github.com/Stardust011/VoltNest/commit/a9b5d2b82bbc990b5657be702890bd6a16ae648c))


### 构建

* **deps:** bump @lucide/vue ([#2](https://github.com/Stardust011/VoltNest/issues/2)) ([0fa4d49](https://github.com/Stardust011/VoltNest/commit/0fa4d4901c1d2a5487b263ddcb61dc006a424cb6))
* **deps:** bump actions/checkout from 4 to 7 in the actions group ([91ccf10](https://github.com/Stardust011/VoltNest/commit/91ccf10862e6610fb4b72559b588cf570c4c04db))
* **deps:** bump actions/checkout from 4 to 7 in the actions group ([91ccf10](https://github.com/Stardust011/VoltNest/commit/91ccf10862e6610fb4b72559b588cf570c4c04db))
* **deps:** 升级 commitlint 到 21.2.4 ([3c79c31](https://github.com/Stardust011/VoltNest/commit/3c79c31dbaabc74ae2aebbca4d0d8b73b8bb6bea))
* **desktop:** 按平台拆分打包目标，Linux 出 deb 与 AppImage ([bf41bf3](https://github.com/Stardust011/VoltNest/commit/bf41bf3736d1522f717fb2b621980e38bcefa3ce))
* **pnpm:** 前端脚本内部串联改用 pnpm ([661c638](https://github.com/Stardust011/VoltNest/commit/661c6380836567c199c78e598aa9224a012fa060))
* **pnpm:** 包管理从 npm 迁移到 pnpm workspace ([49c0838](https://github.com/Stardust011/VoltNest/commit/49c08385a487f9b95f85480da1801f1191f34fa3))
* **release:** 让 release PR 同步更新 Cargo.lock 版本 ([ad4e386](https://github.com/Stardust011/VoltNest/commit/ad4e386bf60462e4cb0f7a348c49114e1c8fdb86))
* **scripts:** 前端脚本统一从仓库根目录运行 ([f4b9a7d](https://github.com/Stardust011/VoltNest/commit/f4b9a7d2b1df0ec87a9efbb0757caff069c378db))


### 持续集成

* **deps:** cargo 生态补上依赖冷却期 ([dab1e95](https://github.com/Stardust011/VoltNest/commit/dab1e957a8eac14e502261762236cc28f965a40b))
* **deps:** 禁止 dependabot 升级 typescript 主版本 ([9a1c820](https://github.com/Stardust011/VoltNest/commit/9a1c820bbb1862dd8ad7c20eb4a77cfaaf0beced))
* fmt、clippy、测试、覆盖率、前端与矩阵校验 ([41cdc25](https://github.com/Stardust011/VoltNest/commit/41cdc25731483cc0822a998579c1365a43935959))
* **format:** 补齐根目录文本与 Python 脚本的格式门禁 ([1ea86de](https://github.com/Stardust011/VoltNest/commit/1ea86de0a96a6b897e80ed34d212ef1457987fe1))
* **mappings:** 仅在 devices 有差异时开 PR ([13d6ab1](https://github.com/Stardust011/VoltNest/commit/13d6ab1061ac3379bee97bc7ca0854396fed81fc))
* **mappings:** 抓取后统一型号表排版再落盘 ([f3b454f](https://github.com/Stardust011/VoltNest/commit/f3b454fb80d2ca1f600189fd98627e46d5ab5ab1))
* **mappings:** 每月自动抓取厂商型号表 ([db97f11](https://github.com/Stardust011/VoltNest/commit/db97f116f9fe4a01785df26b6e29df472bc721ae))
* release 构建只留在 release.yml ([8e79310](https://github.com/Stardust011/VoltNest/commit/8e79310906a19b0e129d2b3a7edeeb084edf4d62))
* **release:** 尝试修复代码签名流程 ([b6e9b26](https://github.com/Stardust011/VoltNest/commit/b6e9b26ef1931c50ff632201d5aed4b65fa1bd51))
* **release:** 恢复 master 触发，补齐 manifest 并支持按 tag 重打包 ([9a9be41](https://github.com/Stardust011/VoltNest/commit/9a9be41d067109841c41d140aee34bf18e9ea81d))
* **release:** 打包前增加 release 模式编译检查 ([410931a](https://github.com/Stardust011/VoltNest/commit/410931a7d99f5f9ee8f665c3aa3a05c940b2bc0c))
* **release:** 改由 release-please 生成版本与更新日志 ([7e8d2ce](https://github.com/Stardust011/VoltNest/commit/7e8d2ce40414fda74ea3a1ae62d4641e0930cddb))
* **release:** 添加代码签名流程 ([80480c8](https://github.com/Stardust011/VoltNest/commit/80480c8066d64a32adc54640ff836c5907588ea0))
* **workflows:** skip release PRs and remove release check ([4bc91b5](https://github.com/Stardust011/VoltNest/commit/4bc91b5a076f8bb9b99fcadbdfd84c9289c0f2bb))
* 优化ci流程 ([3c61bfe](https://github.com/Stardust011/VoltNest/commit/3c61bfe17ea38fcefabe9dd2a961ef3778634518))
* 修正触发分支并补 release 构建冒烟 ([a38bf55](https://github.com/Stardust011/VoltNest/commit/a38bf5547615692670490c65c51217dffbc4933a))
* 升级 pnpm/action-setup 到 v6 ([3429db3](https://github.com/Stardust011/VoltNest/commit/3429db3f1783e71858db85595db9ce8a656005d1))
* 升级 tauri-action 1 与 create-pull-request 8 ([01db60a](https://github.com/Stardust011/VoltNest/commit/01db60a1a1ca4bb12385bac7778bd1f773551392))
* 增加 Linux 构建与测试任务 ([5016d49](https://github.com/Stardust011/VoltNest/commit/5016d4915e182e90fb70e937438ddfb8db2c152a))
* 新增 tag 发布、依赖扫描与每周漂移检测 ([6f9f747](https://github.com/Stardust011/VoltNest/commit/6f9f74779aec8850be5660833fb3600ec25a2b67))
* 移除覆盖率门禁 ([bc2282b](https://github.com/Stardust011/VoltNest/commit/bc2282b9fa894f6e1c7c709454aed14f8402ef4e))


### 维护

* **bundle:** 卸载器勾选「删除应用数据」时清理 %APPDATA%\VoltNest ([4e33f5e](https://github.com/Stardust011/VoltNest/commit/4e33f5e84b269c0d6b0b461c5778f48594538ac3))
* **comments:** 精简 hid crate Linux 后端选型注释 ([72b0d8f](https://github.com/Stardust011/VoltNest/commit/72b0d8f26a407021ad7a80edba865b61938180ed))
* **config:** 关闭 WebView 调试模式 ([5059830](https://github.com/Stardust011/VoltNest/commit/5059830b3f0e7f8b9c9ee3a7ff858bdd4c159301))
* **deps:** 升级 uuid 1.29 与 winreg 0.56 ([961240c](https://github.com/Stardust011/VoltNest/commit/961240c87b3399ef058fdf69568ad5d1c6d6fdca))
* **deps:** 取消版本上限并按最新版本解析 ([51daeef](https://github.com/Stardust011/VoltNest/commit/51daeef445944d0391984ad037bd0aca64eedb8b))
* **format:** npm run format 改为全仓格式化 ([5c9c8bd](https://github.com/Stardust011/VoltNest/commit/5c9c8bd7e28367b7e597f6b5eae6e606584dd4f4))
* **format:** 格式化脚本纳入 Python（ruff） ([7ca5b60](https://github.com/Stardust011/VoltNest/commit/7ca5b60d07deb507c0a7abe659fcadbd30a5d1c1))
* **frontend:** 引入 Prettier 统一前端排版 ([b6398b9](https://github.com/Stardust011/VoltNest/commit/b6398b925199775a0ba8149dd1943ff4c2400981))
* **hooks:** 增加提交信息与代码格式的 Git 钩子 ([c96beed](https://github.com/Stardust011/VoltNest/commit/c96beede4e0b32bed852841045da21ac4ef2f62d))
* **hooks:** 提交钩子改用 husky 与 commitlint ([ae12ae7](https://github.com/Stardust011/VoltNest/commit/ae12ae73c152d1be0d9277047de5d34e3371ddf2))
* **lint:** 收敛新工具链下的 clippy 与 rustfmt 告警 ([d4e9609](https://github.com/Stardust011/VoltNest/commit/d4e96094b9c33be0e5c3ab0f9fa2f6c894786ce0))
* **mappings:** 同步厂商型号表 ([#5](https://github.com/Stardust011/VoltNest/issues/5)) ([00255ce](https://github.com/Stardust011/VoltNest/commit/00255cee5e1a79edbc013c388fe11b2a033a4921))
* **mappings:** 同步迈从型号表 ([d789096](https://github.com/Stardust011/VoltNest/commit/d7890966db04ec277a1c18d93a3071ecc8abf2ed))
* **mappings:** 型号表统一为 LF ([3c7f83b](https://github.com/Stardust011/VoltNest/commit/3c7f83bfb9fb981e6906fef6b3a192e8539ec7c1))
* **mappings:** 摘要写入 PR 正文并以机器人身份签名提交 ([5d6e36b](https://github.com/Stardust011/VoltNest/commit/5d6e36beb9d752c956e351be5567fb06fbb90465))
* **master:** release 0.1.0 ([55d5108](https://github.com/Stardust011/VoltNest/commit/55d51085d7b4f87eaa1ef6affd7a3ccd2c8ecdb7))
* **master:** release 0.1.0 ([af793c0](https://github.com/Stardust011/VoltNest/commit/af793c098597c6bad2976e1cc745a4923ad17b12))
* **master:** release 0.1.1 ([fcef3ee](https://github.com/Stardust011/VoltNest/commit/fcef3ee604796fba04d9f816ec37b4f633237d47))
* **master:** release 0.1.1 ([bc0ba2b](https://github.com/Stardust011/VoltNest/commit/bc0ba2bc167421a1c2678f23dbc991ec5cf7460b))
* **master:** release 0.2.0 ([fa2359e](https://github.com/Stardust011/VoltNest/commit/fa2359eb632b3f370af3fe841a44b48cca305988))
* **master:** release 0.2.0 ([b646f2a](https://github.com/Stardust011/VoltNest/commit/b646f2a8556234a222981f8a2fb1693f85a2049f))
* **master:** release 0.2.1 ([014fa67](https://github.com/Stardust011/VoltNest/commit/014fa6773a60298bbcf07fb2dfd499be479d931f))
* **master:** release 0.2.1 ([a03e022](https://github.com/Stardust011/VoltNest/commit/a03e0226b2f042137644e615dea2442330cb4ecd))
* **master:** release 0.2.2 ([d9b3268](https://github.com/Stardust011/VoltNest/commit/d9b3268f3add42562c603b990b274c97d8dbb1ba))
* **master:** release 0.2.2 ([8db82b5](https://github.com/Stardust011/VoltNest/commit/8db82b59be3df1b7716b996a3d043611192be77e))
* **master:** release 0.2.3 ([3a2494c](https://github.com/Stardust011/VoltNest/commit/3a2494ced71df2b89c2a1fda28504239a43e8fd8))
* **master:** release 0.2.3 ([5b445c9](https://github.com/Stardust011/VoltNest/commit/5b445c9fa0268bc372ea0172e2ca04d8623b01fd))
* **master:** release 0.2.4 ([a91cc88](https://github.com/Stardust011/VoltNest/commit/a91cc889fc1db6b4aa1f015602dc5cc2947ac5dd))
* **master:** release 0.2.4 ([36d5a73](https://github.com/Stardust011/VoltNest/commit/36d5a73c7cd91aa7677602f4e8e5b19a739156cb))
* **master:** release 0.2.4 ([#4](https://github.com/Stardust011/VoltNest/issues/4)) ([889dba9](https://github.com/Stardust011/VoltNest/commit/889dba9de57d8f906b6c69729c519ae1727b055b))
* release 0.2.4 ([3c3e127](https://github.com/Stardust011/VoltNest/commit/3c3e127c6412283ef5f9a86d70490b7cfe8f54c6))
* **release:** 同步 Cargo.lock 至 0.2.0 ([f906724](https://github.com/Stardust011/VoltNest/commit/f90672442d2a1bb3f4bad44e19573b2a115b0dda))
* **repo:** 忽略 .voltnest 兜底目录 ([335b09e](https://github.com/Stardust011/VoltNest/commit/335b09ecd857d521f31ed5fbc27c42921cc623bd))
* **repo:** 清理内部规格文档与探针工具，统一注释表述 ([3f7c5b5](https://github.com/Stardust011/VoltNest/commit/3f7c5b52ab78a60caacd2ac42d5110e217fc2bf2))
* **tauri:** 允许 MSVC 链接器提示 ([fc6dd78](https://github.com/Stardust011/VoltNest/commit/fc6dd78d147c5b3b7fe6a0513b020c2f2fa4e80b))
* **web:** 升级前端工具链至 vite 8 / vitest 5 / TS 6 ([0b9d051](https://github.com/Stardust011/VoltNest/commit/0b9d051e81fba268e96edd1b26b297aa1764de03))
* 初始化 workspace 骨架 ([b64ff8d](https://github.com/Stardust011/VoltNest/commit/b64ff8d1ed798a54ac04ceb0163e132937e31e8a))


### 样式

* **ble:** 修正 runtime 超长行的 rustfmt 差异 ([2083846](https://github.com/Stardust011/VoltNest/commit/2083846b9d7b3d00360fa86fbbb82cbd85710910))
* **core:** 按 rustfmt 归一 notify.rs ([ef930a0](https://github.com/Stardust011/VoltNest/commit/ef930a0a01c0cc64a63c8a183df3ebe8257d0e5b))
* **hid:** 按 rustfmt 归一 frame.rs 与 mchose.rs ([3b52f5a](https://github.com/Stardust011/VoltNest/commit/3b52f5a49334fae9fa53e02cb925d24323dbf760))
* **mappings:** 型号表纳入 prettier 统一排版 ([da62636](https://github.com/Stardust011/VoltNest/commit/da62636c7ae4f22f3cb4001ea3c4f0efea59c340))
* **panel:** 重排设备卡片布局，收紧徽章与错误文本尺寸 ([98c6fbb](https://github.com/Stardust011/VoltNest/commit/98c6fbbdf1a6aa941a3191ed244248372abccc9e))
* **platform:** 应用 rustfmt ([46a94a8](https://github.com/Stardust011/VoltNest/commit/46a94a89181d546218b16cf9ef8e56af52c10f4f))
* **provider:** 统一 rustfmt 格式并整理多行构造 ([ecd26aa](https://github.com/Stardust011/VoltNest/commit/ecd26aae2ade031fd15ae669dd202c6d92375393))
* 格式化 Markdown 表格、JSON 与 Python 抓取脚本 ([e81478f](https://github.com/Stardust011/VoltNest/commit/e81478f79626b0296ed2b5aed62f584c471c636b))
* 统一 rustfmt 与 prettier 格式 ([3983ec3](https://github.com/Stardust011/VoltNest/commit/3983ec359b4b20c7eb22fca47fad3943de67ea6a))

## [0.2.4](https://github.com/Stardust011/VoltNest/compare/v0.2.4...v0.2.4) (2026-11-02)


### ⚠ BREAKING CHANGES

* **hid:** 型号表改用厂商映射表并统一到 data/mappings
* **hid:** `Snapshot.provider`、日志前缀与诊断分组键的家族 ID 由 `hid-nearlink-ajazz` 改为 `hid-vbas`，不保留旧值兼容读取（v1 未发版）。
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌

### 新功能

* **app:** Tauri 宿主、宿主事件循环与采集线程监管 ([7bd4f81](https://github.com/Stardust011/VoltNest/commit/7bd4f81e938a9cd3ea197626697bceb120b5dd9b))
* **autostart:** 自启与手动启动统一受「启动时最小化到托盘」管控 ([a0fec4b](https://github.com/Stardust011/VoltNest/commit/a0fec4bc7ddff13f2903d237b99330c87151a674))
* **ble:** 出现检测独立于读数轮询，连上蓝牙即被发现 ([03c45c6](https://github.com/Stardust011/VoltNest/commit/03c45c6bc6f0aed1b6b5045ac47e877945c2d110))
* **ble:** 实现 ble-bas 解析与协调/会话模型 ([713ff36](https://github.com/Stardust011/VoltNest/commit/713ff36dd357cff44e1438ad62985e6cae78dd94))
* **ble:** 支持 Linux bluez 后端 ([8125054](https://github.com/Stardust011/VoltNest/commit/8125054a43dc50f5fa5d74ae3eb6c948c08c1787))
* **ble:** 断链改为事件驱动，巡检降为保底 ([2746b24](https://github.com/Stardust011/VoltNest/commit/2746b24262add68839d2ef9ab88f35b0e481162d))
* **brand:** 应用图标改为「V + 闪电」标识 ([e0f6e53](https://github.com/Stardust011/VoltNest/commit/e0f6e536c6ceec0198dc011198833b8797fd762f))
* **classify:** 增加 HID 与 BLE 设备类型识别，证据不足返回 Unknown ([14c9055](https://github.com/Stardust011/VoltNest/commit/14c905519e7b07eda45f6dae406688acb5280bd5))
* **classify:** 补齐 Generic Desktop 页 HID 设备类 Usage ([300ee51](https://github.com/Stardust011/VoltNest/commit/300ee5138ba75743708324a046ee4868eb93a7a6))
* **core:** 回填通知投递的真实结果 ([013baed](https://github.com/Stardust011/VoltNest/commit/013baed4eeed1603285a9f671787605f504dd4e3))
* **core:** 状态机、归一化、L2 分组与快照 ([50a0bb4](https://github.com/Stardust011/VoltNest/commit/50a0bb4dec8a0720d0152038a5f2d397639fe047))
* **desktop-ui:** 启动回填时用缓存电量填充主读数 ([8c9fd1b](https://github.com/Stardust011/VoltNest/commit/8c9fd1b3363bd69cb691499eaf699acf0cb7ef96))
* **desktop:** Linux 平台适配与采集失败原因透传 ([f329097](https://github.com/Stardust011/VoltNest/commit/f329097ffb7499e59f7c20838a553a71f5b2717b))
* **diagnostics:** 坏帧记录成因与原始字节，按种类去重 ([3851f01](https://github.com/Stardust011/VoltNest/commit/3851f019e44ceeff5d09f89aba8822f9c322b7bc))
* **diagnostics:** 底部加红框调试区，一键重现首次引导 ([5049304](https://github.com/Stardust011/VoltNest/commit/5049304b067513e70db80a81c94375a1f1d5f8a3))
* **diagnostics:** 未支持组合降为 debug 级，结论行只留参与判定的证据 ([802c946](https://github.com/Stardust011/VoltNest/commit/802c946a6dd28f3826f2119af94d07a25b9e7daa))
* **hid:** add multi-brand battery support ([63649b8](https://github.com/Stardust011/VoltNest/commit/63649b834877ae9f0480babc0aa5660780fa13c3))
* **hid:** 型号表改为运行时加载 ([2c571ee](https://github.com/Stardust011/VoltNest/commit/2c571ee704bb7dbf9a79ad84d63bb4d981f8c879))
* **hid:** 实现 hid-nearlink-ajazz 解析器与读取线程组 ([f0d551e](https://github.com/Stardust011/VoltNest/commit/f0d551eded79d0952447dad78eb0ef9715dc56ba))
* **hid:** 支持 Linux hidraw 后端 ([3bd8f31](https://github.com/Stardust011/VoltNest/commit/3bd8f31a82e0af153d81fedcb1adef9bb938fde5))
* **mappings:** restrict rapoo device map to vendor id 24AE ([c092e5e](https://github.com/Stardust011/VoltNest/commit/c092e5ef042cd9e46c785072773a559906292df2))
* **mchose:** qhw 族改走设备主动推送，查询只在读数过期前探活 ([d24db37](https://github.com/Stardust011/VoltNest/commit/d24db37df6b611703ed3fe81a09582c64aedadd7))
* **mchose:** 对齐官方驱动的 GLW 电量采集 ([9b706e7](https://github.com/Stardust011/VoltNest/commit/9b706e75a2bbc544d5f9188d4f1fd87ffc45a7d1))
* **mchose:** 接入 CZ/QHW 族键盘（G98 V3 / K87 V3） ([f84a108](https://github.com/Stardust011/VoltNest/commit/f84a108789b51e32cf176e4cb651e7cb04a2c1cf))
* **onboarding:** 引导页两步结构重做，补齐进度、示意面板与无障碍关联 ([aa27ecf](https://github.com/Stardust011/VoltNest/commit/aa27ecfc88d61a5305d6624828d1a367bcaaaa77))
* **panel:** 充电读数改绿色，与托盘环同色 ([1f68cc4](https://github.com/Stardust011/VoltNest/commit/1f68cc43ff33db93324ab1be55dfda76d57ea030))
* **platform:** Linux 上实现自启、单实例与 AUMID 适配 ([0adc9ec](https://github.com/Stardust011/VoltNest/commit/0adc9ecfb3ee99173c7a7d986a055eba17ab18c1))
* **platform:** 自启动注册、单实例与 AUMID 声明 ([93cb9d8](https://github.com/Stardust011/VoltNest/commit/93cb9d83a5c33bc9c4f76471c39509d147249535))
* **probe:** 新增「生成上报数据」，未识别设备可抓包产出 issue 报告 ([16d66a8](https://github.com/Stardust011/VoltNest/commit/16d66a82be74a7085615c2d595868925c79117eb))
* **probe:** 移除确认改用 AlertDialog，删记录一并删报告文件，预览不再落盘 ([f416d10](https://github.com/Stardust011/VoltNest/commit/f416d109911e7ad542adc81e8512094b066c8d6c))
* **provider:** 落地 Provider 契约与 PC-1..PC-7 不变量 ([d9655af](https://github.com/Stardust011/VoltNest/commit/d9655af3ba317c1ef11f9e2b872476964a10cc74))
* **provider:** 采集节奏与出现检测间隔移入 data/profiles.json ([91b88c9](https://github.com/Stardust011/VoltNest/commit/91b88c99ff2ab952d6f93f18c6659ffb98922353))
* **settings:** 低电量档位拆成两行列表，输入实时标红并分离临时输入 ([941ddfb](https://github.com/Stardust011/VoltNest/commit/941ddfbcd077776dd3d4929e5e9c813c05b0c8b4))
* **settings:** 阈值输入改用数字框并放开到 0～95，失焦不再自动调整 ([8e0049b](https://github.com/Stardust011/VoltNest/commit/8e0049b93bc3b7d92fb06572ce8ab5f1624e47a4))
* **storage:** settings.json / state.json 原子读写与损坏恢复 ([37ac706](https://github.com/Stardust011/VoltNest/commit/37ac70625e7d413b3da02925e0f0201c62616eae))
* **storage:** 数据目录改按 XDG Base Directory 解析 ([f34a239](https://github.com/Stardust011/VoltNest/commit/f34a2398deb78afac9db6ad0d97249b58c874548))
* **storage:** 设置加载告警提供可读文案 ([3c67375](https://github.com/Stardust011/VoltNest/commit/3c67375755be9bf53a92fb40744d4746dc155f7a))
* **titlebar:** 标题栏改为最小化到托盘与退出程序 ([1afe3ba](https://github.com/Stardust011/VoltNest/commit/1afe3ba428dfd14613d14f710bca748bc2b62e9a))
* **tray:** 基础图标聚合「还有 N 台」 ([b050d81](https://github.com/Stardust011/VoltNest/commit/b050d819fc011032560dfb1de41039b7352be900))
* **tray:** 没有设备时托盘用应用图标占位 ([510ab50](https://github.com/Stardust011/VoltNest/commit/510ab50c124bd75aee4123876d177c728afa4e35))
* **ui:** Vue 面板壳、快照订阅与纯函数测试 ([e45437c](https://github.com/Stardust011/VoltNest/commit/e45437cae88300e01ca78a577299bdd662e24cf1))
* **ui:** 设备卡最左侧显示类型图标，去掉类型文字标签 ([a76e38e](https://github.com/Stardust011/VoltNest/commit/a76e38e657fa743cbf50d4b71740941b2ac6976e))


### 修复

* **ble:** 已连接设备改用 retrieve_peripherals 枚举，修复取不到电量 ([c745384](https://github.com/Stardust011/VoltNest/commit/c74538453f61ecf187f2b4da8e842963f57ce930))
* **ci:** 修复CI错误 ([acf0f6c](https://github.com/Stardust011/VoltNest/commit/acf0f6c4bb41acb3aa6a96c03cdf492a64dc9e99))
* **ci:** 修正缓存作用域与冷缓存写入 ([d11965a](https://github.com/Stardust011/VoltNest/commit/d11965a58d6fa9ed8ad42998402b3945e392382a))
* **ci:** 授权 detect changes 读取 PR 改动文件 ([27f7a05](https://github.com/Stardust011/VoltNest/commit/27f7a057de5fe79663a0a7fc4834dca058d04590))
* **ci:** 更新 CI 配置以排除 changelog 变更时的构建步骤 ([604f9bd](https://github.com/Stardust011/VoltNest/commit/604f9bd0bcef93411cf7cfd37a5eef46a5087567))
* **desktop-tray:** 冷启动无读数时托盘画蓝环，与面板「等待读数」同色 ([1d9fc48](https://github.com/Stardust011/VoltNest/commit/1d9fc489bbde057b70d63e63a0deef32e203c4e6))
* **desktop-tray:** 托盘每张图标右键可用，有设备时隐藏基础灰环 ([76b3c95](https://github.com/Stardust011/VoltNest/commit/76b3c95d17a49f2613b00f888c9bb5f52d5d4cc7))
* **desktop-tray:** 等待读数的蓝环也把电量写进 tooltip ([640f0d6](https://github.com/Stardust011/VoltNest/commit/640f0d6aa3f7f0d2b09c203cd15f2bd6e135cfd7))
* **diagnostics:** 线程组状态变化主动推送，重启次数即时刷新 ([8d4f5de](https://github.com/Stardust011/VoltNest/commit/8d4f5decf5bc66084e5703f282b221d7d13cf88f))
* **diagnostics:** 通知触发节点改按阈值从高到低排列 ([dfb8fb5](https://github.com/Stardust011/VoltNest/commit/dfb8fb57f091f6e686acb058929e6b5777d966c0))
* **grouping:** 合并设备在启动时只呈现一台 ([5f1374e](https://github.com/Stardust011/VoltNest/commit/5f1374e43a8dfee7e8e3fba4ff32a8e152dcd65e))
* **hid:** 补齐 Linux 安装形态的型号表目录探测 ([ce914b5](https://github.com/Stardust011/VoltNest/commit/ce914b5f6c9226dbf20c256d84bdd8afe87f95a1))
* **hid:** 钉死 MCHOSE GLW 键盘集合的 usage ([a09bf26](https://github.com/Stardust011/VoltNest/commit/a09bf26c04874e7177b5932277ba9d4fcdf8d3f4))
* **mchose:** 按官方驱动族分派键盘电量，修复键盘全部枚举不到 ([f94d467](https://github.com/Stardust011/VoltNest/commit/f94d467e9e1e349d123229a2e8aad62f528334b6))
* **mchose:** 鼠标不再周期查询，改为连接时读一次 + 靠实时帧刷新 ([31ade6b](https://github.com/Stardust011/VoltNest/commit/31ade6bc345851fb4f76955384a587a6dde0dbb1))
* **notify:** Toast 显式使用 VoltNest 的 AUMID 与图标 ([ab01ebb](https://github.com/Stardust011/VoltNest/commit/ab01ebb798cf7e6ea46e8b3a24c5ad905228bd82))
* **notify:** 两档反转给出方向提示，改阈值后清理旧触发节点 ([734f81b](https://github.com/Stardust011/VoltNest/commit/734f81befb379531e75beec0ad9e8e9d16f2ae82))
* **notify:** 改阈值后立即重建两档节点，并推送诊断刷新 ([c8d390c](https://github.com/Stardust011/VoltNest/commit/c8d390c8c035930998b1daac84ceeebd3c405080))
* **panel:** 充电闪电与电量数字改垂直居中，行高钉死 1em ([de8d78f](https://github.com/Stardust011/VoltNest/commit/de8d78f7d61ad631b4c6f243a12fc9e126d15076))
* **release:** Locate signtool and add to PATH for Windows builds ([ba927c0](https://github.com/Stardust011/VoltNest/commit/ba927c0200c777004d9fb6a9a7113b7ed2ad0533))
* **release:** release PR 只保留一个 GPG 签名提交 ([d8f7ef7](https://github.com/Stardust011/VoltNest/commit/d8f7ef73b65fb291df95f56e9e141eb2ad238b71))
* **release:** 修正 Windows 签名配置写入方式 ([720f517](https://github.com/Stardust011/VoltNest/commit/720f517ee6088c1c15e55c4bddfa024522e0d2ca))
* **release:** 改用等值匹配更新 Cargo.lock，避免正则被 jsonpath 解析失败 ([3318e2b](https://github.com/Stardust011/VoltNest/commit/3318e2bc8281283bb7a81f69d2e25ba2ea5eb22b))
* **release:** 改由工作流同步 Cargo.lock 版本 ([b526acc](https://github.com/Stardust011/VoltNest/commit/b526acc3d61e644dbb7c76ac75dc0ff5b87e3f27))
* **release:** 更新文件路径以完成构建输出 ([293a6b9](https://github.com/Stardust011/VoltNest/commit/293a6b97fd4804a0ddc310df0bd97991ec231bec))
* **release:** 用 Git Data API 以 github-actions[bot] 身份提交 Cargo.lock ([9ae0fd0](https://github.com/Stardust011/VoltNest/commit/9ae0fd0a2aba376409e2f9c54a71a8a97242df49))
* **release:** 补回 0.1.1 升级时漏掉的 Cargo.lock ([7561428](https://github.com/Stardust011/VoltNest/commit/7561428f905e5b3fd3ac980b3a41ee3827865add))
* **tray,panel:** 充电绿环保留电量弧 + 闪电与数字按基线对齐 ([52c6631](https://github.com/Stardust011/VoltNest/commit/52c6631e9d5d08cbda56c4e8a86d97b63c22bdf5))


### 重构

* **brand:** 界面徽标改用打包位图，便于整体替换 ([fb1ecb7](https://github.com/Stardust011/VoltNest/commit/fb1ecb750356c1694cdce8b86cbfb76eee45049a))
* **desktop-ui:** 精简设置页文案并改用下拉框选择过期速度 ([f332475](https://github.com/Stardust011/VoltNest/commit/f332475cdf9af7b8c309ba1139650ef7d1429c89))
* **desktop-ui:** 精简诊断页字段与文案 ([23a5f88](https://github.com/Stardust011/VoltNest/commit/23a5f88c7f5221bf94e3911139869a92027e0d57))
* **desktop-ui:** 组件层全量切换为 shadcn-vue 与 Tailwind 令牌 ([2180575](https://github.com/Stardust011/VoltNest/commit/21805757976a1766b959d0112a35db956285e465))
* **diagnostics:** 结论行只留类型与方法，全部证据降为 debug ([d187690](https://github.com/Stardust011/VoltNest/commit/d187690c921653b29103d0809f8e8509c2f92064))
* **hid:** 型号表 source 仅作展示，不再按结构解析 ([e3e8669](https://github.com/Stardust011/VoltNest/commit/e3e8669d94f06ab9afb5c8140ea50c97a48d71a2))
* **hid:** 型号表改用厂商映射表并统一到 data/mappings ([91fab0d](https://github.com/Stardust011/VoltNest/commit/91fab0d80000424b12c502380d2628a73be9e2f1))
* **hid:** 家族统一为 hid-vbas 并抽象品牌可扩展协议描述 ([1fde86a](https://github.com/Stardust011/VoltNest/commit/1fde86a0713a286101fe9e3277fad79511daa293))
* **hid:** 读取线程改为按 ReplyTransport 一一对应的会话方法 ([6368358](https://github.com/Stardust011/VoltNest/commit/6368358828517a6a170869b60599fa6768f7d3ad))
* **mappings:** config_version 改为抓取时刻 ([8877380](https://github.com/Stardust011/VoltNest/commit/8877380139def83bfef5d65739fba470bf14cda6))
* **mappings:** make rapoo vendor filter a list ([94aec80](https://github.com/Stardust011/VoltNest/commit/94aec80afda760ffd92747852cbb9a34077f1561))
* **naming:** 统一上位机侧命名并收敛一词多义 ([7798f0c](https://github.com/Stardust011/VoltNest/commit/7798f0cdbfbc049b20ca6b6175ddf7fe1a8f7679))
* **provider:** 三档阈值改为逐档秒值，去掉倍率换算 ([32b0e72](https://github.com/Stardust011/VoltNest/commit/32b0e72ffb51d38d1b2a7925ead7ba992d1043e7))
* **provider:** 阈值链基线与三档倍率移入 data/profiles.json ([164e880](https://github.com/Stardust011/VoltNest/commit/164e8802e09060eac8cf12daf094c7346a1c52e1))
* **settings:** 「数据过期速度」改为「刷新速率」 ([2ad14a8](https://github.com/Stardust011/VoltNest/commit/2ad14a8585bb7f07000498d5dbbc78a217934da1))
* **ui:** 显示调试改为开关并挪进调试区 ([1ba10ad](https://github.com/Stardust011/VoltNest/commit/1ba10ad2fc5de7268c1beec8584468f50777a07d))
* 精简全仓注释与面向用户文案 ([decf80c](https://github.com/Stardust011/VoltNest/commit/decf80cef87f5899470822180d0573d9005975da))


### 文档

* **hid:** 精简 GLW 集合钉死的注释 ([661f162](https://github.com/Stardust011/VoltNest/commit/661f16287af5c66f15223310fbc408208d36c66a))
* README 补充 Linux 构建、运行与已知限制 ([772ccff](https://github.com/Stardust011/VoltNest/commit/772ccffe88b5cc6f134914861bbf76285c0e83d3))
* **readme:** 补充开发流程、指标与已知限制 ([d3f087a](https://github.com/Stardust011/VoltNest/commit/d3f087a9d4affc595db97b2db331cee93cc7135f))


### 测试

* **app:** 采集线程 panic 隔离与退避序列 ([696f30b](https://github.com/Stardust011/VoltNest/commit/696f30b03c7c3a4fbb63564b26f6079356ee311b))
* **core:** 按阈值定位通知条目而不是取下标 ([dd2f12b](https://github.com/Stardust011/VoltNest/commit/dd2f12bedf021c6ab6b02770ce042fd437fd7075))
* **hid:** 补齐命令字过滤、成因文本与布局校验分支的覆盖 ([68286e2](https://github.com/Stardust011/VoltNest/commit/68286e21d99001d06eff7fd371baa4b96e2cadbb))
* **hid:** 补齐诊断文本与解码分支的覆盖 ([035a049](https://github.com/Stardust011/VoltNest/commit/035a049399b94dc7ccf98d90478bb9ee003b687a))
* **storage:** 隔离 Linux 路径语义测试避免 Windows 上误报失败 ([a9b5d2b](https://github.com/Stardust011/VoltNest/commit/a9b5d2b82bbc990b5657be702890bd6a16ae648c))


### 构建

* **deps:** bump @lucide/vue ([#2](https://github.com/Stardust011/VoltNest/issues/2)) ([0fa4d49](https://github.com/Stardust011/VoltNest/commit/0fa4d4901c1d2a5487b263ddcb61dc006a424cb6))
* **deps:** bump actions/checkout from 4 to 7 in the actions group ([91ccf10](https://github.com/Stardust011/VoltNest/commit/91ccf10862e6610fb4b72559b588cf570c4c04db))
* **deps:** bump actions/checkout from 4 to 7 in the actions group ([91ccf10](https://github.com/Stardust011/VoltNest/commit/91ccf10862e6610fb4b72559b588cf570c4c04db))
* **deps:** 升级 commitlint 到 21.2.4 ([3c79c31](https://github.com/Stardust011/VoltNest/commit/3c79c31dbaabc74ae2aebbca4d0d8b73b8bb6bea))
* **desktop:** 按平台拆分打包目标，Linux 出 deb 与 AppImage ([bf41bf3](https://github.com/Stardust011/VoltNest/commit/bf41bf3736d1522f717fb2b621980e38bcefa3ce))
* **pnpm:** 前端脚本内部串联改用 pnpm ([661c638](https://github.com/Stardust011/VoltNest/commit/661c6380836567c199c78e598aa9224a012fa060))
* **pnpm:** 包管理从 npm 迁移到 pnpm workspace ([49c0838](https://github.com/Stardust011/VoltNest/commit/49c08385a487f9b95f85480da1801f1191f34fa3))
* **release:** 让 release PR 同步更新 Cargo.lock 版本 ([ad4e386](https://github.com/Stardust011/VoltNest/commit/ad4e386bf60462e4cb0f7a348c49114e1c8fdb86))
* **scripts:** 前端脚本统一从仓库根目录运行 ([f4b9a7d](https://github.com/Stardust011/VoltNest/commit/f4b9a7d2b1df0ec87a9efbb0757caff069c378db))


### 持续集成

* **deps:** cargo 生态补上依赖冷却期 ([dab1e95](https://github.com/Stardust011/VoltNest/commit/dab1e957a8eac14e502261762236cc28f965a40b))
* **deps:** 禁止 dependabot 升级 typescript 主版本 ([9a1c820](https://github.com/Stardust011/VoltNest/commit/9a1c820bbb1862dd8ad7c20eb4a77cfaaf0beced))
* fmt、clippy、测试、覆盖率、前端与矩阵校验 ([41cdc25](https://github.com/Stardust011/VoltNest/commit/41cdc25731483cc0822a998579c1365a43935959))
* **format:** 补齐根目录文本与 Python 脚本的格式门禁 ([1ea86de](https://github.com/Stardust011/VoltNest/commit/1ea86de0a96a6b897e80ed34d212ef1457987fe1))
* **mappings:** 仅在 devices 有差异时开 PR ([13d6ab1](https://github.com/Stardust011/VoltNest/commit/13d6ab1061ac3379bee97bc7ca0854396fed81fc))
* **mappings:** 抓取后统一型号表排版再落盘 ([f3b454f](https://github.com/Stardust011/VoltNest/commit/f3b454fb80d2ca1f600189fd98627e46d5ab5ab1))
* **mappings:** 每月自动抓取厂商型号表 ([db97f11](https://github.com/Stardust011/VoltNest/commit/db97f116f9fe4a01785df26b6e29df472bc721ae))
* release 构建只留在 release.yml ([8e79310](https://github.com/Stardust011/VoltNest/commit/8e79310906a19b0e129d2b3a7edeeb084edf4d62))
* **release:** 尝试修复代码签名流程 ([b6e9b26](https://github.com/Stardust011/VoltNest/commit/b6e9b26ef1931c50ff632201d5aed4b65fa1bd51))
* **release:** 恢复 master 触发，补齐 manifest 并支持按 tag 重打包 ([9a9be41](https://github.com/Stardust011/VoltNest/commit/9a9be41d067109841c41d140aee34bf18e9ea81d))
* **release:** 打包前增加 release 模式编译检查 ([410931a](https://github.com/Stardust011/VoltNest/commit/410931a7d99f5f9ee8f665c3aa3a05c940b2bc0c))
* **release:** 改由 release-please 生成版本与更新日志 ([7e8d2ce](https://github.com/Stardust011/VoltNest/commit/7e8d2ce40414fda74ea3a1ae62d4641e0930cddb))
* **release:** 添加代码签名流程 ([80480c8](https://github.com/Stardust011/VoltNest/commit/80480c8066d64a32adc54640ff836c5907588ea0))
* **workflows:** skip release PRs and remove release check ([4bc91b5](https://github.com/Stardust011/VoltNest/commit/4bc91b5a076f8bb9b99fcadbdfd84c9289c0f2bb))
* 优化ci流程 ([3c61bfe](https://github.com/Stardust011/VoltNest/commit/3c61bfe17ea38fcefabe9dd2a961ef3778634518))
* 修正触发分支并补 release 构建冒烟 ([a38bf55](https://github.com/Stardust011/VoltNest/commit/a38bf5547615692670490c65c51217dffbc4933a))
* 升级 pnpm/action-setup 到 v6 ([3429db3](https://github.com/Stardust011/VoltNest/commit/3429db3f1783e71858db85595db9ce8a656005d1))
* 升级 tauri-action 1 与 create-pull-request 8 ([01db60a](https://github.com/Stardust011/VoltNest/commit/01db60a1a1ca4bb12385bac7778bd1f773551392))
* 增加 Linux 构建与测试任务 ([5016d49](https://github.com/Stardust011/VoltNest/commit/5016d4915e182e90fb70e937438ddfb8db2c152a))
* 新增 tag 发布、依赖扫描与每周漂移检测 ([6f9f747](https://github.com/Stardust011/VoltNest/commit/6f9f74779aec8850be5660833fb3600ec25a2b67))
* 移除覆盖率门禁 ([bc2282b](https://github.com/Stardust011/VoltNest/commit/bc2282b9fa894f6e1c7c709454aed14f8402ef4e))


### 维护

* **bundle:** 卸载器勾选「删除应用数据」时清理 %APPDATA%\VoltNest ([4e33f5e](https://github.com/Stardust011/VoltNest/commit/4e33f5e84b269c0d6b0b461c5778f48594538ac3))
* **comments:** 精简 hid crate Linux 后端选型注释 ([72b0d8f](https://github.com/Stardust011/VoltNest/commit/72b0d8f26a407021ad7a80edba865b61938180ed))
* **config:** 关闭 WebView 调试模式 ([5059830](https://github.com/Stardust011/VoltNest/commit/5059830b3f0e7f8b9c9ee3a7ff858bdd4c159301))
* **deps:** 升级 uuid 1.29 与 winreg 0.56 ([961240c](https://github.com/Stardust011/VoltNest/commit/961240c87b3399ef058fdf69568ad5d1c6d6fdca))
* **deps:** 取消版本上限并按最新版本解析 ([51daeef](https://github.com/Stardust011/VoltNest/commit/51daeef445944d0391984ad037bd0aca64eedb8b))
* **format:** npm run format 改为全仓格式化 ([5c9c8bd](https://github.com/Stardust011/VoltNest/commit/5c9c8bd7e28367b7e597f6b5eae6e606584dd4f4))
* **format:** 格式化脚本纳入 Python（ruff） ([7ca5b60](https://github.com/Stardust011/VoltNest/commit/7ca5b60d07deb507c0a7abe659fcadbd30a5d1c1))
* **frontend:** 引入 Prettier 统一前端排版 ([b6398b9](https://github.com/Stardust011/VoltNest/commit/b6398b925199775a0ba8149dd1943ff4c2400981))
* **hooks:** 增加提交信息与代码格式的 Git 钩子 ([c96beed](https://github.com/Stardust011/VoltNest/commit/c96beede4e0b32bed852841045da21ac4ef2f62d))
* **hooks:** 提交钩子改用 husky 与 commitlint ([ae12ae7](https://github.com/Stardust011/VoltNest/commit/ae12ae73c152d1be0d9277047de5d34e3371ddf2))
* **lint:** 收敛新工具链下的 clippy 与 rustfmt 告警 ([d4e9609](https://github.com/Stardust011/VoltNest/commit/d4e96094b9c33be0e5c3ab0f9fa2f6c894786ce0))
* **mappings:** 同步厂商型号表 ([#5](https://github.com/Stardust011/VoltNest/issues/5)) ([00255ce](https://github.com/Stardust011/VoltNest/commit/00255cee5e1a79edbc013c388fe11b2a033a4921))
* **mappings:** 同步迈从型号表 ([d789096](https://github.com/Stardust011/VoltNest/commit/d7890966db04ec277a1c18d93a3071ecc8abf2ed))
* **mappings:** 型号表统一为 LF ([3c7f83b](https://github.com/Stardust011/VoltNest/commit/3c7f83bfb9fb981e6906fef6b3a192e8539ec7c1))
* **mappings:** 摘要写入 PR 正文并以机器人身份签名提交 ([5d6e36b](https://github.com/Stardust011/VoltNest/commit/5d6e36beb9d752c956e351be5567fb06fbb90465))
* **master:** release 0.1.0 ([55d5108](https://github.com/Stardust011/VoltNest/commit/55d51085d7b4f87eaa1ef6affd7a3ccd2c8ecdb7))
* **master:** release 0.1.0 ([af793c0](https://github.com/Stardust011/VoltNest/commit/af793c098597c6bad2976e1cc745a4923ad17b12))
* **master:** release 0.1.1 ([fcef3ee](https://github.com/Stardust011/VoltNest/commit/fcef3ee604796fba04d9f816ec37b4f633237d47))
* **master:** release 0.1.1 ([bc0ba2b](https://github.com/Stardust011/VoltNest/commit/bc0ba2bc167421a1c2678f23dbc991ec5cf7460b))
* **master:** release 0.2.0 ([fa2359e](https://github.com/Stardust011/VoltNest/commit/fa2359eb632b3f370af3fe841a44b48cca305988))
* **master:** release 0.2.0 ([b646f2a](https://github.com/Stardust011/VoltNest/commit/b646f2a8556234a222981f8a2fb1693f85a2049f))
* **master:** release 0.2.1 ([014fa67](https://github.com/Stardust011/VoltNest/commit/014fa6773a60298bbcf07fb2dfd499be479d931f))
* **master:** release 0.2.1 ([a03e022](https://github.com/Stardust011/VoltNest/commit/a03e0226b2f042137644e615dea2442330cb4ecd))
* **master:** release 0.2.2 ([d9b3268](https://github.com/Stardust011/VoltNest/commit/d9b3268f3add42562c603b990b274c97d8dbb1ba))
* **master:** release 0.2.2 ([8db82b5](https://github.com/Stardust011/VoltNest/commit/8db82b59be3df1b7716b996a3d043611192be77e))
* **master:** release 0.2.3 ([3a2494c](https://github.com/Stardust011/VoltNest/commit/3a2494ced71df2b89c2a1fda28504239a43e8fd8))
* **master:** release 0.2.3 ([5b445c9](https://github.com/Stardust011/VoltNest/commit/5b445c9fa0268bc372ea0172e2ca04d8623b01fd))
* **master:** release 0.2.4 ([a91cc88](https://github.com/Stardust011/VoltNest/commit/a91cc889fc1db6b4aa1f015602dc5cc2947ac5dd))
* **master:** release 0.2.4 ([36d5a73](https://github.com/Stardust011/VoltNest/commit/36d5a73c7cd91aa7677602f4e8e5b19a739156cb))
* release 0.2.4 ([3c3e127](https://github.com/Stardust011/VoltNest/commit/3c3e127c6412283ef5f9a86d70490b7cfe8f54c6))
* **release:** 同步 Cargo.lock 至 0.2.0 ([f906724](https://github.com/Stardust011/VoltNest/commit/f90672442d2a1bb3f4bad44e19573b2a115b0dda))
* **repo:** 忽略 .voltnest 兜底目录 ([335b09e](https://github.com/Stardust011/VoltNest/commit/335b09ecd857d521f31ed5fbc27c42921cc623bd))
* **repo:** 清理内部规格文档与探针工具，统一注释表述 ([3f7c5b5](https://github.com/Stardust011/VoltNest/commit/3f7c5b52ab78a60caacd2ac42d5110e217fc2bf2))
* **tauri:** 允许 MSVC 链接器提示 ([fc6dd78](https://github.com/Stardust011/VoltNest/commit/fc6dd78d147c5b3b7fe6a0513b020c2f2fa4e80b))
* **web:** 升级前端工具链至 vite 8 / vitest 5 / TS 6 ([0b9d051](https://github.com/Stardust011/VoltNest/commit/0b9d051e81fba268e96edd1b26b297aa1764de03))
* 初始化 workspace 骨架 ([b64ff8d](https://github.com/Stardust011/VoltNest/commit/b64ff8d1ed798a54ac04ceb0163e132937e31e8a))


### 样式

* **ble:** 修正 runtime 超长行的 rustfmt 差异 ([2083846](https://github.com/Stardust011/VoltNest/commit/2083846b9d7b3d00360fa86fbbb82cbd85710910))
* **core:** 按 rustfmt 归一 notify.rs ([ef930a0](https://github.com/Stardust011/VoltNest/commit/ef930a0a01c0cc64a63c8a183df3ebe8257d0e5b))
* **hid:** 按 rustfmt 归一 frame.rs 与 mchose.rs ([3b52f5a](https://github.com/Stardust011/VoltNest/commit/3b52f5a49334fae9fa53e02cb925d24323dbf760))
* **mappings:** 型号表纳入 prettier 统一排版 ([da62636](https://github.com/Stardust011/VoltNest/commit/da62636c7ae4f22f3cb4001ea3c4f0efea59c340))
* **panel:** 重排设备卡片布局，收紧徽章与错误文本尺寸 ([98c6fbb](https://github.com/Stardust011/VoltNest/commit/98c6fbbdf1a6aa941a3191ed244248372abccc9e))
* **platform:** 应用 rustfmt ([46a94a8](https://github.com/Stardust011/VoltNest/commit/46a94a89181d546218b16cf9ef8e56af52c10f4f))
* **provider:** 统一 rustfmt 格式并整理多行构造 ([ecd26aa](https://github.com/Stardust011/VoltNest/commit/ecd26aae2ade031fd15ae669dd202c6d92375393))
* 格式化 Markdown 表格、JSON 与 Python 抓取脚本 ([e81478f](https://github.com/Stardust011/VoltNest/commit/e81478f79626b0296ed2b5aed62f584c471c636b))
* 统一 rustfmt 与 prettier 格式 ([3983ec3](https://github.com/Stardust011/VoltNest/commit/3983ec359b4b20c7eb22fca47fad3943de67ea6a))
