# fixtures —— 验证基线

本目录只放**合成**数据与结构性基线，**绝不包含任何真实个人财务数据**
（真实工作空间仅在本机、以 gitignore 的方式用于人工冒烟）。

## `schema/fresh.sql`

当前格式空库的原始 DDL（`sqlite3 transactions.db .schema` 的输出），逐字节保存。

- 内容：19 张表 + 21 个索引 + 3 条迁移登记记录（`tbl_billadm_schema_migration`）
- 用途：为**全新工作空间**建库时执行；`cargo xtask schema-diff` 校验「Rust 建库结果」与基线逐条一致
- 纪律：只在 `transactions.db` 不存在时执行一次；打开既有工作空间时绝不执行 DDL/DML

## 修改基线的正确姿势

```powershell
# 1. 用一个**已经过人工确认**的空工作空间导出基线（不要拿 schema-diff 的输出回头改基线，那是循环论证）
$ws = "$env:TEMP\tr-fresh"; Remove-Item $ws -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $ws | Out-Null
cargo tauri dev            # 选 $ws 作为工作空间建库
# 2. 导出 DDL，覆盖 fixtures/schema/fresh.sql
sqlite3 "$ws\transactions.db" ".schema"
```

> 基线一旦更新，必须同步更新 `tr-store::schema::REQUIRED_COLUMNS`（只读校验清单），
> 否则打开既有工作空间会误判为"更早格式"。

## 种子工作空间与只读导出

```powershell
cargo xtask seed "$env:TEMP\tr-demo-ws"      # 新建并播种：2 账本 / 19 分类 / 57 标签 /
                                             # 7 条消费记录（含 outlier）/ 关键事件 + 关联 /
                                             # 2 篇日记 / 1 个模板 / 3 个预设图表 + 股票全链路数据
cargo xtask dump "$env:TEMP\tr-demo-ws"      # 只读导出全部业务表为规范化 JSON（列名升序）
cargo xtask dump <dir> --table tbl_billadm_stock_trade
```

`seed` 刻意通过 `tr-service` 的公开函数写入，而不是直接 SQL——种子本身就在跑服务层代码。
它的取值是**写定的基线**：同一份种子反复播种必须得到同样的落库结果，不要随手改常量。

人工冒烟：`cargo xtask seed <dir>` 后把 `~/.transactions-dev.json` 的 `workspaceDir` 指向该目录，
再 `cargo tauri dev`（详见 AGENTS.md 的端口注意事项）。

## `*.ps1`（端到端护栏）

真机启动应用并用 UI Automation 驱动界面，逐个页面验证渲染、写入闭环与各条已知缺陷的回归
（拖拽排序、窗口几何、图片上传、日记导入导出、股票全生命周期等）。
每个脚本的用途与用法见 `AGENTS.md` 的「常用命令」。
