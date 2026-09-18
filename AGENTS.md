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

# 关闭行为三分支：quit → 进程退出；tray → 进程存活且窗口隐藏；空 → 弹「关闭选项」框，
# 选「是」后退出。原生询问框的按钮在 UIA 里是 `是(Y)`/`否(N)`（Pane 类型），脚本已兼容。
pwsh -File fixtures/close-behavior.ps1 [-Exe <exe>] [-Workspace <ws>]

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
- **UIA 驱动这个界面的三条经验**（写自动化脚本时会反复踩）：
  1. Chromium 的 UIA 树是**惰性构建**的：窗口刚出现时首次查询常只返回二十来个元素、连 Button 都没有，
     要**轮询反复查询**把它唤醒（所以所有脚本都是"轮询到标记出现"而不是固定 sleep）；
  2. 页签是 `TabItem`（用 `SelectionItemPattern`），**不是** Button——按 Button 找会静默失败；
  3. 弹窗的确认按钮常与页面入口**同名**（例如页面上有「支取」按钮、弹窗确认也叫「支取」），
     按名字取第一个会点到页面按钮；取**最后一个**（弹窗在 DOM 末尾）或用包围盒位置区分。
- **`Popconfirm` 的触发必须挂捕获阶段**（曾经的真实缺陷）：调用方常在子元素上写
  `stop_propagation()`（列表项里的删除按钮为了不触发整行"选中"），若触发挂在冒泡阶段就会被吃掉，
  气泡永远不弹——「删除图表」「删除事件」「删除关联交易」三处都因此点不动。
  `popconfirm.rs` 现在用 `on:click:capture`，改动共享组件后请用 `fixtures/ui-smoke.ps1` 做回归。
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
