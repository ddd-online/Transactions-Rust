# AGENTS.md

Transactions 是一款桌面个人记账应用，每个工作空间是一个独立的 SQLite 数据库。
技术栈：Tauri 2 外壳 + Leptos(WASM) 界面 + rusqlite 存储，全部为 Rust 代码。

另见：`PRODUCT.md`（产品定位）、`DESIGN.md`（设计系统，UI 的裁决标准）、
`fixtures/README.md`（基线文件与 `lib/TrUia.ps1`）。

**本文件只写"去哪里看"与"为什么"。** 凡是能被机器判定的约定，一律落在代码里（脚本 / 单测 /
护栏），本文件不抄第二份清单 —— 抄来的那份会先过期，然后开始骗人。护栏覆盖什么、为什么需要它，
都写在各自脚本的头注释里；步骤与分组的唯一事实来源是 `fixtures/test.ps1`（`-List` 可查）。

## 架构

| 目录 | 职责 |
|---|---|
| `crates/tr-domain/` | 纯领域层：models / dto / 金额换算 / 费用分摊 / IPC 命令与事件的**名字清单** |
| `crates/tr-draw/` | 界面侧纯算法：绘制（坐标刻度 / SVG 定点改写 / 裁剪）+ 取数决策（query）+ 公历日历 + 分页 + 四象限 + 展示词汇 / 文本解析 / 拖拽重排 / 成交流水行 |
| `crates/tr-store/` | 存储层：建库 + 迁移引擎 + 格式校验 + 各 Dao（rusqlite） |
| `crates/tr-service/` | 服务层：业务规则（账本 / 交易 / … / 股票），不依赖 tauri |
| `crates/tr-ipc/` | IPC 命令面：全部 `#[tauri::command]` + 统一错误信封 |
| `crates/tr-ui/` | 界面：Leptos CSR（只编 wasm32）+ `static/{css,fonts,icons}` |
| `src-tauri/` | 桌面外壳：窗口 / 托盘 / 配置 / 日志 / `trasset://` 资产协议 / 更新 |
| `xtask/` | 验证工具：`schema-diff` 与 `validate` / `dump` / `seed` / `migrate` |
| `fixtures/` | schema 基线 + 种子与端到端脚本（**不含真实个人数据**） |

分层纪律（`static-guards` 的依赖树检查与 wasm 目标都在盯着它）：

- `tr-domain` / `tr-draw` **零 I/O、双可编** —— 界面与后端共用同一份金额与费用算法，
  而且"能在 native 上断言的纯算法"必须放这两处（起因见 `docs/adr/0001-pure-draw-crate.md`）。
  需要 DOM 的渲染断言在 native 上跑不到，留在 `tr-ui` 的 `#[cfg(test)]` 只能当规格说明。
- 只有 `tr-ipc` 依赖 `tauri`：业务规则要能在没有窗口的环境里用 `cargo test` 验证。
- `tr-store` / `tr-service` 不参与 wasm 编译。

## 常用命令

```powershell
# 测试统一入口（分组、步骤、影响面映射都在这个脚本里）
pwsh -File fixtures/test.ps1 -List                 # 列出功能分组与全部步骤
pwsh -File fixtures/test.ps1 -Unit stock           # 单元档：改了某功能（逗号分隔可给多个，永远带上 core）
pwsh -File fixtures/test.ps1 -Unit changed         # 按 git 改动自动挑分组（映射见 $PathMap）
pwsh -File fixtures/test.ps1 -All                  # 全量档（发布前）：构建 + 全部护栏

# 编译 / 单测（不含桌面外壳）
cargo check -p tr-domain -p tr-store -p tr-service -p xtask --all-targets
cargo test  -p tr-domain -p tr-draw -p tr-store -p tr-service
cargo test  -p transactions                        # 应用外壳的纯逻辑（配置 / 命令面 / 日志 / 资产 / 更新器）
cargo check -p tr-ui --target wasm32-unknown-unknown

# 工作空间工具（validate / dump 只读；migrate 升级前自动备份）
cargo xtask schema-diff
cargo xtask validate|migrate|seed <workspace-dir>
cargo xtask dump <workspace-dir> [--table <name>]  # 只读导出规范化 JSON（排障 / 回归对比）

# 界面与外壳（发布与验收链路必须走脚本，理由见下）
trunk serve --config crates/tr-ui/Trunk.toml       # 开发服务 http://127.0.0.1:16000
cargo tauri dev                                    # 开发（会先跑 trunk）
cargo tauri build                                  # 打包（beforeBuildCommand → build/build-ui.ps1）
```

