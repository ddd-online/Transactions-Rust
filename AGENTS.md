# AGENTS.md

Transactions：桌面个人记账应用，每个工作空间是一个独立的 SQLite 数据库。
技术栈：Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite，全 Rust。

另见：`PRODUCT.md`（产品定位）、`DESIGN.md`（设计裁决标准）、`fixtures/README.md`（基线文件与
`lib/TrUia.ps1`）。

**本文件只写"去哪里看"与"为什么"。** 机器能判定的约定一律落进脚本 / 单测 / 护栏，本文不复述：
护栏覆盖什么、为什么，写在各自脚本的头注释；步骤与分组的唯一事实来源是 `fixtures/test.ps1`
（`-List` 可查）。

## 架构

| 目录 | 职责 |
|---|---|
| `crates/tr-domain/` | 纯领域层：models / dto / 金额换算 / 费用分摊 / 命令与事件名字清单 |
| `crates/tr-draw/` | 界面侧纯算法：绘制 / 取数决策 / 日历 / 分页 / 四象限 / 文本解析 / 拖拽 / 成交流水行 |
| `crates/tr-store/` | 存储层：建库 + 迁移引擎 + 格式校验 + 各 Dao |
| `crates/tr-service/` | 服务层：业务规则（账本 / 交易 / … / 股票），不依赖 tauri |
| `crates/tr-ipc/` | IPC 命令面：全部 `#[tauri::command]` + 统一错误信封 |
| `crates/tr-ui/` | 界面：Leptos CSR（只编 wasm32）+ `static/{css,fonts,icons}` |
| `src-tauri/` | 桌面外壳：窗口 / 托盘 / 配置 / 日志 / `trasset://` / 更新 |
| `xtask/` | 工具：`schema-diff` 与 `validate` / `dump` / `seed` / `migrate` |
| `fixtures/` | schema 基线 + 种子与端到端脚本（**不含真实个人数据**） |

分层：`tr-domain` / `tr-draw` **零 I/O、双可编** —— 界面与后端共用同一份金额 / 费用算法，
"能在 native 上断言的纯算法"放这两处（起因见 `docs/adr/0001-pure-draw-crate.md`）。
只有 `tr-ipc` 依赖 tauri（业务规则要能无窗口跑 `cargo test`）；`tr-store` / `tr-service` 不进 wasm。

## 常用命令

```powershell
# 测试统一入口（分组、步骤、影响面映射都在这个脚本里）
pwsh -File fixtures/test.ps1 -List                 # 列出分组与全部步骤
pwsh -File fixtures/test.ps1 -Unit stock           # 单元档：改了某功能（逗号分隔，永远带 core）
pwsh -File fixtures/test.ps1 -Unit changed         # 按 git 改动自动挑分组（映射见 $PathMap）
pwsh -File fixtures/test.ps1 -All                  # 全量档（发布前）：构建 + 全部护栏

# 编译 / 单测（不含桌面外壳）
cargo check -p tr-domain -p tr-store -p tr-service -p xtask --all-targets
cargo test  -p tr-domain -p tr-draw -p tr-store -p tr-service
cargo test  -p transactions                        # 外壳纯逻辑（配置 / 命令面 / 日志 / 资产 / 更新）
cargo check -p tr-ui --target wasm32-unknown-unknown

# 工作空间工具（validate / dump 只读；migrate 升级前自动备份）
cargo xtask schema-diff
cargo xtask validate|migrate|seed <workspace-dir>
cargo xtask dump <workspace-dir> [--table <name>]

# 界面与外壳（发布 / 验收链路必须走脚本，理由见下）
trunk serve --config crates/tr-ui/Trunk.toml       # 开发服务 :16000
cargo tauri dev                                    # 开发（会先跑 trunk）
cargo tauri build                                  # 打包（beforeBuildCommand → build/build-ui.ps1）
```

工具链：rust stable 1.96.0 + `wasm32-unknown-unknown`。故意不放 `rust-toolchain.toml`：
指定具体版本会让 rustup 每次调用都校验并重装组件，实测触发数百 MB 重复下载。

