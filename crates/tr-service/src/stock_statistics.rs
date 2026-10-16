//! 逐笔结算统计。
//!
//! **时间轴 = 建仓时间。** 一轮的锚点是它**首次建仓**的委托时间（`round.opened_at`）：排序、
//! 月份区间与「最近 N 笔」的日期、资金事件（追加本金 / 支取）的插值时点、明细表与曲线上的日期，
//! 用的都是它。为什么不用清仓时间定轴：**后建仓的一轮可能先清仓** —— 按清仓排会把两轮的先后
//! 关系颠倒，统计（编号、分组切分、回撤、区间筛选）会跟着一起颠倒。清仓时间仍带在点上
//! （`closed_at`），只是不再决定顺序。
//!
//! 统计口径：
//! * 胜率 = 盈利笔数 ÷ 总笔数（**平局计入总笔数**，不计胜负）；
//! * 平均盈利/亏损分别只按盈利笔与亏损笔求和取平均，亏损金额取正数；
//! * 盈亏比 = 平均盈利 ÷ 平均亏损（**无亏损样本时为 null**）；
//! * 期望值 = 胜率 × 平均盈利 − (1 − 胜率) × 平均亏损 = 累计盈亏 ÷ 总笔数；
//! * 最大回撤按每轮**建仓时点**的总资产曲线（当时的本金 + 累计已结算盈亏 − 当时累计支取）
//!   从高点跌落的幅度计算；本金追加/支取按记录日期参与时序，占本金比例使用**当时的本金**；
//! * 带筛选（月份区间 / 最近 N 笔 / 标签）时，**区间内从 1 起重新编号**，
//!   累计盈亏、胜率、平均盈亏、回撤全部按筛选集合独立重算；
//! * 标签先过滤轮次，使「最近 N 笔」取该标签内的最近 N 笔。
//!
//! **统计策略**（`tr_domain::statistics::StatisticsStrategy`）只决定"入选的这批结算怎么切成点"，
//! 上面每一条口径都不变：
//! * 逐笔累计（默认）：第 i 个点 = 第 1..=i 笔的累计值，`points` 是一条完整的递进曲线；
//! * 分组统计：每 N 笔一组、**组间互不重叠**，每个组点是**该组自己**的统计 —— 累计盈亏 = 组内盈亏
//!   之和，回撤 = 组内累计盈亏曲线（从 0 起步）的最大回落，占本金比例仍用"该组末笔当时的本金"。
//!   组点放在 `groups` 里，而 `points` **照旧算出来**：顶部合计与「明细」分栏读的都是它，
//!   分组只换曲线画哪条序列（明细是一行一笔结算的原始清单，不跟着口径变）。
//!   最后一组不足 N 笔时按实际笔数收尾，不做补齐。

use chrono::Datelike;
use tr_domain::consts;
use tr_domain::dto::{round_pnl, StockStatisticsDto, StockStatisticsPointDto};
use tr_domain::error::AppError;
use tr_domain::models::{StockFundRecord, StockTradeRound};
use tr_domain::statistics::StatisticsStrategy;
use tr_store::dao::stock::StockDao;
use tr_store::Workspace;

use crate::error::db;
use crate::stock::{contains_tag, get_or_create_account, get_trade_tags, percent_of, unix_to_date};
use crate::{ServiceError, ServiceResult};

/// 一条已归档的完整结算（一次「建仓 → 清仓」轮次）及其盈亏。
struct SettleEvent {
    round: StockTradeRound,
    stock_name: String,
    pnl: i64,
    pnl_rate: f64,
    trade_count: i64,
}

/// 结算统计时序中的一步：资金事件（追加本金/支取）或一笔结算。
struct StatAction {
    date: String,
    /// 0 = 资金事件，1 = 结算事件（**同日资金先于结算**）
    order: i32,
    flow_index: Option<usize>,
    event_index: usize,
}

/// 一笔本金追加 / 支取 / 利息归本：按记录日期参与统计时序，
/// `withdraw` 与 `interest` 都记为正数金额。
struct CapitalFlow {
    date: String,
    add: i64,
    withdraw: i64,
    /// 利息归本（只增加总资产，不改本金口径）
    interest: i64,
    created_at: i64,
    id: String,
}

