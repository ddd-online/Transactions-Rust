# Changelog

本文件记录本仓库的版本变更。版本号以 `src-tauri/tauri.conf.json` 为唯一来源。

## [0.17.0] - 2026-10-09

### 修复

**股票的浮动盈亏率：分母改成「本轮净投入」（股票 · 持仓）**

持仓卡片与详情区的「浮动盈亏」率以前拿**本轮买入总额**（建仓/加仓的成交额 + 费用，`RoundFlow::buy_cost`）当分母，
而减仓的回款并不冲抵它 ⇒ 分母被卖出的那部分撑大、**亏损率被摊薄**。用户真实数据（博敏电子 603936）：市值 3790.00 +
本轮资金变动 −4808.28 = 浮动盈亏 −1018.28，界面显示 **−6.07%**（−1018.28 ÷ 16776.16），按净投入应当是 **−21.18%**
（−1018.28 ÷ 4808.28）——差了三倍多。

现在率 = 浮动盈亏 ÷ **本轮净投入**，净投入 = `−`本轮资金变动合计（买入 −(成交额 + 费用)、卖出 成交额 − 费用，
正是「成交记录 → 资金变动」那一列的逐行和）。算法只有一个入口 `tr_domain::stock::floating_pnl_rate`
（native 真跑；净投入 ≤ 0 —— 卖出回款已超过买入支出 —— 返回 `None`，界面显示 `—`），持仓卡片与详情区两处
调用点共用它。买入总额仍是**已结算轮次**盈亏率的分母（`dto::round_pnl`），两个口径各管各的、别互换。

顺带把界面侧不再有人读的字段删掉：`StockPositionDto` 不再有 `roundCost`（它的唯一消费者就是那个错分母），
界面用 `roundCashFlow` 推出浮动盈亏与率；`RoundFlow::buy_cost` 留在领域层（「这一轮为买入花了多少」的口径，
`tr-domain` 有断言）。

判据：`cargo test -p tr-domain` **126 绿**（新增 3 条，含用户那组真实数字：−101828 分 ÷ 480828 分 = −21.18%，
以及"净投入为 0 或为正时没有率"）；`cargo test -p tr-service` **159 绿**（`unrealized_pnl_counts_current_round_cash_flow`
改成钉"服务层送出去的就是净投入口径"）；`fixtures/ui-stock.ps1` 第 2 步新增一条端到端断言：
卡片上的浮动盈亏 ÷ 库里算出的本轮净投入 = 卡片上显示的率（±0.01%）。

**写入之后只重拉了持仓：加仓 / 减仓 / 清仓 / 改成交后界面半新半旧（股票 · 持仓）**

「持仓」子功能有两个读模型 —— 持仓列表（卡片 + 详情区）与本轮「成交记录」表 —— 而写入是**同一批**：
下单（建仓/加仓/减仓/清仓）、改成交、删委托都会同时改到两边（成交变了 → 本轮资金变动与持仓成本跟着变；
持仓变了 → 本轮成交里就多/少一笔）。四个写入点原来都只 `reload()` 了**其中一个**：下单 / 改成交（影响预演那条）/
删委托只重拉持仓，改成交的直写那条只重拉成交。于是用户报的"加仓、减仓、清仓之后成交记录还是旧的"
（选中代码没变 ⇒ 取数的 key 没变 ⇒ 那一侧永远不重拉）；镜像的那一半是"改完成交价，卡片上的浮动盈亏/率还是旧的"。

现在页面用一个具名闭包 `refresh_trades_and_positions` 把"会一起变的那两条查询"圈起来，四个写入点各调一次；
`crates/tr-ui/src/query.rs` 的模块文档把这条纪律写下来（一条写入常常改到多个读模型，只重拉其中一个
就是半新半旧的界面），免得下一个人再各写一半。

判据：`fixtures/ui-stock.ps1` 新增两条端到端断言 —— 减仓后「成交记录」里必须出现刚提交的成交价
（提交前界面上没有这个价：本轮只有那笔 100.00 的建仓），以及改成交后卡片上的浮动盈亏率必须跟上新的净投入。
两条都先在**修前**的产物上跑红：卡片停在 `+346731.70 +1155.57%`（期望 1144.13%）、成交记录等 20 秒也没出现 `120.00`；
修后与其余步骤一起全绿（`fixtures/test.ps1 -Unit stock`）。

### 新增

**股票支持场内基金（ETF / LOF / 封闭式基金 / REITs）：价格按厘存、只有佣金**

**一、报价单位是 0.001 元，所以成交价改按「厘」（1/1000 元）存。** 场内基金的交易所报价单位就是 0.001 元
（实测 `510300` = 4.389、`159915` = 3.053），而价格以前按"分"存 ⇒ 4.389 会被四舍五入成 4.39，成交额、持仓成本、
浮动盈亏跟着偏。现在**价格走厘、金额走分**：换算唯一入口 `tr_domain::money`
（`price_yuan_to_milli` / `milli_to_yuan` / `milli_to_price_yuan`），展示走 `tr-draw::format::price`
（能整除分两位小数、带厘位三位），`价格 × 股数 → 分` 那一次折算的唯一入口是 `tr_domain::stock::amount_of`
（`round(price_milli × shares / 10)`，整手永远是整数分）。`tbl_billadm_stock_trade.price` 因此改存厘，
旧工作空间由迁移 `20261009_stock_trade_price_milli` 把整列 ×10（只改值、不动结构，幂等靠登记表；
`fixtures/schema/fresh.sql` 同步登记）。wire 上的 `price` 仍是元（`f64`），由 `stock_trade_create` /
`stock_trade_update` 折成厘。

**二、场内基金只有佣金**：免印花税、免过户费 —— 沪市基金也是（过户费只对沪市**股票**收，别把它当成"沪市就收"）。
品种与**代码校验**收成同一个判据 `tr_domain::fee::Instrument::of`：股票 沪 `60`/`68`、深 `00`/`30`；
场内基金 沪 `5[0-8]`xxxx、深 `1[5-8]`xxxx（可转债 11/12 开头不在支持范围）。它同时决定行情接口的市场前缀，
所以不会出现"界面能填、行情那儿被拒"；服务层对不认识的代码（老数据里的北交所等）回落成"深市股票"，
不让一条历史记录把重放打断。下单弹窗的费用预估、费用设置页的印花税/过户费说明也跟着更新。

判据：`cargo test -p tr-domain` **131 绿**（新增：厘/元的换算与展示、`amount_of` 的厘→分、`Instrument` 分类与
"基金只有佣金"）、`-p tr-service` **161 绿**（新增 `a_fund_order_is_charged_commission_only`：同样金额的沪市
股票卖出有印花税 + 过户费，510300 一笔都没有）、`-p tr-store` **78 绿**（新增价格迁移：×10、其余列一字不变、
重复应用不再乘）、`-p tr-draw` **112 绿**（`format::price` / `signed_price`）、`-p tr-ipc` 12 绿（4.389 → 4389 厘）。
端到端：`fixtures/ui-stock.ps1` 第 11 步在**重置之后**独立走一遍 510300 的建仓（3 手 @ 4.389）→ 清仓（@ 4.400），
逐字段核 `price=4389/4400`、`amount=131670/132000`、`stamp_duty=transfer_fee=0`、`fee=commission`；
`fixtures/migrate-workspace.ps1` 把降级口径扩到价格（除回 10）并断言升级后 ≠ 备份里的分、再启动不再变。

## [0.16.0] - 2026-10-08

### 新增

**记账 · 分析：图表列表支持拖动排序（`chart_update_sort`）**

左侧图表列表原来只能点选，现在可以拖拽换位 —— 与分类/标签、模板、待办卡片同一套 HTML5 `draggable`
（`components/ui/drag_sort.rs`），手柄与落点指示线也共用既有那套（`.ui-drag-handle` + `Icon::DragHandle`）。
拖动后本地顺序立即生效，落库只对 `sort_order` 与下标确实不一致的项发请求
（纯算法 `tr_draw::list_order::reorder_with_changes`，native 上真跑）。

