# Product

<!-- impeccable:product-schema 1 -->

## Platform

windows desktop (Tauri 2 + Rust)

## Users

- 单人个人记账：主要使用者是用户本人，日常记录与分析个人财务。
- 公开分发：产品通过 GitHub Release 公开发布；除本人外暂无已确认的其他具体用户。

## Product Purpose

桌面端个人记账工具：在一个工作空间内管理多个账本，覆盖消费记录（含分析图表）、分类标签、事件、日记与股票，让个人财务的记录、回顾与分析都在本地完成。

## Positioning

纯本地存储、数据完全自主：所有记账与业务数据保存在用户自己选择的本地 SQLite 工作空间中，无云端账户，核心财务数据不离开本地。

## Operating Context

- Windows 桌面应用（Tauri 2 外壳 + Rust 内核 + Rust/Leptos WASM 界面），无边框自绘标题栏，单实例运行。
- 每个工作空间一个独立 SQLite 数据库，工作空间内包含多个账本（ledger）。
- 金额一律以整数分存储，界面层负责分/元换算。
- 开发调试用 `cargo tauri dev` 一键启动 trunk(WASM, :16000) + 桌面窗口；构建走 `cargo tauri build`，发布走 `build/` 脚本与 GitHub Release。
- 界面语言为中文。

## Capabilities and Constraints

- 功能面：**记账**（记录 / 分析 / 标签 / 模板 四个子功能；含模板、分类标签、账本间同步与图表分析）、股票（账户/持仓/交易记录/统计/重置）、事件、日记（按账本隔离）、设置（通用/日记/股票/关于）。
- 支持 HEIC 图片导入（界面层在 WebView2 内转换后上传，后端只接受 JPEG/PNG/GIF/WebP），
  图表为界面层自绘 SVG（不引入图表 JS 库）。
- 技术约束：浅色/深色双主题、默认跟随系统、单一强调色 `#3964fe`（见 DESIGN.md）、CSS 变量统一使用 `--transactions-` 前缀；界面为 Rust(Leptos/WASM)，仓库内无 Node 依赖；金额恒为整数分。
- 明确边界：除股票行情查询（含股票名称查询）与更新检查外，其余功能完全离线可用。
- 网络请求可走 **HTTP 代理**（不做 SOCKS/HTTPS 代理、不解析 PAC）：`自动探测` 读环境变量与
  Windows 系统代理，也可手动指定 `http://host:port`；「不使用代理」= 真正直连。

## Brand Commitments

- 产品名：Transactions（一款桌面端记账工具）。
- 应用内「Tr」图形（AboutSetting 中的 SVG）是唯一品牌图形；无外部品牌资产。
- 中文界面与文案；公开分发渠道为 GitHub Release。

## Evidence on Hand

- 代码库是功能与行为的唯一权威（当前无 DESIGN.md 历史，无用户证言、案例、演示素材）。
- README.md 包含功能概览与安装/调试/构建说明；后续工作不得凭空编造用户证言、案例、数据或市场声明。

## Product Principles

- 数据自主优先：核心财务数据只存本地，任何联网能力都必须由用户显式配置与授权。
- 忠实记录：保存原始事实（金额、价格、手数、费用、日期等），派生值按需计算，不存储冗余派生数据。
- 一体化个人财务工作台：记账、分析、事件、日记与股票在同一工作空间内协同。
- 公开分发不改变个人工具的本质：以本人真实使用为基准打磨，克制复杂功能堆砌。
- 桌面效率优先：信息密度、键盘可达与操作流畅优先于表达性。