工具链：rust stable 1.96.0 + `wasm32-unknown-unknown`。仓库**故意不放** `rust-toolchain.toml`
（指定具体版本会让 rustup 每次调用都校验并重装组件，实测触发数百 MB 重复下载）。

产物约定：测试产物全在 `target\tests\` 下 —— `target\tests\<脚本名>\{home,out,ws}` 是各护栏自己的
工作目录（可随手删，脚本会自己重建/播种），每次运行的日志与汇总在 `_runs\<时间>-<档位>\`；
`target\` 顶层只应出现 cargo 自己的目录。

## 改界面的迭代循环

改 `crates/tr-ui` 下的 `.rs` / `.css` 走 dev 服务 + 热更新（一轮几秒）；`build/build-ui.ps1` +
`cargo build --release` 是最终验收/发布用的（约 4 分钟），迭代期别用。两个脚本的用法与参数都写在
它们自己的头注释里：

```powershell
# 起一次（之后一直开着）：trunk serve + dev 外壳 + 盯 trunk 日志；重建完自动 Ctrl+R 并截图
pwsh -File fixtures/dev-hot.ps1 -Trunk -Launch -Workspace <ws> -ShotDir target\tests\dev-shot\out
# 只看某页 / 全部页面（不重建、不重启）：真实鼠标点侧栏，**轮询确认页面真切过去了**再截图
pwsh -File fixtures/dev-shot.ps1 -Page 股票        # 或 -AllPages
```

边界：改 `src-tauri/`（外壳）不适用热更新，要 `cargo tauri dev` 重启；调 trunk 前要清掉
`NO_COLOR`（`=1` 会让它直接报 `invalid value '1' for '--no-color'`，脚本自己清）；
**别在 trunk 写 `dist` 的同时跑 `cargo build`** —— Tauri 编译期读 `crates/tr-ui/dist` 做资源内嵌，
并发会嵌到半新半旧的资源（表现为空白窗口），顺序固定：`build/build-ui.ps1` 先跑完再 `cargo build`；
手工跑 release exe 必须带 `--features tauri/custom-protocol`，否则窗口里是"127.0.0.1 拒绝连接"，
而外壳日志只有"启动/初始化完成"，看着像界面代码坏了。

## 验证纪律

- **能机器判定的别靠人读**。`core` 那几条便宜且每次必跑（`fmt` / `clippy` / `design-audit` /
  `static-guards`）；`static-guards` 收的是跨文件、跨脚本的约定 —— 开发端口一致、滚动条禁令、
  构建脚本纪律、`wasm-bindgen` 版本、`tr-domain` 分层、markup 类名必须有 CSS 规则、
  CHANGELOG 标题不许粘连、**文档里的引用不许烂**（引用的路径要存在、发布资产名不许写死版本号、
  "`AGENTS.md` 的「小节」"要对得上真实标题）—— 逐条判据与理由在 `fixtures/static-guards.ps1`
  的头注释里。
- **共享代码会扩大影响面**：改 `components/ui/`、`shell.rs`、`store.rs`、全局 CSS 时，
  影响面由 `$PathMap` 算（`-Unit changed`），别自己估。连带效果也算：一个 fixture 的检查
  可能由**别处**触发，新增/改动护栏时把这类依赖写进 `$Groups` 的注释里。
- **靠截图，别靠 UIA 的矩形**：UIA **看不见**"被裁掉 / 没画出来"（元素矩形照报出来），
  下拉面板被 `overflow` 截断、图标画到 `view_box` 外都属于这一类。判据优先级：
  **人眼看窗口 > UIA 树里控件的矩形 > 截图像素**（`PrintWindow` 会把未画完的内联 SVG 抓成一条
  涂鸦，人眼看完全正常 —— 怀疑图标坏了先开一次窗口）。
- 护栏一律**真的启动应用**、用 UI Automation 或真实鼠标键盘驱动，**断言落在库/磁盘上**，
  不落在"点到了没有"。各护栏靠 `USERPROFILE` 指向一次性配置目录（`config.rs` 的 `home_dir()`），
  所以不碰真实的 `~/.transactions.json`。
- 别用 `| Select-Object -First N` 截断界面脚本的输出：管道提前关闭会终止上游脚本，
  它的 `finally`（关掉测试实例）不执行，下一个脚本就会因"已有实例在运行"拒绝启动。
  要么 `-Last N`，要么 `*> 文件`。
- 新护栏照抄现成脚本的开头：`$explicitWorkspace` 要在默认值赋值**之前**取，否则
  "不用显式工作空间就重新播种"恒不成立，断言绝对状态时会叠加。**"脏基线"会让断言以完全无关的
  面目变红**，看到奇怪的连坐失败先确认工作空间是不是新的。

**UIA 公共底座是 `fixtures/lib/TrUia.ps1`**（轮询到标记出现 / 排除 ±∞ 幽灵矩形 /
启动时认"主窗口" / 受控输入要真的"打"进去 / 按完整路径判重），**新脚本一律用它，别自己写一套** ——
每条为什么这么写都在那个文件的注释里。页面专用的招数留在各自脚本（如 `ui-crud.ps1` 的
`Find-RowButton`：行内操作按钮要先 hover 才进树；`ui-crud.ps1` 的 `Find-ConfirmButton`：
确认键按**类名**定位，因为行内图标按钮与弹窗确认键的可访问名都含「删除」；`ui-upload.ps1` /
`ui-diary-io.ps1`：原生文件框与选目录框的提交姿势、`AutomationId` 1148/1152、面包屑复核）。

## 硬契约

- **金额恒为整数分**：库 / IPC / 算法全用 `i64` 分，只有展示层做分↔元换算（`tr_domain::money`，
  含负号与 `.5` 的边界是硬契约）。**股票价格走「厘」**（1/1000 元，`tbl_billadm_stock_trade.price`
  也是厘），价格×股数→分的唯一入口是 `tr_domain::stock::amount_of`。
- **股票费用**：一次委托多笔成交时，**佣金按委托总额收一次**（最低佣金只在这里生效），
  **印花税 / 过户费逐笔取整再相加**（不是"先求和再取整"，两者差一分）。算法只有
  `tr-domain/src/fee.rs` 一份，界面侧唯一调用点是 `estimate_fee`；场内基金（`fee::Instrument::of`
  判定，同时是代码校验与行情前缀的唯一判据）免印花税与过户费。费率随「本轮」走：建仓时快照进
  本轮每一笔成交行的 `round_*` 四列，**4 列全 NULL = 老数据 ⇒ 沿用系统配置**（不回填、不回溯）。
- **「浮动盈亏率」的分母是「本轮净投入」**（= `−`本轮资金变动合计，减仓回款会冲抵），
  不是本轮买入总额 —— 拿后者当分母会把亏损率摊薄。唯一入口
  `tr_domain::stock::floating_pnl_rate`（净投入 ≤ 0 返回 `None`，界面显示 `—`）。
- **资金记录的余额是派生值**，口径只有一处：`tr_domain::fund::cash_chain` 把记录按
  **(日期 → 创建时间 → ID)** 升序累加（起点 = 本金 − Σ追加本金）。**链序 = 资金变化发生的先后**，
  不是录入的先后 —— 倒填的记录排在该在的位置上，"链末条的余额 = 本金 + Σ非追加本金变动"恒成立，
  可用现金取的就是它。四个写入口（下单 / 追加本金 / 支取 / 利息归本）都在**同一事务**里复算。
- **时间戳语义**：库里是 Unix 秒；`%Y-%m` 这类分桶用 SQL 的 `strftime(..., 'unixepoch')`，
  别在 Rust 侧重算。两个域把"只选到日"落在**不同时刻**：记账域落在**本地 12:00**（按 UTC 分桶
  仍是同一天），股票域落在**本地 00:00**（服务层必须按**宿主本地时区**取日期，`chrono::Local`；
  按 UTC 会得到前一天）。
- **数据库结构变更只走迁移引擎**：库不存在时用 `fixtures/schema/fresh.sql` 建；已存在时由
  `tr-store/src/migrations.rs` 按登记表升级后只读校验（`schema::validate_current`），格式过旧就
  明确拒绝。迁移写法是硬规范：id 唯一稳定、一个事务、幂等防御、只碰本次涉及的表、留单测；
  升级前 `VACUUM INTO` 备份（失败就不升级），同一工作空间只留最近一份 `.bak`。
- **IPC 契约**：命令统一只收一个 `req` 形参（个别命令的 `req` 本身就是数组 / DTO），字段名是
  硬契约。失败载荷 `{"code","msg","status"}`，其中 `code` 是**失败种类**（取值在 `tr_domain::error`），
  界面按种类分支、不比 `msg` 文案。命令名/事件名只有 `tr_domain::commands` / `events` 两份清单，
  界面侧不出现字符串字面量（拼错是编译错误）；"本 crate 实现 == 清单业务段"与
  "注册表 == 清单全表"各有一条**带负向断言**的守卫，规格见那两处测试。
- **JSON 字段命名不统一，但必须保持不变**：核心记账模型 snake_case，事件/日记/股票模型 camelCase，
  DTO 两种混用；数据库列名恒为 snake_case，映射在 DAO 层显式书写。
- **SQL 只允许拼接常量**：列名/表名走 `const …_COLUMNS` 或常量数组，值一律 `?` 占位符；
  `ORDER BY` 字段必须过白名单（`build_sort_clause` 只认四项）。
- **生产代码里的 `unwrap/expect/panic!` 必须有据可依**：锁中毒、已校验的不变式、启动期构建失败。
- **别在 `Effect` 体内创建信号**：`Effect` 每次重跑都会 dispose 上一次建的响应式值，而界面那时
  往往还在读它（表现是"界面空白 / 点了没反应"，控制台只有一句 disposed）。状态建在组件或根
  owner 下，`Effect` 里只读、只做副作用。
- **配置文件是用户数据**：`~/.transactions.json`（dev 为 `~/.transactions-dev.json`）键名与位置
  都不变，读写必须保留未知键（`AppConfig.extra`）。`features` 四个开关**缺省全开**
  （`#[serde(default)]` + 默认 `true`），`keyEventLinkedOpen` 缺省展开；两侧靠 `feature_key`
  字符串对齐 —— 名字写错不会编译报错，只会得到一句"无效的功能开关"。
