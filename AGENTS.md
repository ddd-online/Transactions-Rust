# AGENTS.md

Transactions 是一款桌面个人记账应用，每个工作空间是一个独立的 SQLite 数据库。
技术栈：Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite 存储，全部为 Rust 代码。

另见：`PRODUCT.md`（产品定位）、`DESIGN.md`（设计系统，UI 的裁决标准）、
`fixtures/README.md`（基线文件与 `lib/TrUia.ps1` 的说明）。

## 架构

```
crates/tr-domain/    # 纯领域层：models / dto / 金额换算 / 费用分摊（native + wasm 双可编，无 I/O）
crates/tr-store/     # 存储层：建库 + 迁移引擎（migrations）+ 格式校验 + 各 Dao（rusqlite）
crates/tr-service/   # 服务层：业务规则（账本/交易/…/股票），不依赖 tauri
crates/tr-ipc/       # IPC 命令面：全部 #[tauri::command] + 统一错误信封
crates/tr-ui/        # 界面：Leptos CSR（cdylib，只编 wasm32）+ static/{css,fonts,icons}
src-tauri/           # 桌面外壳：窗口/托盘/配置/日志/trasset:// 资产协议/更新
xtask/               # 验证工具：schema-diff（建库护栏）、validate / dump / seed（工作空间工具）
fixtures/            # schema 基线（fresh.sql）+ 种子与端到端脚本（**不含真实个人数据**）
```

分层纪律：

- `tr-domain` 不得引入任何 I/O 依赖（rusqlite、reqwest、tauri 都不行）：它同时被 native 与 wasm 依赖，
  界面和后端因此共用同一份金额与费用算法。
- 只有 `tr-ipc` 依赖 `tauri`。业务规则必须能在没有窗口的环境里用 `cargo test` 验证。
- `tr-store` / `tr-service` 不参与 wasm 编译（`cfg(not(target_arch = "wasm32"))`）。

## 常用命令

```powershell
# 类型检查 / 测试（不含桌面外壳）
cargo check -p tr-domain -p tr-store -p tr-service -p xtask --all-targets
cargo test  -p tr-domain -p tr-store -p tr-service
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# 界面与外壳（发布链路必须走脚本，原因见「构建与工具链」）
trunk serve --config crates/tr-ui/Trunk.toml      # 开发服务 http://127.0.0.1:16000
cargo tauri dev                                   # 开发（会自动先跑 trunk）
cargo tauri build                                 # 打包，产出 NSIS 安装包

# 工作空间工具（validate / dump 只读）
cargo xtask schema-diff                           # Rust 建库结构与 fixtures/schema/fresh.sql 逐条一致
cargo xtask validate <workspace-dir>              # 只读校验格式（会报告待应用的迁移）
cargo xtask migrate <workspace-dir>               # 升级到当前格式（升级前自动备份）
cargo xtask seed <workspace-dir>                  # 新建并播种一份示例数据（人工冒烟用）
cargo xtask dump <workspace-dir> [--table <name>] # 只读导出业务表为规范化 JSON（排障/回归对比）

# 护栏（前两个不启动界面；下面每个都可以加 -Workspace <ws> / -Exe <exe> / -OutDir <dir>）
pwsh -File fixtures/design-audit.ps1              # 设计令牌
pwsh -File fixtures/contract-audit.ps1            # 界面 api/*.rs 请求结构体 vs tr-ipc
pwsh -File fixtures/smoke.ps1
pwsh -File fixtures/ui-smoke.ps1  [-WriteFlow] [-Discover]
pwsh -File fixtures/ui-shots.ps1
pwsh -File fixtures/chart-tests.ps1               # 图表纯函数（Y 轴范围/填充基线）真跑一遍（tr-ui 是 wasm-only）
pwsh -File fixtures/window-bounds.ps1
pwsh -File fixtures/close-behavior.ps1
pwsh -File fixtures/migrate-workspace.ps1
pwsh -File fixtures/ui-crud.ps1
pwsh -File fixtures/ui-transactions.ps1
pwsh -File fixtures/ui-stock.ps1
pwsh -File fixtures/ui-drag.ps1
pwsh -File fixtures/ui-upload.ps1
pwsh -File fixtures/ui-sync-ledger.ps1
pwsh -File fixtures/ui-key-event.ps1
pwsh -File fixtures/ui-link-event.ps1
pwsh -File fixtures/ui-diary-edit.ps1
pwsh -File fixtures/ui-diary-ledger.ps1
pwsh -File fixtures/ui-diary-io.ps1
pwsh -File fixtures/ui-proxy.ps1
pwsh -File fixtures/ui-about.ps1
pwsh -File fixtures/ui-features.ps1
pwsh -File fixtures/ui-update-restore.ps1
```

护栏各自管什么（一律真的启动应用、用 UI Automation 或真实鼠标键盘驱动；**断言落在库/磁盘上**，
不落在"点到了没有"）：

- `design-audit`：`tokens.css` 之外不得有硬编码颜色、引用的令牌必须已定义、深色主题与
  `prefers-color-scheme` 兜底必须覆盖同一组令牌。
- `contract-audit`：界面 `api/*.rs` 的请求结构体是手抄 `tr-ipc` 的（两侧不共用类型，见「JSON 字段命名」），
  字段名抄错**不会编译报错**、serde 静默取默认值，所以按命令逐字段比对。
- `smoke`：已配置工作空间 → 只有主窗口；首启动 → 只有初始化窗口；并要求日志里有 IPC `config_get`。
- `ui-smoke`：逐个点开 5 个顶级功能 + 9 个子功能（记账 4：记录/分析/标签/模板；股票 5：账户/持仓/记录/统计/设置）
  并断言内容渲染；`-WriteFlow` 还在工作空间副本里通过界面记一笔；`-Discover` 导出每页元素清单
  （用来维护脚本顶部的页面标记表）。