写库走**新命令** `chart_update_sort`（`{ chartId, sortOrder }`）而不是复用 `chart_update`：排序是"只写序号"，
不该有机会覆盖 `title` / `granularity` / `chart_lines`（与分类/标签/模板三条拖拽排序同形状）。命令清单因此
111 → 112（外壳 21 + 更新 5 + 业务 86）。

顺带收掉一条旧规则：图表 DAO 的读序从 `is_preset DESC, sort_order ASC, created_at DESC` 改成
`sort_order ASC, created_at DESC` —— **预设图表不再固定在最前**，自定义图表可以拖到预设之前
（`is_preset` 只继续决定面板里哪些项可编辑）。

判据：`cargo test -p tr-domain`（`chart_update_sort` 的 camelCase 请求形状 + 清单计数）、
`-p tr-store`（DAO 只写序号并刷新 `updated_at`；排序测试按新口径改写为"预设不再优先"）、
`-p tr-service`（服务层只写序号、图表内容一字不动）、`-p transactions`（注册表 == 全表 112 条）；
`fixtures/ui-drag.ps1` 第 3、4 步用真实鼠标把自定义图表拖到第一项预设之前 → 库里落成稠密 `0..N-1` →
**切走再回来顺序仍与库一致**（DAO 里若还留着 `is_preset DESC`，这一条会红）。

## [0.15.2] - 2026-10-07

### 修复

**图表 Y 轴负数的千分位（`#47`）**

跨零的折线图（累计盈亏那类）Y 轴刻度以前同一根轴上两种写法并存：正数 `50,000`、负数 `-50000`。根因在
charts-rs 的 `thousands_format_float`（第一行 `if value < 1000.0 { return format_float(value) }` 把负数全挡进了
不分组那条分支），而它只给了 `axis_formatter` 这一个**模板字符串**口子（`{c}` / `{t}` 字面替换，没有自定义格式化
函数）。照本模块既有的做法在生成的 SVG 上定点改写：`tr_draw::chart::group_negative_tick_labels` 只改"负号 +
≥ 4 位数字（可带 `%` 这类后缀）"的文本节点 ⇒ `-50,000` / `-100,000`，正数、`-500`、类目轴上的 `2026-01`
一个字节不动。

判据：`cargo test -p tr-draw` **110 绿**（新增 2 条：一条用 charts-rs **真实输出**的 8 个刻度文字节点做的断言
—— 属性、`x`/`y`、文字前后的换行都是原样搬过来的，另一条锁住"只认负号 + ≥ 4 位数字"与不成对标签宁可不动）。
轴宽不用跟着改：charts-rs 的预留宽度按改写前的最长标签量，文字左边缘定死在 `x` 上，多出的逗号往右长进
`name_gap` 那 8px 里（实证：`-100000` 的 `x = 4`，补逗号后距绘图区还有约 1px）。

**股票现金链按日期复算，倒填的资金记录不再被跳过（`#48`）**

修前 `recalculate_cash_chain` 按**录入顺序**结算（每条的前值取"前 i 条里 (日期, 创建时间, ID) 最大一条"的余额），
**倒填**的记录（日期更早、录入更晚）够不着那条 ⇒ 它的金额进不了链，而可用现金读的正是链上日期最大那条的余额 ⇒
**可用现金虚高**（用户真实数据上正好虚高了一笔建仓的钱）。三个资金事件（追加本金 / 支取 / 利息归本）更糟：它们
只在插入时算一次 `prev + amount`、之后从不复算，倒填日期时可用现金**根本不减**。

现在链的口径只有一处：`tr_domain::fund::cash_chain` 把记录按 **(日期 → 创建时间 → ID) 升序**逐条累加
（起点 = 本金 − Σ追加本金），`stock::write::recalculate_cash_chain` 只是"取出来 → 算 → 写回去"，
四个写入口（下单 / 追加本金 / 支取 / 利息归本）都在**同一事务**里复算，插入时不再自己算一次
（`recalculate_cash_chain` 因此从模块私有改成 `pub(super)`）。于是"链末条（日期最大那条）的余额 =
本金 + Σ 非追加本金的变动"恒成立，与录入先后无关 —— 可用现金不再虚高，逐行余额也按"资金变化发生的先后"排。
顺带删掉两处不再有调用点的东西：`tr_domain::fund::cash_after` 与 `is_newer`（"谁更新"那个谓词），
以及 `stock::write::fund_record_after`；`StockDao::list_fund_records_in_insert_order` 改名
`list_all_fund_records`（它返回的顺序不再是链序，只要求"同一次调用里稳定"）。

用户可见的两处小改动：资金记录的**备注**不再写"现金 A → B"（改成 `支取 500.00 元` / `利息 3.00 元`）——
那两个数字是录入时的口径，日期倒填后与同一行的「余额」列对不上；「追加本金」的备注是**本金**口径
（`本金 A → B`，与链无关），保留。

判据：`cargo test -p tr-domain` **122 绿**（新增 5 条：链序与累加 / 倒填进链 / 同日比创建时间与 id / 空链与单条 /
余额可以为负）、`cargo test -p tr-service` **158 绿**（新增 `backdated_fund_events_are_counted_in_available_cash`；
两条钉住旧行为的断言按新口径改写 —— `rebuild_keeps_cash_chain_for_backdated_trade` 更名
`backdated_trade_is_counted_in_the_cash_chain`，`repair_legacy_trade_fund_dates_fixes_date_and_cash_chain` 里
"可用现金虚高"那段换成"日期错只影响逐行余额，总额本来就对"）；`fixtures/ui-stock.ps1` 的 `Assert-FundChain`
改成按链序（(日期, 创建时间, ID)）取相邻两条，不再按录入顺序（否则它自己就钉着旧口径）。

护栏：`fixtures/test.ps1 -All` **34/35 绿**（1338.5s，含重新构建的 release 产物）。唯一红的是 `ui-drag`——
真实鼠标拖拽这一轮没生效（界面顺序与库都没变），单独复跑 `fixtures/ui-drag.ps1` 全绿；本次改动面里
没有拖拽路径（`tr-draw` 只多了图表 SVG 文字的定点改写、`tr-service` 只有股票资金链），按偶发记录在此。

### 调整

**把写路径的"在飞 + 失败面"铺到其余页面（候选 2 的延续，`#34`）**

`tr_ui::change::submit` 的判据只有两条 —— **在飞标记成败都要复位**、**失败必须带前缀提示带出错误详情**。
按它过完 `crates/tr-ui/src/pages` 下 8 个页面：**37 处换**（待办 10、日记 1、记账 5、模板 2、分类标签 4、
事件 3、股票 5、设置 7），**32 类不换**，四类形状都不换并写明理由：多次请求/分支/补偿（编辑成交、影响面确认、
记账页编辑保存、保存图片、初始化默认分类——失败要两次提示，第二次文案取决于错误的 `message`，而
`submit` 失败时把错误值吃掉了）、多条请求的循环（四处拖拽排序）、**失败面不是"前缀 + 错误详情"**
（图表页 5 处用 `Err(_)` + 固定文案，换掉会改变用户看到的文案）、以及不是业务写的（设置页的读与对话框、
`workspace_open` 的多段后果、更新链路的状态机落点）。

**一条明确的"不做"**：`config_set_feature` 的在飞态是**集合**（哪几个开关正在保存），要收它得把 `submit` 的
`running` 泛化成闭包、代价是 40 多处调用点都要包一层 —— deletion test 不过，所以只收失败面，泛化不做。

用户可见行为不变；`fixtures/test.ps1 -All` **35/35 绿**（1228.2s）。

