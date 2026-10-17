# Transactions

Windows 桌面个人记账应用。每个工作空间是一个独立的 SQLite 数据库，放在你自己选的目录里，
账本、流水、股票、事件、日记、待办都在其中。没有云端账户，不用注册；除了查股票行情和
检查更新，断网照样能用。

## 功能

### 记账

日常流水，模板一键填充，可编辑、删除，也能复制到其他账本。列表支持按关键词、类型、分类、
标签、离群消费和时间范围筛选，能排序、翻页，底部有统计条。

「分析」把分类占比、时间趋势、标签云、离群消费画成图，图表由界面直接输出 SVG，不引 JS 图表库。
标签和模板各有独立入口。

### 股票

账户与持仓，建仓、加仓、减仓、清仓，成交记录可编辑。一轮持仓（建仓到清仓）结束后归档为
一条历史，可以按轮次看盈亏。费用设置含佣金、最低佣金、印花税、过户费，场内基金免印花税
和过户费。股票数据能整体归档到新账本。

### 事件

按日期整理事件，可配色，写 Markdown 描述，加图片附件，关联或解除关联消费记录。

### 待办

卡片式待办，一张卡是一个主题。事项有开始和截止时间、紧急度和重要度（各 -5 到 5）、状态和
多条进度记录，完成后进历史。「四象限图」把进行中的事项画进坐标系里。

### 日记

同一天每个账本各一篇，互不干扰。可以标心情，带字数统计。

### 账本与工作空间

一个工作空间里放多个账本，随手切换、新建、删除。单实例运行，托盘常驻，支持浅色和深色主题，
关窗时可选最小化到托盘还是直接退出。

## 下载

到 [Releases](https://github.com/ddd-online/Transactions-Rust/releases) 下最新的
`Transactions-x64-v<版本>.exe`。第一次启动会让你选工作空间目录，也就是放数据库的文件夹，
选个空目录就行。

## 数据

工作空间里就是 `transactions.db` 这一个文件，拷走它等于拷走全部数据。

结构以 `fixtures/schema/fresh.sql` 为基线。旧库启动时自动升级，升级前留一份 `.bak`，
同一个工作空间只保留最近一份；格式比程序认识的还旧、又没有迁移路径时，会拒绝打开并说明原因。

金额在库里一律存整数分，只在界面上换算成元。配置放在 `~/.transactions.json`，开发构建用
`~/.transactions-dev.json`，位置和键名不会变。

## 从源码构建

需要 Rust stable 1.96.0、`wasm32-unknown-unknown` target、`trunk`、Tauri CLI 2（`cargo tauri`），
以及与 `Cargo.lock` 里 `wasm-bindgen` 版本一致的 `wasm-bindgen-cli`。

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install wasm-bindgen-cli --version 0.2.128 --locked

cargo tauri dev                 # 开发：trunk serve(:16000) + 桌面窗口
pwsh -File build/build.ps1      # 打包：安装包和免安装版都进 build/target/
```

产物是 `build\target\Transactions-x64-v<版本>.exe`（安装包）

## 开发

改界面（`crates/tr-ui` 下的 `.rs` / `.css`）走 dev 服务，一轮几秒，不用打包。调试请用独立目录，
别拿真实账本试：

```powershell
cargo xtask seed target\tests\dev-ws

# 拉起 dev 窗口；改完 .rs/.css 会自动刷新（加 -ShotDir 还能截图）
pwsh -File fixtures/dev-hot.ps1 -Trunk -Launch -Workspace target\tests\dev-ws

# 只看某一页，不重建也不重启
pwsh -File fixtures/dev-shot.ps1 -Page 股票
```

`dev-hot.ps1` 用独立的配置目录，不碰 `~/.transactions-dev.json`。改 `src-tauri/` 外壳要
`cargo tauri dev` 重启，热更新管不到那里。

## 验证

提交前至少跑前三条；`xtask` 的几条用来检查工作空间数据。

```powershell
cargo test --workspace
cargo fmt --check
cargo clippy --all-targets -- -D warnings

cargo xtask schema-diff                     # 建库结构与基线逐条一致
cargo xtask validate <workspace-dir>        # 只读校验工作空间格式
cargo xtask seed <workspace-dir>            # 播种可复现的示例数据
cargo xtask dump <workspace-dir>            # 只读导出业务表为 JSON
```

## 仓库结构

应用代码全部是 Rust：界面用 Leptos 编译成 WebAssembly，外壳是 Tauri 2，存储用 rusqlite。

```
crates/tr-domain/    # 纯领域层：模型 / DTO / 金额换算 / 费用分摊（无 I/O）
crates/tr-store/     # 存储层：建库、迁移、格式校验、各 Dao
crates/tr-service/   # 业务规则，不依赖 tauri
crates/tr-ipc/       # IPC 命令面与统一错误信封
crates/tr-ui/        # 界面：Leptos CSR + static/{css,fonts,icons}
src-tauri/           # 桌面外壳：窗口 / 托盘 / 配置 / 资产协议 / 更新
xtask/               # 工作空间工具：validate / dump / seed / migrate
fixtures/            # schema 基线、种子数据、端到端脚本（不含真实个人数据）
```

## 文档

| 文件 | 内容 |
|---|---|
| `AGENTS.md` | 架构、常用命令、验证纪律与硬契约（开发前必读） |
| `PRODUCT.md` | 产品定位、用户、能力边界 |
| `DESIGN.md` | 设计系统与设计令牌（界面改动的裁决标准） |
| `fixtures/README.md` | 数据基线、种子数据与端到端脚本说明 |

## 许可证

[Apache License 2.0](LICENSE)