产物约定：测试产物全在 `target\tests\` 下，`target\tests\<脚本名>\{home,out,ws}` 是各护栏的
工作目录（可随手删，脚本会重建 / 播种），日志在 `_runs\<时间>-<档位>\`；`target\` 顶层只放
cargo 自己的目录。

## 改界面的迭代循环

改 `crates/tr-ui` 下的 `.rs` / `.css` 走 dev 服务 + 热更新（一轮几秒）；`build/build-ui.ps1` +
`cargo build --release` 是验收 / 发布用的（约 4 分钟），迭代期别用。参数都在脚本头注释里：

```powershell
# 起一次（之后一直开着）：trunk serve + dev 外壳 + 盯日志；重建完自动 Ctrl+R 并截图
pwsh -File fixtures/dev-hot.ps1 -Trunk -Launch -Workspace <ws> -ShotDir target\tests\dev-shot\out
# 只看某页 / 全部页面（不重建、不重启）：真实鼠标点侧栏，确认页面真切过去了再截图
pwsh -File fixtures/dev-shot.ps1 -Page 股票        # 或 -AllPages
```

- 改 `src-tauri/` 外壳要 `cargo tauri dev` 重启，热更新管不到。
- 调 trunk 前清 `NO_COLOR`（`=1` 会报 `invalid value '1' for '--no-color'`，脚本自己清）。
- 别让 trunk 写 `dist` 与 `cargo build` 并发：Tauri 编译期读 `crates/tr-ui/dist` 做资源内嵌，
  会嵌到半新半旧的资源（空白窗口）。顺序固定：`build/build-ui.ps1` 跑完再 `cargo build`。
- 手跑 release exe 必须带 `--features tauri/custom-protocol`，否则窗口里是"连接被拒绝"，
  而外壳日志只有"启动 / 初始化完成"，看着像界面坏了。

## 验证纪律

- **能机器判定的别靠人读**。`core` 每次必跑（`fmt` / `clippy` / `design-audit` /
  `static-guards`）；逐条判据在 `fixtures/static-guards.ps1` 头注释。
- **影响面别自己估**：改 `components/ui/`、`shell.rs`、`store.rs`、全局 CSS 用 `-Unit changed`
  （`$PathMap`）；新增 / 改动护栏时把连带依赖写进 `$Groups` 注释。
- **截图优先于 UIA 矩形**：UIA 看不见被裁掉 / 没画出来的东西。判据优先级：人眼看窗口 >
  UIA 矩形 > 截图像素（`PrintWindow` 可能把未画完的 SVG 抓成涂鸦）。
- 护栏真启动应用，用 UIA 或真实鼠标键盘驱动，断言落在库 / 磁盘上；`USERPROFILE` 指向一次性
  配置目录（`config.rs` 的 `home_dir()`），不碰真实配置。
- 界面脚本输出别用 `Select-Object -First N`：管道提前关会杀掉上游脚本的 `finally`，
  下一个脚本就因"已有实例在运行"起不来。用 `-Last N` 或 `*> 文件`。
- 新脚本照 `fixtures/lib/TrUia.ps1` 写；`$explicitWorkspace` 要在默认值赋值**之前**取，脏基线
  会让断言以完全无关的面目变红，先确认工作空间是新的。页面招数留在各自脚本：行内按钮先 hover
  才进树、确认键按类名定位、原生文件框的 `AutomationId` 1148/1152 与面包屑复核。

## 硬契约

- **金额恒为整数分**：库 / IPC / 算法全用 `i64` 分，只有展示层换元（`tr_domain::money`，
  含负号与 `.5` 的边界）。**股票价格走「厘」**，价格 × 股数 → 分的唯一入口是
  `tr_domain::stock::amount_of`。
- **股票费用**：一次委托多笔成交时，佣金按委托总额收一次（最低佣金只在这里生效），
  印花税 / 过户费逐笔取整再相加（不是先求和再取整）。算法只在 `tr-domain/src/fee.rs`，
  界面侧只调 `estimate_fee`；场内基金（`fee::Instrument::of`）免印花税与过户费。
  费率随本轮快照进成交行的 `round_*` 四列，**4 列全 NULL = 老数据 ⇒ 沿用系统配置**
  （不回填、不回溯）。
- **「浮动盈亏率」的分母是本轮净投入**（= `−`本轮资金变动合计，减仓回款会冲抵），不是本轮
  买入总额。唯一入口 `tr_domain::stock::floating_pnl_rate`（净投入 ≤ 0 返回 `None`，显示 `—`）。
- **资金余额是派生值**：`tr_domain::fund::cash_chain` 按 (日期 → 创建时间 → ID) 升序累加
  （起点 = 本金 − Σ追加本金），链序 = 资金变化先后。四个写入口（下单 / 追加本金 / 支取 /
  利息归本）在同一事务里复算。
- **时间戳**：库里是 Unix 秒，`%Y-%m` 分桶走 SQL 的 `strftime(..., 'unixepoch')`。「只选到日」
  记账域落本地 12:00（按 UTC 分桶仍是同一天），股票域落本地 00:00（服务层用 `chrono::Local`）。
- **库结构变更只走迁移引擎**：库不存在用 `fixtures/schema/fresh.sql` 建；已存在由
  `tr-store/src/migrations.rs` 升级后只读校验。迁移：id 唯一稳定、一个事务、幂等、只碰涉及
  的表、留单测；升级前 `VACUUM INTO` 备份（失败就不升级），同一工作空间只留最近一份 `.bak`。
- **IPC**：命令只收一个 `req` 形参，字段名是硬契约。失败载荷 `{"code","msg","status"}`，
  `code` 是失败种类（`tr_domain::error`），界面按种类分支、不比文案。命令名 / 事件名只在
  `tr_domain::commands` / `events`，界面侧不写字符串字面量。
- **JSON 字段名保持现状**：核心记账模型 snake_case，事件 / 日记 / 股票 camelCase，DTO 混用；
  数据库列名恒为 snake_case，映射在 DAO 层显式写。
- **SQL 只拼常量**：列名 / 表名走 `const …_COLUMNS`，值一律 `?`；`ORDER BY` 过白名单。
- 生产代码的 `unwrap/expect/panic!` 要有据：锁中毒、已校验的不变式、启动期构建失败。
- **不在 `Effect` 体内建信号**：Effect 重跑会 dispose 上一次建的响应式值，界面还在读就白屏 /
  点了没反应。状态建在组件或根 owner 下，Effect 里只读、只做副作用。
- **配置文件是用户数据**：`~/.transactions.json`（dev 为 `~/.transactions-dev.json`）键名与
  位置不变，读写保留未知键（`AppConfig.extra`）。功能开关两层：`features` 缺省全开，
  `ledgerFeatures` 按账本 id 覆盖；两侧靠 `feature_key` 对齐。
- **只有一个窗口，首启动也不例外**：`workspaceDir` 为空也挂主窗口，再叠不可关闭的
  「选择工作空间」，背景带 `inert`、外壳拒绝关闭。界面不自己写工作空间目录，
  `workspace_open` 是唯一入口。
- **图片资产**：`<workspace>/data/assets/key_events/<date>/<uuid>.<ext>` + `thumb_<uuid>.jpg`，
  库里存相对路径，界面走 `trasset://`（处理器防路径穿越）。后端只收 JPEG/PNG/GIF/WebP；
  HEIC 在界面层转 JPEG（`tr-draw::heic`），别交给 WebView2（它没有 HEIF 解码器）。
  缩略图宽 > 300 缩到 300、JPEG q75。