/// 分组口径下一组的累加器（逐笔累计口径不用它）。
///
/// 与逐笔累计唯一的差别是**起点**：组的累计盈亏从 0 起步，回撤只看组内那条曲线的峰谷，
/// 于是每个组点回答"这 N 笔打得怎么样"，而不是"打到这儿一共怎么样"。
/// 组内每笔的算法与全量那条完全一致（平局计入笔数、盈亏分别求和、回撤取峰值回落）。
///
/// 组点 = **挂在组末那笔结算上的、覆盖整组的统计值**：`pnl` / `pnl_rate` / `trade_count` /
/// 日期 / 股票 / 标签都取组末那笔（与逐笔累计点的"本笔"字段同一套语义），
/// 只有 `total_pnl` / 胜率 / 平均盈亏 / 期望值 / 回撤 / `span` 是本组的。
#[derive(Default)]
struct GroupAcc {
    /// 组内笔数（末组可能不足 `group_size`）
    count: i64,
    win: i64,
    loss: i64,
    win_sum: i64,
    loss_sum: i64,
    /// 组内盈亏合计（平局记 0，但仍计入 `count`）
    pnl: i64,
    /// 组内累计盈亏曲线的当前值 / 峰值 / 最大回落，都从 0 起步
    cum: i64,
    peak: i64,
    drawdown: i64,
}

impl GroupAcc {
    /// 收一笔结算（调用顺序必须与结算时序一致）。
    fn push(&mut self, pnl: i64) {
        self.count += 1;
        self.pnl += pnl;
        if pnl > 0 {
            self.win += 1;
            self.win_sum += pnl;
        } else if pnl < 0 {
            self.loss += 1;
            self.loss_sum += -pnl;
        }
        self.cum += pnl;
        if self.cum > self.peak {
            self.peak = self.cum;
        }
        let drawdown = self.peak - self.cum;
        if drawdown > self.drawdown {
            self.drawdown = drawdown;
        }
    }

    /// 收尾成一个统计点：`anchor` 是本组**最后一笔**的结算事件，`sequence` 是该笔在入选集合里的
    /// 序号，`principal_at` 是那时的本金（回撤率的分母）。
    fn finish(
        &self,
        anchor: &SettleEvent,
        sequence: i64,
        principal_at: i64,
    ) -> StockStatisticsPointDto {
        let mut point = StockStatisticsPointDto {
            sequence,
            span: self.count,
            opened_at: anchor.round.opened_at,
            closed_at: anchor.round.closed_at,
            stock_code: anchor.round.stock_code.clone(),
            stock_name: anchor.stock_name.clone(),
            stock_round_no: anchor.round.round_no,
            tag: anchor.round.tag.clone(),
            pnl: anchor.pnl,
            pnl_rate: anchor.pnl_rate,
            trade_count: anchor.trade_count,
            total_pnl: self.pnl,
            win_count: self.win,
            loss_count: self.loss,
            max_drawdown: self.drawdown,
            ..StockStatisticsPointDto::default()
        };
        if self.count > 0 {
            point.win_rate = percent_of(self.win, self.count);
            // 期望值 = 组内盈亏合计 ÷ 组内笔数（与全量的"累计盈亏 ÷ 总笔数"同一条式子）
            point.expectancy = (self.pnl as f64 / self.count as f64).round() as i64;
        }
        if self.win > 0 {
            point.avg_win = (self.win_sum as f64 / self.win as f64).round() as i64;
        }
        if self.loss > 0 {
            point.avg_loss = (self.loss_sum as f64 / self.loss as f64).round() as i64;
            let ratio = if point.avg_win > 0 {
                point.avg_win as f64 / point.avg_loss as f64
            } else {
                0.0
            };
            point.pnl_ratio = Some(ratio);
        }
        if principal_at > 0 {
            point.max_drawdown_pct = percent_of(self.drawdown, principal_at);
        }
        point
    }
}