## [0.15.1] - 2026-10-07

### 调整

**统一写路径：`tr_ui::change::submit`（候选 2 的延续，`#34`）**

候选 2 原本要的是"与 `Query` 对称的写路径 module"，用三处真实调用点检验后改成了**只收两件事**：
"在飞标记必须复位（成败都要）"与"失败必须带前缀提示"。`0.15.0` 已在待办页落 10 处，这一版把
**日记页的删除**也换过来（`deleting` 标记 + 成功提示 + 一串界面动作 + 失败提示）。

同一页的**自动保存不换**，理由写在 `#34`：它是三态（快照过期 → 静默返回 / 成功 → 合并服务端返回值 /
失败 → 置 `SaveStatus::Failed`），而 `submit` 的 `Option` 会把"过期"和"失败"并成一个 `None`，
正好抹掉这一处最要紧的区别。

用户可见行为不变（同一套判据、同一批断言：`-Unit diary` 6/6 绿）。

## [0.15.0] - 2026-10-07

### 架构复盘（2026-10-06）13 条候选全部落地 —— 总账

一次跨 13 条候选的重构，按"每条先立 issue 作规格、能在 native 上断言的纯算法进 `tr-draw`/`tr-domain`
并真跑、护栏随改动补齐、全量档收尾"的纪律做。issue 编号即规格：`#29`–`#43`（全部已关）。
下面按候选列**用户可见的效果**与**判据**；实现细节在各 issue 的评论里。

| # | 候选 | 效果 / 判据 |
|---|---|---|
| 1 | 更新会话一个持有者 | 查出「已是最新版本」后换页切回不再被抹掉；`ui-update-restore.ps1` 第 2 步就是这条回归（修前用 UIA 探针实测 `same=False`，修后 `True`） |
| 2 | 写路径判据 `WriteTarget` | **9 处手写过期守卫归零**（全仓 `current_ledger_id… != ledger_id` 计数 0）；"失效表"那半按 deletion test 刻意不建（无可达的过期读） |
| 3 | 现金链与本金写入 | 支取超限文案 / 余额算式 / 时序判据各一份实现；**删掉我自己造的重复实现**（`FundEntry`/`latest_index`）；"倒填记录被跳过"钉成断言 |
| 4 | 列表区四态 | 判据 `tr_draw::section` + `AsyncSection`，**10 处调用点**；其中 5 处让"查询失败"不再显示成「暂无××」，1 处修掉首帧闪空态 |
| 5 | ADR-0001 第二波 | `tr-draw` 真跑断言 41 → **108**；日历（含两个 DST 截断缺陷）、分页、四象限、展示词汇、输入文本、拖拽重排、成交流水行、日期树、统计指标、年份筛选 |
| 6 | 统计取数 | `recent` 的取值规则一处（**发现注释与代码矛盾**：非数字串实际按"没传"处理）；生产路径只收 `&StatisticsFilter` |
| 7 | 轮次与现金流算术 | 手/股/成交额魔数链四处合一；`current_round_trades`（+7 断言）、`RoundFlow`/`round_flow`（+4）、`next_round_no` 两处合一 |
| 8 | 级联删除 SQL | **`tr-service` 下不再出现 `DELETE FROM`**（计数 0）；清单与"带 ledger_id 的表必须纳入"的覆盖断言一起搬进 `tr-store` |
| 9 | 清单守卫 | 新增两条能跑的守卫：功能开关键两侧对齐（源码扫描）、事件名不许以字面量出现；Req/Res 类型那半**刻意不做**（今天已是编译错误） |
| 10 | 失败面 | 「未打开工作空间」两条路径合一（都 reject）；失败带**种类** `code`（`-2`），界面不再比 `msg` 文案 |
| 11 | 工作空间打开 | 删掉无人调用的 `workspace_init`（清单里唯一没有调用点的条目）；六步与每步失败处置写成契约并抽成 `open_pipeline`；`workspace_is_open` 唯一判据 |
| 12 | 待办卡片接口 | `card_view` 形参 **16 → 2**（`TodoCtx` + `TodoMutation`） |
| 13 | 死表格与样式归位 | 删掉无调用点的 `Table` 组件与它的死样式；交易页 39 个样式块从 `app.css` 归位到 `transactions.css` |

**顺带修掉的两条护栏偶发**：`close-behavior`（「选「是」后进程退出」）与 `ui-crud`（行内「删除」按钮）、
`ui-stock`（建仓弹窗选标签）都改成**轮询到状态出现**，不再"睡一觉就当成了"（AGENTS.md 的四条通用纪律第 1 条）。

**两次自我纠正**（都记在 issue 里，而不是悄悄改掉）：候选 3 里我造过一份重复实现 + 死代码，后来删掉；
候选 6/7 各核出一次"报告的数字与代码不符"（"11 条测试没有归宿"实际在 `tr-service` 且可测；
"5 个调用点"实际 3 个；`recalculate_cash_chain` 是我自己的 grep 漏了子目录）。

**评审（`/code-review`：Standards + Spec 两轴）与它的产出**：13 条落地后跑了两轴评审，
它抓到一个**我自己引进的真实缺陷** —— 文件保存那条路径改成 reject 时用了 `bad_request(...)` 拼同一句文案，
而同一批刚把界面判据改成看"失败种类"（`code = -2`）：**文案对了、种类错了，等于没修**（`482bb19`）。
评审还核出候选 4 漏迁了待办的四象限（`7c03622`）、事件名守卫缺负向断言（`71593ef`）、
两处重复实现（`is_buy` / 魔数 100）与四处过时注释（`f10bd3d`），以及三条"未核"逐条核实。
产出的**逐条处置**（做 / 不做 / 怎么做）记在 `#45`；评审产出落地后 `-All` **35/35 绿**（1184.3s），
候选 2 的写路径（`change::submit` + 待办页 10 处）与候选 4 的四象限补齐后**再跑一次 `-All` 仍 35/35 绿**（1186.7s），
随后 13 条候选 + 16 个 issue 全部闭环。
最值得记的一条：我自己的守卫只断言了**常量**、从不碰**调用点**，所以调用点写错照样绿 ——
"守卫要有一条'故意改坏就变红'的证据"这件事，评审说对了。

### 调整

**公历日历 / 分页窗口 / 四象限 / 实心点改写搬进 `tr-draw`（ADR-0001 的第二波）**

- **问题**：ADR-0001 立的规矩是"界面里凡是能在 native 上断言的纯算法都放 `tr-draw`"，但第二波纯算法
  又攒在只编 wasm32 的文件里：月长靠 `js_sys::Date` 的"下月第 0 天"取、周几靠 `Date.get_day()`、
  "区间是不是整周"靠**本地秒差**除以 86400、页码窗口与象限判定直接写死在组件/页面里。
  `tr-ui` 只编 wasm32，`cargo test -p tr-ui --lib` 是**绿的 0 个测试** —— 这些规则一条都跑不到。
- **搬走的东西**（各一个小 interface，`cargo test -p tr-draw` 真跑）：
  * `tr_draw::calendar`：`Ymd` / `parse_ymd` / `parse_year_month` / `format_ymd_cn` / `weekday_cn` /
    `days_in_month` / `add_months` / `add_days` / `days_between` / `month_span` / `year_span` /
    `week_bounds` / `monday_offset` / `is_six_days` / `normalize_range` / `shift_period` ——
    全是**无时区的公历事实**（日序号用 Howard Hinnant 的 days_from_civil：无循环、无时区）；
  * `tr_draw::paging::page_slots`：页码窗口的收敛规则（首页 / 末页 / 当前页 ±1 必显，两端多显示几个）；
  * `tr_draw::quadrant`：`Quadrant::of` / `rows`（稳定排序，同档位保持卡片序）/ `jitter_of`
    （`DefaultHasher` 固定种子，重渲染不让点自己抖）；
  * `tr_draw::chart::solid_dots`：实心点的 SVG 定点改写 —— 连同那条"只碰 `<circle>`、别把折线填成面积"
    的断言一起搬（它从前住在 `tr-ui` 的 `#[cfg(test)]` 里，注释还写着"tests 里锁了这条"，而那条测试永不执行）。
