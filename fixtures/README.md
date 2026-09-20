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
                                             # 7 条消费记录（含 outlier）/ 事件 + 关联 /
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

## `lib/TrUia.ps1`（共享 UIA 底座）

所有 `fixtures/*.ps1` 共用**同一份**界面自动化底座：`fixtures/lib/TrUia.ps1`。
每个脚本在 `param()` 之后（`$ErrorActionPreference = 'Stop'` 之后）dot-source 它：

```powershell
. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')
```

**新增脚本请 dot-source 它，不要从别的脚本再抄一份**。里面是这些内容：

- 程序集加载（`UIAutomationClient` / `UIAutomationTypes` / `System.Drawing` / `System.Windows.Forms`）与 `$UIA`；
- C# 鼠标/窗口 P/Invoke 类 `TrUia`（`SetCursorPos` / `mouse_event` / `Click` / `SetForegroundWindow` / `ShowWindow`）
  —— 各脚本的 `Add-Type` 里**只留自己独有的方法**（如 `ui-drag` 的 `Move/ButtonDown/ButtonUp`、
  `ui-upload` 的 `SendMessage/PressDefaultButton`、`ui-diary-io` 的 `SetWindowText/GetWindowText`、
  `window-bounds` 的 `GetDpiForSystem/GetDpiForWindow/PostMessage/CloseWindow`、
  `ui-shots`/`dev-shot`/`dev-hot` 的 `GetWindowRect/PrintWindow` 等），公共调用的写法统一是 `[TrUia]::…`；
- 基础查询/激活/取值/等待：`Assert-True`、`Show-TrSummary`、`Get-Elements`、`Find-First`、`Find-All`、`Find-Like`、
  `Find-ElementLike`、`Test-Rect`、`Wait-Element`、`Wait-Like`、`Get-ReadyWindow`、`Set-Value`、`Invoke-Element`、
  `Click-Element`、`Find-RowButton`、`Find-DateTrigger`、`Find-DateCell`、`Save-Screenshot`、`Add-Record`；
- 前导/收尾样板：`Initialize-TrSmokeHome`、`Assert-NoRepoInstance`、`Start-App`、`Stop-TrApp`；
- `Read-Table -Repo <repo> -Workspace <ws> -Table <表名> -OutDir <dir>`
  （**必须显式传参**：早先它靠调用方作用域里的 `$repo`/`$ws`/`$OutDir`，
  正是"断言走文件系统、被测进程走另一个 cwd"那类坑的来源）。

两条使用约束：

1. **它靠 dot-source（不是 `Import-Module`）才能工作**：脚本作用域里的 `$failures` / `$UIA` 要对模块里的函数可见
   （`Assert-True` 就靠动态作用域取调用方的 `$failures`，取不到会**抛错**而不是静默漏计）。
   因此各脚本仍然自己 `$failures = New-Object System.Collections.Generic.List[string]`，这一行不要动。
2. **单独把某个 `.ps1` 拷出仓库将无法运行**（缺 `fixtures/lib/`）；
   同样地，只改 `lib/TrUia.ps1` 就会同时影响所有护栏脚本。

各脚本里刻意保留了**语义不同**的本地同名实现（例如 `ui-crud` 的 `Click-Element` 多了
`SetForegroundWindow` + 窗口矩形校验、`ui-transactions` 的 `Invoke-Element` 多了 `try/catch`、
`ui-upload` 的 `Find-First` 走 `Find-ByName`、`Wait-Element` 的默认超时在不同脚本里是 20/25/30 秒）。
脚本里后定义的同名函数会覆盖模块版，这是刻意的——**不要把较弱变体统一成较强变体**
（那属于行为改变，可能把假红变绿）。
