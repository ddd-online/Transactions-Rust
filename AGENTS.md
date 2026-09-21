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
crates/tr-ui/        # 界面：Leptos CSR（cdylib，仅编译到 wasm32）+ static/{css,fonts,icons}
src-tauri/           # 桌面外壳：窗口/托盘/配置/日志/trasset:// 资产协议/更新
xtask/               # 验证工具：schema-diff（建库护栏）、validate / dump / seed（工作空间工具）
fixtures/            # schema 基线（fresh.sql）+ 种子与端到端脚本（**不含真实个人数据**）
```

分层纪律：

- `tr-domain` 不得引入任何 I/O 依赖，rusqlite、reqwest、tauri 都不行。它同时被 native 侧和 wasm 侧依赖，界面与后端因此共用同一份金额与费用算法。
- 只有 `tr-ipc` 依赖 `tauri`。业务规则必须能在没有窗口的环境里用 `cargo test` 验证。
- `tr-store` / `tr-service` 不参与 wasm 编译（`cfg(not(target_arch = "wasm32"))`）。

## 常用命令

```powershell
# 类型检查 / 测试（不含桌面外壳）
cargo check -p tr-domain -p tr-store -p tr-service -p xtask --all-targets
cargo test  -p tr-domain -p tr-store -p tr-service

# 界面与外壳（发布链路必须走脚本，原因见下方环境注意事项）
trunk serve --config crates/tr-ui/Trunk.toml      # 开发服务 http://127.0.0.1:1600
cargo tauri dev                                   # 开发（会自动先跑 trunk）
cargo tauri build                                 # 打包，产出 NSIS 安装包

# 验证护栏
cargo xtask schema-diff                           # Rust 建库结构与 fixtures/schema/fresh.sql 逐条一致
cargo xtask validate <workspace-dir>              # 只读校验既有工作空间是否为当前格式（会报告待应用的迁移）
cargo xtask migrate <workspace-dir>               # 升级既有工作空间到当前格式（升级前自动备份）
cargo xtask seed <workspace-dir>                  # 新建并播种一份示例数据（人工冒烟用）
cargo xtask dump <workspace-dir> [--table <name>] # 只读导出业务表为规范化 JSON（排障与回归对比）

pwsh -File fixtures/design-audit.ps1              # 设计令牌护栏，见下方说明
pwsh -File fixtures/contract-audit.ps1            # 跨 crate 契约审计：api/*.rs 的请求结构体 vs tr-ipc
pwsh -File fixtures/smoke.ps1     [-Workspace <既有工作空间>] [-Exe <exe>]
pwsh -File fixtures/ui-smoke.ps1  [-Workspace <ws>] [-WriteFlow] [-Discover]
pwsh -File fixtures/ui-shots.ps1  [-Workspace <ws>] [-OutDir <dir>]