- **留在界面侧的**：日期串 ↔ Unix 秒的换算（`tr-ui::time` 的 `format_timestamp` / `today_ymd` /
  `now_seconds` / `ymd_to_seconds` / `range_to_seconds`）—— 那需要一个时区数据库，只有宿主有；
  外加"今天"（`date_picker::today`）与全部渲染。
- **顺带修掉两个只在夏令时区才露头的算法问题**：`is_six_days`（"整周"判定）与待办的"已过 N 天"从前
  都拿本地秒差除以 86400，夏令时切换那一周会少算一天（整周被判成单日、翻页只挪一天）；现在按**日序号差**算。
- **收窄的对外面**：`components/ui/mod.rs` 不再转发没人用的 `parse_ymd` / `add_months` / `today` / `Ymd`，
  `page_slots` 的对外路径改为直接指向 `tr_draw::paging`；`time::DAY_SECONDS` 降为私有。

回归：`cargo test -p tr-draw` **57 绿**（新增 16 条：闰年与月长 / 日序号 200 年往返 / 周几锚点 /
区间对齐 / 整周判定 / 页码窗口不变量 / 象限判定与稳定排序 / 抖动散度 / 实心点只碰 `<circle>`）；
全量档 `fixtures/test.ps1 -All` **35/35 绿**（1247.7s，含重新构建的 release 产物、
`ui-stock` 真实行情、`ui-proxy` 假代理日志、`ui-todo` 四象限、`ui-transactions` 的时间范围与分页）。

**展示词汇与页面规则也进 `tr-draw`（ADR-0001 第二波·续）**

- **搬走的东西**：`tr-ui/src/format.rs` 整个模块（229 行：金额符号 / 紧凑金额 / 百分比 / 盈亏文字 /
  交易类型与股票文案 / 手数 / 短日期 / 截断，此前**一条断言都没有**）+ `scaled_text`（统计标尺）；
  `tr_draw::text`（`parse_amount_cents` 金额文本 → 分、`parse_rate` 费率文本 → f64 —— 两处实现合成一处，
  文案逐字不变）；`tr_draw::list_order::reorder_with_changes`（拖拽重排后**哪些行要落库**，
  它决定发几条写请求）；`tr_draw::stock_rows`（`TradeRow` + `group_trades`：按委托分组、组内按
  `orderSeq` 排序、**加权均价**、费用求和、单笔不合并的键约定）。
- **`tr-ui` 侧零调用点改动**：`format` 用 `pub use tr_draw::format;` 转发（131 处 `crate::format::*`
  一个字没改，同 `components/ui/mod.rs` 转发裁剪几何的先例）；重排从 `SortOrder` trait 改成两个取值闭包
  —— trait 与 DTO 都在别的 crate 里，impl 会撞孤儿规则，闭包顺手解掉这层耦合。
- **新增依赖**：`tr-draw` → `tr-domain`（金额换算与文本工具）。不破它的两条纪律：
  `tr-domain` 本身也是 native + wasm32 双可编、零 I/O。

回归：`cargo test -p tr-draw` **85 绿**（本批新增 28 条：两个符号口径 / 百分比三档与 `NaN` 兜底 /
盈亏比 `∞` / 紧凑金额阈值 / 行情缺失占位 / 手数向下取整 / 按字符截断 / 金额文本的合法与非法集合与
两句中文文案 / 费率文本的 `NaN`·`inf` / 重排"只报序号变化的行"与越界 / 成交分组的加权均价与求和）；
`clippy --all-targets -D warnings` 与 `check-ui-wasm` 干净；全量档 `fixtures/test.ps1 -All`
**35/35 绿**（1240.4s）。

**页面里剩下的纯函数也收进 `tr-draw`（ADR-0001 第二波·收尾）**

- **搬走的东西**：`tr_draw::diary_tree`（日记目录树的年 → 月 → 日分组与排序 + 三种节点的 DOM id ——
  渲染侧与"滚动定位"侧必须用同一份 id 规则，从前分居两处）；`tr_draw::stock_stats::Metric`
  （统计曲线七档指标的取值 / Y 轴语义与**上下界规则** / 0 轴参考线 —— 胜率那一档不给上下界就会多画一条
  150% 的网格线）；`tr_draw::stock_rows` 补上 `removed_rounds_text` / `impact_summary`
  （影响预览的两段文案，顺带把那三个从来没用过的形参删掉）；`tr_draw::text` 补上
  `parse_year_bound` / `parse_year_month_bound`（应用设置里"自定义年份"的筛选输入，**空 = 不限**是
  这条规则的一半）。
- **刻意留在界面侧的**（ADR-0001 的"渲染配置留在界面侧"）：`data_analysis.rs` 的
  `series_color` / `transaction_type_color` —— 它们返回的是 CSS 令牌字符串
  （`var(--transactions-color-*)`），而且 `series_color` 还依赖界面侧的调色板；`visible_type`
  只是"按名字找一条曲线"的五行查表，没有规则可断言。

回归：`cargo test -p tr-draw` **98 绿**（本批新增 13 条：日期的分组与降序 / 解析不出的日期跳过 /
id 规则 / 七档指标的取值与缺省 / 胜率轴边界 / 参考线条件 / 影响预览文案 / 年份筛选的"空 = 不限"）；
`clippy --all-targets -D warnings` 与 `check-ui-wasm` 干净。
全量档第一次 **34/35**：唯一红项是 `close-behavior` 场景 3「选「是」后进程退出」——
与本批改动无关的既有偶发（单跑三次全绿；全量档里它跑在另外 15 个界面护栏之后，机器正忙，
点「是」那一击可能落在上一帧的矩形上）。按仓库纪律把它加固成"点完等一小会儿、没退就重新找一次
按钮再点"（判据仍然是**进程必须退出**，只是不再把一次点击当成一次保证）：加固后单跑 **3/3 绿**，
`fixtures/test.ps1 -All -SkipBuild` **33/33 绿**（1009.4s，跳过两条构建步骤）。

**列表区四态（加载中 / 失败 / 空 / 就绪）的判据收进 `tr-draw`**

- **问题**：这条优先级从前在每个页面各判一遍（界面里 11 处 `if loading { "正在加载…" } else { "暂无××" }`），
  于是有两个真实后果：**失败被渲染成业务空态**（用户被告知"暂无××"，其实是查询失败了）；
  **首帧闪一下空态**（`loading` 在取数 module 的 effect 里才置位，第一次渲染时是 `false`、结果还是默认值，
  "还没回来"被判成了"确实是空"）。
- **判据**（`tr_draw::section`，`cargo test -p tr-draw` 真跑）：没跑完过（或正在重拉）一律 `Loading`
  —— 包括"失败了正在重试"那一段（手上那份错已经过期，显示加载比显示旧错误更贴近事实）；
  跑完了且有失败文案 → `Failed`（**失败不是空**）；跑完了、没失败、没数据 → `Empty`；其余 → `Ready`。
- **容器**（`components/ui/async_section.rs`）：`AsyncSection` 接四态信号 + 三个槽（加载 / 失败 / 空 + 内容），
  槽是 `ViewFn`（可反复求值，与 `Empty` 的 `actions` 同一套写法）。不给加载/失败槽时渲染默认那一行。