- `ui-shots`：抓窗口位图断言每页非空白 + 比较浅/深色平均亮度，14 张 PNG 落 `target\ui-shots\` 供人工验收。
  补 UIA 的盲区（UIA 看不见"被裁掉/没画出来"）。
- `ui-drag`：真实鼠标（按下 → 20 段移动 → 抬起）把第 1 个分类拖到第 3 位，断言顺序变化 + `sort_order`
  落库 + 界面同步 + 重进页面仍一致。
- `window-bounds`：逻辑尺寸启动 → 物理窗口 = 逻辑 × DPI → 关闭写回仍是逻辑值 → 再启动尺寸/位置一致。
- `close-behavior`：`quit` → 进程退出；`tray` → 进程存活且窗口隐藏；空 → 弹「关闭选项」框（原生框的按钮在
  UIA 里是 `是(Y)`/`否(N)`、Pane 类型），选「是」后退出。
- `ui-upload`：点「添加图片」拉起原生文件框 → 选一张自己生成的 600×400 PNG → 断言原图按原字节落盘 +
  缩略图缩到 300×200 同目录同名前缀 + 库里 `file_path`/`thumb_path`/`event_date` + 界面出现「下载图片」。
- `ui-diary-io`：驱动原生选目录框导入临时目录（UTF-8 + GBK + 非法文件名 + `.md`）→ 断言正文逐字节落库、
  `word_count` 按标量值、非法文件名与**非 `.txt`** 都跳过；再导出到空目录断言文件数/命名（`<日期>.txt`）/
  正文一致且没有 `.md`。日记导入导出**只支持 `.txt`**（正文是纯文本），判据唯一：
  `crates/tr-service/src/diary.rs` 的 `parse_diary_file_name`（扫描与 `import_file` 两侧共用）。
- `ui-diary-edit`：进日记页（首屏是预览态，先点页脚「编辑」）→ 写内容 → `Ctrl+S`（无保存按钮，靠 `input`
  后 1500ms 防抖自动保存）→ 断言正文/字数/心情落库 → 点心情「开心」断言 `mood=😊` 且 id 不变（同天 upsert）
  →「预览」里 Markdown 渲染出标题 → 删除。
- `ui-diary-ledger`：当前账本写一篇 → 切到另一个账本（编辑器里看不见它）→ 同一天写不同内容 → 断言库里两行、
  分属两个账本（复合唯一键回归）→ 切回来正文原样。
- `ui-crud`：分类「新增 → 删除」、标签、图表、事件「点色板改颜色 / 写 Markdown 描述 / 删除」、
  记账·模板「新建模板 → 删除」，断言都落库。
- `ui-sync-ledger`：记一笔 → 行内「同步到其他账本」→ 选目标账本 → 断言目标账本多一份新 id 副本、
  金额/类型/分类/记录时间一致、源记录保留（复制而非移动）→ 切账本后界面能看到副本。
- `ui-transactions`：记三笔 → 编辑一笔（断言「先建后删」：换 `transaction_id`、旧记录消失、行数不变）→
  保存为模板（记一笔弹窗里的子弹窗）→ 排序（重置 → 加「金额 降序」→ 应用 → 断言金额非递增、最大值第一）→
  筛选（悬浮按钮 → 关键词 → 添加条件 → 确认 → 收敛到 1 条）。
- `ui-stock`：建仓（真实行情查名）→ 编辑成交 → 删除委托 → 减仓 → 清仓 → 费用设置 → 统计 → 操作记录与回滚
  → 重置股票数据。断言：成交（价按分/手数/股数/成交额/手续费）→ 持仓（数量、成本 = 成交额 + 手续费；
  减仓按比例结转 `cost_basis = round(total_cost × 本次股数 / 持仓股数)`、`realized_pnl = amount - fee - cost_basis`）
  → 资金记录（余额链、买入 -(成交额+手续费)、卖出 +(成交额-手续费)）→ 清仓归档（新轮次 + 回填成交的
  `round_id`）→ 统计分栏页签（`TabItem`；工具栏左侧「统计 / 明细」，切「明细」要出现逐笔结算明细表 +
  页脚「共 N 条」+ 分页控件）→ 操作记录与回滚（「追加本金」→「查看记录」要出现它 →「回滚 → 确认回滚」
  把本金与资金记录还原；这一步净效果为零）→ 重置（不动记账数据）。
- `ui-key-event`：行内「新建」→ DatePicker 选一个**不是今天**的日期 → 断言事件落在那天 → 同一天再建一次断言
  upsert（一条、id 保留、标题被覆盖）→ 行内「删除事件」→ 库里清空。（`ui-crud` 只验了默认日期。）
- `ui-link-event`：记一笔 → 行内「关联到事件」→ 弹窗里用日期选择器选**不是今天**的日子（`link_date` 默认今天，
  选今天等于没测选择器）→ 断言触发器显示该日期 + `key_event_date` 落库 + 那天懒创建了一条空事件 →
  「修改关联」→「解除关联」→ 断言 `key_event_date` 清空。
- `migrate-workspace`：复制已播种工作空间 → 降级成旧格式（日记去掉 `ledger_id`、恢复旧的全工作空间唯一索引、
  抹掉登记行）→ 启动真实外壳 → 断言能正常打开、结构升级到位、老日记正文一字不差并归到最早账本、
  升级前留了备份（且备份里是旧结构）、再启动不重复备份（幂等）。**迁移引擎这条最高风险路径就靠它。**
- `ui-proxy`：起本机假 HTTP 代理当判据 → 界面「通用设置 → 代理」手动指向它 → 断言行情（`qt.gtimg.cn`）与
  更新检查（`api.github.com`）都出现在假代理日志里、行情名显示假代理返回的名字 → 切「不使用」→
  断言假代理一条都收不到。（只断言配置文件写了什么 = 什么都没验。）
- `ui-features`：「应用设置 → 功能开关」端到端：关掉「日记」→ 断言**侧栏当场少一项** + 配置文件
  `features.diary=false`（其余开关不受影响）→ 开回来 → 关掉再**重启**，断言侧栏仍没有它（证明真读了配置）。
  开关是 `<button role="switch">`（UIA 带 `TogglePattern`），侧栏条目是普通按钮 —— 两边可访问名都可能叫「日记」，
  所以定位不能只看名字。
- `ui-update-restore`：「关于软件」的**下载状态跨页面恢复**（下载是外壳侧单例任务，界面切走不中断）：
  下载中切到「记账」再切回来，断言界面自己恢复了下载态（还在下载 → 「取消下载」；已下完 → 「安装并退出」），
  并验证取消后回到「立即更新」。⚠ 依赖真实 GitHub API：已是最新版时只验证检查链路，网络不通会明确报红。

工具链要求：rust stable 1.96.0 + `wasm32-unknown-unknown` target。仓库故意不放 `rust-toolchain.toml`
（指定具体版本会让 rustup 每次调用都校验并重装组件，实测触发数百 MB 重复下载），版本写文档里就够。

## 改界面的迭代循环

改 `crates/tr-ui` 下的 `.rs` / `.css` 走 dev 服务 + 热更新（一轮 5~10 秒）。`build/build-ui.ps1` +
`cargo build --release` 是最终验收/发布用的（实测约 4 分钟，其中 release 链接 2m40s），迭代期别用。

1. 起一次（之后一直开着）：`pwsh -File fixtures/dev-hot.ps1 -Trunk -Launch -Workspace <ws> -ShotDir target\dev-shots`
   它拉起 `trunk serve`（:16000）+ dev 外壳（`target\debug`，走 `devUrl`）+ 盯 trunk 日志；trunk 重建完就给窗口
   发 `Ctrl+R`（trunk 的自动刷新信号浏览器吃、Tauri 的 WebView2 不吃，但 WebView2 吃键盘刷新，实测 0.8s）；
   带 `-ShotDir` 时把刷新后的窗口存成 `target\dev-shots\current.png`。
2. 只看某页/全部页面（不重建、不重启）：`pwsh -File fixtures/dev-shot.ps1 -Page 股票` 或 `-AllPages`。
   它用真实鼠标点侧栏并**轮询确认页面真切过去了**再截图。
3. 需要「内嵌界面的 release 产物」（端到端护栏、发布）才做完整构建：
   `powershell -NoProfile -ExecutionPolicy Bypass -File build/build-ui.ps1` 然后
   `cargo build --release -p transactions --features tauri/custom-protocol`。

边界：改 `src-tauri/`（外壳）不适用热更新，要 `cargo tauri dev` 重启；`-Launch` / `-Trunk` 用独立配置目录
（`target\smoke\home-hot`），不碰真实的 `~/.transactions-dev.json`。**不给 `-Workspace` 就沿用配置里已有的
工作空间**（早期版本会把它写空 → 外壳进首启动流程、只开 600×560 初始化窗口，主循环要的侧栏「记账」永远不出现，
最后只看到一句"没找到主窗口"）。`NO_COLOR` 由脚本自己清掉。

## 只跑受影响的测试（默认）

**默认不跑全量**（十几个 fixture 全跑要十几分钟，换不到新信息）：按改动范围挑，只有大改动才全跑。

| 改了哪里 | 跑哪个脚本 |
|---|---|
| 记账 · 记录（记一笔 / 编辑 / 排序 / 筛选 / 模板） | `ui-transactions` |
| 记账 · 分析 / 标签 / 模板 | `ui-crud` |
| 记账 · 标签拖拽排序 | `ui-drag` |
| 事件页 | `ui-key-event`；涉及关联交易卡 → `ui-link-event` |
| 日记页 | `ui-diary-edit`、`ui-diary-ledger`；导入导出 → `ui-diary-io` |
| 股票页（任意子功能 / 费用 / 标签 / 下单 / 清仓） | `ui-stock` |
| 应用设置（通用 / 功能开关 / 日记配置 / 关于软件） | `ui-proxy`（代理）、`ui-about`（关于）、`ui-features`（开关） |
| 图片上传 / 资产协议 | `ui-upload` |
| 侧栏 / 外壳 / 窗口（窗口几何、关闭行为、首启动） | `window-bounds`、`close-behavior`、`smoke` |
| 建库 / 迁移 / schema | `cargo xtask schema-diff`、`migrate-workspace`、`cargo test -p tr-store` |

- **共享代码会扩大范围**：`components/ui/` 下的组件（`Modal`、`Popconfirm`、`Button`、`Table`、`chart`…）、
  `shell.rs`、`store.rs`、`fixtures/lib/TrUia.ps1` —— 改这些至少跑覆盖到的 2~3 个脚本（例：改 `Modal`
  宽度档位 → `ui-transactions` / `ui-stock` / `ui-smoke` / `ui-crud` / `ui-key-event`）。
- **连带效果算进影响面**：一个 fixture 里的检查可能由**别处**触发（如 `ui-upload` 最后一步要删事件，
  所以改「删除事件」就得带上它）。
- **什么时候才跑全量**：① 动 `fixtures/lib/TrUia.ps1` / 公共组件 / 外壳；② 一次改动跨 3 个以上页面；
  ③ 合并或发布前；④ 长时间没跑过（不确定基线还绿不绿）。
- **每次必跑的三条便宜的**：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、
  `pwsh -File fixtures/design-audit.ps1`（改界面再加 `cargo check -p tr-ui --target wasm32-unknown-unknown`）。
  单测按包挑（`cargo test -p tr-store` 之类），不必每次跑全部包。改 `components/ui/chart.rs` 的纯函数
  （Y 轴范围 / 填充基线）时加跑 `pwsh -File fixtures/chart-tests.ps1`。

## 本机环境注意事项（踩过的坑）

### 构建与工具链

- **cargo 不读 Windows 系统代理**（PowerShell/浏览器走系统代理，cargo 只认环境变量）：本机直连 crates.io 会超时，
  先设 `$env:HTTPS_PROXY='http://127.0.0.1:7890'`、`$env:HTTP_PROXY='http://127.0.0.1:7890'`、
  `$env:CARGO_NET_RETRY='10'`、`$env:CARGO_HTTP_TIMEOUT='120'`；索引拉取偶发中断就重复跑 `cargo metadata`
  （已缓存条目会累积）。
- **不要用 PowerShell 管道接 cargo 输出**（`cargo … | Select-Object -Last N`）：管道缓冲写满会让 cargo 阻塞假死。
  一律 `*> 文件`。
- **`NO_COLOR=1` 会让 trunk 直接报错**（`invalid value '1' for '--no-color'`）：调 trunk 前 `Remove-Item Env:NO_COLOR`。
- **开发端口是 16000，且 Windows 保留段会漂**：保留段（WinNAT/Hyper-V）归管理员，落在里面的端口非提权进程绑不上
  （`os error 10013`）。历史上 1420 → 1520 → 1600 先后被吞。起不来先跑
  `netsh interface ipv4 show excludedportrange protocol=tcp`，把端口挪到段外；选端口时**同时避开动态段**
  （`netsh int ipv4 show dynamicport tcp`，本机 1024-15000）更省事。**换端口必须同时改三处**：
  `crates/tr-ui/Trunk.toml` 的 `[serve] port`、`src-tauri/tauri.conf.json` 的 `devUrl`、
  `fixtures/dev-hot.ps1 -Port`（`shell.rs` 的 `is_allowed_navigation` 单测里还有两条 localhost 样例）。
  `dev-hot.ps1` 会先试绑该端口，几秒内报"端口不可用"而不是干等 240 秒。
- **`devUrl` 是编译期读进 `tauri.conf.json` 的**：改了它就必须重编 dev 外壳（`cargo build -p transactions`），
  否则窗口里是 `ERR_CONNECTION_REFUSED`（trunk 其实服务得好好的），看着像界面代码坏了。`dev-hot.ps1`
  会做时间戳比对并直接 throw。
- **trunk 下不了 wasm-bindgen**（GitHub release 资产在本机网络 TLS 失败）：改从 crates.io 装同版本 CLI
  `cargo install wasm-bindgen-cli --version 0.2.128 --locked`（版本必须与 `wasm-bindgen` crate 一致，当前 0.2.128）。
- **不要用 `trunk build --release`**：它调用缓存里的 wasm-opt 123，而该版本校验不了 Rust 1.96 的 bulk-memory 指令
  （`memory.copy … requires --enable-bulk-memory-opt`），更新版本又下不下来。发布构建统一走 `build/build-ui.ps1`：
  跑 trunk 的 debug 模式（跳过 wasm-opt）+ `CARGO_PROFILE_DEV_OPT_LEVEL=3` + `CARGO_PROFILE_DEV_DEBUG=false`
  把优化拉满、去掉调试信息（产物 wasm 7.3 MiB；不优化带调试约 15 MB）。`tauri.conf.json` 的 `beforeBuildCommand`
  已指向该脚本。
- **`build/*.ps1` 被 `powershell`(5.1) 调用时必须 ASCII-only**：5.1 把无 BOM 的 UTF-8 当 ANSI 解码，中文会破坏
  脚本解析（`build-ui.ps1` 因此全英文注释，它确实被 `cargo tauri build` 用 5.1 调用，不能改）。
  `build.ps1` / `release.ps1` 含中文注释，约定用 pwsh 7；它们开头有一段 ASCII-only 守卫：检测到
  `PSEdition -ne 'Core'` 就用 `pwsh` 重跑自己（`$PSCommandPath` + `@args`）。
  加守卫是因为一次事故：5.1 跑 `build.ps1` 时中文行被误解析、最后一步的 `$appExe` 变成 `$null`，便携版 exe
  没留档而退出码仍是 0。**构建脚本的最后一步也要有产物断言**（`Test-Path` + `Fail`），别只看退出码。
- **手跑 exe 必须带 `custom-protocol`**：Tauri 只在启用 `tauri/custom-protocol` 时内嵌界面资源；debug 构建则走
  `devUrl`。漏掉时窗口里是 Chromium 的 `127.0.0.1 拒绝连接`（ERR_CONNECTION_REFUSED），外壳日志却只有
  "启动/外壳初始化完成"，看着像界面代码坏了。三种正确做法：`cargo tauri build`、
  `cargo build --release -p transactions --features tauri/custom-protocol`（冒烟用，`smoke.ps1` / `ui-smoke.ps1`
  要的就是这种内嵌产物）、或起 `trunk serve` 后跑 debug 构建（等价 `cargo tauri dev`）。
- **别在 `trunk build` 写 dist 的同时跑 `cargo build`**：Tauri 编译期读 `crates/tr-ui/dist` 做资源内嵌，
  并发时可能嵌到半新半旧的资源（同样是空白窗口）。顺序固定：`build/build-ui.ps1` 先跑完，再 `cargo build`。
- **配置目录可被 `USERPROFILE` 覆盖**：`config.rs` 的 `home_dir()` 先读 `USERPROFILE`（非 Windows 读 `HOME`）。
  `smoke.ps1` 就靠这个把实例指向一次性配置目录，不碰真实的 `~/.transactions.json`。
- **别按进程名判断"应用是否在运行"**：本机别的目录可能也装着同名 `Transactions.exe`（如
  `D:\software\Transactions\`）。判重必须按完整路径，否则会误判、甚至误杀用户正在用的应用。

### UI 自动化（fixtures 的踩坑）

四条通用纪律（下面各条都是它们的实例）：

1. **一律轮询到标记出现，不要固定 sleep 后取元素**：Chromium 的 UIA 树是**惰性构建**的（窗口刚出现时首查常只
   返回二十来个元素、连 Button 都没有）；固定 sleep 还会抓到上一页/上一个弹窗。
2. **同名元素要按"可见 + 落在窗口矩形内 + 取最后一个/最靠右"筛**：页面里常有多个同名按钮（未展开的面板、
   别的页签也在 DOM 里、重渲染留下的幽灵节点），按名字取第一个常拿到不可见的那个，点了毫无反应。
3. **`BoundingRectangle` 报 ±∞ 的是幽灵**（没真正渲染出来的元素 `Width/Height=-∞`，`IsOffscreen` 却可能是
   `False`）。判据要显式排除非有限值（`[double]::IsInfinity/IsNaN` 四个分量都查一遍）——`Width -le 0`
   挡得住 `-∞`，但 `+∞` 参与 `[int]` 转换会抛"无法将值 "∞" 转换为类型 "System.Int32""。
   浮层（弹窗/气泡是 portal 出去的节点）可见性不稳，**别拿 `IsOffscreen` 当判据**：要么"按名字取可见且在
   窗口内的最后一个"，要么只排除 ±∞ 幽灵。
4. **启动时抓「主窗口」而不是「该进程的第一个窗口」**：启动期先出现初始化窗口（600×560、无侧栏），随后才切
   主窗口；抓到前者后面所有按名字查找都会落空。见 `fixtures/ui-crud.ps1` 的 `Get-ReadyWindow`：轮询取窗口元素
   直到它包含侧栏条目（如「记账」）为止。

**对话框、路径、cwd**

- **原生对话框的窗口归属分三种**（找错地方就表现为"点了按钮但没弹框"）：
  * WebView2 自己的文件框（`<input type=file>`，`#32770`、标题「打开」，见 `ui-upload.ps1`）：它是应用窗口的
    **子窗口**、`ProcessId` 属于 `msedgewebview2.exe` → 在应用窗口元素的 `TreeScope::Descendants` 里按
    `ClassName='#32770'` 找。
  * Tauri `dialog_open` 插件的选文件/选目录框（见 `ui-diary-io.ps1`）：**桌面顶层窗口**、`ProcessId` 是应用
    自己 → 在 `RootElement` 的 Children 里按进程号找。
  * DevTools 窗口：也是桌面顶层，但 `ProcessId` 属于 `msedgewebview2.exe`（`cls=Chrome_WidgetWin_1`、
    `name='DevTools - …'`），按应用进程号枚举永远看不到，要扫全系统顶层窗口按名字/类名筛。
  两类框的路径输入框 `AutomationId` 不同：选文件 = 1148（`文件名(N):` 组合框）、选目录 = 1152（`文件夹(F):`
  编辑框）。选文件时回车 = 「打开」；选目录时回车只是进入该目录，必须点「选择文件夹」，且只接受已存在的目录。
- **填进原生对话框的路径必须是绝对路径**：选目录框按自己的"当前目录"解析相对路径（传 `-OutDir target\pkg-diary`
  就落到了别处）；同理，被启动进程按**它自己的 cwd** 解析 `-workspace` 这类参数。所有 fixture 入口都做
  `GetFullPath` 归一化（`-OutDir` 在内），`Select-Directory` 里另有 `IsPathRooted` 断言兜底，
  调 `xtask dump` 时把 cargo 的 stderr 收进异常（原来写 `2>$null`，只剩一句"dump 失败"）。
  **断言红了先确认两侧说的是同一个库。**
- **选目录的正确姿势**（`ui-diary-io.ps1` 的 `Select-Directory`）：① 把绝对路径**粘贴**进底部「文件夹(F):」框
  （共享版 `Set-Clipboard` + `Ctrl+A` + `Ctrl+V`；别逐字符 `SendKeys`，中文输入法会把 `\` 变 `、`）；
  ② 回车进目录（不要用地址栏 Alt+D），再点「选择文件夹」——它受"记住的上次目录"影响，粘贴偶尔不生效就停在
  上次那个目录，"选错了却看不出错"，所以要③ 验证导航到位（面包屑每段是独立元素，出现目标目录名才算到位），
  没到位就重试；④ 断言看结果（文件落在哪、库里多了哪几行），别只看"对话框关了没有"。
- **WebView2 文件框的提交用「右方向键 + 回车」**：直接回车 = 接受 shell 自动补全（完整路径被换成裸文件名 →
  「找不到文件」）；`TAB` 收起补全但焦点移到"文件类型"，回车不再触发默认按钮；`WM_COMMAND(IDOK)` 会关掉框却
  跳过 modern 对话框的内部步骤，等同取消。也别用 UIA 坐标点「打开(O)」（底部那行是 legacy provider，
  报出的矩形不可信：「打开」的 rect 恰好等于对话框下边缘），更不能按 `AutomationId` 找控件（文件列表项的
  AutomationId 恰好是 1..N，`id=1` 会点中列表第一项）。临时 HOME 里要先建 `Desktop` 目录，否则文件框起始
  目录不存在，会先弹「位置不可用」。

**UIA 树的读法**

- **确认/保存键按"类名"定位，别按名字取最后一个**：三类按钮的**可访问名都含「删除」**（曾经让 `ui-crud` 的
  分类/标签/图表/事件/模板删除断言整段连坐变红）：

  | 位置 | 类名 |
  |---|---|
  | 行内图标按钮（列表行右侧） | `ui-icon-btn--danger` |
  | 弹窗底栏确认键（`ok_danger=true`） | `ui-btn--primary-danger` |
  | 气泡（`Popconfirm`）确认键 | `ui-btn--primary` |
  | 弹窗**自绘**底栏的按钮（如只读弹窗的「关闭」） | `ui-btn--secondary` |

  `ui-crud.ps1` 用 `Find-ConfirmButton -ClassPart <类名>`（+ 可见、矩形有效、取最靠下/最靠右），并用
  `Wait-ConfirmButton` 轮询等它出现（点完行内按钮只 sleep 一次就查会偶发查不到）。
  只断言"页面存在叫『删除』的元素"是无效断言：行内那颗按钮就叫这个名字，条件恒真。
- **别按名字点「关闭」**：外壳窗口三键里那颗**关闭窗口**的按钮 `aria-label="关闭"`（class `window-btn`），
  弹窗的 × 也叫「关闭」。`Wait-VisibleButton -Name '关闭'` 很容易点到窗口三键上 → **应用当场退出**，
  后面每一步都拿不到窗口矩形（表现是 `New-Object Bitmap` 报 "Parameter is not valid"，看着像截图代码坏了）。
  界面按钮的类名一律以 `ui-btn` 开头，`ui-stock.ps1` 的 `Find-VisibleButton` 因此有 `-ClassPart`；
  点任何叫「关闭」的按钮都必须带上 `-ClassPart 'ui-btn'`。
- **图标只渲染 `<path>`，且路径坐标必须落在 `view_box` 内**：`icons.rs` 的 `icon()` 输出内联
  `<svg><path d=…/></svg>`（`fill: currentColor`），没有 `<circle>`/`<rect>`。新增图标两件事一起核：
  ① 路径坐标范围要与 `view_box()` 匹配（历史事故：24 格线稿套了 `64 64 896 896`，箭头整体被裁到看不见）；
  ② 别因为"它是 Ant Design 图标"就放心 —— 有的旧条目路径本身越界（实测 `Icon::Sync` 的 x 到 1133，
  而 896 格只到 960，在股票页图标条上画成一条扭曲的线）。取新图标用官方包：
  `curl -sSL https://cdn.jsdelivr.net/npm/@ant-design/icons-svg@4.4.2/inline-namespaced-svg/outlined/<name>.svg`
  （本机走代理；GitHub 原仓库连不上），它给的 `d` 可与 `view_box = 64 64 896 896` 直接配对。
- **翻译/新增 UI 文案前先看有没有同名元素**：图标条按钮的可访问名 = 子功能名，容易与页面标题、侧栏条目重名
  （「记录」两边都有、「设置」与侧栏「应用设置」相邻）。fixtures 用 `Invoke-SubFunction`（同名 Button 里
  最靠左的那个）定位。
- **`Input` 的 UIA 判据是 `ControlType.Edit`，不是 ClassName**：本项目的输入框渲染成
  `class='ui-input__control'`，用 `ClassName -eq 'Edit'` 一个都找不到（表现为"建仓弹窗里找不到股票代码输入框"）。
  只有原生 `<textarea>` 那类才是 `ClassName='Edit'`，所以判据写成"`ControlType` 是 Edit **或** ClassName 是 Edit"。
- **页签是 `TabItem`**（用 `SelectionItemPattern`），不是 Button，按 Button 找只会静默失败。
- **股票下单弹窗的三个输入框在 UIA 里没有名字**，只能按包围盒定位 `Edit`（同一列 Y 递增、同一行 X 分列），
  再靠 `ValuePattern` 塞值。

**写值与点击**

- **受控 `<input>` / 文本域要把值真的"打"进去**：`ValuePattern.SetValue` 不保证触发 DOM `input` 事件
  （日记/记账的自动保存挂在 `input` 上），Leptos 的 `<input value=signal>` 会在下一次重渲染时把 DOM 值刷回
  信号里的空串 —— 字段看着被填过、点保存却弹「请输入金额」，而 `Set-Value` 还返回 `true`。纯 `Ctrl+V` 也不行：
  窗口不是前台时 `AutomationElement.SetFocus()` 静默无效，"内容没改、保存却成功了"，看着像假绿/假红。
  共享版做法（新脚本直接用，别再各写一份）：`Set-InputByPaste`（单行，`Find-EditByName` 只认
  `ControlType`/`ClassName` 是 Edit 的节点）与 `Paste-Text` / `Set-DiaryContent`（文本域）——
  先 `SetForegroundWindow` → `SetFocus` → 校验 `FocusedElement` 是 Edit → 粘贴 → 用 `ValuePattern` 读回校验
  → 失败整段重试（最多 3 次）。
- **只读的"行内操作按钮"必须先 hover**（`ui-crud.ps1` 的 `Find-RowButton`）：分类/标签行的操作区是
  `.ct-item-actions { display: none }`，只在 `:hover` 或 `.is-active` 时 `display: flex`（刻意的交互约定）。
  `display: none` 的元素不进 UIA 树，所以"新建的那行能删、别的行删不了"。做法：真实鼠标移到行中心、等 ~0.5s
  再按名字查，并用"中心 Y 最近且在该行右侧"区分同一列里的多个「删除」。
- **别用 `| Select-Object -First N` 截断界面脚本的输出**：管道提前关闭会终止上游脚本，它的 `finally`
  （关掉测试实例）不执行，下一个脚本就会因"本仓库已有实例在运行"而拒绝启动。要么 `-Last N`，要么 `*> 文件`。
- **「是否复用工作空间」的判断要在默认值赋值之前取**：写成 `if (-not $Workspace) { $Workspace = <默认> }` 之后
  再判 `if (-not $Workspace) { 重新播种 }` 是恒 false → 永远不重新播种，断言"持仓数量"这类绝对状态时会叠加
  （实测 300 股变 600 股）。做法：函数开头先 `$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)`，
  不用显式工作空间时就 `if (-not $explicitWorkspace) { Remove-Item $ws -Recurse -Force }`。
  `ui-crud` 中过这条：残留的 `UIA分类*` 越积越多，下拉里出现多个**同名**分类后脚本点到陈旧节点，
  表单里分类其实是空的 → 保存被「分类不能为空」拦住（断言 `库里出现新模板` 变红）。
  **"脏基线"会让断言以完全无关的面目变红** —— 看到奇怪的连坐失败，先确认工作空间是不是新的。
- **托盘图标能抓到，但浮出面板太"脆"，所以没做成常驻护栏**：Win11 下图标在
  `TopLevelWindowForOverflowXamlIsland`（「系统托盘溢出窗口」）里、是个 `Button`（`Name='Transactions'`），
  点任务栏「显示隐藏的图标」能弹出来、右键开 `#32768` 菜单；但实测两次里有一次面板没弹出来（失焦即关、
  出现时机不固定），做成 fixture 会红绿随机。探针在 `target/tray2-probe.ps1`（不提交）。
- **股票下单 + 行情链路的验收配方**（一条流程验四件事）：持仓页 → `建仓` 弹窗 → 代码填 `600519` →
  点「查询股票名称」，名称框自动变成「贵州茅台」（这一步打通真实行情接口）→ 填价格/手数 → 提交。
  随后断言：页面行显示现价/涨跌幅/浮盈、`tbl_billadm_stock_trade` 多一笔、持仓数量与成本对得上
  （成本含手续费：`价格×股数 + 佣金`，单位是分）。

### 界面与外壳的真实缺陷

- **Tauri 默认会吃掉页内 HTML5 拖拽**（"分类/标签/模板拖不动"）：窗口默认 `dragDropEnabled: true`，wry 于是在
  WebView2 宿主 HWND 上 `RegisterDragDrop` 并 `SetAllowExternalDrop(false)`
  （`wry-0.55.1/src/webview2/mod.rs:150`）；而 Chromium 在 Windows 上的页内拖拽也走 OLE 拖放，`drop` 永远到不了
  页面。现象很有欺骗性：`dragstart`/`dragover` 正常（行变半透明、插入指示线也画），只有 `drop` 不触发。
  修法：建窗口时 `.disable_drag_drop_handler()`（`shell.rs` 的 `create_main_window` / `create_init_window`）。
  代价：拿不到 `tauri://drag-drop` 原生文件落盘事件。回归 `fixtures/ui-drag.ps1`。
- **关掉拖放处理器之后必须补导航守卫**（`shell.rs` 的 `is_allowed_navigation`）：拖放处理器同时也"吃"掉了拖入
  文件时的默认导航，之后把文件从资源管理器拖进窗口会让 WebView2 直接导航到 `file:///…`、界面整个被换掉。
  守卫只放行 `tauri.localhost` / `localhost` / `127.0.0.1`，其余（file:、外部站点、data:）一律拦截；
  `trasset://` 是子资源、不走导航，不受影响。单测覆盖放行/拦截两侧。
- **窗口几何的单位是逻辑像素（DIP）**（"记不住窗口大小和位置"）：Windows 上 `window.inner_size()` /
  `outer_position()` 返回**物理**像素，而 `WebviewWindowBuilder::inner_size()` / `position()` 收**逻辑**像素，
  配置（`~/.transactions.json`）里存的也必须是逻辑值；存成物理值会让 150% 缩放的机器每次启动放大 1.5 倍、
  位置越跑越偏。`save_window_bounds` 现在做物理 → 逻辑换算（纯函数 `logical_bounds` + 4 个单测），
  回归 `fixtures/window-bounds.ps1`。
- **`Popconfirm` 的触发必须挂捕获阶段**：调用方常在子元素上写 `stop_propagation()`（列表项里的删除按钮为了不
  触发整行"选中"），挂冒泡阶段会被吃掉、气泡不弹 —— 「删除图表」「删除事件」「删除关联交易」三处都因此点不动。
  `popconfirm.rs` 现在用 `on:click:capture`。改共享组件后用 `fixtures/ui-smoke.ps1` 回归。
  注：「删除事件」已改成**弹窗**（`key_event.rs` 的 `confirm_delete_modal`），不再是这条的调用点；仍在用气泡的
  是「删除图表」与「删除关联交易」。改卡片上的删除按钮时别忘了 CSS：定位原先在 `Popconfirm` 的包裹层上
  （`.key-event-card .ui-popconfirm`），去掉包裹层后要落到按钮自己身上（`.key-event-card__delete`）。
- **弹窗里的下拉面板会被 `overflow: hidden` 裁掉**：DatePicker / Select 的下拉都是绝对定位子元素，而
  `.ui-modal__content` 原来带 `overflow: hidden`（只为圆角），小弹窗（如「关联事件」）里日历被裁到只剩月份标题，
  中等弹窗被裁掉一半，看着像"面板画坏了"。现改成 `overflow: visible`：header/footer 是透明底只有一条边框线、
  body 有内边距，没有子元素会画到圆角外。**下拉面板的正确做法是 portal 到 body。**
  排查提示：UIA 里看不到"被裁掉"（被裁元素的矩形照样报出来），**只有截图（全屏 PNG）才看得出"元素没画出来"**。
- **`scrollbar-width` 会让 `::-webkit-scrollbar` 整套失效**：规范里只要 `scrollbar-width` / `scrollbar-color`
  被设成非 `auto`，`::-webkit-scrollbar-*` 伪元素就整体不生效（`base.css` 原来那个 `.u-custom-scrollbar` 两个都写，
  那套"5px 圆角滑块 + 透明轨 + 悬浮加深"从未渲染过，而且它全仓一个使用者都没有）。本机实测占用宽度（WebView2 /
  Chrome 153，DPI 1.5）：原生 17px / `scrollbar-width:thin` 11px / 只写 webkit 6px。现在滚动条是全站默认，
  **只用 `::-webkit-scrollbar`，不要再加 `scrollbar-width`**。另：`body { user-select: none }`（桌面窗口惯例）
  让全局 `::selection` 基本只在输入框里可见，同节的 `caret-color` 与选区配色是配套的。
  排查提示：`::-webkit-scrollbar` 的效果从 `getComputedStyle` 读不到，要么查 CSSOM 规则、要么量
  `offsetWidth - clientWidth`、要么看像素。
- **Tip 的宽度：只写 `max-width` 会被"包含块"坑死**（"应用设置 → 股票 → 过户费 ⓘ"的气泡被压成一列窄条、贴着
  窗口右缘）：气泡是 `.ui-tooltip::after` 绝对定位伪元素，包含块是触发器本身（那个 ⓘ 只有 ~14px 宽），
  于是 shrink-to-fit 的可用宽度就是 14px。正确写法：先用 `width: max-content` 按文案铺开，再用 `max-width`
  收上限（上限同时受视口约束 `min(320px, calc(100vw - …))`）。换行用 `white-space: pre-line`（短提示与
  `nowrap` 表现一致，带显式 `\n` 的多行说明能按原样折行）。触发器在版心右缘时套 `.ui-tooltip--end`。
- **别把 `PrintWindow` 截图里的"残缺图标"当成缺陷**：`ui-shots.ps1` / `dev-shot.ps1` 抓的是
  `PrintWindow(PW_RENDERFULLCONTENT)` 位图，WebView2 在某些状态下**合成器还没画完**，内联 SVG 图标会被抓成一条
  竖长的涂鸦（正文文字、布局、表格都正常），人眼看窗口完全正常。判据优先级：**人眼看窗口 > UIA 树里控件的矩形 >
  截图像素**；怀疑图标坏了先开一次窗口看看。
- **删/改一条 CSS 规则前先 grep 谁还在用那个类名**（"应用设置 → 关于软件"不再垂直居中）：`8d55b17` 把设置页面板
  从 `.st-pane` 换成 `.page-pane`（前者整套规则被删），但 `<div class="st-pane st-about">` 漏改了，它退化成普通
  块级容器，于是 `.st-about` 的 `align-items/justify-content: center` 全部失效、`margin: auto` 也垂直居中不了
  （`auto` 外边距只在 flex/grid 容器里吃掉剩余空间），内容停在顶部。症状很欺骗：样式不报错、面板照常渲染，
  只是"居中没了"。改 CSS 结构时用 `grep -rn 'st-pane\b' crates/tr-ui` 之类核一遍调用点。
- **详情区「减仓/清仓/加仓」点不动 = 产品 bug**（曾被误记成"UIA 自动化不了"）：三个按钮的 `on_click` 里先
  `selected_code.set(code)` 再 `open_trade(...)`，而详情区本来就按 `current_position()`（= `selected_code`
  命中的那条）渲染、写的是同一个值；但 `RwSignal` **同值写入依然会通知订阅者** → 点击瞬间详情子树重渲染 →
  交易弹窗永远不 mount（DOM 里连 `ui-modal__mask` 都没有）。表头那个不写 `selected_code` 的「建仓」按钮一直正常，
  对比才看出差别。修法：三处去掉冗余的 `selected_code.set`（`src/pages/stock.rs` 有注释）。
  教训（手法可复用）：先分清"没点到"和"点了没反应" —— 真实鼠标 / `SetFocus`+回车 / `InvokePattern` 三种激活都试，
  用 `AutomationElement.FromPoint` + `WindowFromPoint` 确认坐标上是谁（Tauri 的 `TAURI_DRAG_RESIZE_BORDERS`
  浮层会抢答 `FromPoint`，但鼠标照样进 WebView，所以它单独用会误判），再往 handler 里塞 `Notifier` 确认 handler
  跑了、`trade_open=true`，最后并排放裸闭包节点与 `<Show>` 探针 + 弹窗体里放挂载标记，才锁定"信号写对了、
  是 Modal 没挂载"。别用"这按钮自动化不了"给产品的 bug 打掩护。
- **不要在 `Effect` 体内创建信号**（踩过两次，都表现为"界面空白 / 点了没反应"）：`Effect` 每次重跑都会 dispose
  上一次创建的 reactive 值，而界面那时往往还在读它，控制台里只有一句
  `you tried to access a reactive value … but it has already been disposed`。
  * `settings.rs` 的 `UpdateState`（「关于软件」）首次在组件里创建 → 切走再切回面板空白；修法是把状态提前建到
    根 owner（`init_update_state()` 由 `shell::App` 调用）。
  * `transactions.rs` 排序弹窗在 `Effect` 里 `SortRow::new(...)` → 关掉再打开必 panic；修法是"行信号池"：
    最多 4 行的信号在页面 owner 下一次建好，打开只回填值、增删只改一个 `count`（见 `sort_modal` 注释）。
  正确姿势：状态建在组件/根 owner 下，`Effect` 里只读、只做副作用。
- **charts-rs 的坑都在 `components/ui/chart.rs` 里绕开了，别改回去**（细节见该文件的注释）：
  ① Y 轴刻度它按数量级把步长往上取整（`36000 ÷ 4` → 9,500）⇒ 范围由 `nice_axis_range` 自己算
  （⚠ 自定义上界必须**严格**大于数据最大值，否则它静默退回自己的阶梯）；② 面积填充的基线它写死成绘图区底边
  （跨零时负的那段被填成"从折线一路铺到图底"的大色块）⇒ 在生成的 SVG 上定点改写（`anchor_fill_at_zero`；
  该公式假设**绘图区顶边 = `margin.top`**，所以本组件始终把 charts-rs 的标题/副标题置空、自带图例关掉）；
  ③ `{t}` 千分位**对负数不生效**（同一根轴上 `50,000` 与 `-50000` 并存），已知、未修。
  另：`tr-ui` 的 `#[cfg(test)]` 在 native 上 `cargo test` 跑 0 个（crate 只编 wasm32），要真跑就把函数**原文**
  抽出来配桩在 native 上执行 —— 现成例子 `fixtures/chart-tests.ps1`（含用 charts-rs **真实输出**的
  SVG 做的定点断言）。

### 更新与代理

- **更新链路的两个纯函数已抽出来单测**（`updater.rs`）：`parse_release`（release JSON → 更新信息：跳过预发布、
  `v` 前缀、取第一个 `.exe` 资产、body 缺失给空串、没有 `.exe` 仍算"有更新"）与 `digest_matches` /
  `normalize_digest`（`sha256:ABCD…` 大写去前缀后比较；缺失/空串则跳过校验）。界面「检查更新」另有实测：
  真实 GitHub API 返回「已是最新版本」（探针 `target/update-probe.ps1`）。
- **自动更新是自研实现**（`src-tauri/src/updater.rs`），没有用 `tauri-plugin-updater`：沿用 GitHub Releases +
  `asset.digest`(sha256) 校验，不需要签名密钥与 `latest.json`。命令 `update_check` / `update_download`
  （发 `update:download-progress|complete|error` 事件）/ `update_cancel` / `update_install`；行为：仅 GitHub 域名
  白名单、已下载复用、`.part` 中转、取消清理、打开安装包后退出。因此 `tauri.conf.json` 里**不要**加
  `plugins.updater`，capabilities 也不需要 `updater:default`。
- **代理：三态 + 自动探测系统代理**：「应用设置 → 通用设置 → 代理」= `off`（不使用）/ `auto`（自动探测，默认）/
  `manual`（手动 `http://host:port`），落在 `~/.transactions.json` 的 `proxy: { mode, url }`（缺该键的老配置按
  `auto`）。配置由外壳持有，`src-tauri/main.rs` 启动时调 `tr_service::proxy::set(...)`，`config_set_proxy`
  落盘后立即再推一次；`tr-service/src/quote.rs`（行情）与 `src-tauri/updater.rs`（更新，`agent()` 是两处共用的
  唯一入口）都从同一处取，不会"更新走代理、行情不走"。`tauri-plugin-opener` 打开浏览器那次跳转不受影响。
  * `auto` 的探测顺序：`ALL_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY`（含小写变体，与 ureq 的 `Proxy::try_from_env()`
    同序）→ WinINET 注册表 `HKCU\…\Internet Settings`（`ProxyEnable=1` 时取 `ProxyServer` 的 `http=` 段）→
    同路径 `HKLM` → 直连。只读注册表（`winreg`，`cfg(windows)`），每次请求重新探测，改系统代理不必重启。
  * 只支持 HTTP 代理：地址只认 `http://`（`host:port` 自动补 scheme；可带 `user:pass@`）；`socks*://` 与
    `https://` 明确拒绝并给中文文案；HTTPS 目标（GitHub）走同一个 HTTP 代理的 CONNECT 隧道。
    不支持 PAC（`AutoConfigURL` 只作为提示上报，不解析），也不支持 `ProxyOverride` 绕过列表
    （环境变量路径下由 ureq 的 `NO_PROXY` 处理）。
  * `off` 会显式 `.proxy(None)`：ureq 的 `Config::default()` 本身就读环境变量代理，不显式覆盖则"不使用代理"
    名不副实 —— 这是本仓库唯一一处必须传 `Option` 的地方。回归 `fixtures/ui-proxy.ps1`。
  * 失败形态：代理不可达时更新检查报错、行情静默失败（计入既有的 `quote_failed_count`），都不 panic；
    手改配置成非法值时按"未配置"（直连）处理并记日志。

## 关键约定与陷阱

- **股票费用：一次委托多笔成交时，印花税/过户费"逐笔取整再相加"**（`tr-domain/src/fee.rs`，两侧共用同一份算法，
  界面侧只有 `estimate_fee` 一个调用点）：**佣金**按**委托总额**收一次，最低佣金也只在这里生效（按笔收会变成
  N 份最低佣金）；**印花税 / 过户费**逐笔按成交额算、**逐笔**四舍五入到分再求和 —— 不是"先求和再取整"。
  两者会差一分：`36.61×100` + `36.67×100` 两笔卖出、沪市，过户费逐笔是 `0.04+0.04=0.08`，先求和只有
  `0.07328→0.07`（用户报的就是这一分钱）。因此 `compute_order_fee` 收的是**各笔成交额**（`&[i64]`）而不是总额；
  `allocate_order_fee` 与它同口径，保证"每笔分摊之和 = 委托级合计"。回归：`cargo test -p tr-domain fee`
  （含用户那个例子的逐项断言）、`cargo test -p tr-service create_trade_order_charges_fee_once_per_order`。
- **SQL 只允许拼接常量**：列名/表名用 `const …_COLUMNS` 或常量数组（如 `STOCK_TABLES`），值一律走 `?` 占位符
  （`instr(description, ?)` 也是占位符）；`ORDER BY` 的字段必须过白名单 —— `build_sort_clause` 只认
  `transactionAt` / `transactionType` / `price` / `category` 这 4 项，多一项都不认，方向强制 `asc|desc`。
- **生产代码里的 `unwrap/expect/panic!` 必须有据可依**：只允许锁中毒（`.expect("…锁中毒")`）、已校验不变式
  （月份 `1..=12`、`valid_up_to` 前缀、池在生命周期内有效）、启动期构建失败（`main.rs`）。新增前先问
  "它真的不可失败吗"。
- **金额恒为整数分**：数据库、IPC、算法全用 `i64` 分；只有展示层做分/元换算（`tr_domain::money`）。
  这两个换算函数的行为是硬契约（含负号、`.5` 输入）。
- **时间戳语义**：`transaction_at`、`trade_time` 等是 Unix 秒；`%Y-%m` 这类分桶在 SQL 里用
  `strftime(..., 'unixepoch')` 完成，不要在 Rust 侧重算。
- **数据库结构变更只走迁移引擎**：`transactions.db` 不存在时用 `fixtures/schema/fresh.sql` 建库（当前格式）；
  已存在时先由 `tr-store/src/migrations.rs` 的迁移引擎按 `tbl_billadm_schema_migration` 登记表升级，再按当前格式
  只读校验（`schema::validate_current`）。校验不通过（比已知格式更早且没有对应迁移）时明确拒绝，用户可见文案是
  `该工作空间不是当前格式（格式过旧）：…`。
- **迁移的写法是硬规范**（`migrations::MIGRATIONS` 是唯一入口）：① id 唯一稳定（`YYYYMMDD_描述`）；
  ② 一个事务（执行 + 写登记行，失败整体回滚）；③ 幂等、防御式（先查 `PRAGMA table_info` / `sqlite_master`，
  结构已在就只补登记行）；④ 只碰本次升级涉及的表，不许"顺手修复"别的结构；⑤ 留单测（旧格式 → 升级后校验通过、
  数据一字不差、重复应用无副作用）。升级前必须先备份（`VACUUM INTO` 出 `transactions.db.pre-migration-<时间戳>.bak`，
  备份失败就不升级），同一工作空间只保留最近一份（旧的 `.bak` 在新备份成功之后才清掉），只认自己的命名规则、
  不动工作空间里别的文件。`cargo xtask migrate` 手工升级，`validate` / `dump` 仍只读。
- **IPC 契约**：命令统一只收一个 `req` 结构体参数，字段名是硬契约，改动即破坏兼容。成功时 promise 直接
  resolve 为数据本身；失败时 reject 载荷为 `{"code":-1,"msg":"...","status":500}`。`msg` 是用户可见文案。
- **JSON 字段命名不统一，但必须保持不变**：核心记账模型是 snake_case（`ledger.created_at`），事件/日记/股票模型
  是 camelCase（`ledgerId`、`createdAt`），DTO 里两种混用（`tr_query_result` 的 `page_size` 与 `trStatistics`
  并存）。数据库列名恒为 snake_case，列映射在 DAO 层显式书写，不依赖 serde。
- **图片资产**：`<workspace>/data/assets/key_events/<date>/<uuid>.<ext>` + `thumb_<uuid>.jpg`，库里存相对
  `data/assets` 的路径，界面走 `trasset://` 自定义协议（处理器带路径穿越校验，只允许 `data/assets` 下的相对路径）。
- **后端只接受 JPEG/PNG/GIF/WebP**：HEIC 转换留在界面层（web-sys canvas 交给 WebView2/系统解码器转 JPEG 再上传），
  后端不引入 libheif/WIC。缩略图：宽度 > 300 时等比缩到 300（CatmullRom）、JPEG q75。
- **没有本地网络面**：不监听端口、没有 API 令牌、没有 CORS、没有子进程后端。进程即应用。
- **首启动的窗口切换由外壳负责**：`workspaceDir` 为空时进初始化窗口（600×560、不可缩放），界面在"未配置工作空间"
  时展示选目录引导；选完目录后 `workspace_open` 打开数据库，外壳随即 `show_main_window` + `destroy` 初始化窗口。
  界面**不要**再调 `workspace_init`（它只是保留的幂等入口）：曾因全仓无调用点，导致首启动只在一个 600×560 窗口里
  渲染整个应用、主窗口一直不出现（托盘"显示主窗口"才补出来，于是变成两个窗口）。
- **配置文件是用户数据**：`~/.transactions.json`（dev 为 `~/.transactions-dev.json`）的键名与位置都不变，读写时
  必须保留未知键（`AppConfig.extra`）。`features: { accounting, stock, keyEvent, diary }` 是「应用设置 → 功能开关」，
  **缺省全开**（`#[serde(default)]` + 字段默认 `true`）；它跨层面：外壳只落盘（`config_set_feature`），
  侧栏读界面侧 `store::AppStores::enabled_features`（`shell::Page::is_enabled` 是唯一判据），两边靠 `feature_key`
  字符串对齐 —— 名字写错不会编译报错，只会得到一句"无效的功能开关"。`cargo test -p transactions config`
  锁住键名与"缺省全开"，端到端见 `fixtures/ui-features.ps1`。另有 `keyEventLinkedOpen`（事件页右栏「关联交易」
  展开/收起，**缺省展开**）走同一条路：外壳只落盘（`config_set_key_event_linked_open`），界面读
  `store::AppStores::key_event_linked_open` —— 放在**全局状态**而不是页面局部，换页回来才不必等 IPC 往返、
  也不会先展开再收起闪一帧；端到端见 `fixtures/ui-key-event.ps1` 第 4 步（落盘 + 换页 + 重启）。
- **界面无 Node**：仓库里没有 npm/package.json，没有 vendor 的 JS 库。图表、Markdown、拖拽排序、日期选择全部是
  Rust 实现（见 `tr-ui`）。
- **设计令牌**：`--transactions-*` CSS 变量是颜色/尺寸的唯一来源（对应 `DESIGN.md` 的调色板；`PRODUCT.md` 历史
  文本里的 `#4A8E70` 是过时信息）。只支持浅色/深色两套主题（共用同一组令牌名），主题通过
  `<html data-theme="light|dark">` 切换；历史上遗留的令牌允许保留。
- **`crates/tr-ui/dist/` 由 trunk 生成，不入库。**

## 发布

**远程仓库**：`https://github.com/ddd-online/Transactions-Rust`（分支 `main`）。应用内更新检查、
「关于软件」的 GitHub 链接、`build/release.ps1` 的 `$repo` 都必须指向它，否则会比对到不相干的版本、
提示"有新版本"却下载到错误的安装包。改这三处时自检：

```powershell
Select-String -Path src-tauri\src\updater.rs,crates\tr-ui\src\pages\settings.rs,build\release.ps1 -Pattern 'ddd-online'
```

**版本控制**：`.gitignore` 已排除 `/target`、`/build/target`、`/crates/tr-ui/dist`、`/src-tauri/gen/schemas`、
`*.db(-wal|-shm)`、`transactions.log` 与 `/fixtures/private/`，**真实工作空间数据绝不入库**。提交前
`git status --short` 核一眼，别把本地验证用的库或截图带进去。

**发布链路**：`build/clean.ps1` → `build/build.ps1`（trunk → cargo tauri build → 重命名产物为
`Transactions-x64-v{version}.exe`）→ `build/release.ps1`（`gh release create` 上传该 .exe）。版本号唯一来源是
`src-tauri/tauri.conf.json`（`Cargo.toml` 的 workspace 与 `src-tauri` 两处也要同步）；应用内更新读 release 的
`tag_name` 与首个 `.exe` 资产的 `digest`。许可证以仓库根 `LICENSE` 为准（Apache-2.0）。

**发布页的说明**：`release.ps1` 用 `gh release create --generate-notes`，而 GitHub 对**直接 push 的提交**
（没有 PR）只会生成一行 `Full Changelog` 链接 —— 想给正文就自己填：
`gh release edit vX.Y.Z --notes-file <CHANGELOG 对应小节 + 安装包名 + compare 链接>`（只改元数据，不动资产；
改完照例 `gh release view --json assets` 回读一次 digest）。

**`clean.ps1` 会连 `target\` 一起删**，而护栏用的工作空间就在里面（`target\ws-rust` 给 `ui-smoke`、
`target\smoke\ws-write` 给 `ui-shots` 与 `close-behavior`）。所以"发布前跑全量护栏"要在 `clean` **之前**做，
或者 clean 之后先补种 `cargo xtask seed target\ws-rust` 与 `cargo xtask seed target\smoke\ws-write` ——
否则这三个脚本会因为「工作空间里没有 transactions.db」直接红（是环境问题，别去改代码）。

**踩过的坑：发布资产可能是上一版的安装包**：0.2.0 的 release 资产其实是 0.1.0 的安装包（两个 release 的资产
字节数与 `sha256` 一模一样，用户装完看到的还是 0.1.0 的界面）。根因是 `cargo tauri build` 不清
`target\release\bundle\nsis\`，上一版遗留的 `Transactions_0.1.0_x64-setup.exe` 与新的并排存在，而旧脚本用
`Get-ChildItem *-setup.exe | Select-Object -First 1` 取字典序第一个（0.1.0 在前）。现在 `build.ps1` 构建前会删掉
陈旧安装包、只认 `Transactions_{版本}_x64-setup.exe`，并断言它的 `LastWriteTime` 晚于本轮构建开始时刻；
安装包与便携版都必须落盘成功，否则非零退出。**别只信退出码，也别按"第一个匹配"取产物**：发布前用
`fixtures/ui-about.ps1` 对着产物核一次自报版本号，发布后再 `gh release view <tag> --json assets` 核对 `digest`
与本地 `Get-FileHash` 一致（同一个 digest 出现在两个 tag 下就是发错了）。
