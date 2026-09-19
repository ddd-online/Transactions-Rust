# Transactions

桌面端个人记账应用：**Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite 内核**，全部由 Rust 实现。
所有记账数据保存在你自己选择的本地工作空间（一个 SQLite 数据库）里，无云端账户、无后台服务、无 Node 依赖。

本文档描述 **0.1.0**。

## 功能

- **消费记录**：记一笔（模板一键填充）、编辑、删除、复制同步到其他账本、筛选（关键词/类型/分类/标签/离群/时间范围）、排序、分页、统计条。
- **数据分析**：分类占比、时间趋势、标签云、离群消费等图表（界面层自绘 SVG，不引入图表 JS 库）。
- **股票交易**：账户与持仓、建仓/加仓/减仓/清仓（真实行情查名与现价）、成交记录与编辑、交易历史归档为轮次、费用设置（佣金/最低佣金/印花税/过户费）、盈亏统计、重置股票数据。
- **关键事件**：按日期管理事件、配色、Markdown 描述、关联/解除关联消费记录、图片附件（含 HEIC 在界面层转码后上传）。
- **日记**：按日期一篇，Markdown 预览/编辑，导入/导出目录，心情标记，字数统计。
- **账本与工作空间**：多账本切换/新建/删除，单实例运行，托盘菜单，浅色/深色双主题，关闭行为可选。
- **数据自主**：除股票行情查询与更新检查外，全部功能离线可用；这两项也可完全不用。

## 技术栈

| 层 | 实现 |
|---|---|
| 桌面外壳 | Tauri 2（窗口/托盘/单实例/`trasset://` 资产协议/自研更新检查） |
| 界面 | Leptos 0.8 CSR → WebAssembly（`cdylib`，仅编译到 `wasm32`），手写 CSS + 设计令牌 |
| 内核 | Rust + `rusqlite`（`bundled` SQLite）+ `r2d2` 连接池 |
| 界面↔内核 | Tauri IPC（`invoke`，单一 `req` 结构体 + 统一错误信封），无 HTTP 内核、不监听端口 |

```
crates/tr-domain/    # 纯领域层：模型 / DTO / 金额分元换算 / 费用分摊（native + wasm 双可编，无 I/O）
crates/tr-store/     # 存储层：当前 schema 建库 + 只读格式校验 + 各 Dao
crates/tr-service/   # 服务层：业务规则（账本 / 交易 / 图表 / 关键事件 / 日记 / 股票），不依赖 tauri
crates/tr-ipc/       # IPC 命令面：全部 #[tauri::command] + 统一错误信封
crates/tr-ui/        # 界面：Leptos CSR + static/{css,fonts,icons}
src-tauri/           # 桌面外壳：窗口 / 托盘 / 配置 / 日志 / 资产协议 / 更新
xtask/               # 验证工具：schema-diff（建库护栏）、seed / dump（示例数据与只读导出）
fixtures/            # schema 基线、端到端脚本（**不含任何真实个人数据**）
```

分层纪律：`tr-domain` 不得引入任何 I/O 依赖（native 与 wasm 共用同一份金额/费用算法）；
只有 `tr-ipc` 依赖 `tauri`，业务规则都能在没有窗口的环境里用 `cargo test` 验证。

## 数据

- 工作空间结构以 `fixtures/schema/fresh.sql` 为基线：`transactions.db` 不存在时按基线建库，
  已存在时**只做只读校验，绝不执行任何 DDL/DML 去改结构**（`cargo xtask validate <dir>`）。
- **本仓库没有、也不会有数据迁移代码**：更早格式的工作空间会被明确拒绝
  （提示改用其他工作目录，或用支持该格式的旧版本升级）。
- 金额恒为整数分（`i64`），只有展示层做分/元换算。
- 用户配置文件位置与键名稳定（`~/.transactions.json`，开发构建 `~/.transactions-dev.json`），读写时保留未知键。

## 下载安装