- **`ListQuery` 补 `loaded` / `failed`**：从前只有 `value` / `loading`，页面想区分"空"与"失败"也拿不到判据。
- **页面迁移（已完成 10 处）**：待办页的卡片视图与历史页签、模板页的列表区、事件页列表的空态判据
  （顺带修掉"第一帧显示「暂无事件」"）、分析页图表列表、股票页的统计两个页签与持仓/历史/明细/标签设置。
  其中五处让**失败**不再显示成「暂无图表 / 暂无统计 / 暂无持仓 / 暂无标签 / 还没有已完成的事项」
  （它们的查询会记 `failed`）；标签那处还保留了"还没选工作空间"这一档单独说明。
- **刻意没动的三处**（它们不是同一条规则）：`category_tag.rs` 的 `init_loading` 是一颗按钮自己的
  loading 态；`stock.rs` 的 `overview_loading` 是单个数字的骨架屏占位（没有"空"与"失败"两种渲染）；
  `transactions.rs` 的 `if loading || !loaded` **本来就是这条规则的参照实现**，逐字与 `tr_draw::section`
  一致，留在原处不影响"只有一份实现"。
- 分两步是为了让回归面一次只动一件事：先立判据与容器（`375f4a8`），再换调用点（`c38a8fc` / 本提交）。
回归：`cargo test -p tr-draw` **104 绿**（新增 6 条：首帧是加载不是空 / 失败永远不当空 /
重试期间显示加载 / 跑完为空 / 有数据就绪 / 16 种组合的优先级表）；`clippy --all-targets -D warnings`
与 `check-ui-wasm` 干净。

**写路径的判据核心：`WriteTarget`（候选 2 的第一片）**

- **问题**：读半边有 `query.rs` + `tr_draw::query`（去重 / generation / 复核判定，10 个真跑的测试），
  写半边没有对应的东西 —— "账本在写的过程中被切走了"由每个页面各写一遍，全仓 **9 处**
  `if AppStores::global().current_ledger_id.get_untracked() != ledger_id { return; }`；
  而读半边那份 `QueryCore::settle` 比的是整个 `(key, generation)`，比这些手写比较**更强**。
- **本片**：`tr_draw::change::WriteTarget` —— 出发时记下这次写替哪个账本做，回来后
  `is_stale(现在的账本)` 判断还该不该落地；规则钉住了一个容易漏的边界：**空目标（还没选账本）
  永远算过期**（那种写迟早被后端拒，落地只会让空态闪出旧数据）。日记的防抖自动保存是第一个调用点。
- **刻意不建的另一半**（"成功之后哪些读作废 / 回填"那张表）：全仓只有两个跨视图缓存读
  （`stock.statistics` / `stock.statistics.tags`），它们已经由取数 module 的**挂载复核**
  （`tr_draw::query::unchanged`）兜住；其余 27 处 `.reload()` 是"挂载即拉 / 本份读重拉"，
  与写同处一个函数。按报告自己的 deletion test：删掉那张表复杂度不会搬回调用点（因为今天没有
  可达的过期读），所以保留现状。证据见 #34 的收口评论。

**9 处手写过期守卫归零**

- `crates/tr-ui/src/change.rs` 把那条判据接到全局账本信号上（`stale(ledger_id)` / `current(ledger_id)`）：
  `transactions.rs` 7 处、`key_event.rs` 1 处、日记自动保存 1 处全部换过去 ——
  `Select-String 'current_ledger_id.get_untracked() [!=]= ledger_id'` 现在**一处都没有**，
  "什么算过期"只剩一份已测实现（`tr_draw::change::WriteTarget`）。
- 行为未变：同账本 / 切账本 / 空目标（还没选账本 = 永远过期）三档判定逐条照旧。

回归：`cargo test -p tr-draw` **108 绿**（本片 +4）；`fixtures/test.ps1 -Unit diary` 6/6 绿；
`fixtures/test.ps1 -Unit ui-transactions,ui-sync-ledger,key-event -SkipBuild` 7/7 绿（重建之后）；
`clippy -D warnings` 与 `check-ui-wasm` 干净。

**`recent` 的取值规则收进 `tr-domain`（删掉一份与注释矛盾的实现）**

- `tr-ipc` 里那段解析带着一句与代码**互相矛盾**的注释（"非数字串或 <= 0 一律报错"）—— 实际是
  **非数字串按"没传"处理**（宽松），只有数字 `<= 0` 才报 `recent 必须为正整数`。界面上看不出来
  （界面只送数字），但它是契约的一部分。
- `tr_domain::statistics` 现在是这条规则的唯一实现：`parse_recent_text` / `normalize_recent` /
  `normalize_month` + `StatisticsFilter`（三段的同一份样子；`start()` / `end()` / `tag_filter()` /
  `is_unfiltered()` 给出"哪一端不限"的判据）；7 条断言把规则逐条钉住，含"不做 trim"与
  "小数文本按没传处理"两个边界。`tr-ipc` 只留一句 `normalize_recent(...)` + 错误信封转换。
- 还没做：把 `get_statistics_range` 的 6 个形参收成一个 `&StatisticsFilter`（见 #35）。

回归：`cargo test -p tr-domain` **96 绿**（+7）；`cargo check -p tr-ipc --all-targets` 与
`clippy --all-targets -D warnings` 干净。

**统计筛选的两条自洽规则也收进 `tr-domain`**

- `StatisticsFilter::validate()` 承接 `get_statistics_range` 开头那两句与数据库无关的 `if`
  （`recent` 非负、月区间与笔数互斥），错误文案是共享常量；服务层只剩"标签必须存在""月份要能解析成日"
  两件数据库相关的事。
- **既有测试抓到我一次回归**：第一版把 `recent = -1` 归一化成"不限"，负数不再报错 ——
  `statistics_range_validation` 的 `("", "", -1)` 立刻变红；改成 `(recent != 0).then_some(recent)`
  后全绿。这条正说明那 11 条测试的价值。

回归：`cargo test -p tr-service` **157 绿**、`cargo test -p tr-domain` **98 绿**、
`fixtures/test.ps1 -Unit stock -SkipBuild` **4/4 绿**（ui-stock 202.8s，重建之后）、
`clippy -D warnings` 干净。

**统计取数的生产路径只收一个 `StatisticsFilter`**

- 新入口 `get_statistics_for(workspace, ledger_id, &StatisticsFilter)`；`tr-ipc` 在命令体里把请求的
  四段拼成筛选条件再往下传，命令层不再把四个字段当位置参数递下去。
- 保留 `get_statistics_range(...)` 作为**与 wire 请求同形**的适配器（一行），18 处服务层测试因此
  不必重写 —— 那些测试正是按"某月 / 某标签 / 最近 N 笔"逐条写规格的。
- wire 契约与 `tr-ui` 调用点**零改动**（请求字段名不变）。

回归：`cargo test -p tr-service` 157 绿、`cargo test -p tr-domain` 98 绿、
`fixtures/test.ps1 -Unit stock -SkipBuild` 4/4 绿（ui-stock 199.6s，重建之后）、clippy 干净。

### 修复

**更新状态只有一个持有者：外壳推进状态机，界面只渲染快照**

- **用户可见的缺陷**：查出「已是最新版本 / 发现新版本」之后，切到别的页面再切回
  「应用设置 → 关于软件」，结果会被抹掉、退回初始的「检查更新」。
- **根因**：`update_check` 连 `State<UpdaterState>` 都没有，外壳因此**从没被喂过**
  `CheckFinished`（`UpdaterState::apply` 是唯一改状态的地方，而生产代码里只有下载那几条在调它）。
  界面自己 `apply(CheckStarted)` / `apply(CheckFinished)` 跑第二台状态机，于是
  `update_download_status` 返回的永远是外壳那份 `idle` 快照 —— 而界面再次进入关于页时会把它整份
  `accept` 下来。"状态的唯一持有者是外壳"（AGENTS.md 与 `tr-domain::update` 的模块注释都这么写）
  从前不是真的。