- **只有一个窗口，首启动也不例外**：`workspaceDir` 为空时同样是主窗口（界面照常挂载、显示空态），
  再把不可关闭的「选择工作空间」屏叠在上面，背景那层带 `inert`、外壳同时拒绝关闭请求。
  界面**不要**自己写配置里的工作空间目录：`workspace_open` 是唯一入口。
- **图片资产**：`<workspace>/data/assets/key_events/<date>/<uuid>.<ext>` + `thumb_<uuid>.jpg`，
  库里存相对 `data/assets` 的路径，界面走 `trasset://`（处理器带路径穿越校验）。后端只接受
  JPEG/PNG/GIF/WebP：**HEIC 在界面层就转成 JPEG**（`tr-draw::heic`），**别改回"交给 WebView2 解
  HEIF"**（Chromium 内核没有 HEIF 解码器，Windows 侧要靠商店里的「HEIF 图像扩展」，干净机器上
  必然失败，实测见 `fixtures/heic/README.md`）。缩略图：宽 > 300 等比缩到 300、JPEG q75。
- **没有本地网络面**：不监听端口、没有 API 令牌、没有 CORS、没有子进程后端。进程即应用。
- **界面无 Node**：仓库里没有 npm / package.json / vendor 的 JS 库，图表、Markdown、拖拽排序、
  日期选择全是 Rust 实现。