- **没有本地网络面**：不监听端口、无令牌、无 CORS、无子进程。**界面无 Node**：没有 npm /
  package.json，图表、Markdown、拖拽排序、日期选择全是 Rust。
- **设计令牌**：`--transactions-*` 是颜色 / 尺寸唯一来源，只有浅 / 深两套，切换走
  `<html data-theme>`；`crates/tr-ui/dist/` 由 trunk 生成，不入库。
- **图标只渲染 `<path>`**：`d` 的坐标必须落在 `view_box()` 里，不匹配就整段画到画布外。
  新图标取 Ant Design 官方包
  （`https://cdn.jsdelivr.net/npm/@ant-design/icons-svg@4.4.2/inline-namespaced-svg/outlined/<name>.svg`，
  本机走代理），`view_box = 64 64 896 896` 可直接配对。

## 本机环境的硬事实

- **cargo / git 都不读 Windows 系统代理**：直连 crates.io 超时、GitHub 会 reset。先设
  `HTTPS_PROXY` / `HTTP_PROXY=http://127.0.0.1:7890`、`CARGO_NET_RETRY=10`、
  `CARGO_HTTP_TIMEOUT=120`；索引中断就重跑（缓存会累积）。git 用一次性
  `-c http.proxy=http://127.0.0.1:7890`，别写进仓库配置。