pwsh -File fixtures/close-behavior.ps1     [-Exe <exe>] [-Workspace <ws>]
pwsh -File fixtures/window-bounds.ps1      [-Exe <exe>] [-OutDir <dir>]
pwsh -File fixtures/ui-drag.ps1            [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-upload.ps1          [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-diary-io.ps1        [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-diary-edit.ps1      [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-diary-ledger.ps1    [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-crud.ps1            [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-sync-ledger.ps1     [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-transactions.ps1    [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-stock.ps1           [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-key-event.ps1       [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-link-event.ps1      [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/ui-proxy.ps1           [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
pwsh -File fixtures/migrate-workspace.ps1  [-Exe <exe>] [-OutDir <dir>]

# 代码规范
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

护栏各自管什么（脚本都真的启动应用、用 UI Automation 或真实鼠标键盘驱动界面；断言优先落在库与磁盘上）：

- `design-audit`：`tokens.css` 之外不得有硬编码颜色、引用的令牌必须已定义、深色主题与
  `prefers-color-scheme` 兜底必须覆盖同一组令牌。
- `contract-audit`：界面 `api/*.rs` 的请求结构体是手抄 `tr-ipc` 的（两侧不能共用类型，
  见「JSON 字段命名」），抄错字段名不会编译报错、serde 只静默取默认值，所以这条按命令逐字段比对。
- `smoke`：已配置工作空间 → 只有主窗口；首次启动 → 只有初始化窗口；并断言界面确实启动
  （日志里有 IPC `config_get`）。用临时 `USERPROFILE` 启动，碰不到真实的 `~/.transactions.json`。
- `ui-smoke`：挨个点开 5 个顶级功能 + 9 个子功能（记账 4 个：记录 / 分析 / 标签 / 模板；
  股票 5 个：账户 / 持仓 / 记录 / 统计 / 设置；都走左侧图标条）并断言内容渲染；
  `-WriteFlow` 还会在工作空间副本里通过界面记一笔（弹窗 → 填表 → 保存 → 列表出现）；
  `-Discover` 导出每页元素清单，用来维护脚本顶部的页面标记表。
- `ui-shots`：抓窗口位图断言每页不是空白，并比较浅/深色的平均亮度；14 张 PNG 落到 `target\ui-shots\`
  供人工验收。补 UIA 的盲区。
- `ui-drag`：真实鼠标（按下 → 20 段移动 → 抬起）把第 1 个分类拖到第 3 个位置，断言顺序变化 +
  `sort_order` 落库 + 界面同步 + 重进页面仍一致。
- `window-bounds`：逻辑尺寸启动 → 物理窗口 = 逻辑 × DPI → 关闭 → 配置里写回的仍是逻辑值 →
  再启动一次尺寸/位置完全一致。
- `close-behavior`：`quit` → 进程退出；`tray` → 进程存活且窗口隐藏；空 → 弹「关闭选项」框，
  选「是」后退出（原生询问框的按钮在 UIA 里是 `是(Y)`/`否(N)`，Pane 类型）。
- `ui-upload`：点「添加图片」拉起原生文件框，选一张自己生成的 600×400 PNG，断言原图按原字节落盘 +
  缩略图缩到 300×200 且同目录同名前缀 + 库里 `file_path`/`thumb_path`/`event_date` + 界面出现「下载图片」。
- `ui-diary-io`：驱动原生选目录框导入一个临时目录（UTF-8 + GBK + 一个非法文件名），断言正文逐字节落库、
  `word_count` 按标量值、非法文件名被跳过；再导出到空目录断言文件数/命名/正文与库一致。
- `ui-diary-edit`：进日记页（首屏是预览态，要先点页脚的「编辑」）→ 写内容 → `Ctrl+S`（没有保存按钮，
  靠 `input` 后 1500ms 防抖自动保存）→ 断言正文/字数/心情落库 → 点心情「开心」→ 断言 `mood=😊`
  且 id 不变（同一天 upsert）→「预览」里 Markdown 渲染出标题 → 删除。
- `ui-diary-ledger`：当前账本写一篇 → 切到另一个账本（编辑器里看不见它）→ 在同一天写不同内容 →
  断言库里两行、分别属于两个账本（复合唯一键的回归）→ 切回来正文原样。
- `ui-crud`：分类「新增 → 删除」、标签、图表、事件「点色板改颜色 / 写 Markdown 描述 / 删除」、
  记账·模板子功能「新建模板 → 删除」，断言都落在数据库上。
- `ui-sync-ledger`：记一笔 → 行内「同步到其他账本」→ 选目标账本 → 断言目标账本多一份新 id 的副本、
  金额/类型/分类/记录时间一致、源记录保留（复制而非移动）→ 切账本后界面里能看到副本。
- `ui-transactions`：记三笔 → 编辑一笔（断言「先建后删」：换 `transaction_id`、旧记录消失、行数不变）
  → 保存为模板（记一笔弹窗里的子弹窗）→ 排序（重置 → 加「金额 降序」→ 应用 → 断言金额序列非递增、
  最大值排第一）→ 筛选（悬浮按钮 → 关键词 → 添加条件 → 确认 → 断言收敛到 1 条）。
- `ui-stock`：建仓（真实行情查名）→ 编辑成交 → 删除委托 → 减仓 → 清仓 → 费用设置 → 重置股票数据。
  断言成交（价按分/手数/股数/成交额/手续费）→ 持仓（数量、成本 = 成交额 + 手续费，减仓按比例结转
  `cost_basis = round(total_cost × 本次股数 / 持仓股数)`、`realized_pnl = amount - fee - cost_basis`）
  → 资金记录（余额链、买入 -(成交额+手续费)、卖出 +(成交额-手续费)）→ 清仓归档（新轮次 + 回填成交的
  `round_id`）→ 持仓卡片与交易历史的展示。
- `ui-key-event`：行内「新建」→ DatePicker 任选一个不是今天的日期 → 断言事件落在那一天 → 同一天再建一次
  断言 upsert（一条、id 保留、标题被覆盖）→ 行内「删除事件」→ 库里清空。（`ui-crud` 只验了默认日期。）
- `ui-link-event`：记一笔 → 行内「关联到事件」→ 弹窗里用日期选择器选一个不是今天的日子
  （`link_date` 默认今天，选今天就等于没测选择器）→ 断言触发器显示所选日期 + `key_event_date` 落库 +
  该日期懒创建了一条空事件 →「修改关联」→「解除关联」→ 断言 `key_event_date` 清空。
- `migrate-workspace`：复制一份已播种的工作空间 → 降级成旧格式（日记去掉 `ledger_id`、旧的全工作空间
  唯一索引回来、抹掉登记行）→ 启动真实外壳 → 断言能正常打开、结构升级到位、老日记正文一字不差并归到
  最早账本、升级前留了备份（且备份里是旧结构）、再启动一次不重复备份（幂等）。它在真库上验证迁移引擎
  这条最高风险路径。
- `ui-proxy`：起一个本机假 HTTP 代理当判据 → 界面「通用设置 → 代理」手动指向它 → 断言行情
  （`qt.gtimg.cn`）与更新检查（`api.github.com`）都出现在假代理日志里、行情名称显示的是假代理返回的
  名字 → 切「不使用」→ 断言假代理一条都收不到。只断言配置文件写了什么等于什么都没验。

工具链要求：rust stable 1.96.0 加 `wasm32-unknown-unknown` target。仓库故意不放 `rust-toolchain.toml`，
因为指定具体版本会让 rustup 每次调用都校验并重装组件（实测会触发数百 MB 的重复下载），
版本要求写在文档里就够了。

## 改界面的迭代循环

改 `crates/tr-ui` 下的 `.rs` / `.css` 一律走 dev 服务 + 热更新，一轮约 5~10 秒；`build/build-ui.ps1`
加 `cargo build --release` 是最终验收/发布用的，实测约 4 分钟（其中 release 链接 2m40s），迭代期别用。

1. 起一次（之后一直开着）：

   ```powershell
   pwsh -File fixtures/dev-hot.ps1 -Trunk -Launch -Workspace <ws> -ShotDir target\dev-shots
   ```

   它拉起 `trunk serve`（:1600）+ dev 外壳（`target\debug`，走 `devUrl`）+ 盯 trunk 日志；trunk 重建完
   就给窗口发 `Ctrl+R`，窗口自己刷新；带 `-ShotDir` 时还会把刷新后的窗口存成
   `target\dev-shots\current.png`（不用手动截图）。原理：trunk 的自动刷新信号浏览器吃、Tauri 的
   WebView2 不吃，但 WebView2 吃键盘刷新（实测 0.8s）。

2. 只想看某一页/全部页面（不重建、不重启）：`pwsh -File fixtures/dev-shot.ps1 -Page 股票`，
   或 `-AllPages`。它用真实鼠标点侧栏并轮询确认页面真的切过去了再截图（固定 sleep 会抓到上一页）。

3. 只有需要「内嵌界面的 release 产物」（端到端护栏、发布）才做完整构建：

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File build/build-ui.ps1;
   cargo build --release -p transactions --features tauri/custom-protocol
   ```

边界：改 `src-tauri/`（外壳）不适用热更新，要 `cargo tauri dev` 重启；`-Launch` / `-Trunk` 用独立配置
目录（`target\smoke\home-hot`），不碰真实的 `~/.transactions-dev.json`。
**不给 `-Workspace` 就沿用配置里已有的工作空间**（早期版本会把它写空：外壳于是进首启动流程、只开 600×560
的初始化窗口，主循环要的侧栏「记账」永远不出现，最后只看到一句"没找到主窗口"）。`NO_COLOR` 也由脚本自己
清掉，不必先 `Remove-Item Env:NO_COLOR`。

## 本机环境注意事项（踩过的坑）

### 构建与工具链

- **cargo 不读 Windows 系统代理**（curl 只认 `HTTP(S)_PROXY` 环境变量；PowerShell/浏览器走系统代理）。
  本机直连 crates.io 会超时，先设：
  ```powershell
  $env:HTTPS_PROXY='http://127.0.0.1:7890'; $env:HTTP_PROXY='http://127.0.0.1:7890'
  $env:CARGO_NET_RETRY='10'; $env:CARGO_HTTP_TIMEOUT='120'
  ```
  索引拉取偶发中断时重复执行 `cargo metadata` 即可（已缓存的条目会累积）。
- **不要用 PowerShell 管道接 cargo 输出**（`cargo ... | Select-Object -Last N`）：
  管道缓冲写满会让 cargo 阻塞假死；一律 `*> 文件` 重定向。
- **`NO_COLOR=1` 会让 trunk 直接报错**（`invalid value '1' for '--no-color'`）：
  调用 trunk 前 `Remove-Item Env:NO_COLOR`。
- **开发端口留 1600，且保留段会漂**（踩过两次）：本机 Windows 的保留端口段（WinNAT/Hyper-V）
  归管理员所有，落在里面的端口非提权进程绑不上，trunk/`cargo tauri dev` 直接 `os error 10013` 起不来。
  先撞的是 1420（当时保留 1332-1431），2026-09 又漂到 **1463-1562**，把 1520 也吞了。
  所以不要以为"某端口实测能绑"是永久的：起不来先跑 `netsh interface ipv4 show excludedportrange protocol=tcp`，
  把端口挪到段外（现用 1600）。三处**必须一起改**：`crates/tr-ui/Trunk.toml` 的 `[serve] port`、
  `src-tauri/tauri.conf.json` 的 `devUrl`、`fixtures/dev-hot.ps1 -Port`（`shell.rs` 的
  `is_allowed_navigation` 单测里还有两条 localhost 样例）。`fixtures/dev-hot.ps1` 现在会先试着
  绑定该端口，几秒内直接报"端口不可用"而不是干等 240 秒。
- **改了 `devUrl` 就必须重编 dev 外壳**：dev URL 是**编译期**读进 `tauri.conf.json` 的，不是运行时读的。
  外壳二进制比 `Trunk.toml` / `tauri.conf.json` 旧时，窗口里是 `ERR_CONNECTION_REFUSED`（trunk 其实
  服务得好好的），看起来像界面代码坏了。改完配置跑一次 `cargo build -p transactions`；
  `fixtures/dev-hot.ps1` 会做这个时间戳比对并直接 throw（不然只能看到一句连接被拒绝）。
- **trunk 无法下载 wasm-bindgen**（GitHub release 资产在本机网络上 TLS 失败），
  改从 crates.io 安装同版本 CLI：`cargo install wasm-bindgen-cli --version 0.2.128 --locked`
  （版本必须与 `wasm-bindgen` crate 一致，当前 0.2.128）。
- **不要用 `trunk build --release`**：trunk 在 release 模式会调用它缓存里的 wasm-opt 123，
  而该版本无法校验 Rust 1.96 生成的 bulk-memory 指令
  （`memory.copy ... requires --enable-bulk-memory-opt`），更新版本又无法下载。
  发布构建统一走 `build/build-ui.ps1`：它跑 trunk 的 debug 模式（跳过 wasm-opt），
  并用 `CARGO_PROFILE_DEV_OPT_LEVEL=3` + `CARGO_PROFILE_DEV_DEBUG=false` 把优化拉满、去掉调试信息。
  实测 wasm 7.3 MiB / 7.6 MB，5 个顶级功能 + 记账的 4 个子功能都在；P6-a 只有 4 个页面时是 3.9 MB。
  作为对照，完全不优化、带调试信息的 debug 构建约 15 MB。`tauri.conf.json` 的 `beforeBuildCommand` 已指向该脚本。
- **`build/*.ps1` 被 `powershell`(5.1) 调用时必须 ASCII-only**：Windows PowerShell 把无 BOM 的 UTF-8
  当 ANSI 解码，中文会破坏脚本解析，`build-ui.ps1` 因此全英文注释，而且它确实就是被 `cargo tauri build`
  用 5.1 调用的，不能改。`build.ps1` / `release.ps1` 含中文注释，约定用 pwsh 7 运行；它们现在开头
  自带一段 ASCII-only 的守卫：检测到 `PSEdition -ne 'Core'` 就用 `pwsh` 重跑自己（`$PSCommandPath` + `@args`）。
  加这段守卫是因为一次真实事故：我用 5.1 跑 `build.ps1`，中文行被误解析后最后一步的 `$appExe` 变成 $null，
  便携版 exe 没被留档（`build\target` 里只有安装包），而退出码依然是 0。
  教训：构建脚本的"最后一步"也要有产物断言（`Test-Path` + `Fail`），别只看退出码。
- **手跑 exe 必须带 `custom-protocol`**（两条都踩过）：
  1. debug 构建会走 `devUrl`（`http://127.0.0.1:1600`），所以 `cargo build -p transactions`
     之后直接运行 `target\debug\transactions.exe` 只会得到一个空白窗口；
  2. release 也一样。Tauri 只有在启用 `tauri/custom-protocol` 特性时才会内嵌界面资源，
     而 `cargo tauri build` 会自动加上它、裸 `cargo build --release` 不会。
     漏掉它时窗口里是 Chromium 的 `127.0.0.1 拒绝连接`（ERR_CONNECTION_REFUSED），
     外壳日志却只有"启动/外壳初始化完成"，看起来像界面代码坏了。
  正确的三种做法：`cargo tauri build`（发布链路用的就是这个）、
  `cargo build --release -p transactions --features tauri/custom-protocol`（冒烟用）、
  或起 `trunk serve` 后跑 debug 构建（等价 `cargo tauri dev`）。
  `fixtures/smoke.ps1` / `fixtures/ui-smoke.ps1` 都要求前者那种"界面内嵌"的产物。
- **别在 `trunk build` 写 dist 的同时跑 `cargo build`**：Tauri 在编译期读 `crates/tr-ui/dist`
  做资源内嵌，两个进程并发时可能嵌到半新半旧的资源（表现同样是空白窗口）。
  集成顺序固定为：`build/build-ui.ps1` 先跑完，再 `cargo build`。
- **配置目录可被 `USERPROFILE` 覆盖**：`config.rs` 的 `home_dir()` 先读 `USERPROFILE`（非 Windows 读 `HOME`）。
  `fixtures/smoke.ps1` 正是靠这一点把冒烟启动的实例指向一次性配置目录，
  不会碰用户真实的 `~/.transactions.json`；手测 release 版时也可以这么隔离。
- **别按进程名判断"应用是否在运行"**：本机别的目录下可能也装着同名的 `Transactions.exe`
  （例如 `D:\software\Transactions\`）。判重要按完整路径，不能按进程名，
  否则会误判，甚至误杀用户正在用的应用。

### UI 自动化（fixtures 的踩坑）

**对话框、路径、cwd**

- **原生对话框的窗口归属分三种**（找错地方就会"对话框没弹出来"）：
  * WebView2 自己的文件框（`<input type=file>`，`#32770`、标题「打开」，见 `fixtures/ui-upload.ps1`）：
    它是应用窗口的子窗口，`ProcessId` 属于 `msedgewebview2.exe` → 在应用窗口元素的
    `TreeScope::Descendants` 里按 `ClassName='#32770'` 找。按 `RootElement` 的 Children 找
    或按应用进程号过滤都找不到，表现成"点了按钮但没弹框"。
  * Tauri `dialog_open` 插件的选文件/选目录框（见 `fixtures/ui-diary-io.ps1`）：桌面顶层窗口，
    `ProcessId` 就是应用自己 → 在 `RootElement` 的 Children 里按进程号找。
  * DevTools 窗口（设置 → 通用 → 开发者工具 → 「打开」）：也是桌面顶层窗口，但 `ProcessId` 属于
    `msedgewebview2.exe`（`cls=Chrome_WidgetWin_1`、`name='DevTools - …'`），按应用进程号枚举永远
    看不到它。要验证"按钮是否真的打开了 DevTools"就扫全系统顶层窗口按名字/类名筛。
  两类框的路径输入框 `AutomationId` 不同：选文件 = 1148（`文件名(N):` 组合框）、选目录 = 1152
  （`文件夹(F):` 编辑框）。选文件时回车等于「打开」；选目录时回车只是进入该目录，必须点「选择文件夹」，
  而且它只接受已存在的目录，否则弹「…不存在」。
- **填进原生对话框的路径必须是绝对路径**（被用户当场抓到过）：选目录框按它自己的"当前目录"解析相对路径，
  脚本传 `-OutDir target\pkg-diary` 时导入/导出就落到了别处（甚至弹「没有找到匹配的项目」）。
  同一个坑还有第二个受害者：脚本把相对路径传给以 `-WorkingDirectory <别的目录>` 启动的被测进程时，
  它按自己的 cwd 解析 `-workspace` 这类参数，而驱动用相对本仓库的 `xtask dump <同一相对路径>` 去找库，
  于是库落到别的地方——现象极具欺骗性，进程启动正常、前面几十次调用全绿（它们只跟进程说话），
  直到第一个落库断言才炸，trap 还把原因吞成了 `ScriptHalted`（我为此白查了三轮）。
  现在所有 fixture 入口都做 `GetFullPath` 归一化（`-OutDir` 也在内），`Select-Directory` 里另有
  `IsPathRooted` 断言兜底，调 `xtask dump` 时把 cargo 的 stderr 收进异常（原来写 `2>$null`，
  只剩一句"dump 失败"）。
  教训：断言走文件系统、被测进程走另一个 cwd 时路径必须绝对化；断言红了先确认两侧说的是同一个库。
- **选目录的正确姿势**（`fixtures/ui-diary-io.ps1` 的 `Select-Directory`，逐条都踩过）：
  1. 把绝对路径**粘贴**进底部「文件夹(F):」框
     （用共享版 `Set-Clipboard` + `Ctrl+A` + `Ctrl+V`，别逐字符 `SendKeys`：中文输入法会把 `\` 变 `、`）；
  2. 回车进入该目录（不要用地址栏 Alt+D 导航），再点「选择文件夹」。它受"对话框记住的上次目录"影响，
     粘贴偶尔不生效就停在上次那个目录上，结果"选错了却看不出错"；
  3. 验证导航到位（面包屑每一段都是独立元素，出现目标目录名才算到位），没到位就重试；
  4. 断言要看结果（导出文件落在哪、库里多了哪几行），不要只看"对话框关了没有"——
     提交方式选错时对话框确实关了，却什么都没发生。
- **WebView2 文件框的提交用「右方向键 + 回车」**，别的都不行：直接回车 = 接受 shell 自动补全，
  完整路径被换成裸文件名 → 「找不到文件」；`TAB` 能收起补全下拉但焦点移到"文件类型"，回车不再触发默认按钮；
  `WM_COMMAND(IDOK)` 会关掉框却跳过 modern 对话框"把文件名变成选中项"的内部步骤，等同于取消。
  也别用 UIA 坐标点「打开(O)」：底部那行是 legacy provider，报出的矩形不可信（「打开」的 rect 恰好等于
  对话框下边缘）；更不能按 `AutomationId` 找控件，文件列表项（`.agents`、`.ssh`…）的 AutomationId
  恰好是 1..N，`id=1` 会点中列表第一项。另外临时 HOME 里要先建 `Desktop` 目录，否则文件框起始目录
  不存在，会先弹「位置不可用」。

**UIA 树的读法**

- **"空矩形"元素是幽灵**：没真正渲染出来的元素 `BoundingRectangle` 报 ±∞（`X/Y=+∞`、`Width/Height=-∞`），
  `IsOffscreen` 却仍可能是 `False`。它一度让 `fixtures/ui-link-event.ps1` 全绿却什么都没发生。判据
  `Width -le 0` 挡得住 `-∞`（但 `+∞` 参与 `[int]` 转换会抛"无法将值 "∞" 转换为类型 "System.Int32""），
  所以必须显式排除非有限值（`[double]::IsInfinity/IsNaN` 四个分量都查一遍）；而且要轮询等真正渲染出来的
  那个出现，不要固定 sleep 后取第一个同名元素——命中的很可能是页面里没显示、格子却还挂在树上的另一个
  日期选择器，点它当然毫无反应。
- **浮层里的按钮别拿 `IsOffscreen` 当判据**（真实假红：`ui-crud` 的「删除图表」气泡确认、
  `ui-transactions` 排序弹窗的「应用」）：弹窗/气泡是 portal 出去的浮层节点，UIA 可见性不稳
  （有时报 `IsOffscreen=true`），而重渲染留下的幽灵节点反而报 `false`。两条正解：① 按名字取可见且在
  窗口内的**最后一个**（`ui-transactions` 的 `Invoke-ButtonByName`、共享版的 `Add-Record` 都这么做）；
  ② 只排除 ±∞ 幽灵、不过滤可见性（`ui-crud` 删图表/删事件）。更稳的是别把"点到没点到"当断言，
  改成断言结果（库里少一行、表格顺序变了），点击只做尽力而为。
- **`Input` 的 UIA 判据是 `ControlType.Edit`，不是 ClassName**：本项目的输入框渲染成
  `class='ui-input__control'`，用 `ClassName -eq 'Edit'` 过滤一个都找不到（当时表现为"建仓弹窗里找不到
  股票代码输入框"，整轮全红）。只有原生 `<textarea>` 那类才是 `ClassName='Edit'`，所以判据要写成
  "`ControlType` 是 Edit 或 ClassName 是 Edit"。
- **页签是 `TabItem`**（用 `SelectionItemPattern`），不是 Button，按 Button 找只会静默失败。
- **Chromium 的 UIA 树是惰性构建的**：窗口刚出现时首次查询常只返回二十来个元素、连 Button 都没有，
  要轮询反复查询把它唤醒，所以所有脚本都是"轮询到标记出现"，而不是固定 sleep。
- **启动时要抓「主窗口」而不是「该进程的第一个窗口」**：启动期先出现初始化窗口（600×560、无侧栏），
  随后才切成主窗口；抓到前者后面所有按名字的查找都会落空（整轮 26 项全红的假故障）。稳妥做法见
  `fixtures/ui-crud.ps1` 的 `Get-ReadyWindow`：轮询取窗口元素直到它包含侧栏条目（如「记账」）为止，
  每轮重新查询也顺带规避句柄失效。
- **同名按钮要按"可见 + 在窗口矩形内"筛**（`fixtures/ui-stock.ps1` 的 `Find-VisibleButton`）：
  同一页面里可能有多个同名按钮（未展开的面板、另一个页签里也在 DOM 里），按名字取第一个常拿到
  不可见的那个，点它毫无反应。`Find-VisibleButton` 会同时校验 `IsOffscreen=false` 与矩形落在窗口内；
  弹窗的确认按钮还常与页面入口同名
  （页面上有「支取」，弹窗确认也叫「支取」），要取最后一个（弹窗在 DOM 末尾）或用包围盒位置区分。
  股票下单弹窗尤其明显：代码/价格/手数三个输入框在 UIA 里没有名字，只能按包围盒定位 `Edit`
  （同一列 Y 递增、同一行 X 分列），再靠 `ValuePattern` 塞值，别指望按 label 找。

**写值与点击**

- **受控 `<input>` / 文本域要把值真的"打"进去**：`ValuePattern.SetValue` 不保证触发 DOM `input`
  事件（日记/记账这类页面的自动保存挂在 `input` 上），Leptos 的 `<input value=signal>` 会在下一次重渲染时
  把 DOM 值刷回信号里的空串——字段看着被填过，点保存却弹「请输入金额」，而脚本的 `Set-Value` 还返回 `true`
  （实测："记一笔"弹窗填金额）。纯靠剪贴板 `Ctrl+V` 也不行：窗口不是前台时
  `AutomationElement.SetFocus()` 静默无效，`Ctrl+A/Ctrl+V` 贴到别处，"内容没改、保存却成功了"，
  看着像偶发假绿/假红。共享版的做法（新脚本直接用，别再各写一份）：`Set-InputByPaste`（单行，
  `Find-EditByName` 只认 `ControlType`/`ClassName` 是 Edit 的节点）与 `Paste-Text` / `Set-DiaryContent`
  （文本域）——先 `SetForegroundWindow` → `SetFocus` → 校验 `FocusedElement` 是 Edit → 粘贴 →
  用 `ValuePattern` 读回校验 → 失败整段重试（最多 3 次）。
- **只读的"行内操作按钮"必须先 hover**（`fixtures/ui-crud.ps1` 的 `Find-RowButton`）：分类/标签行的操作区
  是 `.ct-item-actions { display: none }`，只在 `:hover` 或 `.is-active` 时才 `display: flex`
  （刻意的交互约定，不是缺陷）。`display: none` 的元素不进 UIA 树，所以"新建的那一行能删、别的行删不了"
  ——新建的行是 active。做法：先用真实鼠标把指针移到行中心、等 ~0.5s，再按名字查按钮，
  并用"中心 Y 最近且在该行右侧"区分同一列里的多个「删除」。
- **别用 `| Select-Object -First N` 截断界面脚本的输出**：管道提前关闭会终止上游脚本，它的 `finally`
  （关掉测试实例）不执行，于是下一个脚本会因"本仓库已有实例在运行"而拒绝启动。我为此白查了一轮。
  要么 `-Last N`，要么 `*> 文件` 再读文件。
- **"复用工作空间"的判断要在默认值赋值之前取**（踩过）：脚本常写成
  `if (-not $Workspace) { $Workspace = <默认路径> }` 之后再 `if (-not $Workspace -or ...) { 重新播种 }`，
  但那时 `$Workspace` 已经非空，条件恒为 false → 永远不重新播种。断言"持仓数量"这类绝对状态时必须每次
  重新播种，否则上一轮的持仓会叠加（实测 300 股变 600 股，后面全崩）。做法：函数开头先
  `$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)`。
- **托盘图标能抓到，但浮出面板太"脆"，所以没做成常驻护栏**：Win11 下我们的托盘图标在
  `TopLevelWindowForOverflowXamlIsland`（名字「系统托盘溢出窗口」）里，是一个 `Button`，
  `Name='Transactions'`；点任务栏的「显示隐藏的图标」按钮能把它弹出来，右键会开一个 `#32768` 菜单。
  但实测两次里有一次浮出面板没弹出来（面板失焦即关、出现时机不固定），做成 fixture 会变成红绿随机，
  所以托盘菜单交互仍留在人工清单里。要再试的话，探针在 `target/tray2-probe.ps1`（不随仓库提交）。
- **股票下单 + 行情链路的验收配方**：一条流程同时验四件事。持仓页 → `建仓` 弹窗 → 代码填 `600519` →
  点「查询股票名称」，名称框会自动变成「贵州茅台」（这一步打通了真实行情接口）→ 填价格/手数 → 提交。
  随后断言三处：页面行显示现价/涨跌幅/浮盈、`tbl_billadm_stock_trade` 多一笔、持仓数量与成本对得上
  （成本含手续费：`价格×股数 + 佣金`，单位是分）。

### 界面与外壳的真实缺陷

- **Tauri 默认会吃掉页内 HTML5 拖拽**（真实缺陷，"分类/标签/模板拖不动"）：
  Tauri 窗口默认 `dragDropEnabled: true`，wry 于是在 WebView2 宿主 HWND 上 `RegisterDragDrop`
  并 `SetAllowExternalDrop(false)`（`wry-0.55.1/src/webview2/mod.rs:150`）；而 Chromium 在 Windows 上的
  页内拖拽也走 OLE 拖放，所以 `drop` 永远到不了页面。现象很有欺骗性：
  `dragstart`/`dragover` 都正常（行会变半透明、插入指示线也会画），只有 `drop` 不触发。
  修法：建窗口时调 `.disable_drag_drop_handler()`（`shell.rs` 的 `create_main_window` / `create_init_window`）。
  代价是拿不到 `tauri://drag-drop` 原生文件落盘事件。回归：`fixtures/ui-drag.ps1`。
- **关掉拖放处理器之后必须补导航守卫**（`shell.rs` 的 `is_allowed_navigation`）：Tauri 的拖放处理器
  同时也"吃"掉了拖入文件时的默认导航，`disable_drag_drop_handler()` 之后把文件从资源管理器拖进窗口会让
  WebView2 直接导航到 `file:///…`，界面整个被换掉。所以加一道守卫挡住这类导航：只放行
  `tauri.localhost` / `localhost` / `127.0.0.1`，其余（file:、外部站点、data:）一律拦截；
  `trasset://` 是子资源，不走导航，所以不受影响。单测覆盖放行/拦截两侧。
- **窗口几何的单位是逻辑像素（DIP），不是物理像素**（真实缺陷，"记不住窗口大小和位置"）：
  Windows 上 `window.inner_size()` / `outer_position()` 返回物理像素，
  而 `WebviewWindowBuilder::inner_size()` / `position()` 收的是逻辑像素；
  界面配置（`~/.transactions.json`）里存的窗口几何必须是逻辑像素。若把物理值原样存回去，
  150% 缩放的机器上窗口每次启动都会放大 1.5 倍、位置越跑越偏。
  `save_window_bounds` 现在做物理 → 逻辑换算（纯函数 `logical_bounds` + 4 个单测），
  回归：`fixtures/window-bounds.ps1`（启动 → 关闭 → 再启动，尺寸/位置必须一致）。
- **`Popconfirm` 的触发必须挂捕获阶段**（真实缺陷）：调用方常在子元素上写 `stop_propagation()`
  （列表项里的删除按钮为了不触发整行"选中"），若触发挂在冒泡阶段就会被吃掉，气泡不会弹，
  「删除图表」「删除事件」「删除关联交易」三处都因此点不动。`popconfirm.rs` 现在用 `on:click:capture`。
  验证方式分两侧看：触发侧看气泡是否弹出（标题 + `取消/删除` 按钮）；确认侧用设置页删模板
  （普通 `<Button>` 子元素）走一遍"气泡 → 点确认 → 库里行数 -1"，它与那三处用的是同一个组件、
  同一条确认链路。改动共享组件后请用 `fixtures/ui-smoke.ps1` 做回归。
- **弹窗里的下拉面板会被 `overflow: hidden` 裁掉**（真实缺陷）：DatePicker / Select 的下拉都是绝对定位的
  子元素，而 `.ui-modal__content` 原来带 `overflow: hidden`（只为圆角），小弹窗（如「关联事件」，
  只有一个表单项）里日历被裁到只剩月份标题和星期行，日期格子看不见也点不动；中等高度的弹窗
  （事件新增、日记编辑等）则被裁掉一半，看着像"面板画坏了"。现已改成 `overflow: visible`，
  圆角不依赖裁剪：header/footer 都是透明底、只有一条边框线，body 有内边距，没有子元素会画到圆角外。
  下拉面板的正确做法是 portal 到 body，挂在弹窗内容里就必然受裁剪影响。排查提示：UIA 里看不到"被裁掉"
  （被裁元素的矩形照样报出来），只有截图（全屏 PNG）才看得出"元素其实没画出来"。
- **`scrollbar-width` 会让 `::-webkit-scrollbar` 整套失效**（真实缺陷，且它把设计"静默作废"了）：
  CSS 规范里只要 `scrollbar-width` / `scrollbar-color` 被设成非 `auto`，`::-webkit-scrollbar-*`
  伪元素就整体不生效。`base.css` 原来那个 `.u-custom-scrollbar` 两个都写，
  于是那套"5px 圆角滑块 + 透明轨 + 悬浮加深"从未渲染过（而且它全仓一个使用者都没有）。
  本机实测占用宽度（WebView2 / Chrome 153，DPI 1.5）：原生 17px / `scrollbar-width:thin` 11px /
  只写 webkit 6px。现在滚动条是全站默认（`base.css` 第 1 节的「浏览器自带的表面」），
  只用 `::-webkit-scrollbar`，不要再加 `scrollbar-width`。
  另：`body { user-select: none }`（桌面窗口惯例）让全局 `::selection` 基本只在输入框里可见，
  同节的 `caret-color` 与选区配色是配套的。
  排查提示：`::-webkit-scrollbar` 的效果从 `getComputedStyle` 读不到，
  要么查 CSSOM 规则、要么量 `offsetWidth - clientWidth`、要么看像素。
- **Tip 的宽度：只写 `max-width` 会被"包含块"坑死**（真实缺陷："应用设置 → 股票 → 过户费 ⓘ"
  的气泡被压成一列窄条、贴着窗口右缘）：气泡是 `.ui-tooltip::after` 的绝对定位伪元素，
  包含块是触发器本身（那个 ⓘ 只有 ~14px 宽），于是 shrink-to-fit 的"可用宽度"就是 14px。
  只给 `max-width: 320px` 的话文案会被压成十几像素宽的一列（还带出滚动条）。
  正确写法是先用 `width: max-content` 按文案铺开，再用 `max-width` 收上限
  （上限同时受视口约束：`min(320px, calc(100vw - …))`，贴窗口边的提示不再顶出去被裁）。
  换行语义用 `white-space: pre-line`：普通短提示与 `nowrap` 表现一致，而带显式 `\n` 的多行说明
  （如股票费用那三条）能按原样折行。触发器在版心右缘时套 `.ui-tooltip--end`（右对齐向左铺开），
  居中的长气泡在那会越出窗口。
- **别把 `PrintWindow` 截图里的"残缺图标"当成缺陷**：`fixtures/ui-shots.ps1` / `dev-shot.ps1`
  抓的是 `PrintWindow(PW_RENDERFULLCONTENT)` 的位图，WebView2 在某些状态下**合成器还没画完**，
  于是内联 SVG 图标会被抓成一条竖长的涂鸦（正文文字、布局、表格都正常）。
  实测同一份产物：截图里图标残缺，人眼看窗口完全正常（用 `dev-shot` 直接抓也一样）。
  判据优先级：**人眼看窗口 > UIA 树里控件的矩形 > 截图像素**。怀疑图标坏了先开一次窗口看看，
  别照着截图去改图标路径或 CSS（`icons.rs` 的 `view_box` 坑是另一回事，那条看路径坐标范围）。
- **删除共享的 CSS 规则前先 grep 类名**（同一条纪律见下面那条"删/改一条 CSS 规则"）：
  整段删掉一个选择器时要确认没有别处还在用它，否则那块样式会静默退化成默认值。
- **删/改一条 CSS 规则前先 grep 谁还在用那个类名**（真实缺陷："应用设置 → 关于软件"的内容
  不再垂直居中）：`8d55b17` 把设置页的面板从 `.st-pane` 换成 `.page-pane`（前者整套规则被删），
  但 `<div class="st-pane st-about">` 这个类名漏改了。它退化成普通块级容器，
  于是 `.st-about` 的 `align-items/justify-content: center` 全部失效、`margin: auto` 也垂直居中不了
  （`auto` 外边距只在 flex/grid 容器里吃掉剩余空间），内容就停在顶部。
  症状很有欺骗性：样式没报错、面板照常渲染，只是"居中没了"。
  改 CSS 结构时用 `grep -rn 'st-pane\b' crates/tr-ui` 之类核一遍调用点，别只改样式。
- **详情区「减仓/清仓/加仓」点不动 = 真实缺陷**（曾被我误记成"UIA 自动化不了"，其实是产品 bug）：
  三个按钮的 `on_click` 里先 `selected_code.set(code)` 再 `open_trade(...)`。详情区本来就按
  `current_position()`（= `selected_code` 命中的那条）渲染，写的是同一个值；但 `RwSignal` 同值写入
  依然会通知订阅者 → 点击瞬间详情子树重渲染 → 交易弹窗永远不 mount（DOM 里连 `ui-modal__mask` 都没有）。
  表头那个不写 `selected_code` 的「建仓」按钮一直正常，对比之下才看出差别。
  怎么查出来的（这套手法留着复用）：先证明"点击确实送达"。真实鼠标 / `SetFocus`+回车 / `InvokePattern`
  三种激活都试，同时用 `AutomationElement.FromPoint` 与 `WindowFromPoint` 确认坐标上是谁
  （Tauri 的 `TAURI_DRAG_RESIZE_BORDERS` 浮层会抢答 `FromPoint`，但鼠标其实照样进 WebView，
  所以 `FromPoint` 单独用会误判）；再往 handler 里塞 `Notifier` 提示，确认 handler 跑了、`trade_open=true`；
  最后在同一视图里并排放裸闭包节点与 `<Show>` 探针（都正常反应）＋弹窗体里放挂载标记（始终不出现），
  才锁定"信号写对了，是 Modal 没挂载"。教训：先分清"没点到"和"点了没反应"，
  别用"这按钮自动化不了"给产品的 bug 打掩护。
  修法：三处去掉冗余的 `selected_code.set`（`src/pages/stock.rs` 有注释）。
- **不要在 `Effect` 体内创建信号**（同一个坑已经踩过两次，两次都表现为"界面空白 / 点了没反应"）：
  `Effect` 每次重跑都会 dispose 上一次创建的 reactive 值，而界面那时往往还在读它们。
  现象不是报错闪退，而是整块视图不渲染，或者点击毫无反应，只有控制台里有一句
  `you tried to access a reactive value ... but it has already been disposed`。
  * 第一次：`settings.rs` 的 `UpdateState`（「关于软件」）首次在组件里创建 → 切走再切回面板空白；
    修法是把状态提前建到根 owner（`init_update_state()` 由 `shell::App` 调用）。
  * 第二次：`transactions.rs` 排序弹窗在 `Effect` 里 `SortRow::new(...)` → 关掉再打开必 panic；
    修法改成"行信号池"：最多 4 行的信号在页面 owner 下一次建好，打开只回填值、
    增删只改一个 `count`（见 `sort_modal` 注释）。
  正确姿势：状态建在组件/根 owner 下，`Effect` 里只读、只做副作用。

### 更新与代理

- **更新链路的两个纯函数已抽出来单测**（`updater.rs`）：`parse_release`（release JSON → 更新信息：
  跳过预发布、`v` 前缀、取第一个 `.exe` 资产、body 缺失给空串、没有 `.exe` 仍算"有更新"）
  与 `digest_matches` / `normalize_digest`（`sha256:ABCD…` 大写去前缀后比较；缺失/空串则跳过校验）。
  界面上的「检查更新」另有实测：真实 GitHub API 返回「已是最新版本」（探针 `target/update-probe.ps1`）。
- **自动更新是自研实现**：代码在 `src-tauri/src/updater.rs`，没有用 `tauri-plugin-updater`，
  沿用 GitHub Releases + `asset.digest`(sha256) 校验的既有发布管线，不需要签名密钥与 `latest.json`。
  命令：`update_check` / `update_download`（发 `update:download-progress|complete|error` 事件）/ `update_cancel` / `update_install`；
  行为：仅 GitHub 域名白名单、已下载复用、`.part` 中转、取消清理、打开安装包后退出。
  因此 `tauri.conf.json` 里**不要**加 `plugins.updater`，capabilities 里也不需要 `updater:default`。
- **代理：三态设置 + 自动探测系统代理**：以前客户端不读系统代理，这个偏差已经修掉。
  「应用设置 → 通用设置 → 代理」= `off`（不使用）/ `auto`（自动探测，默认）/ `manual`（手动 `http://host:port`），
  落在 `~/.transactions.json` 的 `proxy: { mode, url }`（缺该键的老配置按 `auto` 处理）。
  * 生效路径：配置由外壳持有，`src-tauri/main.rs` 启动时调 `tr_service::proxy::set(...)`，
    `config_set_proxy` 落盘后立即再推一次；`tr-service/src/quote.rs`（行情）与
    `src-tauri/updater.rs`（更新检查/下载，`agent()` 是两处共用的唯一入口）都从同一处取，
    不会出现"更新走代理、行情不走"的半生效。`tauri-plugin-opener` 打开浏览器那次跳转不受影响。
  * `auto` 的探测顺序：`ALL_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY`（含小写变体，与 ureq 的
    `Proxy::try_from_env()` 同序）→ WinINET 注册表 `HKCU\…\Internet Settings`
    （`ProxyEnable=1` 时取 `ProxyServer` 的 `http=` 段）→ 同路径 `HKLM` → 直连。
    只读注册表（`winreg`，`cfg(windows)`），每次请求重新探测，改系统代理不必重启。
  * 只支持 HTTP 代理：地址只认 `http://`（`host:port` 会自动补 scheme；可带 `user:pass@`）。
    `socks*://` 与 `https://` 明确拒绝并给中文文案；HTTPS 目标（GitHub）走同一个 HTTP 代理的 CONNECT 隧道。
    不支持 PAC 自动配置脚本（`AutoConfigURL` 只作为提示上报，不解析），也不支持 `ProxyOverride` 绕过列表
    （环境变量路径下由 ureq 的 `NO_PROXY` 处理）。
  * `off` 会显式 `.proxy(None)`：ureq 的 `Config::default()` 本身就会读环境变量代理，
    不显式覆盖的话"不使用代理"会名不副实，这也是本仓库唯一一处必须传 `Option` 的地方。回归见
    `fixtures/ui-proxy.ps1`（护栏清单里那条）。
  * 失败形态：代理不可达时更新检查报错、行情静默失败（计入既有的 `quote_failed_count`），都不 panic；
    手改配置成非法值时按"未配置"处理（直连）并记日志。

## 关键约定与陷阱

- **SQL 只允许拼接常量**：列名/表名用 `const …_COLUMNS` 或常量数组（如 `STOCK_TABLES`），
  值一律走 `?` 占位符（`instr(description, ?)` 也是占位符）；`ORDER BY` 的字段必须过白名单，
  也就是 `build_sort_clause` 只认 `transactionAt` / `transactionType` / `price` / `category` 这 4 项，
  多一项都不认，排序方向强制 `asc|desc`。改 DAO 时别把请求里的字符串直接拼进 SQL。
- **生产代码里的 `unwrap/expect/panic!` 必须有据可依**：允许的只有锁中毒
  （`.expect("…锁中毒")`）、已校验不变式（月份 `1..=12`、`valid_up_to` 前缀、池在生命周期内有效）、
  以及启动期构建失败（`main.rs`）。新增这类调用前先问"它真的不可失败吗"。
- **金额恒为整数分**：数据库、IPC、算法全用 `i64` 分；只有展示层做分/元换算
  （`tr_domain::money`）。这两个换算函数的行为是硬契约（含负号、`.5` 输入）。
- **金额/时间戳语义**：`transaction_at`、`trade_time` 等是 Unix 秒；
  `%Y-%m` 之类的分桶在 SQL 里用 `strftime(..., 'unixepoch')` 完成，不要在 Rust 侧重算。
- **数据库结构变更只走迁移引擎**：`transactions.db` 不存在时用
  `fixtures/schema/fresh.sql` 建库（当前格式）；已存在时先由 `tr-store/src/migrations.rs`
  的迁移引擎按 `tbl_billadm_schema_migration` 登记表升级到当前格式，再按当前格式做只读校验
  （`schema::validate_current`）。校验不通过（比已知格式更早、且没有对应迁移）时明确拒绝，
  用户可见文案是 `该工作空间不是当前格式（格式过旧）：…`。
- **迁移的写法是硬规范**：`migrations::MIGRATIONS` 是唯一入口，新增迁移必须逐条满足下面几条。
  ① id 唯一稳定（`YYYYMMDD_描述`）；② 一个事务（执行 + 写登记行，失败整体回滚）；
  ③ 幂等、防御式（先查 `PRAGMA table_info` / `sqlite_master`，结构已在就只补登记行）；
  ④ 只碰本次升级涉及的表，不许"顺手修复"别的结构；⑤ 留单测（旧格式 → 升级后校验通过、
  数据一字不差、重复应用无副作用）。升级前必须先备份（`VACUUM INTO` 出
  `transactions.db.pre-migration-<时间戳>.bak`；备份失败就不升级），并且同一工作空间只保留最近一份：
  旧的 `.bak` 在新备份成功之后才清掉（备份失败时旧的那份还在），只认自己的命名规则，
  不动工作空间里别的文件。手工升级/验证用 `cargo xtask migrate <workspace-dir>`；`validate` / `dump` 仍然只读
  （`validate` 只报告待应用的迁移）。
- **IPC 契约**：命令统一只收一个 `req` 结构体参数，字段名是硬契约，改动即破坏兼容。
  成功时 promise 直接 resolve 为数据本身；失败时 reject 载荷为
  `{"code":-1,"msg":"...","status":500}`。`msg` 是用户可见文案，改动同样破坏契约。
- **JSON 字段命名不统一，但必须保持不变**：核心记账模型是 snake_case
  （`ledger.created_at`），事件/日记/股票模型是 camelCase（`ledgerId`、`createdAt`），
  DTO 里两种混用（`tr_query_result` 的 `page_size` 与 `trStatistics` 并存）。
  数据库列名恒为 snake_case，列映射在 DAO 层显式书写，不依赖 serde。
- **图片资产**：布局为 `<workspace>/data/assets/key_events/<date>/<uuid>.<ext>` +
  `thumb_<uuid>.jpg`，数据库存相对 `data/assets` 的路径。界面通过 `trasset://` 自定义协议访问，
  协议处理器带路径穿越校验（只允许落在 `data/assets` 下的相对路径）。
- **后端只接受 JPEG/PNG/GIF/WebP**：HEIC 转换留在界面层（P5 用 web-sys canvas 交给
  WebView2/系统解码器转成 JPEG 再上传），后端不引入 libheif/WIC。
  缩略图规则：宽度 > 300 时按比例缩放到 300（CatmullRom）、JPEG q75。
- **没有本地网络面**：不监听端口、没有 API 令牌、没有 CORS、没有子进程后端。
  进程即应用，不需要"后台服务异常→重启"那套机制。
- **首启动的窗口切换由外壳负责**：`workspaceDir` 为空时启动进入初始化窗口（600×560、不可缩放）。
  初始化窗口与主窗口加载的是同一个 `index.html`，界面在"未配置工作空间"时展示选择目录的引导。
  选完目录后 `workspace_open` 打开数据库，外壳随即 `show_main_window` + `destroy` 初始化窗口。
  因此界面不需要、也不要再调 `workspace_init`，那个命令只是保留的幂等入口，不参与首启动流程。
  曾经踩过的坑：`workspace_init` 全仓无调用点 → 首次启动只在一个不可缩放的 600×560 窗口里
  渲染整个应用，主窗口一直不出现（托盘"显示主窗口"才会补出来，于是变成两个窗口）。
- **配置文件是用户数据**：`~/.transactions.json`（dev 为 `~/.transactions-dev.json`）
  的键名与位置都不变，并且读写时必须保留未知键（`AppConfig.extra`）。
- **界面无 Node**：仓库里没有 npm/package.json，没有 vendor 的 JS 库。
  图表、Markdown、拖拽排序、日期选择等全部是 Rust 实现（见 `tr-ui`）。
- **设计令牌**：`--transactions-*` CSS 变量是颜色/尺寸的唯一来源（对应 `DESIGN.md` 的调色板；
  `PRODUCT.md` 历史文本里的 `#4A8E70` 是过时信息）。
  只支持浅色/深色两套主题（两套共用同一组令牌名），主题通过 `<html data-theme="light|dark">` 切换；
  历史上遗留的令牌允许保留。
- **供应商文档**：`crates/tr-ui/dist/` 由 trunk 生成，不入库。

## 发布

**远程仓库**：`https://github.com/ddd-online/Transactions-Rust`（分支 `main`，首次发布 `v0.1.0`）。
应用内更新检查与「关于软件」的 GitHub 链接、`build/release.ps1` 的 `$repo` 都必须指向本仓库
`ddd-online/Transactions-Rust`，否则会比对到不相干的版本、提示"有新版本"却下载到错误的安装包。
改动这三处时一并自检：
```powershell
Select-String -Path src-tauri\src\updater.rs,crates\tr-ui\src\pages\settings.rs,build\release.ps1 -Pattern 'ddd-online'
```

**版本控制**：`.gitignore` 已排除 `/target`、`/build/target`、`/crates/tr-ui/dist`、
`/src-tauri/gen/schemas`、`*.db(-wal|-shm)`、`transactions.log` 与 `/fixtures/private/`，
**真实工作空间数据绝不入库**。提交前请 `git status --short` 核一眼，别把本地验证用的库或截图带进去。

`build/clean.ps1` → `build/build.ps1`（trunk → cargo tauri build → 重命名产物为
`Transactions-x64-v{version}.exe`）→ `build/release.ps1`（`gh release create` 上传该 .exe）。
版本号唯一来源是 `src-tauri/tauri.conf.json`（`Cargo.toml` 的 workspace/`src-tauri` 两处也要同步）；
应用内更新读 release 的 `tag_name` 与首个 `.exe` 资产的 `digest`。
许可证以仓库根 `LICENSE` 为准（Apache-2.0）。

**踩过的坑：发布资产可能是上一版的安装包**（真实事故）：0.2.0 的 release 资产其实是 0.1.0 的安装包，
两个 release 的资产字节数与 `sha256` 一模一样，用户装完看到的还是 0.1.0 的界面。
根因是 `cargo tauri build` 不清 `target\release\bundle\nsis\`，上一版遗留的
`Transactions_0.1.0_x64-setup.exe` 与新的 `Transactions_0.2.0_x64-setup.exe` 并排存在，
而旧脚本用 `Get-ChildItem *-setup.exe | Select-Object -First 1` 取字典序第一个（0.1.0 在前）。
现在 `build.ps1` 会在构建前删掉陈旧安装包、只认 `Transactions_{版本}_x64-setup.exe`，
并断言它的 `LastWriteTime` 晚于本轮构建开始时刻；安装包与便携版都必须落盘成功，否则非零退出。
根子上还是那条老教训：**别只信退出码，也别按"第一个匹配"取产物**。发布前用
`fixtures/ui-about.ps1` 对着产物核一次自报版本号，发布后再 `gh release view <tag> --json assets`
核对 `digest` 与本地 `Get-FileHash` 一致（同一个 digest 出现在两个 tag 下就是发错了）。