- **改法**：三个会推进状态的命令各自在**命令体里**推进状态机，并返回推进后的完整快照
  （`update_check` / `update_download` / `update_cancel` 的返回类型改成 `UpdateSnapshot`；
  `update_download_status` 保持"只读"）。`update_download` 的两条早退路径（地址为空 / 不在白名单）
  改用新的 `UpdateEvent::DownloadRejected` 落档 —— 落点与"下载中途失败"一致，界面不必再自己拼
  （`UpdateSnapshot::failed_after_download_attempt` 与 `UpdateResponse::is_cancelled` /
  `update_state::ALREADY_DOWNLOADING` 一并删除，`UpdateResponse` 只剩 `update_install` 一个使用者）。
- **界面侧删掉第二台状态机**：`UpdateState::advance_with` 与 `api::update` 的 `DownloadOutcome` /
  `download_outcome` / `download_followup` 全部删除；界面只剩 `accept(快照)` 一个改状态的入口，
  "正在检查"降级成它自己的一个渲染用 bool（`display_status` 读它，不推进状态机）。
- **护栏**：`ui-update-restore.ps1` 新增第 2 步「换页再切回：检查结果必须还在」——
  从前它只覆盖"下载中切走再切回"，而那一刻外壳状态恰好非 idle，所以一直是绿的。
- **实测**：用真产物 + 真鼠标探针（`target/tests/update-reentry/probe.ps1`，不入库）对比
  修复前后 —— 修复前 `before=重新检查 after=检查更新 same=False`，修复后
  `before=重新检查 after=重新检查 same=True`。

回归：`fixtures/test.ps1 -Unit update` **6/6 绿**（31.6s），其中新加的第 2 步实测
`更新检查终态: no-update` → 换页切回后仍是 `no-update`；`fmt` / `clippy --all-targets -D warnings` /
`design-audit` 一并绿。`cargo test`：tr-domain 89、transactions 44 全绿。
（当前版本 == 最新 release，所以第 3~5 步（下载）按设计跳过；跑法写在该脚本头部注释里。）

### 调整

**外壳与更新那 27 条命令也进了同一份清单 —— 界面侧 112/112 都走常量**

- **收尾 #17 剩下的那一半**：`tr_domain::commands` 从前只有业务命令（85 条），外壳 22 条与更新 5 条
  的名字仍以字符串字面量出现在 `crates/tr-ui/src/api/{desktop,update}.rs`。现在 27 条也进了清单，
  界面侧的**命令封装**（`api/` 下 11 个模块）里一条命令名字符串都没有了。
- **清单按模块分组、条目形状一致**：`command_catalog!` 宏现在接一个"名字清单常量名"参数，
  `SHELL_COMMANDS`（22）/ `UPDATE_COMMANDS`（5）/ `BUSINESS_COMMANDS`（85）各由一次声明展开
  （常量与名字同源），`COMMAND_GROUPS` 是三组的并集，供"全表不重名 / 总数对得上"这类断言用。
  没有 `req` 形参的命令在条目里写成 `()`（`CONFIG_GET = "config_get": () => ConfigSnapshot`），
  界面侧走新加的 `ipc::call_no_args` 与 `ipc::call_void_no_args`；分组的名字与注册表的路径前缀
  一一对应（`registry.rs` 的 `SEGMENTS`），所以"哪条命令归谁"只有一处出处。
- **守卫扩到全表**：`src-tauri/src/registry.rs` 的 `catalog_matches_registration` 不再按前缀只查业务
  那一段，而是把注册表按"外壳 / 更新"与"业务"两段分别与清单比对，再断言注册表里没有两段都不认的
  路径（模块名打错）。负向断言相应加厚：少一条（每段各试一次）/ 改名 / 重复 / 整段没注册 / 混进
  不认的路径，五类都断言为红。既然全表都核对，先前那条"外壳命令还在注册表里"的兜底测试就删了
  —— 它已被全表比对覆盖。
- **过渡入口删掉**：`crates/tr-ui/src/ipc.rs` 的 `call_by_name` / `call_void_by_name` /
  `call_no_args_by_name` / `call_void_no_args_by_name` 四个"按名字调用"的内部入口完成了使命，
  一并删除 —— 现在只有条目入口，没有第二条路。
- **对使用者零行为变化**：命令名、入参、返回、错误信封与文案一处未改；不新增依赖，
  也不需要数据库迁移。`workspace_init`（界面从不调用它）与其余 26 条一样只是换了引用方式。
- **顺带修掉 `ui-crud` 的一处偶发假红**：图表删除那一步会截图取样，断言 Popconfirm 的确认键
  **真的画在屏幕上**（"气泡被滚动容器裁掉"那个回归唯一的护栏）。但气泡有淡入动画，而 UIA 里
  元素一进树就报得出来 —— 只取一次图会抓到"画了一半"的那一帧（实测取到过 `#A7B9FD`：
  主色与底色各一半，看着像没画出来）。现在改成**反复取图直到取到主色**（最多 8 秒）：
  真被裁掉的话等多久都取不到主色，只是慢一帧的话下一次就成了。

回归：全量档 `fixtures/test.ps1 -All -SkipBuild` **33/33 全绿**（构建复用了上一轮 release 产物；
两轮全量共 35 步全覆盖，含 `ui-stock` 的真实行情与 `ui-proxy` 的假代理日志）；
`cargo test` 各包 89 / 157 / 76 / 11 / 44 / 37 全绿；`fmt` + `clippy --all-targets -D warnings` +
`check-ui-wasm --all-targets` 干净。守卫的负向断言做过真实变异实验（从注册表里删掉
`update_cancel` / `config_get` → 报出"`crate::updater::` 那一段：清单里有、实际没有：[…]"，
还原后复绿）。既有 `ui-*` 端到端护栏的断言一处未改。

## [0.14.0] - 2026-10-05

### 调整

**命令名与事件名收成一份来源（界面侧不再有命令名字符串）**
- **问题**：请求 / 响应类型早就收在 `tr_domain` 里（字段名不可能漂移），但**名字**还停在手抄 ——
  85 条业务命令的名字是界面侧 10 个封装里的字符串字面量，事件名则一半是两侧**各写一份 `const`**
  （`update:*` 三条 + `devtools:state-changed`）、一半干脆是两侧各写一个字面量
  （`window-state-changed` / `workspace-changed`），"唯一权威，逐字照抄"这句注释出现在至少四个文件里。
  后果：名字打错、事件名改一边忘另一边、注册表漏一条，都只在**运行时**表现为"命令不存在"或
  "事件永远不来"，而且这些约定没有任何测试。
- **名字靠共享清单**：新增 `tr-domain::commands`（一条 `Command<Req, Res>` = **名字 + 请求类型 +
  响应类型**，不含逻辑）与 `tr-domain::events`（6 个事件名）。界面侧的调用点改成
  `ipc::call(commands::LEDGER_LIST, req)` —— 名字与两个类型从同一条目取，写错名字**编译期**就报错。
- **三处一致由能跑的断言闭合**（不靠人读注释，两处守卫各带负向断言）：
  * `tr-ipc` 的 `implementations_match_the_catalog` 断言"本 crate 的命令实现 == 清单"，
    `registration_name_is_the_function_name` 钉住"注册名 = 函数名"这个前提（命令不许 `rename`）；
  * `src-tauri/src/registry.rs` 新增注册清单宏：`generate_handler![]` 的调用处与守卫读的是
    **同一份 token**，`catalog_matches_registration` 断言"注册表 == 清单"。
  * 两条合起来 = "注册的每一条都真的存在、存在的每一条都真的注册了"。
- **事件名两侧合成一份**：应用里**每一个** IPC 事件名都进了 `tr-domain::events`（共 6 条：spec 点名的
  `update:*` 三条与 `devtools:state-changed`，外加同一类问题的 `window-state-changed` 与
  `workspace-changed` —— 前者两侧各写一个字面量，正是本票要消灭的形态）。外壳（`updater.rs` /
  `commands.rs` / `shell.rs`）与界面（`api/update.rs` / `api/desktop.rs` / `shell.rs`）现在都引用同一份常量。
