# Changelog

本文件记录本仓库的版本变更。版本号以 `src-tauri/tauri.conf.json` 为唯一来源。

## [0.2.0] - 2026-09-20

图表引擎换代 + 界面布局与组件全面细化。**数据格式与配置键名均无变化**，
既有的 0.1.0 工作空间可直接打开（schema 未动）。

### 变更

- **图表**：折线图引擎由 `plotters` 换成 `charts-rs`（`default-features = false`，直出 SVG，
  `THEME_LIGHT` / `THEME_DARK` 跟随主题）；折线改直线段、数据点实心、图例与 Y 轴刻度列对齐、
  单序列面积填充；画布尺寸随容器自适应；游标 / tooltip / 参考线改由 DOM 覆盖层绘制。
- **股票**：修通四个分栏的高度链，「资金变化记录」分页行贴到卡片底部；持仓页左栏通栏、
  「建仓」贴底，卡片三态（软灰卡面 / 纸面悬停 / 强调色选中）；成交记录页详情改纸面卡、
  轮次详情标题独占一行；间距收敛为「区域 24 / 卡内 16 / 内容 12」三级节奏。
- **日记**：移除 Markdown 与预览、去掉底栏，工具栏承载保存状态与删除，编辑框上下留白一致。
- **关键事件**：卡片与关联交易卡改纸面 + 发丝描边，选中用强调色底。
- **消费记录 / 设置 / 分类标签 / 数据分析**：分页、抽屉、日期选择器、空态等细节统一；
  新增 `time_range_picker` 组件，移除未使用的 `float_button`。

### 修复

- **发布：0.2.0 的安装包资产曾经是 0.1.0 的安装包**（用户下载安装后仍是 0.1.0 的界面，
  两个 release 的资产字节数与 `sha256` 完全相同）。原因：`cargo tauri build` 不清 NSIS 产物目录，
  `build/build.ps1` 用 `Get-ChildItem *-setup.exe | Select-Object -First 1` 取**字典序第一个**，
  于是上一版遗留的 `Transactions_0.1.0_x64-setup.exe` 被改名成 `Transactions-x64-v0.2.0.exe` 上传。
  现在构建前先清掉陈旧安装包，只认 `Transactions_{版本}_x64-setup.exe`，
  并断言它是**本轮构建**产出的文件，安装包与便携版都要落盘成功，否则直接失败退出。

### 验证

- 新增 `fixtures/ui-about.ps1`：断言打包产物自报的版本号（「设置 → 关于软件」）与
  `tauri.conf.json` 一致，并覆盖关于页其余固定内容与更新检查终态。
- 新增 `fixtures/dev-hot.ps1`（trunk 与外壳的热更新桥）与 `fixtures/dev-shot.ps1`（按页截图）。

## [0.1.0] - 2026-09-19

首个版本：**Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite 内核**的纯 Rust 桌面记账应用。

### 新增

- **桌面外壳**（Tauri 2）：无边框自绘标题栏、单实例、托盘菜单与"显示主窗口/退出"、
  三种关闭行为（退出 / 最小化到托盘 / 每次询问）、窗口大小与位置按逻辑像素记忆、
  自定义 `trasset://` 资产协议（含路径穿越校验）、深/浅色双主题。
- **界面**（Leptos 0.8 CSR → WASM，仓库内无 Node/npm）：消费记录（记一笔/模板/编辑/删除/同步到其他账本/
  筛选/排序/分页/统计条）、数据分析图表（自绘 SVG）、股票交易全链路（建仓/加仓/减仓/清仓/成交记录/
  轮次归档/费用设置/盈亏统计/行情查询）、关键事件（日期唯一、配色、Markdown、关联消费、图片附件）、
  日记（Markdown 预览与编辑、导入/导出、心情、字数）、设置（通用/消费模板/日记/股票/关于软件）。
- **内核**（Rust）：`tr-domain`（纯领域层，金额恒为整数分、费用分摊）/ `tr-store`（rusqlite + 连接池）/
  `tr-service`（业务规则，不依赖 tauri）/ `tr-ipc`（唯一依赖 tauri 的命令面）。
  界面与内核之间只走 Tauri IPC，**没有本机 HTTP 内核、不监听端口、无子进程**。
- **数据**：每个工作空间一个独立 SQLite 数据库，建库以 `fixtures/schema/fresh.sql` 为基线；
  已存在的工作空间**只做只读校验**，绝不执行 DDL/DML 改结构；`~/.transactions.json` 位置与键名稳定，
  读写保留未知键。**不含任何数据迁移代码**：更早格式的工作空间会被明确拒绝。
- **更新检查**（自研，不用 `tauri-plugin-updater`）：查本仓库 GitHub Release，
  下载安装包并按 `sha256` 校验后安装；支持进度/取消/已下载复用。
- **验证护栏**：`cargo xtask schema-diff`（建库结构与基线逐条一致）、
  17 个端到端脚本（启动形态、关闭行为、7 页渲染+写入闭环、股票全生命周期、日记编辑与导入导出、
  关键事件、拖拽排序、窗口几何、图片上传、像素与主题、设计令牌与跨 crate 契约审计等），
  清单见 `AGENTS.md` 的「常用命令」。

### 说明

- 界面按 `DESIGN.md` 的设计令牌实现；图表与 Markdown 都是界面层自绘 / 纯 Rust 渲染，不引入任何 JS 库。
- 已知偏差与取舍（行情与更新检查的 HTTP 客户端不读系统代理、资金记录 `created_at` 的严格递增规则等）
  记在 `AGENTS.md`。

[0.2.0]: https://github.com/ddd-online/Transactions-Rust/releases/tag/v0.2.0
[0.1.0]: https://github.com/ddd-online/Transactions-Rust/releases/tag/v0.1.0
