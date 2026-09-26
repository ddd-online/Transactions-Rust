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

## 测试分层（`test.ps1` 是唯一入口）

| 档位 | 命令 | 含义 |
|---|---|---|
| 单元档 | `pwsh -File fixtures/test.ps1 -Unit <分组>` | 改某个功能时跑的相关测试：`core`（fmt / clippy / design-audit / contract-audit）+ 该功能的包单测与界面护栏；`-Unit changed` 按 git 改动自动挑 |
| 全量档 | `pwsh -File fixtures/test.ps1 -All` | 发布前跑：先构建（trunk + `cargo build --release --features tauri/custom-protocol`），再跑全部静态检查、Rust 单测、schema-diff、chart-tests 与 20 个界面/外壳护栏 |

```powershell
pwsh -File fixtures/test.ps1 -List                       # 分组与步骤的权威清单
pwsh -File fixtures/test.ps1 -Unit diary                 # 单元档
pwsh -File fixtures/test.ps1 -Unit changed -DryRun       # 按改动挑，只看计划
pwsh -File fixtures/test.ps1 -All -SkipBuild -SkipNetwork
```

- 分组、步骤、以及「改动 → 分组」的映射都写在 `fixtures/test.ps1`（`$Groups` / `$Steps` / `$PathMap`）。
  **新增护栏必须同时登记 `$Steps` 与所属 `$Groups`**，否则 `-All` 会漏掉它。
- 目录约定（`target/` 顶层只留给 cargo 自己的目录）：

  ```
  target/tests/<脚本名>/home          一次性 USERPROFILE（应用看到的"用户目录"）
  target/tests/<脚本名>/out           该护栏的产物：截图、seed.log、以及 out/ws 里的测试数据库
  target/tests/_runs/<时间>-<档位>/   每次运行的日志 + summary.md / summary.json
  ```

- 界面护栏一律**自己播种**工作空间：没显式给 `-Workspace` 就先删掉再 `cargo -q xtask seed`（干净基线），
  显式给了就只补齐缺失的库。
- 每个脚本都能单独跑，也都支持 `-Exe` / `-SmokeHome` / `-Workspace` / `-OutDir` 覆盖；默认 `-Exe`
  一律是 `target\release\transactions.exe`（要核验 `build\` 里的打包产物时显式传 `-Exe`）。
- 例外：`chart-tests.ps1` **不驱动界面** —— 它把 `chart.rs` 里 wasm-only 的单元测试抽出来在 native 上跑
  一遍（`cargo test -p tr-ui` 跑不到它们），所以不 dot-source `lib/TrUia.ps1`。

## 加一个新护栏的步骤

1. 脚本放 `fixtures/ui-<功能>.ps1`（界面）或 `fixtures/<外壳 / 审计>.ps1`，开头 dot-source `lib/TrUia.ps1`；
2. 三个默认值按上面的目录约定写：`$SmokeHome = ...\target\tests\<脚本名>\home`、`$OutDir = ...\out`、
   `$Workspace = ...\out\ws`（并保留 `-Exe` / `-SmokeHome` / `-Workspace` / `-OutDir` 覆盖参数）；
3. 前导样板用共享版：`Initialize-TrSmokeHome` → 播种 → `Assert-NoRepoInstance` → `Start-App`，
   收尾用 `Stop-TrApp`（见 `ui-crud.ps1` 的播种段与 `window-bounds.ps1` 的收尾）；
4. 断言落在**库或磁盘**上（`Read-Table` / 文件哈希 / 配置内容），不要只断言"元素存在 / 点到了"；
5. 在 `fixtures/test.ps1` 里加 `$Steps` 条目、登记进 `$Groups`，必要时补 `$PathMap` 规则；
6. `pwsh -File fixtures/test.ps1 -Unit <分组>` 跑一遍，确认它出现在汇总里且是"通过"。

## `lib/TrUia.ps1`（共享 UIA 底座）

所有**驱动界面的** `fixtures/*.ps1`（`chart-tests.ps1` 除外）共用**同一份**界面自动化底座：`fixtures/lib/TrUia.ps1`。
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