- **对使用者零行为变化**：命令名、入参、返回、错误信封与文案一处未改，不新增依赖、不引入代码生成、
  继续用 Tauri 官方的注册宏，也不需要数据库迁移。事件名逐字未改，只是从"两侧各写一遍"变成"引用同一份"。
- **范围**：本轮清单里的是**业务命令那 85 条**。外壳 22 条 + 更新 5 条仍以字面量出现在
  `api/{desktop,update}.rs`（走 `ipc::call_by_name` 等内部入口），把它们也收进清单是 #28；
  它们的注册侧由 `src-tauri/src/registry.rs` 的注册清单覆盖（与 `generate_handler![]` 同源）。
- **顺带修掉三处**：`api/ledger.rs` 的 `ledger_list` 从前发的是 `IdRequest`、命令收的是
  `LedgerListRequest`（形状相同所以一直没暴露，线上 JSON 一字不差）；`fixtures/test.ps1` 里
  `crates/tr-ui/src/api/update.rs` 被更靠前的通配规则挡住、拉不到 `update` 分组；
  `fixtures/close-behavior.ps1` 的 `Get-MainWindow` 从前取"本进程的第一个顶层窗口"，
  而 Tauri 的拖拽/缩放浮层同样是本进程的顶层窗口、且没有界面 —— 抓错时后半段按名字找
  「关闭」按钮必然全落空（正是 AGENTS.md 那条"启动时抓主窗口"说的情形，全量档里偶发红过一次），
  现在改成只认"窗口里已经有 Button"的那个。

回归：全量档 `fixtures/test.ps1 -All` **34/35**——唯一红项就是上面那条 `close-behavior` 的
偶发假红（同一份产物单独重跑两次全绿，证明与本次改动无关；加固后又连跑三次全绿）。
其余 34 步都在本次最终代码上通过：`fmt` / `clippy` / `design-audit` / `test-domain`(88) /
`test-store`(157) / `test-service`(76) / `test-ipc`(11) / `test-src-tauri`(44) / `test-draw`(37) /
`check-ui-wasm` / `schema-diff` / `smoke` / `window-bounds` / `migrate-workspace` /
`ui-smoke` / `ui-shots` / `ui-transactions` / `ui-crud` / `ui-drag` / `ui-key-event` /
`ui-link-event` / `ui-diary-edit` / `ui-diary-ledger` / `ui-diary-io` / `ui-sync-ledger` /
`ui-upload` / `ui-proxy` / `ui-about` / `ui-features` / `ui-stock`（真实行情）/ `ui-todo` /
`ui-update-restore`。守卫的负向断言做过真实变异实验（删一条注册 / 重复注册 / 删一条清单条目
→ 全红，还原后复绿）；既有 `ui-*` 端到端护栏的断言一处未改。

**更新流程：状态机收成一处拥有，界面只渲染快照**

- **问题**：一次"检查 → 下载 → 安装"的状态同时活在两个 module 里 —— 外壳只记"正在下载的键、
  已下载路径、取消标记、百分比、速度"，界面那侧拿着七个状态字符串（`idle` / `checking` /
  `available` / `no-update` / `downloading` / `downloaded` / `error`）与版本、地址、digest、
  发行说明、错误、进度、速度，两边靠**三个只带局部字段的事件 + 进页面时一次轮询**对齐。
  后果有三：**状态机的规则写在渲染函数里**（"检查失败 ≠ 已是最新"得靠界面自己判断
  `hasUpdate == false` 且有 `error`）、**跨页面恢复是特例**（只能靠一条依赖真实 GitHub API
  的端到端护栏兜底）、**这条链路的单元测试根本不执行**（应用外壳不在测试矩阵里）。
- **词表与合法迁移进领域层**：新增 `tr-domain::update`（纯逻辑、零 I/O，native 与 wasm 共用）。
  `UpdateStatus` 是那七个取值的**类型**（序列化取值与从前的字符串逐字一致，`no-update` 仍是连字符），
  `UpdateEvent::apply` 是唯一的迁移入口：**"检查失败 ≠ 已是最新"**（带 `error` 一律进 `failed`）、
  进度只在 `downloading` 时生效（迟到的进度不会把状态拉回来）、重复事件幂等、没有下载地址不进
  下载态。界面侧不再出现 `"downloading"` 这类裸字符串比较（`match` 穷尽，加一档编译器会逼着补文案）。
- **外壳是唯一持有者**：`UpdaterState` 直接持有快照，推进状态机、发快照；`update_download_status`
  返回**同一份快照** —— "进页面补状态"从特例变成常规读法，与事件谁先到都不影响结果。
- **事件载荷从"局部字段"改成"完整快照"**（`update:download-progress|complete|error` 三个**名字不变**，
  改的是载荷）：界面侧只剩"最后一次快照"一个信号 + 应用名 / 版本，不再自己拼状态。
- **对使用者零行为变化**：检查中 → 有新版 / 已是最新 / 失败、下载中（进度 + 速度 + 取消）、
  下载完成（安装并退出）、失败（原因 + 重试）的可见状态与文案逐字未改；不上后台自动更新、
  不改下载实现与 sha256 校验、不动代理探测、不动发布链路、不新增依赖。
- **顺带把应用外壳的测试纳入验收**：`fixtures/test.ps1` 新增一步 `cargo test -p transactions`
  （登记进 `shell` 与 `update` 两个分组）。测试目标跳过入口（`main` 被 `cfg(test)` 排除）——
  `generate_context!()` 要嵌 `crates/tr-ui/dist`，而那份产物由 trunk 生成、不入库，
  少了它连 `cargo test` 都编译不过。之前那边 39 条 `#[cfg(test)]` 从没被执行过。

回归：`cargo test -p tr-domain`（77 项，其中 `update` 19 项覆盖状态迁移、乱序 / 幂等 / 取消 / 文案）、
`cargo test -p transactions`（39 项，以前一次都不跑）、`fixtures/test.ps1 -Unit update` 全绿
（`test-domain` + `test-src-tauri` + `ui-update-restore`，端到端那条仍依赖真实 GitHub API）。

**发布后补验：这次真的走了更新链路（v0.14.0 已发出之后）**

本地版本一直等于最新 release，所以 `ui-update-restore` 从前只能走「已是最新版本」那一支 ——
下载、进度、跨页面恢复这几段**从来没在真机上跑过**。这次发布补上了：用一个只改 Tauri 版本
（`TAURI_CONFIG='{"version":"0.13.1"}'`，**源码一行没动**）的探针构建走真实链路 ——
检查到「有新版 v0.14.0」→ 点「立即更新」→ **下载完成**（这条同时证明发布资产的 `asset.digest`
sha256 在应用自己的校验里通过）→ 切到「记账」再切回「关于软件」仍是下载态 →
`fixtures/ui-update-restore.ps1` 全绿。仍未覆盖的两支：「取消下载」（5.7 MB 下得太快，
点不到）与「安装并退出」（会真的把 0.14.0 装上）。

## [0.13.1] - 2026-10-05

### 修复

**筛选之后，底部统计条跟着筛选走**

- 底栏那三个数（收入 / 支出 / 转账）原来只按「账本 + 时间范围」汇总，`items`（筛选条件）
  一律不参与 —— 于是筛出 3 条支出、底部却写着整月合计（用户报的正是这个：列表 3 条
  合计 1428.90，底栏 3488.21）。
- 现在统计与列表**共用同一份 WHERE**：账本 + 时间范围 + 条件项；分页与排序不进 WHERE，
  所以翻页不影响这三个数。实现上只留一处拼 SQL 的地方
  （`TransactionRecordDao::query_statistics_where`）：`query_filtered` 传"带条件"、
  `query_statistics`（分析页图表面板用）传"不带条件"，两个口径各自写在注释里。