- **设计令牌**：`--transactions-*` 是颜色/尺寸的唯一来源（对应 `DESIGN.md` 的调色板），
  只支持浅/深两套主题（共用同一组令牌名），通过 `<html data-theme="light|dark">` 切换。
  `crates/tr-ui/dist/` 由 trunk 生成，**不入库**。
- **图标只渲染 `<path>`**（`icons.rs`，`fill: currentColor`），所以新增图标要同时核两件事：
  路径坐标范围与 `view_box()` 匹配（不匹配的后果是"图标整段画到画布外、看起来没了"或
  "缩到看不见"—— 左右翻页箭头与 `Icon::Sync` 都栽过），以及别只因为它来自 Ant Design 就放心。
  取新图标用官方包：
  `https://cdn.jsdelivr.net/npm/@ant-design/icons-svg@4.4.2/inline-namespaced-svg/outlined/<name>.svg`
  （本机走代理；GitHub 原仓库连不上），它给的 `d` 可与 `view_box = 64 64 896 896` 直接配对。

## 本机环境的硬事实

- **cargo / git 都不读 Windows 系统代理**（PowerShell 与浏览器走系统代理，它们只认环境变量）：
  本机直连 crates.io 会超时、连 GitHub 会 `Recv failure: Connection was reset`。先设
  `$env:HTTPS_PROXY='http://127.0.0.1:7890'`、`$env:HTTP_PROXY='http://127.0.0.1:7890'`、
  `$env:CARGO_NET_RETRY='10'`、`$env:CARGO_HTTP_TIMEOUT='120'`；索引拉取偶发中断就重复跑
  （已缓存条目会累积）。git 失败时用一次性的 `git -c http.proxy=http://127.0.0.1:7890 push`，
  别把它写进仓库配置。