/// 全量逐笔结算统计（自第 1 笔起累计口径）。
pub fn get_statistics(workspace: &Workspace, ledger_id: &str) -> ServiceResult<StockStatisticsDto> {
    statistics(
        workspace,
        ledger_id,
        &tr_domain::statistics::StatisticsFilter::default(),
        "",
        "",
    )
}

/// 筛选统计：时间范围（含首尾整月）与最近 N 笔二选一，两者均可与交易标签叠加；
/// 统计口径（逐笔累计 / 分组统计）见 [`tr_domain::statistics::StatisticsStrategy`]。
///
/// **生产路径上的取数入口**（界面 → `tr-ipc` → 这里），只收一个筛选条件
/// （`tr_domain::statistics::StatisticsFilter`）：自洽规则由它的 `validate()` 判，
/// 这里只做"数据库相关"的两件 —— 标签必须存在、月份区间要能解析成日。
pub fn get_statistics_for(
    workspace: &Workspace,
    ledger_id: &str,
    filter: &tr_domain::statistics::StatisticsFilter,
) -> ServiceResult<StockStatisticsDto> {
    filter.validate().map_err(AppError::bad_request)?;
    let (start_month, end_month) = (filter.start_month.as_str(), filter.end_month.as_str());
    let tag = filter.tag.as_str();
    if !tag.is_empty() {
        let available_tags = get_trade_tags(workspace, ledger_id)?;
        if !contains_tag(&available_tags, tag) {
            return Err(AppError::bad_request("无效的交易标签").into());
        }
    }
    let (from_day, to_day) = normalize_statistics_month_range(start_month, end_month)?;
    statistics(workspace, ledger_id, filter, &from_day, &to_day)
}

/// 四个裸字段形状的适配器（**与 wire 请求同形**，方便测试按"某月某标签某笔数"逐条写规格）。
///
/// 它固定是**逐笔累计**口径（`strategy` 缺省即 Cumulative）；要分组口径就不要再加第 7、8 个
/// 位置参数了 —— 用 [`get_statistics_for`] 收一个 [`tr_domain::statistics::StatisticsFilter`]。
pub fn get_statistics_range(
    workspace: &Workspace,
    ledger_id: &str,
    start_month: &str,
    end_month: &str,
    recent: i64,
    tag: &str,
) -> ServiceResult<StockStatisticsDto> {
    get_statistics_for(
        workspace,
        ledger_id,
        &tr_domain::statistics::StatisticsFilter::new(
            start_month,
            end_month,
            // `0` 是"不限"；**负数要原样交给判据**（它才管得了"必须为正整数"这条）
            (recent != 0).then_some(recent),
            tag,
        ),
    )
}

