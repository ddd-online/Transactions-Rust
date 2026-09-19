# AGENTS.md

Transactions 是一款桌面个人记账应用。本仓库是它的**纯 Rust 重写版**：Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite 内核。
每个工作空间是一个独立的 SQLite 数据库。

参考实现（**只读**）：`D:\github\Transactions`（原 Electron + Vue 3 + Go/Gin 版本，v0.27.0）。
本仓库的职责是"行为等价 + 数据兼容"地把它重写为纯 Rust 栈。

另见：`PRODUCT.md`（产品定位）、`DESIGN.md`（设计系统，UI 的裁决标准）、
`docs/ACCEPTANCE.md`（逐页人工验收清单 + 与原实现的有意偏离）、
`docs/UI-KIT.md`（组件套件清单：28 个文件 / 约 32 个导出组件 ↔ 原 48 个 Vue 组件的对应关系）。

## 架构

```
crates/tr-domain/    # 纯领域层：models / dto / 金额换算 / 费用分摊（native + wasm 双可编，无 I/O）
crates/tr-store/     # 存储层：最新 schema 建库 + 只读格式校验 + 各 Dao（rusqlite）
crates/tr-service/   # 服务层：业务规则（账本/交易/…/股票），不依赖 tauri
crates/tr-ipc/       # IPC 命令面：全部 #[tauri::command] + 统一错误信封
crates/tr-ui/        # 界面：Leptos CSR（cdylib，仅编译到 wasm32）+ static/{css,fonts,icons}
src-tauri/           # 桌面外壳：窗口/托盘/配置/日志/trasset:// 资产协议/更新
xtask/               # 验证工具：schema-diff（数据兼容护栏）、parity（黄金对比）
fixtures/            # 最新 schema 基线、种子工作空间、黄金 JSON（**不含真实个人数据**）
```

分层纪律：

- `tr-domain` **不得**引入任何 I/O 依赖（rusqlite / reqwest / tauri 都不行）——
  它同时被 native 侧与 wasm 侧依赖，界面与后端因此共享同一份金额/费用算法。
- 只有 `tr-ipc` 依赖 `tauri`；业务规则必须能在没有窗口的环境下用 `cargo test` 验证。
- `tr-store` / `tr-service` 不参与 wasm 编译（`cfg(not(target_arch = "wasm32"))`）。

## 常用命令

