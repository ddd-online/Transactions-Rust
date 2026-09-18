# fixtures —— 数据兼容与行为等价的验证基线

本目录只放**合成**数据与从参考实现导出的结构性基线，**绝不包含任何真实个人财务数据**
（真实工作空间仅在本机、以 gitignore 的方式用于人工冒烟）。

## `schema/fresh_v0_27.sql`

原参考实现（`D:\github\Transactions`，Go 0.27.0）在空目录上打开工作空间后，
由 `sqlite3 transactions.db .schema` 导出的原始 DDL，逐字节复制。

- 内容：19 张表 + 21 个索引 + 3 条迁移登记记录（`tbl_billadm_schema_migration`）
- 用途：Rust 版为**全新工作空间**建库时执行；`cargo xtask schema-diff` 校验
  「Rust 建库结果」与「基线/Go 建库结果」逐条一致
- 纪律：只在 `transactions.db` 不存在时执行一次；打开既有工作空间时绝不执行 DDL/DML

## 重新生成基线（仅在参考实现升级 schema 时）

```powershell
# 1. 用参考实现（Go）建一个空工作空间
$ws = "$env:TEMP\tr-fresh"; Remove-Item $ws -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $ws | Out-Null
cd D:\github\Transactions\kernel
Start-Process go -ArgumentList 'run','main.go','-port','39143','-mode','release','-workspace',$ws -PassThru
# 2. 等健康检查通过后打开工作空间
Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:39143/api/v1/workspace' -ContentType 'application/json' -Body (@{workspaceDir=$ws} | ConvertTo-Json)
# 3. 导出 DDL，覆盖 fixtures/schema/fresh_v0_27.sql，并重新录制黄金 JSON
sqlite3 "$ws\transactions.db" ".schema"
```

> 基线一旦更新，必须同步更新 `tr-store::schema::REQUIRED_COLUMNS`（只读校验清单）
> 与 `cargo xtask schema-diff` 的预期条数，否则打开既有工作空间会误判为"旧格式"。

## 种子工作空间与只读导出

```powershell
cargo xtask seed "$env:TEMP\tr-demo-ws"      # 新建并播种：2 账本 / 19 分类 / 57 标签 /
                                             # 7 条消费记录（含 outlier）/ 关键事件 + 关联 /
                                             # 2 篇日记 / 1 个模板 / 3 个预设图表
cargo xtask dump "$env:TEMP\tr-demo-ws"      # 只读导出全部业务表为规范化 JSON（列名升序）
cargo xtask dump <dir> --table tbl_billadm_stock_trade
```

`seed` 刻意通过 `tr-service` 的公开函数写入，而不是直接 SQL——种子本身就在跑服务层代码。
人工冒烟：`cargo xtask seed <dir>` 后把 `~/.transactions-dev.json` 的 `workspaceDir` 指向该目录，
再 `cargo tauri dev`（详见 AGENTS.md 的端口注意事项）。

## `golden/`（P4 起）

`cargo xtask parity` 的录制产物：在同一份种子工作空间上，通过参考实现的 HTTP API
执行全部 68 个操作并记录归一化后的 JSON（UUID、时间戳、并列排序归一）。
校验模式只跑 Rust 侧并逐条比对，**门槛是 68/68 全绿**。