/// 结算统计实现。筛选条件直接收 [`StatisticsFilter`]（不再摊成位置参数 —— 那正是 `recent` 的
/// 取值规则曾经散成两处的原因，而且摊开后参数会越过 clippy 的 7 条线）。
/// `from_day`/`to_day` 是已经归一化好的日期边界（空串 = 不限）。
///
/// `filter.strategy` 是分组口径时额外算一份 `groups`（每 `filter.group_size` 笔一组）；
/// `points`（逐笔累计）**两个口径下都照旧算出来** —— 顶部合计与「明细」分栏读的都是它（见模块文档）。
fn statistics(
    workspace: &Workspace,
    ledger_id: &str,
    filter: &tr_domain::statistics::StatisticsFilter,
    from_day: &str,
    to_day: &str,
) -> ServiceResult<StockStatisticsDto> {
    let recent = filter.recent.unwrap_or(0);
    let tag = filter.tag.as_str();
    let strategy = filter.strategy;
    // 分组口径下 `group_size` 已由 `validate()` 保证落在 2..=1000；逐笔累计口径下它不参与计算
    let group_size = filter.group_size.unwrap_or(0);
    let conn = workspace.connection();
    crate::stock::ensure_trade_history_backfill(&conn, ledger_id)?;
    let histories = db(StockDao::list_trade_histories(&conn, ledger_id))?;

    let mut events: Vec<SettleEvent> = Vec::with_capacity(histories.len());
    for history in &histories {
        let rounds = db(StockDao::list_trade_rounds_by_stock(
            &conn,
            ledger_id,
            &history.stock_code,
        ))?;
        for round in rounds {
            // 0 手的标记成交（清仓"只归档"那一笔）不算成交：笔数与金额都不该被它带偏
            let trades: Vec<tr_domain::models::StockTrade> =
                db(StockDao::list_trades_by_round(&conn, &round.id))?
                    .into_iter()
                    .filter(|trade| trade.shares > 0)
                    .collect();
            let (pnl, pnl_rate, _) = round_pnl(&trades);
            events.push(SettleEvent {
                stock_name: history.stock_name.clone(),
                round,
                pnl,
                pnl_rate,
                trade_count: trades.len() as i64,
            });
        }
    }
    if !tag.is_empty() {
        events.retain(|event| event.round.tag == tag);
    }
    // 稳定排序：**建仓时间** → 清仓时间 → created_at → id
    // （后两档是同值兜底；「同日建仓」时先清仓的排前面，读起来仍是那一批的先后）
    events.sort_by(|left, right| {
        left.round
            .opened_at
            .cmp(&right.round.opened_at)
            .then(left.round.closed_at.cmp(&right.round.closed_at))
            .then(left.round.created_at.cmp(&right.round.created_at))
            .then(left.round.id.cmp(&right.round.id))
    });

    let account = get_or_create_account(workspace, ledger_id)?;
    let flows = list_capital_flows(&conn, ledger_id)?;
    // 初始本金 = 当前本金 − 全部「追加本金」；本金追加/支取按记录日期参与时序重放
    let mut initial_principal = account.principal;
    for flow in &flows {
        initial_principal -= flow.add;
    }

    let total_events = events.len();
    let mut included_total = 0_i64;
    for (index, event) in events.iter().enumerate() {
        let date = unix_to_date(event.round.opened_at);
        if include_statistics_event(&date, index, total_events, from_day, to_day, recent) {
            included_total += 1;
        }
    }

    let mut result = StockStatisticsDto {
        principal: account.principal,
        round_count: included_total,
        points: Vec::with_capacity(included_total as usize),
        groups: Vec::new(),
    };
    if included_total == 0 {
        return Ok(result);
    }
    let use_window = !from_day.is_empty() || recent > 0 || !tag.is_empty();
    let grouped = strategy == StatisticsStrategy::Grouped;
    // 分组口径的组点（逐笔累计口径下始终为空）；`group_size` 已由 `validate()` 保证 `>= 2`
    let mut groups: Vec<StockStatisticsPointDto> = if grouped {
        Vec::with_capacity(((included_total + group_size - 1) / group_size).max(1) as usize)
    } else {
        Vec::new()
    };
    let mut group_acc = GroupAcc::default();

    // 资金事件与结算事件按日期合成同一条时序，保证本金追加/支取在正确时点影响总资产峰值与回撤
    // （结算事件落在**建仓日**上，见模块文档的"时间轴 = 建仓时间"）
    let mut actions: Vec<StatAction> = Vec::with_capacity(flows.len() + events.len());
    for (index, flow) in flows.iter().enumerate() {
        actions.push(StatAction {
            date: flow.date.clone(),
            order: 0,
            flow_index: Some(index),
            event_index: 0,
        });
    }
    for (index, event) in events.iter().enumerate() {
        actions.push(StatAction {
            date: unix_to_date(event.round.opened_at),
            order: 1,
            flow_index: None,
            event_index: index,
        });
    }
    // 稳定排序：date → order（同日资金先于结算）
    actions.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then(left.order.cmp(&right.order))
    });

    let mut total_count = 0_i64;
    let mut win_count = 0_i64;
    let mut loss_count = 0_i64;
    let mut win_sum = 0_i64;
    let mut loss_sum = 0_i64; // 亏损金额合计（正数）
    let mut cum_pnl = 0_i64;
    let mut principal_at = initial_principal;
    let mut withdrawn_at = 0_i64;
    // 累计利息归本：进总资产，但不进本金口径（最大回撤率的分母保持"投入本金"）
    let mut interest_at = 0_i64;
    // 总资产曲线：从「初始本金」起步（`principal_at + interest_at + cum_pnl - withdrawn_at`）
    let mut equity = principal_at + interest_at + cum_pnl - withdrawn_at;
    let mut peak_equity = equity;
    let mut max_drawdown = 0_i64;

    let mut window_count = 0_i64; // 区间内笔数（区间内从 1 重新编号）
    let mut window_win_count = 0_i64;
    let mut window_loss_count = 0_i64;
    let mut window_win_sum = 0_i64;
    let mut window_loss_sum = 0_i64; // 区间内亏损金额合计（正数）
    let mut window_cum_pnl = 0_i64;
    let mut window_peak_pnl = 0_i64;
    let mut window_max_drawdown = 0_i64;

    for action in &actions {
        if let Some(flow_index) = action.flow_index {
            let flow = &flows[flow_index];
            principal_at += flow.add;
            withdrawn_at += flow.withdraw;
            interest_at += flow.interest;
            equity = principal_at + interest_at + cum_pnl - withdrawn_at;
            if equity > peak_equity {
                peak_equity = equity;
            }
            let drawdown = peak_equity - equity;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
            continue;
        }

        let event = &events[action.event_index];
        let date = unix_to_date(event.round.opened_at);
        if !include_statistics_event(
            &date,
            action.event_index,
            total_events,
            from_day,
            to_day,
            recent,
        ) {
            continue;
        }

        total_count += 1;
        window_count += 1;
        cum_pnl += event.pnl;
        window_cum_pnl += event.pnl;
        if event.pnl > 0 {
            win_count += 1;
            win_sum += event.pnl;
            window_win_count += 1;
            window_win_sum += event.pnl;
        } else if event.pnl < 0 {
            loss_count += 1;
            loss_sum += -event.pnl;
            window_loss_count += 1;
            window_loss_sum += -event.pnl;
        }
        // 区间回撤曲线：从 0 起步逐笔累计区间盈亏，追踪该曲线峰值的最大回落
        if window_cum_pnl > window_peak_pnl {
            window_peak_pnl = window_cum_pnl;
        }
        let window_drawdown = window_peak_pnl - window_cum_pnl;
        if window_drawdown > window_max_drawdown {
            window_max_drawdown = window_drawdown;
        }
        // 当时总资产 = 当时本金 + 累计利息归本 + 累计已结算盈亏 − 当时累计支取（全量口径）
        equity = principal_at + interest_at + cum_pnl - withdrawn_at;
        if equity > peak_equity {
            peak_equity = equity;
        }
        let drawdown = peak_equity - equity;
        if drawdown > max_drawdown {
            max_drawdown = drawdown;
        }

        let (sequence, stat_count, total, wins, losses, win_amount_sum, loss_amount_sum, drawdown) =
            if use_window {
                (
                    window_count,
                    window_count,
                    window_cum_pnl,
                    window_win_count,
                    window_loss_count,
                    window_win_sum,
                    window_loss_sum,
                    window_max_drawdown,
                )
            } else {
                (
                    total_count,
                    total_count,
                    cum_pnl,
                    win_count,
                    loss_count,
                    win_sum,
                    loss_sum,
                    max_drawdown,
                )
            };

        let mut point = StockStatisticsPointDto {
            sequence,
            // 累计口径下"本点覆盖 1..=sequence 笔"（区间内重新编号后同样成立）
            span: sequence,
            opened_at: event.round.opened_at,
            closed_at: event.round.closed_at,
            stock_code: event.round.stock_code.clone(),
            stock_name: event.stock_name.clone(),
            stock_round_no: event.round.round_no,
            tag: event.round.tag.clone(),
            pnl: event.pnl,
            pnl_rate: event.pnl_rate,
            trade_count: event.trade_count,
            total_pnl: total,
            win_count: wins,
            loss_count: losses,
            max_drawdown: drawdown,
            ..StockStatisticsPointDto::default()
        };
        if stat_count > 0 {
            point.win_rate = percent_of(wins, stat_count);
        }
        if wins > 0 {
            point.avg_win = (win_amount_sum as f64 / wins as f64).round() as i64;
        }
        if losses > 0 {
            point.avg_loss = (loss_amount_sum as f64 / losses as f64).round() as i64;
            let ratio = if point.avg_win > 0 {
                point.avg_win as f64 / point.avg_loss as f64
            } else {
                0.0
            };
            point.pnl_ratio = Some(ratio);
        }
        if stat_count > 0 {
            // 期望值 = 胜率 × 平均盈利 − 亏损率 × 平均亏损 = 累计盈亏 ÷ 总笔数
            point.expectancy = (total as f64 / stat_count as f64).round() as i64;
        }
        if principal_at > 0 {
            point.max_drawdown_pct = percent_of(drawdown, principal_at);
        }
        result.points.push(point);

        // 分组口径：每 `group_size` 笔收一组，最后一组不足也收（不做补齐）。
        // 分组**不改**上面那条逐笔累计序列，只额外产出 `groups`。
        if grouped {
            group_acc.push(event.pnl);
            if window_count % group_size == 0 || window_count == included_total {
                groups.push(group_acc.finish(event, window_count, principal_at));
                group_acc = GroupAcc::default();
            }
        }
    }
    result.groups = groups;
    Ok(result)
}