- **别用 PowerShell 管道接 cargo 输出**（`cargo … | Select-Object`）：管道缓冲写满会让 cargo
  阻塞假死，一律 `*> 文件`。
- 开发端口 16000，`Trunk.toml` / `tauri.conf.json` / `dev-hot.ps1 -Port` 与 `shell.rs` 单测样例
  四处一致由 `static-guards` 盯着；Windows 的保留端口段（WinNAT/Hyper-V）会漂、非提权进程绑不上
  （`os error 10013`），`dev-hot.ps1` 会先试绑并直接报错而不是干等。
- 本机别的目录也可能装着同名 `Transactions.exe`（如 `D:\software\Transactions\`）：
  一律按**完整路径**判是不是本仓库的实例，别按进程名（否则会误判、甚至误杀用户正在用的应用）。
- **代理**：三态 `off` / `auto`（默认，探测环境变量 → WinINET 注册表）/ `manual`，落在配置的
  `proxy: { mode, url }`；配置由外壳持有，`main.rs` 启动时推给 `tr_service::proxy`，
  `config_set_proxy` 落盘后立即再推一次。行情与更新共用同一个 `agent()` 入口，
  不会"更新走代理、行情不走"。只支持 HTTP 代理（`socks*://` / `https://` 明确拒绝并给中文文案），
  不支持 PAC 与 `ProxyOverride`；`off` 必须**显式** `.proxy(None)`（ureq 的默认配置本身就读环境变量）。