- **别用 PowerShell 管道接 cargo 输出**：缓冲写满会阻塞假死，一律 `*> 文件`。
- 端口 16000 由 `Trunk.toml` / `tauri.conf.json` / `dev-hot.ps1 -Port` / `shell.rs` 单测四处
  一致，`static-guards` 盯着；Windows 保留端口段会漂（`os error 10013`），`dev-hot.ps1` 先试绑。
- 别的目录也可能有同名 `Transactions.exe`（如 `D:\software\Transactions\`）：按**完整路径**
  认本仓库的实例，别按进程名，否则会误判甚至误杀用户正在用的应用。
- **代理**：`off` / `auto`（默认，环境变量 → WinINET 注册表）/ `manual`，配置在
  `proxy: { mode, url }`；配置由外壳持有，`config_set_proxy` 落盘后立即推给
  `tr_service::proxy`。行情与更新共用 `agent()`。只支持 HTTP 代理，`off` 必须显式
  `.proxy(None)`（ureq 默认读环境变量）。
- **更新自研**（GitHub Releases + `asset.digest`，不用 `tauri-plugin-updater`，别加
  `plugins.updater`）。状态唯一持有者是外壳，词表与迁移在 `tr-domain::update`：七个状态是
  序列化契约，`UpdateEvent::apply` 是唯一迁移入口；事件载荷是完整快照，只有
  `update_check` / `update_download` / `update_cancel` 推进状态机，界面只 `accept(快照)`。

## 发布

远程仓库 `https://github.com/ddd-online/Transactions-Rust`（`main`）。应用内更新检查、
「关于软件」链接、`build/release.ps1` 的 `$repo` 三处一致：

```powershell
Select-String -Path src-tauri\src\updater.rs,crates\tr-ui\src\pages\settings.rs,build\release.ps1 -Pattern 'ddd-online'
```

链路：`build/clean.ps1` → `build/build.ps1` → `build/release.ps1`。版本号唯一来源是
`src-tauri/tauri.conf.json`（`Cargo.toml` 的 workspace 与 `src-tauri` 两处同步）；更新读 release 的
`tag_name` 与首个 `.exe` 的 `digest`；许可证以 `LICENSE` 为准（Apache-2.0）。

构建脚本最后一步必须是**产物断言**（`Test-Path` + 失败），别只看退出码 —— 5.1 跑 `build.ps1`
时中文行被误解析，便携版没留档而退出码仍是 0。发布前用 `fixtures/ui-about.ps1` 核自报版本，
发布后用 `gh release view <tag> --json assets` 比 `digest` 与本地 `Get-FileHash`。

`clean.ps1` 会连 `target\` 一起删，护栏脚本会自己重新播种，clean 之后可以直接跑 `-All`（变慢）。
`.gitignore` 已排除 `/target`、`/crates/tr-ui/dist`、`*.db(-wal|-shm)`、`/fixtures/private/`，
真实数据绝不入库，提交前 `git status --short` 核一眼。

**Release 正文取自 `build/release_notes.md`**（新增 / 修改 / 调整 / 优化四节，空节不出现，
每项 ≤50 字），也就是应用内「更新说明」。骨架在 `build/release_template.md`，判据是
`fixtures/static-guards.ps1` 第 8 项。版本变动只写这里，根因 / 判据写进当次提交正文。

## AI 工具

- **项目 skill** 在 `.dsh/skills/`（`.dsh/` 不入库）。每个 skill 是 `<name>/SKILL.md`，
  frontmatter 必须有 kebab-case 的 `name` 与 `description`，否则静默丢弃；带
  `disable-model-invocation: true` 的只作命令。别建第二个 skills 根。动界面前先读 `DESIGN.md`。
- **问题与规格**记 GitHub Issues（`ddd-online/Transactions-Rust`，走 `gh`）：见
  `docs/agents/issue-tracker.md`、`docs/agents/triage-labels.md`、`docs/agents/domain.md`。