/// 判断某笔结算是否落入当前筛选：
/// `recent > 0` 时取排序后最近 N 笔；`from_day` 非空时按**建仓日期**区间（含首尾日）判断。
fn include_statistics_event(
    date: &str,
    index: usize,
    total: usize,
    from_day: &str,
    to_day: &str,
    recent: i64,
) -> bool {
    if recent > 0 {
        return index as i64 >= total as i64 - recent;
    }
    if !from_day.is_empty() {
        return date >= from_day && date <= to_day;
    }
    true
}

/// 校验起止月份（`YYYY-MM`）并返回首尾两天的日期边界。
///
/// 两端必须同时提供、格式必须是严格 `YYYY-MM`、
/// `end_month` 不能早于 `start_month`；返回 `（当月 1 日, 末月最后一天）`。
pub fn normalize_statistics_month_range(
    start_month: &str,
    end_month: &str,
) -> ServiceResult<(String, String)> {
    if start_month.is_empty() && end_month.is_empty() {
        return Ok((String::new(), String::new()));
    }
    if start_month.is_empty() || end_month.is_empty() {
        return Err(
            AppError::bad_request("start_month 与 end_month 需同时提供（格式 YYYY-MM）").into(),
        );
    }
    let (start_year, start_month_number) = parse_strict_month(start_month)
        .ok_or_else(|| ServiceError::App(AppError::bad_request("start_month 格式应为 YYYY-MM")))?;
    let (end_year, end_month_number) = parse_strict_month(end_month)
        .ok_or_else(|| ServiceError::App(AppError::bad_request("end_month 格式应为 YYYY-MM")))?;
    if (start_year, start_month_number) > (end_year, end_month_number) {
        return Err(AppError::bad_request("end_month 不能早于 start_month").into());
    }
    let last_day = last_day_of_month(end_year, end_month_number);
    Ok((
        format!("{start_year:04}-{start_month_number:02}-01"),
        format!("{end_year:04}-{end_month_number:02}-{last_day:02}"),
    ))
}