到 [Releases](https://github.com/ddd-online/Transactions-Rust/releases) 下载
`Transactions-x64-v0.1.0.exe`（NSIS 安装包，简体中文，按当前用户安装，无需管理员权限）。

首次启动会让你选择一个工作空间目录：空目录会按当前 schema 建库，已经是当前格式的目录会直接打开。
应用内「设置 → 关于软件」会检查本仓库的 Release，发现新版本可下载并校验 `sha256` 后安装。

## 从源码构建

前置：**Rust stable 1.96.0** + `wasm32-unknown-unknown` target、`trunk`、
与 `wasm-bindgen` 版本一致的 `wasm-bindgen-cli`（当前 0.2.128）。**不需要 Node/npm**。

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install wasm-bindgen-cli --version 0.2.128 --locked

# 开发：trunk serve（界面 :1520）+ 桌面窗口
cargo tauri dev

# 发布：界面(WASM) + 桌面应用(NSIS) 一键构建 → build/target/
pwsh -File build/build.ps1
```

产物：`build\target\Transactions-x64-v0.1.0.exe`（安装包）、`build\target\transactions.exe`（免安装版）。
发布流程：`build/clean.ps1` → `build/build.ps1` → `build/release.ps1`（`gh release create` + 上传安装包）。
版本号唯一来源是 `src-tauri/tauri.conf.json`。

> `build/build.ps1` 与 `build/release.ps1` 含中文注释，**必须用 PowerShell 7（`pwsh`）运行**；
> 脚本自身会检测并在 5.1 下自动改用 `pwsh` 重跑（Windows PowerShell 5.1 会把无 BOM 的 UTF-8 当 ANSI 解码，
> 曾导致最后一步静默失败、退出码却仍是 0）。`build/build-ui.ps1` 由 `cargo tauri build` 用 5.1 调用，
> 因此它是 ASCII-only 的。

> 发布构建**必须**走 `build/build-ui.ps1`（trunk debug 模式 + 优化拉满，跳过 wasm-opt），
> 不要直接用 `trunk build --release`；手跑 exe 时必须带 `tauri/custom-protocol` 特性，
> 否则窗口里是空的或"连接被拒绝"。原因见 `AGENTS.md` 的「踩过的坑」。

## 验证护栏

```powershell
cargo test --workspace                      # 领域/存储/服务/外壳 单元测试
cargo fmt --check
cargo clippy --all-targets -- -D warnings

cargo xtask schema-diff                     # 建库结构与 fixtures/schema/fresh.sql 基线逐条一致
cargo xtask validate <workspace-dir>        # 只读校验既有工作空间是否为当前格式
cargo xtask seed <workspace-dir>            # 播种一份可复现的示例数据（人工冒烟）
cargo xtask dump <workspace-dir>            # 只读导出业务表为规范化 JSON

# 端到端：真机启动应用，用 UI Automation 驱动窗口
pwsh -File fixtures/smoke.ps1               # 首次启动/已配置 两种启动形态
pwsh -File fixtures/ui-smoke.ps1 -WriteFlow # 7 个页面渲染 + 界面写入闭环
pwsh -File fixtures/ui-stock.ps1            # 股票全生命周期（138 项断言）
pwsh -File fixtures/ui-transactions.ps1     # 消费记录：编辑/模板/排序/筛选
pwsh -File fixtures/ui-diary-edit.ps1       # 日记编辑链路
pwsh -File fixtures/ui-key-event.ps1        # 关键事件：任选日期新建 + 同日 upsert
# 其余脚本见 AGENTS.md 的「常用命令」
```

## 文档

| 文件 | 内容 |
|---|---|
| `CHANGELOG.md` | 版本变更记录 |
| `AGENTS.md` | 架构、全部常用命令、本机环境陷阱与经验教训（开发前必读） |
| `PRODUCT.md` | 产品定位、用户、能力边界 |
| `DESIGN.md` | 设计系统与设计令牌（界面改动的裁决标准） |
| `fixtures/README.md` | 数据基线、种子数据与端到端脚本说明 |

## 许可证

[Apache License 2.0](LICENSE)