```powershell
# 类型检查 / 测试（不含桌面外壳）
cargo check -p tr-domain -p tr-store -p tr-service -p xtask --all-targets
cargo test  -p tr-domain -p tr-store -p tr-service

# 界面（WASM）：开发服务 / 发布构建（发布必须走脚本，见下方 wasm-opt 说明）
trunk serve --config crates/tr-ui/Trunk.toml      # http://127.0.0.1:1520
powershell -NoProfile -ExecutionPolicy Bypass -File build/build-ui.ps1   # 发布用界面

# 桌面应用：开发 / 打包（会自动先跑 trunk）
cargo tauri dev
cargo tauri build                                 # 产出 NSIS 安装包

# 验证护栏
cargo xtask schema-diff                           # Rust 建库结构与基线逐条一致
cargo xtask schema-diff --go-db <path>            # 直接与 Go 0.27 建出的库比对
cargo xtask validate <workspace-dir>              # 只读校验既有工作空间是否最新格式
cargo xtask seed <workspace-dir>                  # 新建并播种一份示例数据（人工冒烟/黄金对比用）
cargo xtask dump <workspace-dir> [--table <name>] # 只读导出业务表为规范化 JSON
cargo xtask parity normalize|diff <json…>         # 黄金对比的归一化与差异报告

# 数据级黄金对比（一条命令，验收护栏）：
#   同一批输入分别由 Go 参考实现（HTTP 内核）与 Rust 侧写入两个全新工作空间，
#   再逐表逐字段比较落库结果；退出码 0 = 一致。
#   当前覆盖面：阶段 1（新建）+ 2（更新/删除）+ 3（股票减仓/多轮次/预演）+ P1（账本改名、
#   图表更新、4 个 sort_order、关键事件覆盖写与删除、非法目标预演）——go-driver 里
#   **66+ 个直接写调用 + 8 次"记一笔"+ 22 次回查**，18 张表全列逐字段比对。
#   ⚠ 已知的**非确定性来源**（不是 bug，但决定"红了先怀疑谁"）：股票资金记录若在同一秒落多条，
#   Go 侧（GORM 秒级 `autoCreateTime`）相对顺序可能退化，而 Rust 侧把 `created_at` 拉成严格递增
#   （见"有意偏离"）。两侧现金链都按录入顺序取前值，顺序一变余额就变；重放会在一秒内重写整批派生
#   资金记录，所以这类并列**无法完全消除**。脚本会打印同秒组的提示——遇到"差异只出现在
#   `tbl_billadm_stock_fund_record[*].cash_balance`"的红色，先重跑一次再判断。
pwsh -File fixtures/parity/run-parity.ps1 [-OutDir target\parity] [-Port 29143]

# 设计令牌护栏：tokens.css 之外不得有硬编码颜色、引用的令牌必须已定义、
# 深色主题与 prefers-color-scheme 兜底必须覆盖同一组令牌。
pwsh -File fixtures/design-audit.ps1

# 端到端冒烟：真的把应用起来，验证"已配置工作空间→只有主窗口"与
# "首次启动→只有初始化窗口"，并断言界面确实启动（日志里有 IPC config_get）。
# 用临时 USERPROFILE 启动，碰不到你真实的 ~/.transactions.json。
pwsh -File fixtures/smoke.ps1 [-Workspace <既有工作空间>] [-Exe <exe>]

# 逐页界面冒烟：用 UI Automation 驱动真实窗口，挨个点开 7 个页面并断言内容渲染；
# `-WriteFlow` 还会在**工作空间副本**里通过界面记一笔，验证"弹窗→填表→保存→列表出现"闭环；
# `-Discover` 导出每页元素清单，用来维护脚本顶部的页面标记表。
pwsh -File fixtures/ui-smoke.ps1 [-Workspace <ws>] [-WriteFlow] [-Discover]

# 跨 crate 契约审计：界面 api/*.rs 的请求结构体是**手抄** tr-ipc 的（不能共用类型，见下），
# 抄错字段名不会编译报错、serde 只会静默取默认值。这条按命令逐字段比对两侧。
pwsh -File fixtures/contract-audit.ps1

# 像素级逐页验证 + 主题切换验证：抓窗口位图断言每页不是空白，并比较浅/深色平均亮度；
# 同时把 7 张 PNG 落到 target\ui-shots\（人工验收可以先翻图）。补 UIA 的盲区。
pwsh -File fixtures/ui-shots.ps1 [-Workspace <ws>] [-OutDir <dir>]

# 图片上传端到端：真的点「添加图片」拉起**原生文件选择框**、选一张自己生成的 600×400 PNG，
# 断言 原图按原字节落盘 + 缩略图缩到 300×200 且同目录同名前缀 + 库里 file_path/thumb_path/event_date
# + 界面出现「下载图片」。失败会留一张全屏截图（target\upload-smoke\failure.png）。
pwsh -File fixtures/ui-upload.ps1 [-Workspace <ws>] [-OutDir <dir>]

# 关闭行为三分支：quit → 进程退出；tray → 进程存活且窗口隐藏；空 → 弹「关闭选项」框，
# 选「是」后退出。原生询问框的按钮在 UIA 里是 `是(Y)`/`否(N)`（Pane 类型），脚本已兼容。
pwsh -File fixtures/close-behavior.ps1 [-Exe <exe>] [-Workspace <ws>]

# 拖拽排序端到端：真实鼠标（按下 → 20 段移动 → 抬起，OS 级输入对 Chromium 就是真拖拽）
# 把第 1 个分类拖到第 3 个位置，断言 顺序变化 + sort_order 落库 + 界面同步 + 重进页面仍一致。
# 它锁的是"Tauri 默认 dragDropEnabled 会吃掉页内 HTML5 拖放"这个真实缺陷（见下方经验）。
pwsh -File fixtures/ui-drag.ps1 [-Workspace <ws>] [-OutDir <dir>]

# 窗口几何往返：用逻辑尺寸启动 → 断言物理窗口 = 逻辑 × DPI 缩放 → 关闭 → 断言配置里写回的仍是
# 逻辑值 → 再启动一次断言尺寸/位置完全一致（"记住上次的窗口大小和位置"的回归测试）。
pwsh -File fixtures/window-bounds.ps1 [-Exe <exe>] [-OutDir <dir>]

# 日记导入/导出端到端：驱动**原生选目录对话框**导入一个临时目录（UTF-8 + GBK + 一个非法文件名），
# 断言 落库（正文逐字节、word_count 按标量值、非法文件名被跳过）→ 再导出到空目录 →
# 断言文件数/命名/正文与库一致。数据链路另有 `cargo test -p tr-service diary::` 的单测。
pwsh -File fixtures/ui-diary-io.ps1 [-Workspace <ws>] [-OutDir <dir>]

# UI 增删改端到端（断言都落在数据库上）：分类/标签/图表的"新增→删除"、
# 关键事件"点色板改颜色 / 写 Markdown 描述 / 删除事件"、设置页"新建模板→删除"。
pwsh -File fixtures/ui-crud.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 同步到其他账本端到端（此前**零覆盖**：IPC 里没有 sync 命令，界面是"复制 DTO + 换账本 + 清 id"）：
# 记一笔 → 点行内「同步到其他账本」→ 选目标账本 → 断言 目标账本多一份**新 id** 的副本、
# 金额/类型/分类/记录时间一致、**源记录保留**（复制而非移动）→ 切账本后界面里能看到副本。
pwsh -File fixtures/ui-sync-ledger.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 消费记录页：记三笔 → **编辑一笔**（断言"先建后删"：换 transaction_id、旧记录消失、行数不变）
# → **保存为模板**（记一笔弹窗里的子弹窗：填名称 → 断言模板落库且类型/分类/描述取当前表单）
# → **排序**（重置 → 加「金额 降序」→ 应用 → 断言表格区金额序列非递增、最大值排第一）
# → **筛选**（悬浮按钮 → 关键词 → 添加条件 → 确认 → 断言收敛到 1 条）。
pwsh -File fixtures/ui-transactions.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 股票全生命周期端到端：建仓（真实行情查名）→ **编辑成交** → **删除委托** → **减仓** → **清仓** → **费用设置生效** → **重置股票数据** → 界面展示。
# 断言 成交（价按分/手数/股数/成交额/手续费）→ 持仓（数量、成本=成交额+手续费，减仓按比例结转
# `cost_basis = round(total_cost × 本次股数 / 持仓股数)`、`realized_pnl = amount - fee - cost_basis`）
# → 资金记录（余额链、买入 -(成交额+手续费)、卖出 +(成交额-手续费)）→ 清仓归档（新轮次 + 回填三笔
# 成交的 round_id + 轮次指向该股票的交易历史）→ 持仓卡片与交易历史的展示。
# 它同时是"详情区「减仓/清仓/加仓」点不动"这个真实缺陷的回归（见下方经验）。
pwsh -File fixtures/ui-stock.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 日记**编辑**链路端到端（与 ui-diary-io 互补：那条只覆盖导入/导出）：
# 进日记页（首屏是**预览态**，要先点页脚的「编辑」）→ 写内容 → Ctrl+S（没有保存按钮，
# 输入后 1500ms 防抖自动保存）→ 断言 正文/字数/心情落库 → 点心情「开心」→ 断言 mood=😊
# 且 **id 不变**（同一天 upsert）→ 改写内容再断言 →「预览」里 Markdown 渲染出标题 → 删除。
pwsh -File fixtures/ui-diary-edit.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 关键事件「新建」端到端：DatePicker **任选一个不是今天的日期** → 断言事件落在那一天 →
# 同一天再建一次 → 断言 **upsert（一条、id 保留、标题被覆盖）** → 行内「删除事件」→ 库里清空。
# （ui-crud 只验了用**默认日期（今天）**建事件 + 改颜色/写描述/删除；日期唯一性这条在这里锁。）
pwsh -File fixtures/ui-key-event.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 关联/解除关键事件端到端（唯一自动化 DatePicker 的脚本）：记一笔 → 行内「关联到关键事件」→
# 弹窗里用日期选择器选一个**不是今天**的日子（`link_date` 默认今天，选今天就等于没测选择器）
# → 断言 触发器显示所选日期 + `key_event_date` 落库 + 该日期懒创建了一条空事件
# → 再「修改关联」→「解除关联」→ 断言 `key_event_date` 清空。
# 它同时是"弹窗里的下拉面板被 `overflow: hidden` 裁掉"这个真实缺陷的回归（见下方经验）。
pwsh -File fixtures/ui-link-event.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

# 代码规范
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

工具链要求：rust **stable 1.96.0** + `wasm32-unknown-unknown` target。
仓库**故意不提供 `rust-toolchain.toml`**：指定具体版本会让 rustup 在每次调用时校验并重装组件
（实测会触发数百 MB 的重复下载），版本要求写在文档里即可。

## 本机环境注意事项（踩过的坑）

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
- **开发端口是 1520（不是 1420）**：本机 Windows 的保留端口段（Hyper-V/WSL）覆盖 1332-1431，
  在 1420 上绑定会失败（`os error 10013`），trunk serve 与 `cargo tauri dev` 都会起不来。
  `crates/tr-ui/Trunk.toml` 的 `[serve] port` 与 `src-tauri/tauri.conf.json` 的 `devUrl` 必须成对修改。
  自查命令：`netsh interface ipv4 show excludedportrange protocol=tcp`。
- **trunk 无法下载 wasm-bindgen**（GitHub release 资产在本机网络上 TLS 失败），
  改从 crates.io 安装同版本 CLI：`cargo install wasm-bindgen-cli --version 0.2.128 --locked`
  （版本必须与 `wasm-bindgen` crate 一致，当前 0.2.128）。
- **不要用 `trunk build --release`**：trunk 在 release 模式会调用它缓存里的 wasm-opt 123，
  而该版本无法校验 Rust 1.96 生成的 bulk-memory 指令
  （`memory.copy ... requires --enable-bulk-memory-opt`），更新版本又无法下载。
  发布构建统一走 `build/build-ui.ps1`：跑 trunk 的 **debug 模式**（跳过 wasm-opt），
  并用 `CARGO_PROFILE_DEV_OPT_LEVEL=3` + `CARGO_PROFILE_DEV_DEBUG=false` 把优化拉满、去掉调试信息
  （实测 wasm **7.3 MiB / 7.6 MB**，全 7 个页面都在；P6-a 只有 4 个页面时是 3.9 MB。
  作为对照：完全不优化、带调试信息的 debug 构建约 15 MB）。`tauri.conf.json` 的 `beforeBuildCommand` 已指向该脚本。
- **`build/*.ps1` 被 `powershell`(5.1) 调用时必须 ASCII-only**：Windows PowerShell 把无 BOM 的 UTF-8
  当 ANSI 解码，中文会破坏脚本解析（`build-ui.ps1` 因此全英文注释；由用户手动跑的
  `clean/build/release.ps1` 走 pwsh 7，可保留中文）。
- **手跑 exe 必须带 `custom-protocol`**（两条都踩过）：
  1. debug 构建会走 `devUrl`（`http://127.0.0.1:1520`），所以 `cargo build -p transactions`
     之后直接运行 `target\debug\transactions.exe` 只会得到一个**空白窗口**。
  2. **release 也一样**：Tauri 只有在启用 `tauri/custom-protocol` 特性时才会内嵌界面资源，
     而 `cargo tauri build` 会自动加上它、**裸 `cargo build --release` 不会**。
     漏掉它时窗口里是 Chromium 的 `127.0.0.1 拒绝连接`（ERR_CONNECTION_REFUSED）——
     外壳日志却只有"启动/外壳初始化完成"，看起来像界面代码坏了。
  正确的三种做法：`cargo tauri build`（发布链路用的就是这个）、
  `cargo build --release -p transactions --features tauri/custom-protocol`（冒烟用）、
  或起 `trunk serve` 后跑 debug 构建（等价 `cargo tauri dev`）。
  `fixtures/smoke.ps1` / `fixtures/ui-smoke.ps1` 都要求前者那种"界面内嵌"的产物。
- **别在 `trunk build` 写 dist 的同时跑 `cargo build`**：Tauri 在编译期读 `crates/tr-ui/dist`
  做资源内嵌，两个进程并发时可能嵌到半新半旧的资源（表现同样是空白窗口）。
  集成顺序固定为：`build/build-ui.ps1` 先跑完 → 再 `cargo build`。
- **配置目录可被 `USERPROFILE` 覆盖**：`config.rs` 的 `home_dir()` 先读 `USERPROFILE`（非 Windows 读 `HOME`）。
  `fixtures/smoke.ps1` 正是靠这一点把冒烟启动的实例指向一次性配置目录，
  **不会碰用户真实的 `~/.transactions.json`**；手测 release 版时也可以这么隔离。
- **别按进程名判断"应用是否在运行"**：原 Electron 版的 exe 也叫 `Transactions.exe`
  （本机装在 `D:\software\Transactions\`）。判重要按**完整路径**，否则会误判、误杀用户正在用的应用。
- **Tauri 默认会吃掉页内 HTML5 拖拽**（曾经的真实缺陷，"分类/标签/模板拖不动"）：
  Tauri 窗口默认 `dragDropEnabled: true`，wry 于是在 WebView2 宿主 HWND 上 `RegisterDragDrop`
  并 `SetAllowExternalDrop(false)`（`wry-0.55.1/src/webview2/mod.rs:150`）；而 **Chromium 在 Windows 上的
  页内拖拽也走 OLE 拖放**，于是 `drop` 永远到不了页面 —— 现象很有欺骗性：
  `dragstart`/`dragover` 都正常（行会变半透明、插入指示线也会画），只有 `drop` 不触发。
  修法：建窗口时调 `.disable_drag_drop_handler()`（`shell.rs` 的 `create_main_window` / `create_init_window`）。
  代价是拿不到 `tauri://drag-drop` 原生文件落盘事件——本项目与原 Electron 版都没有这个功能。
  回归：`fixtures/ui-drag.ps1`。
- **窗口几何的单位是逻辑像素（DIP），不是物理像素**（曾经的真实缺陷，"记不住窗口大小和位置"）：
  Windows 上 `window.inner_size()` / `outer_position()` 返回**物理**像素，
  而 `WebviewWindowBuilder::inner_size()` / `position()` 收的是**逻辑**像素；
  原 Electron 版两边都是 DIP（`getBounds()` / `new BrowserWindow({...})`），
  且 `~/.transactions.json` 是**与原版共用**的配置文件。若把物理值原样存回去，
  150% 缩放的机器上窗口每次启动都会放大 1.5 倍、位置越跑越偏。
  `save_window_bounds` 现在做物理 → 逻辑换算（纯函数 `logical_bounds` + 4 个单测），
  回归：`fixtures/window-bounds.ps1`（启动 → 关闭 → 再启动，尺寸/位置必须一致）。
- **关掉拖放处理器之后必须补导航守卫**（`shell.rs` 的 `is_allowed_navigation`）：
  Tauri 的拖放处理器同时也"吃"掉了拖入文件时的默认导航；`disable_drag_drop_handler()` 之后，
  把文件从资源管理器拖进窗口会让 WebView2 **直接导航到 `file:///…`**，界面整个被换掉。
  原 Electron 版用 `main.js` 里的 `will-navigate` 守卫挡住这类导航（只允许自己的界面），
  这里等价地只放行 `tauri.localhost` / `localhost` / `127.0.0.1`，其余（file:、外部站点、data:）一律拦截；
  `trasset://` 是**子资源**不走导航，所以不受影响。单测覆盖放行/拦截两侧。
- **两种原生对话框的窗口归属不一样**（找错地方就会"对话框没弹出来"）：
  * **WebView2 自己的文件框**（`<input type=file>`，见 `fixtures/ui-upload.ps1`）：
    应用窗口的**子窗口**，`ProcessId` 属于 `msedgewebview2.exe` → 在应用窗口 Descendants 里找；
  * **Tauri `dialog_open` 插件的选文件/选目录框**（见 `fixtures/ui-diary-io.ps1`）：
    **桌面顶层窗口**，`ProcessId` 就是应用自己 → 在 `RootElement` 的 Children 里按进程号找。
  另外两类框的路径输入框 AutomationId 不同：**选文件 = 1148**（`文件名(N):` 组合框）、
  **选目录 = 1152**（`文件夹(F):` 编辑框）。**选目录时回车只是进入该目录，必须点「选择文件夹」按钮**
  （选文件时回车才等于「打开」）；选目录框还只接受**已存在**的目录，否则弹「…不存在」提示框。
- **填进原生对话框的路径必须是绝对路径**（这条被用户当场抓到过）：选目录框会把**相对**路径按
  **它自己的"当前目录"**解析，于是脚本传 `-OutDir target\pkg-diary` 时，导入/导出就落到了别的地方
  （甚至弹「没有找到匹配的项目」）。所有 fixtures 的 `-OutDir` 现在都过 `GetFullPath` 归一化，
  `Select-Directory` 里还有 `IsPathRooted` 断言兜底。排查这类"选错目录"时**先看路径是不是绝对的**。
- **同一个坑还有第二个受害者：黄金对比里的工作空间路径**（我为此白查了三轮）：`run-parity.ps1 -OutDir`
  传**相对**路径时，Go 内核是以 `-WorkingDirectory <KernelDir>` 启动的，它按**自己的 cwd** 解析
  `-workspace`，于是库落到 `<KernelDir>\target\parity-finalX\ws-go`（**污染了只读参照仓库**
  `D:\github\Transactions\kernel\target\`），而驱动用 `xtask dump <同一相对路径>`（相对**本仓库**）去找它 ——
  现象极具欺骗性：内核启动正常、前面几十个 HTTP 调用**全绿**（它们只跟内核说话），
  直到第一个落库断言才炸，而且 trap 把真正的原因吞成了 `ScriptHalted`。
  现在 `run-parity.ps1` / `go-driver.ps1` 入口处都做 `GetFullPath` 归一化；
  `go-driver.ps1` 的 `Invoke-Dump` 也把 cargo 的 stderr 收进异常（原来写的是 `2>$null`，只剩一句"dump 失败"）。
  教训：**断言走文件系统、被测进程走另一个 cwd 时，路径必须绝对化**；diff 红之前先确认两侧说的是同一个库。
- **选目录的正确姿势**（`fixtures/ui-diary-io.ps1` 的 `Select-Directory`，逐条都踩过）：
  1. 把**绝对路径**粘进底部「文件夹(F):」框（剪贴板粘贴，别逐字符 SendKeys——输入法会把 `\` 变 `、`）；
  2. 回车**进入**该目录，再点「选择文件夹」；**不要**用地址栏（Alt+D）导航——它受"对话框记住的上次
     目录"影响，粘贴偶尔不生效就会停在上次的目录上，结果"选错了却看不出错"；
  3. 最后**验证**导航到位（面包屑的每一段都是独立元素，出现目标目录名才算到位），没到位就重试；
  4. 断言也要看**结果**（导出文件落在哪、库里多了哪几行），不要只看"对话框关了没有"。
- **只读的"行内操作按钮"必须先 hover**（`fixtures/ui-crud.ps1` 的 `Find-RowButton`）：分类/标签行的操作区
  是 `.ct-item-actions { display: none }`，只在 `:hover` 或 `.is-active` 时才 `display: flex`
  （**与原 `CategoryColumn.vue` 逐字一致，是 parity 不是缺陷**）。`display: none` 的元素**不进 UIA 树**，
  所以"新建的那一行能删、别的行删不了"——因为新建的行是 active。做法：先用真实鼠标把指针移到行中心、
  等 ~0.5s，再按名字查按钮，并用"中心 Y 最近且在该行右侧"来区分同一列里的多个「删除」。
- **往多行文本域写内容要"两条腿走路"**（写 `fixtures/ui-diary-edit.ps1` 时踩的）：
  `ValuePattern.SetValue` 塞值不保证触发 DOM `input` 事件（而日记/记账这类页面的自动保存挂在 `input` 上），
  纯靠剪贴板 `Ctrl+V` 又偶发失败——**窗口不是前台时 `AutomationElement.SetFocus()` 静默无效**，
  于是 `Ctrl+A/Ctrl+V` 贴到别处，"内容没改、保存却成功了"，看着像偶发假绿/假红。
  现在的做法：先 `SetForegroundWindow` → `SetFocus` → 校验 `FocusedElement` 是 Edit →
  粘贴 → **用 ValuePattern 读回校验** → 断言失败就整段重试（最多 3 次）。
- **别用 `| Select-Object -First N` 截断界面脚本的输出**：管道提前关闭会**终止上游脚本**，
  它的 `finally`（关掉测试实例）不执行，于是下一个脚本会因"本仓库已有实例在运行"而拒绝启动——
  我为此白查了一轮。要么 `-Last N`，要么 `*> 文件` 再读文件。
- **启动时要抓"主窗口"而不是"该进程的第一个窗口"**：启动期会先出现初始化窗口（600×560、无侧栏），
  随后才切成主窗口。抓到前者的话后面所有按名字的查找都会落空（整轮 26 项全红的假故障）。
  稳妥做法（见 `fixtures/ui-crud.ps1` 的 `Get-ReadyWindow`）：**轮询**取窗口元素、
  直到它包含侧栏条目（如「消费记录」）为止，每轮重新查询也顺带规避了句柄失效。
- **托盘图标能抓到，但浮出面板太"脆"，所以没做成常驻护栏**：Win11 下我们的托盘图标在
  `TopLevelWindowForOverflowXamlIsland`（名字「系统托盘溢出窗口」）里，是一个 `Button`，`Name='Transactions'`；
  点任务栏的「显示隐藏的图标」按钮能把它弹出来，右键会开一个 `#32768` 菜单。
  但实测**两次里有一次浮出面板没弹出来**（面板失焦即关、出现时机不固定），做成 fixture 会变成红绿随机。
  所以托盘菜单交互仍留在人工清单里；要再试的话，探针在 `target/tray2-probe.ps1`（不随仓库提交）。
- **更新链路的两个纯函数已抽出来单测**（`updater.rs`）：`parse_release`（release JSON → 更新信息：
  跳过预发布、`v` 前缀、取**第一个** `.exe` 资产、body 缺失给空串、没有 `.exe` 仍算"有更新"）
  与 `digest_matches` / `normalize_digest`（`sha256:ABCD…` 大写去前缀后比较；缺失/空串则跳过校验）。
  界面上的「检查更新」另有实测：真实 GitHub API 返回「已是最新版本」（探针 `target/update-probe.ps1`）。
- **`Input` 的 UIA 判据用 `ControlType.Edit`，别用 ClassName**（踩过）：本项目的输入框渲染成
  `class='ui-input__control'`，ClassName 并不是 `'Edit'`。用 `ClassName -eq 'Edit'` 过滤会
  **一个都找不到**（当时表现为"建仓弹窗里找不到股票代码输入框"，整轮全红）。
  只有原生 `<textarea>` 那类才是 `ClassName='Edit'`，所以判据要写成"ControlType 是 Edit **或** ClassName 是 Edit"。
- **同名按钮要按"可见 + 在窗口矩形内"筛**：同一个页面里可能有多个同名按钮
  （未展开的面板 / 另一个页签里也在 DOM 里），按名字取第一个常常拿到**不可见的那个**，点它毫无反应。
  `fixtures/ui-stock.ps1` 的 `Find-VisibleButton` 会同时校验 `IsOffscreen=false` 与矩形落在窗口内，
  并支持"取最后一个"（弹窗确认按钮与页面入口同名）。
- **详情区「清仓/减仓/加仓」点不动 = 真实缺陷**（曾经被我误记成"UIA 自动化不了"，其实是产品 bug）：
  三个按钮的 `on_click` 里先 `selected_code.set(code)` 再 `open_trade(...)`。详情区本来就按
  `current_position()`（= `selected_code` 命中的那条）渲染，写的是**同一个值**；但 `RwSignal` 同值写入
  依然会通知订阅者 → 点击瞬间详情子树重渲染 → 交易弹窗**永远不 mount**（DOM 里连 `ui-modal__mask` 都没有）。
  表头那个不写 `selected_code` 的「建仓」按钮一直正常，对比之下才看出差别。
  **怎么查出来的（这套手法留着复用）**：先证明"点击确实送达"——真实鼠标 / `SetFocus`+回车 / `InvokePattern`
  三种激活都试，同时用 `AutomationElement.FromPoint` 与 `WindowFromPoint` 确认坐标上是谁
  （Tauri 的 `TAURI_DRAG_RESIZE_BORDERS` 浮层会**抢答** `FromPoint`，但鼠标其实照样进 WebView，
  所以 `FromPoint` 单独用会误判）；再往 handler 里塞 `Notifier` 提示，确认 handler 跑了、`trade_open=true`；
  最后在同一视图里并排放**裸闭包节点**与 `<Show>` 探针（都正常反应）＋弹窗体里放挂载标记（始终不出现），
  才锁定"信号写对了，是 Modal 没挂载"。**教训：先分清"没点到"和"点了没反应"**，
  别用"这按钮自动化不了"给产品的 bug 打掩护。
  修法：三处去掉冗余的 `selected_code.set`（`src/pages/stock.rs` 有注释）；
  回归：`fixtures/ui-stock.ps1`（138 项断言：建仓 → 编辑成交 → 删除委托 → 减仓/清仓 → 费用设置 → 重置股票数据）。
- **"复用工作空间"的判断要在默认值赋值之前取**（踩过）：脚本常写成
  `if (-not $Workspace) { $Workspace = <默认路径> }` 之后再 `if (-not $Workspace -or ...) { 重新播种 }`，
  但那时 `$Workspace` 已经非空，条件恒为 false → 永远不重新播种。
  断言"持仓数量"这类**绝对状态**时必须每次重新播种，否则上一轮的持仓会叠加
  （实测 300 股变 600 股，后面全崩）。做法：函数开头先 `$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)`。
- **UIA 驱动这个界面的四条经验**（写自动化脚本时会反复踩）：
  1. Chromium 的 UIA 树是**惰性构建**的：窗口刚出现时首次查询常只返回二十来个元素、连 Button 都没有，
     要**轮询反复查询**把它唤醒（所以所有脚本都是"轮询到标记出现"而不是固定 sleep）；
  2. 页签是 `TabItem`（用 `SelectionItemPattern`），**不是** Button——按 Button 找会静默失败；
  3. 弹窗的确认按钮常与页面入口**同名**（例如页面上有「支取」按钮、弹窗确认也叫「支取」），
     按名字取第一个会点到页面按钮；取**最后一个**（弹窗在 DOM 末尾）或用包围盒位置区分。
     股票下单弹窗尤其明显：代码/价格/手数三个输入框在 UIA 里**没有名字**，
     只能按包围盒（同一列 Y 递增、同一行 X 分列）定位 `Edit`，再靠 `ValuePattern` 塞值——
     别指望按 label 找。
- **股票下单 + 行情链路的验收配方**（一条流程同时验四件事）：持仓页 → `建仓` 弹窗 →
  代码填 `600519` → 点「查询股票名称」 → 名称框**自动变成「贵州茅台」**（这一步打通了真实行情接口）
  → 填价格/手数 → 提交。随后断言三处：页面行显示现价/涨跌幅/浮盈、`tbl_billadm_stock_trade` 多一笔、
  持仓数量与成本对得上（**成本含手续费**：`价格×股数 + 佣金`，单位是分）。
- **原生文件选择框其实是能自动化的**（`fixtures/ui-upload.ps1` 就是证据，别再想当然写成"人工项"）：
  1. **窗口归属反直觉**：WebView2 的文件框（`#32770`，标题「打开」）**不是桌面顶层窗口**，
     它是**应用窗口 HWND 的子窗口**，而且 `ProcessId` 属于 WebView2 浏览器进程（`msedgewebview2.exe`）。
     所以按 `RootElement` 的 Children 找、或按 app 进程号过滤，**都找不到**——
     表现是"点了按钮但对话框没弹出来"（我为此白跑了好几轮）。正确姿势：在
     **应用窗口元素的 `TreeScope::Descendants` 里按 `ClassName='#32770'` 找**。
  2. **输入路径必须走剪贴板**：`SendKeys` 逐字符输入会被**中文输入法**接走，反斜杠 `\` 变成顿号 `、`，
     于是路径不存在、回车后弹「找不到文件」。用 `Set-Clipboard` + `Ctrl+A` + `Ctrl+V`。
  3. **提交用「右方向键 + 回车」**：输入路径会弹出 shell 自动补全下拉，
     - 直接回车 = "接受补全"，完整路径被换成裸文件名 → 「找不到文件」；
     - `TAB` 能收起下拉，但焦点也移到"文件类型"下拉，回车不再触发默认按钮；
     - `WM_COMMAND(IDOK)` 会关掉对话框但**跳过 modern 对话框"把文件名变成选中项"的内部步骤**——
       等同于取消，界面毫无反应（最难查的一种失败：看起来成功了，但没有文件回来）；
     - 右方向键收起下拉且焦点仍在编辑框 → 回车 = 按「打开」。
  4. **别用 UIA 坐标点那个「打开(O)」**：对话框底部一行是 legacy provider，报出的矩形不可信
     （实测「打开」的 rect 恰好等于对话框下边缘）；而且**不能按 AutomationId 找控件**——
     文件列表项（`.agents`、`.ssh`…）的 AutomationId 恰好是 1..N，`id=1` 会点中列表第一项。
  5. 临时 HOME 里要**先建 `Desktop` 目录**，否则文件框起始目录不存在，会先弹「位置不可用」。
  6. 判定成功要**看结果**（资产目录/数据库/界面文案），不要看"对话框关没关"——
     上面第 3 条的坑里，对话框确实关了但什么都没发生。
- **`Popconfirm` 的触发必须挂捕获阶段**（曾经的真实缺陷）：调用方常在子元素上写
  `stop_propagation()`（列表项里的删除按钮为了不触发整行"选中"），若触发挂在冒泡阶段就会被吃掉，
  气泡永远不弹——「删除图表」「删除事件」「删除关联交易」三处都因此点不动。
  `popconfirm.rs` 现在用 `on:click:capture`。**验证方式**：触发侧看气泡是否弹出（标题 + `取消/删除` 按钮）；
  确认侧可用设置页删模板（普通 `<Button>` 子元素）走一遍"气泡 → 点确认 → 库里行数 -1"，
  它与那三处用的是同一个组件、同一条确认链路。改动共享组件后请用 `fixtures/ui-smoke.ps1` 做回归。
- **弹窗里的下拉面板会被 `overflow: hidden` 裁掉**（真实缺陷，写 `fixtures/ui-link-event.ps1` 时才暴露）：
  DatePicker / Select 的下拉都是**绝对定位的子元素**，而 `.ui-modal__content` 原来带
  `overflow: hidden`（只为圆角）——小弹窗（如「关联关键事件」，只有一个表单项）里日历被裁到
  **只剩月份标题和星期行**，日期格子看不见也点不动；中等高度的弹窗（关键事件新增、日记编辑等）
  则被裁掉一半，看着像"面板画坏了"。原版 antd 把面板 **portal 到 body**，所以不受弹窗裁剪影响，
  这是重写时引入的偏差。现已改成 `overflow: visible`（圆角不依赖裁剪：header/footer 都是透明底、
  只有一条边框线，body 有内边距，没有子元素会画到圆角外）。
  排查提示：**UIA 里看不到"被裁掉"**——被裁元素的矩形照样报出来（见下一条），
  只有截图（全屏 PNG）才看得出"元素其实没画出来"。
- **UIA 的"空矩形"元素是幽灵**（一度让 `fixtures/ui-link-event.ps1` 全绿却什么都没发生）：
  没真正渲染出来的元素，`BoundingRectangle` 会报 ±∞（`X/Y=+∞`、`Width/Height=-∞`），
  但 `IsOffscreen` **仍可能是 `False`**。于是：
  1. 判据 `Width -le 0` 挡不住它（`-∞ <= 0` 为真倒是挡得住，但 `+∞` 参与 `[int]` 转换会**抛异常**：
     `无法将值 "∞" 转换为类型 "System.Int32"`），必须显式排除非有限值
     （`[double]::IsInfinity/IsNaN` 四个分量都查一遍）；
  2. 命中的很可能是**别的日期选择器**（页面里没显示、但格子还挂在 UIA 树上的那些），
     点它当然毫无反应 —— 所以要点"**真正渲染出来**"的那个，并且**轮询等它出现**，
     不要固定 sleep 后取第一个同名元素。
- **自动更新是自研实现**（`src-tauri/src/updater.rs`），**不用** `tauri-plugin-updater`：
  沿用 GitHub Releases + `asset.digest`(sha256) 校验的既有发布管线，不需要签名密钥与 `latest.json`。
  命令：`update_check` / `update_download`（发 `update:download-progress|complete|error` 事件）/ `update_cancel` / `update_install`；
  行为（仅 GitHub 域名白名单、已下载复用、`.part` 中转、取消清理、打开安装包后退出）与原 Electron 版逐条一致。
  因此 `tauri.conf.json` 里**不要**加 `plugins.updater`，capabilities 里也不需要 `updater:default`。
- **更新/行情的 HTTP 客户端不读系统代理**（已知偏差）：`src-tauri/src/updater.rs` 与
  `tr-service/src/quote.rs` 用的是 `ureq`，只按直连走（既不读 WinINET 的 `ProxyEnable/ProxyServer`，
  也不读 `HTTP(S)_PROXY`）；而原 Electron 版用 `net.request`，走 Chromium 网络栈、**会用系统代理**。
  本机实测两个端点直连都能通（`api.github.com`、`qt.gtimg.cn` 均成功，更新检查返回"已是最新版本"），
  所以当前不影响使用；但若哪天直连被挡（历史上 GitHub 资产下载就失败过），更新检查/下载会失败而原版能过。
  **要不要修需要权衡**：直接改成"有系统代理就走代理"会在代理没开时把本来能用的直连也弄坏
  （ureq 没有 Chromium 那套代理失败回退），所以正确的做法是"环境变量优先 + 系统代理仅在直连失败后回退"
  并加单测；截至本轮**刻意未做**，先在文档里记明这个偏差与取舍。

## 关键约定与陷阱

- **SQL 只允许拼接常量**：列名/表名用 `const …_COLUMNS` 或常量数组（如 `STOCK_TABLES`），
  **值一律走 `?` 占位符**（`instr(description, ?)` 也是占位符）；`ORDER BY` 的字段必须过**白名单**
  （`build_sort_clause` 只认 `transactionAt/transactionType/price/category`，与原 `TrSortModal` 的 4 项一致），
  排序方向强制 `asc|desc`。改 DAO 时别把请求里的字符串直接拼进 SQL。
- **生产代码里的 `unwrap/expect/panic!` 必须有据可依**：允许的只有锁中毒
  （`.expect("…锁中毒")`）、已校验不变式（月份 `1..=12`、`valid_up_to` 前缀、池在生命周期内有效）、
  以及启动期构建失败（`main.rs`）。新增这类调用前先问"它真的不可失败吗"。
- **金额恒为整数分**：数据库、IPC、算法全用 `i64` 分；只有展示层做分/元换算
  （`tr_domain::money`）。这两个换算函数的行为是硬契约（含负号、`.5` 输入）。
- **数据兼容只针对最新 schema（v0.27+）**：`transactions.db` 不存在时用
  `fixtures/schema/fresh_v0_27.sql` 建库；已存在时**只做只读校验**，
  **绝不执行任何 DDL/DML 去改结构**。更早版本的工作空间会被明确拒绝（提示先用 0.27 版打开一次）。
- **本仓库没有迁移代码**，也不要新增：没有 AutoMigrate 等价、没有补列/加索引、
  没有版本化迁移、没有 `billadm.db` 改名。任何"顺手修复旧库"的代码都属于越界。
- **IPC 契约**：命令统一只收一个 `req` 结构体参数，字段名与原 HTTP JSON body 逐字段一致。
  成功时 promise 直接 resolve 为数据本身；失败时 reject 载荷为
  `{"code":-1,"msg":"...","status":500}`。`msg` 是用户可见文案，**改动即破坏契约**。
- **JSON 字段命名不统一，且必须照抄**：核心记账模型是 snake_case
  （`ledger.created_at`），关键事件/日记/股票模型是 camelCase（`ledgerId`、`createdAt`），
  DTO 里两种混用（`tr_query_result` 的 `page_size` 与 `trStatistics` 并存）。
  数据库列名恒为 snake_case，列映射在 DAO 层显式书写，不依赖 serde。
- **金额/时间戳语义**：`transaction_at`、`trade_time` 等是 Unix 秒；
  `%Y-%m` 之类的分桶在 SQL 里用 `strftime(..., 'unixepoch')` 完成，不要在 Rust 侧重算。
- **图片资产**：布局为 `<workspace>/data/assets/key_events/<date>/<uuid>.<ext>` +
  `thumb_<uuid>.jpg`，数据库存相对 `data/assets` 的路径。界面通过 `trasset://` 自定义协议访问
  （原实现走本机 HTTP `/api/v1/static/*`），协议处理器复刻了原路径穿越校验。
- **后端只接受 JPEG/PNG/GIF/WebP**（与原实现 `util/image.go` 的 `mimeToExt` + `image.Decode` 一致）：
  **HEIC 转换留在界面层**（P5 用 web-sys canvas 交给 WebView2/系统解码器转成 JPEG 再上传），
  后端不引入 libheif/WIC。缩略图规则不变：宽度 > 300 时按比例缩放到 300（CatmullRom）、JPEG q75。
- **没有本地网络面**：不监听端口、没有 API 令牌、没有 CORS、没有子进程内核。
  "后台服务异常→重启"那套机制整体不存在（进程即应用）。
- **首启动的窗口切换由外壳负责**：`workspaceDir` 为空时启动进入初始化窗口（600×560、不可缩放）。
  初始化窗口与主窗口加载**同一个** `index.html`，界面在"未配置工作空间"时展示选择目录的引导。
  选完目录后 `workspace_open` 打开数据库，外壳随即 `show_main_window` + `destroy` 初始化窗口
  （对应原实现 `electron/src/main.js:396` 的 `workspace:init`）。
  因此**界面不需要、也不要再调 `workspace_init`**：那个命令只是为对齐原命令面而保留的幂等入口。
  曾经踩过的坑：`workspace_init` 全仓无调用点 → 首次启动只在一个不可缩放的 600×560 窗口里
  渲染整个应用，主窗口永远不出现（托盘"显示主窗口"才会补出来，于是变成两个窗口）。
- **配置文件是用户数据**：`~/.transactions.json`（dev 为 `~/.transactions-dev.json`）
  的键名与位置都不变，并且**读写时必须保留未知键**（`AppConfig.extra`）。
- **界面无 Node**：仓库里没有 npm/package.json，没有 vendor 的 JS 库。
  图表、Markdown、拖拽排序、日期选择等全部是 Rust 实现（见 `tr-ui`）。
- **设计令牌**：`--transactions-*` CSS 变量为准，取值来自原项目 `app/src/styles/_variables.scss`
  （对应 `DESIGN.md` 的调色板；`PRODUCT.md` 历史文本里的 `#4A8E70` 是过时信息）。
  仅浅色/深色双主题，主题通过 `<html data-theme="light|dark">` 切换。
- **供应商文档**：`crates/tr-ui/dist/` 由 trunk 生成，不入库。

## 发布

**版本控制**：本仓库已 `git init` 并完成初始提交（197 个文件 / 2.5 MB，含全部源码、fixtures 与文档）。
`.gitignore` 已排除 `/target`、`/build/target`、`/crates/tr-ui/dist`、`/src-tauri/gen/schemas`、
`*.db(-wal|-shm)`、`transactions.log` 与 `/fixtures/private/`——**真实工作空间数据绝不入库**。
提交前请 `git status --short` 核一眼，别把本地验证用的库或截图带进去。

`build/clean.ps1` → `build/build.ps1`（trunk → cargo tauri build → 重命名产物为
`Transactions-x64-v{version}.exe`）→ `build/release.ps1`（`gh release create` 上传该 .exe）。
版本号唯一来源是 `src-tauri/tauri.conf.json`；应用内更新读 release 的 `tag_name` 与首个 `.exe` 资产的 `digest`。