/// 严格解析 `YYYY-MM`，返回 (年, 月)。格式或取值非法时返回 `None`。
fn parse_strict_month(raw: &str) -> Option<(i32, u32)> {
    let bytes = raw.as_bytes();
    if bytes.len() != 7 || bytes[4] != b'-' {
        return None;
    }
    if !bytes[0..4].iter().all(u8::is_ascii_digit) || !bytes[5..7].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let year: i32 = raw[0..4].parse().ok()?;
    let month: u32 = raw[5..7].parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// 某年某月的最后一天（`time.Date(y, m+1, 0, ...)` 的等价物）。
fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .expect("月份取值已在 1..=12 内校验");
    let last = first_of_next.pred_opt().expect("公历日期存在前一天");
    last.day()
}

/// 返回账本全部「追加本金 / 支取 / 利息归本」记录，按日期升序（同日按创建时间、再按 ID）。
fn list_capital_flows(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<Vec<CapitalFlow>> {
    // 按 page_size=100 逐页拉取直到覆盖 total；结果集合等价于全量。
    let mut flows: Vec<CapitalFlow> = Vec::new();
    let mut page = 1_i64;
    loop {
        let (records, total) = db(StockDao::query_fund_records(conn, ledger_id, page, 100))?;
        for record in &records {
            push_capital_flow(&mut flows, record);
        }
        // `page*100 >= total` 时停止翻页
        if page * 100 >= total.max(0) {
            break;
        }
        page += 1;
    }
    flows.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then(left.created_at.cmp(&right.created_at))
            .then(left.id.cmp(&right.id))
    });
    Ok(flows)
}