- **口径变了**，两条旧规格同时改写：`tr-store` 的 `statistics_ignore_items_and_respect_time_range`
  → `statistics_follow_items_and_respect_time_range`（并补一条"分页不影响统计"），
  `tr-service` 的 `condition_filter_and_statistics_are_independent`
  → `statistics_follow_the_condition_while_paging_stays_out`。

回归：`cargo test -p tr-store -p tr-service` 全绿（157 + 76）；
`fixtures/ui-transactions.ps1` 的筛选那一段新增一条断言 —— 筛到唯一那条 88.88 的支出后，
页面上 "88.88" 必须出现**至少两次**（行内 + 统计条），旧实现下它只出现一次。

**iPhone 的 HEIC 照片现在能上传了**

- 现象：一次选 3 张 `IMG_xxxx.HEIC`，三张全是 `HEIC 转换失败: 图片解码失败`（用户报的截图）。
- 根因：转码这一步原来是"把 HEIC 丢给 WebView2 解"（`createImageBitmap` / `<img>`），
  而**这条路根本走不通** —— Chromium 内核里没有 HEIF 解码器（HEVC 图片受专利约束），
  Windows 侧要靠商店里的「HEIF 图像扩展」，而它默认不装。本机实测更直接：
  Windows 自己的 WIC 解同一张图也是 `No imaging component suitable to complete this operation`，
  也就是说**系统里根本没有可用的 HEIC 解码器**。
- 改法：换成随程序分发的纯 Rust 解码器（`heic-rs`，MIT OR Apache-2.0，
  `default-features = false` —— 只要 no_std + alloc 的核，native 与 wasm32 共用）。
  解码落在 `tr-draw::heic`（界面侧纯算法层，字节进像素出，所以能在 native 上断言）；
  界面侧改成：读字节（`readAsArrayBuffer`）→ 解出 RGBA8 → 写进 canvas 的 `ImageData`
  → `toDataURL("image/jpeg", 0.92)`。**后端契约一个字没动**：仍然只接受
  JPEG/PNG/GIF/WebP，HEIC 依旧不落盘；工作空间图标的方形裁剪走同一条路，一并修好。
- 代价：wasm 大了一点 —— 打进安装包的那份（brotli 压缩后）1.50 MiB → 1.59 MiB。
- 回归：`cargo test -p tr-draw` 拿 `fixtures/heic/flat-64.heic` 与**别人的解码器**的解码结果
  （Apple `sips`，`flat-64.ref.png`）逐像素比 —— mean abs ≈ 0.7、max 2，都在 HEVC 有损的预期内；
  `fixtures/ui-upload.ps1` 新增一条真跑：上传 `gradient-512.heic`，断言落盘的是 `.jpg`、
  512×512、缩略图 300×300，且像素仍是那张渐变图（红通道左→右递增，排除"解成一张空图"）。
  那三张测试图是合成图（上游生成脚本产出、MIT OR Apache-2.0），来历见 `fixtures/heic/README.md`。
  真机再走一遍：iPhone 24MP（`IMG_1222.HEIC`，4284×5712，2.1 MiB）→ 提交到**原图落盘 3.1 秒**、
  到**缩略图 + 入库 3.7 秒**，落盘 4284×5712 的 `.jpg`、缩略图 300×400（解码在 WebView 主线程上跑，
  这段时间界面不响应；24MP 这个量级可接受，真卡了再谈挪进 Worker）。
- 顺带修了 `fixtures/ui-upload.ps1` 取窗口的方式：原来按"进程的第一个窗口"取，而进程里有 3 个顶层窗口
  （界面、单实例插件的隐藏窗口、Tao 的 `Thread Event Target`），取错就是"UIA 可读元素 0 个"、
  后面所有按名字的查找全落空（看着像界面没渲染）。现在按"这个窗口里有侧栏「记账」"认。

## [0.13.0] - 2026-10-05

### 调整

**页面取数收进一个 module：九个页面不再各写一份"什么时候拉、失败怎么提示"**

- **决策搬到了能 native 断言的地方**：新增 `tr-draw::query` —— 一个纯状态机
  （`mount` / `observe` / `reload` / `settle` / `failed`），产出只有"什么都不做 / 清空 / 发一次请求"。
  同一份 key 不重发、重拉强制发一次、同 key 在飞时去重、key 为空时清空且不发、
  请求期间换 key 的过期结果落空、失败后下一次观察能重试 —— 这六条以前散在九个页面里，
  现在 `cargo test -p tr-draw` 真跑（10 条断言）。
- **界面侧只剩声明**：`tr-ui::query` 的 `Query<T>` 让页面写"拉什么、key 是什么"。它表达六件事：
  结果不限于是列表（记账·记录要的是"记录 + 统计"一整份）、key 不限于是字符串（筛选 / 分页 / 日期）、
  失败三档（提示 / 静默 / 把 `Err` 当业务空）、有"加载过"这一档（区分"还没回来"与"确实是空"）、
  跨视图缓存是一条显式策略、复合拉取是"多条声明 + loading 取并集"。
- **逐页**：记账·记录、股票（六条查询 + 统计与标签的跨视图缓存）、分类标签与事件、日记都迁走了；
  应用设置按边界**一条都不进**（它是"命令 + 本地状态"，不是取数，理由落在 module 头）。
- **股票页删掉一组补丁**：过去为"统计视图是普通函数、每次重渲染都重建"打的五个模块级
  `thread_local` 槽位（`STATS_DEDUPE` / `STATS_DATA` / `STATS_LOADING` / `STATS_TAGS` /
  `STATS_LEDGER`）与那个手写去重键函数一起退休，换成 `Cache::Keep` + 缓存作用域（= 账本）：
  切走再切回来先显示缓存、再静默复核；复核结果一致就不写信号（曲线不再重播入场动画）；
  切账本整份作废；失败保留手上的旧曲线。
- **文档跟着改**：`AGENTS.md` 的 `tr-draw` 一行改成「界面侧纯算法层：绘制 + 页面取数决策核心」，
  `docs/adr/0001` 补了「绘制的名字窄了」一节；六个页面里"取数为什么不用 ListQuery"的段落
  清掉，换成对 module 头那份清单的引用（旧的 `ListQuery` 收成新 module 上的一层薄壳，
  分析 / 模板 / 待办三页因此一行未改）。
- **对使用者零行为变化**：加载中 → 结果 / 失败提示 / 空态的次序与文案、"查询失败：…"这类前缀、
  股票行情降级（现价 `-`、浮动盈亏与当日涨跌 `—`、持仓市值按成本计入）全部逐字未改；
  既有界面护栏的断言与期望值一处未改。这一版不动 IPC 命令面、不动 wire / DTO、
  不动数据库结构与迁移引擎 —— 不需要迁移，正在使用的工作空间不受影响。

**边界（写进 module 头，不为统一而扩大 interface）**：写入路径（下单 / 改成交 / 回滚 / 重置 /
归档 / 保存设置）与一次性旁路读取（查股票名）不进 module；按 key 的惰性缓存两处
（事件页"按日期拉图片与关联交易"、分析页"按图表 id 拉曲线"）形状对不上跨视图缓存，
留在页面里等第三个同形状的使用面出现再抽。

回归：`cargo test -p tr-draw` 34 项全绿；`pwsh -File fixtures/test.ps1 -All -SkipNetwork`
30 通过 / 2 跳过（跳过的是依赖外网的 `ui-stock` / `ui-update-restore`）；
`ui-stock` 单独跑真实行情全生命周期通过（204s）；
`fmt` / `clippy --all-targets -D warnings` / `check-ui-wasm` / `design-audit` / `schema-diff` 全绿。

（0.12.x 及更早的 release 已从 GitHub 删除，对应小节一并移除；需要时查 git 历史。）