- **更新是自研实现**（GitHub Releases + `asset.digest` 校验，不用 `tauri-plugin-updater`，
  所以 `tauri.conf.json` 里不要加 `plugins.updater`）。**状态的唯一持有者是外壳，词表与迁移规则在
  `tr-domain::update`**：七个状态取值就是序列化契约，`UpdateEvent::apply` 是唯一迁移入口
  （"检查失败 ≠ 已是最新"、进度只在下载态生效、重复事件幂等、没地址不进下载态）；事件载荷是
  **完整快照**，只有 `update_check` / `update_download` / `update_cancel` 三个命令在命令体里推进
  状态机并返回推进后的快照，界面只 `accept(快照)`、不跑状态机。

## 发布

远程仓库 `https://github.com/ddd-online/Transactions-Rust`（分支 `main`）。应用内更新检查、
「关于软件」的 GitHub 链接、`build/release.ps1` 的 `$repo` 三处必须一致：

```powershell
Select-String -Path src-tauri\src\updater.rs,crates\tr-ui\src\pages\settings.rs,build\release.ps1 -Pattern 'ddd-online'
```

链路：`build/clean.ps1` → `build/build.ps1`（trunk → `cargo tauri build` → 产物重命名为
`Transactions-x64-v{version}.exe`）→ `build/release.ps1`（`gh release create`）。版本号唯一来源是
`src-tauri/tauri.conf.json`（`Cargo.toml` 的 workspace 与 `src-tauri` 两处也要同步）；应用内更新读
release 的 `tag_name` 与首个 `.exe` 资产的 `digest`；许可证以仓库根 `LICENSE` 为准（Apache-2.0）。

构建脚本的最后一步必须是**产物断言**（`Test-Path` + 失败），别只看退出码 —— 有过一次事故：
5.1 跑 `build.ps1` 时中文行被误解析，便携版 exe 没留档而退出码仍是 0。发布前用
`fixtures/ui-about.ps1` 对产物核自报版本号，发布后 `gh release view <tag> --json assets` 核对
`digest` 与本地 `Get-FileHash` 一致（同一个 digest 出现在两个 tag 下就是发错了）。

`clean.ps1` 会连 `target\` 一起删，而护栏的工作空间与产物都在 `target\tests\` 下：各脚本自己会
重新播种（缺 `transactions.db` 就跑 `cargo -q xtask seed`），所以 clean 之后可以直接跑 `-All`，
只是会变慢。`CHANGELOG.md` 一律用编辑工具改，别用 PowerShell 字符串替换（`### 修复` 在文件里
出现多次，替换会把新小节塞进每个历史版本；标题粘连有 `static-guards` 兜底，"塞错位置"没有）。
`.gitignore` 已排除 `/target`、`/crates/tr-ui/dist`、`*.db(-wal|-shm)`、`/fixtures/private/` 等，
**真实工作空间数据绝不入库**，提交前 `git status --short` 核一眼。

## AI 工具（DSH）

项目本地 skill 都在 `.dsh/skills/`（整个 `.dsh/` 属本机安装、**不入库**，换机重装即可）：
每个 skill 是 `<name>/SKILL.md`，frontmatter 必须有 kebab-case 的 `name` 与 `description`，
否则会被**静默丢弃**（只在启动日志里留一句警告，目录里看不出少了谁）；带
`disable-model-invocation: true` 的不进模型目录、只作命令，所以"目录里比磁盘上少几个"是设计如此。
**别再建第二处 skills 根**（`.agents/skills/` 已并进这里，两处同名会互相遮）。

动界面之前先读 `DESIGN.md`（调色板 / 排版 / 间距 / 分层的唯一裁决标准）。

## Agent skills

- **Issue tracker**：问题与规格记在 GitHub Issues（`ddd-online/Transactions-Rust`），一律走 `gh` CLI。
  见 `docs/agents/issue-tracker.md`。
- **Triage labels**：沿用 `triage` 的五个默认状态标签，字符串与角色名一致。见 `docs/agents/triage-labels.md`。
- **Domain docs**：单上下文（single-context），见 `docs/agents/domain.md`。