fn push_capital_flow(flows: &mut Vec<CapitalFlow>, record: &StockFundRecord) {
    match record.event_type.as_str() {
        consts::STOCK_EVENT_ADD_PRINCIPAL => flows.push(CapitalFlow {
            date: record.record_date.clone(),
            add: record.amount_change,
            withdraw: 0,
            interest: 0,
            created_at: record.created_at,
            id: record.id.clone(),
        }),
        consts::STOCK_EVENT_WITHDRAW => flows.push(CapitalFlow {
            date: record.record_date.clone(),
            add: 0,
            // amount_change 为负数，取反得到正数支取额
            withdraw: -record.amount_change,
            interest: 0,
            created_at: record.created_at,
            id: record.id.clone(),
        }),
        consts::STOCK_EVENT_INTEREST_PRINCIPAL => flows.push(CapitalFlow {
            date: record.record_date.clone(),
            add: 0,
            withdraw: 0,
            // amount_change 为正数
            interest: record.amount_change,
            created_at: record.created_at,
            id: record.id.clone(),
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一条结算事件（分组收尾只用到轮次上的身份字段与末笔盈亏）。
    fn settle_event(pnl: i64, closed_at: i64) -> SettleEvent {
        SettleEvent {
            round: StockTradeRound {
                stock_code: "600000".to_string(),
                round_no: 3,
                closed_at,
                tag: "打板".to_string(),
                ..StockTradeRound::default()
            },
            stock_name: "浦发银行".to_string(),
            pnl,
            pnl_rate: 1.25,
            trade_count: 4,
        }
    }

    /// 组内曲线从 0 起步：只看本组那几笔的峰谷，组前的盈亏不进回撤。
    #[test]
    fn a_group_curve_starts_from_zero() {
        let mut down_then_up = GroupAcc::default();
        down_then_up.push(-10_000);
        down_then_up.push(5_000);
        assert_eq!(
            (down_then_up.peak, down_then_up.cum, down_then_up.drawdown),
            (0, -5_000, 10_000),
            "先亏后盈：峰值仍是 0（组内起点），回撤是那 10000"
        );

        let mut up_then_down = GroupAcc::default();
        up_then_down.push(100_000);
        up_then_down.push(-30_000);
        up_then_down.push(-50_000);
        assert_eq!((up_then_down.count, up_then_down.pnl), (3, 20_000));
        assert_eq!((up_then_down.win, up_then_down.loss), (1, 2));
        assert_eq!(
            (up_then_down.win_sum, up_then_down.loss_sum),
            (100_000, 80_000)
        );
        assert_eq!(up_then_down.drawdown, 80_000);
    }

    /// 平局计入笔数但不计胜负（与全量那条口径一致）。
    #[test]
    fn a_breakeven_settlement_counts_but_does_not_score() {
        let mut acc = GroupAcc::default();
        acc.push(0);
        acc.push(60_000);
        assert_eq!((acc.count, acc.win, acc.loss), (2, 1, 0));
        assert_eq!((acc.pnl, acc.drawdown), (60_000, 0));
    }

    /// 组点：统计量是本组的，"本笔"字段取组末那笔（与逐笔累计点同一套语义）。
    #[test]
    fn a_group_point_anchors_on_its_last_settlement() {
        let mut acc = GroupAcc::default();
        acc.push(100_000);
        acc.push(-40_000);
        let point = acc.finish(&settle_event(-40_000, 1_700_000_000), 7, 10_000_000);

        assert_eq!((point.sequence, point.span), (7, 2));
        assert_eq!(point.total_pnl, 60_000, "本组盈亏合计");
        assert_eq!(point.expectancy, 30_000);
        assert_eq!(point.win_rate, 50.0);
        assert_eq!((point.win_count, point.loss_count), (1, 1));
        assert_eq!((point.avg_win, point.avg_loss), (100_000, 40_000));
        assert_eq!((point.max_drawdown, point.max_drawdown_pct), (40_000, 0.4));
        // 身份字段来自组末那笔
        assert_eq!(point.pnl, -40_000);
        assert_eq!(point.pnl_rate, 1.25);
        assert_eq!(point.trade_count, 4);
        assert_eq!(point.closed_at, 1_700_000_000);
        assert_eq!(point.stock_code, "600000");
        assert_eq!(point.stock_round_no, 3);
        assert_eq!(point.tag, "打板");
    }

    /// 组内没有亏损样本 → 盈亏比缺省 `null`；本金为 0 时不给回撤比例（与逐笔累计一致）。
    #[test]
    fn a_group_without_losses_has_no_ratio() {
        let mut acc = GroupAcc::default();
        acc.push(10_000);
        let point = acc.finish(&settle_event(10_000, 0), 1, 0);
        assert_eq!(point.pnl_ratio, None);
        assert_eq!(point.avg_loss, 0);
        assert_eq!(point.max_drawdown_pct, 0.0);
    }

    #[test]
    fn month_range_requires_both_ends_and_valid_format() {
        assert_eq!(
            normalize_statistics_month_range("", "").unwrap(),
            (String::new(), String::new())
        );
        assert!(normalize_statistics_month_range("2023-01", "").is_err());
        assert!(normalize_statistics_month_range("", "2023-01").is_err());
        assert!(normalize_statistics_month_range("2023/01", "2023-02").is_err());
        assert!(normalize_statistics_month_range("2023-1", "2023-02").is_err());
        assert!(normalize_statistics_month_range("2023-13", "2023-12").is_err());
        assert!(normalize_statistics_month_range("2023-03", "2023-01").is_err());
    }

    #[test]
    fn month_range_covers_whole_months_including_leap_february() {
        assert_eq!(
            normalize_statistics_month_range("2023-07", "2023-07").unwrap(),
            ("2023-07-01".to_string(), "2023-07-31".to_string())
        );
        assert_eq!(
            normalize_statistics_month_range("2023-07", "2023-12").unwrap(),
            ("2023-07-01".to_string(), "2023-12-31".to_string())
        );
        assert_eq!(
            normalize_statistics_month_range("2024-02", "2024-02").unwrap(),
            ("2024-02-01".to_string(), "2024-02-29".to_string())
        );
        assert_eq!(
            normalize_statistics_month_range("2023-02", "2023-02").unwrap(),
            ("2023-02-01".to_string(), "2023-02-28".to_string())
        );
    }

    #[test]
    fn include_event_honours_recent_then_range() {
        assert!(include_statistics_event("2023-07-01", 0, 3, "", "", 0));
        // 最近 2 笔：只放行末尾两条
        assert!(!include_statistics_event("2023-07-01", 0, 3, "", "", 2));
        assert!(include_statistics_event("2023-07-02", 1, 3, "", "", 2));
        assert!(include_statistics_event("2023-07-03", 2, 3, "", "", 2));
        // 区间（含首尾日）
        assert!(include_statistics_event(
            "2023-07-01",
            0,
            3,
            "2023-07-01",
            "2023-07-31",
            0
        ));
        assert!(!include_statistics_event(
            "2023-08-01",
            0,
            3,
            "2023-07-01",
            "2023-07-31",
            0
        ));
    }
}
