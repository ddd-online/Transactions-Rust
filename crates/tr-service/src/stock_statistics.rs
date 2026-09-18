//! 逐笔结算统计。对照 Go `kernel/service/stock_statistics.go`（400 行）。
//!
//! 口径（与原实现逐条一致，注释直接抄自 Go）：
//! * 胜率 = 盈利笔数 ÷ 总笔数（**平局计入总笔数**，不计胜负）；
//! * 平均盈利/亏损分别只按盈利笔与亏损笔求和取平均，亏损金额取正数；
//! * 实际盈亏比 = 平均盈利 ÷ 平均亏损（**无亏损样本时为 null**）；
//! * 期望值 = 胜率 × 平均盈利 − (1 − 胜率) × 平均亏损 = 累计盈亏 ÷ 总笔数；
//! * 最大回撤按每笔结算时点的总资产曲线（当时的本金 + 累计已结算盈亏 − 当时累计支取）
//!   从高点跌落的幅度计算；本金追加/支取按记录日期参与时序，占本金比例使用**当时的本金**；
//! * 带筛选（月份区间 / 最近 N 笔 / 标签）时，**区间内从 1 起重新编号**，
//!   累计盈亏、胜率、平均盈亏、回撤全部按筛选集合独立重算；
//! * 标签先过滤轮次，使「最近 N 笔」取该标签内的最近 N 笔。

use chrono::Datelike;
use tr_domain::consts;
use tr_domain::dto::{round_pnl, StockStatisticsDto, StockStatisticsPointDto};
use tr_domain::error::AppError;
use tr_domain::models::{StockFundRecord, StockTradeRound};
use tr_store::dao::stock::StockDao;
use tr_store::Workspace;

use crate::stock::{get_or_create_account, get_trade_tags, unix_to_date};
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

/// 一笔本金追加或支取：按记录日期参与统计时序，`withdraw` 为正数金额。
struct CapitalFlow {
    date: String,
    add: i64,
    withdraw: i64,
    created_at: i64,
    id: String,
}

/// 全量逐笔结算统计（自第 1 笔起累计口径）。
pub fn get_statistics(workspace: &Workspace, ledger_id: &str) -> ServiceResult<StockStatisticsDto> {
    statistics(workspace, ledger_id, "", "", 0, "")
}

/// 筛选统计：时间范围（含首尾整月）与最近 N 笔二选一，两者均可与交易标签叠加。
pub fn get_statistics_range(
    workspace: &Workspace,
    ledger_id: &str,
    start_month: &str,
    end_month: &str,
    recent: i64,
    tag: &str,
) -> ServiceResult<StockStatisticsDto> {
    if recent < 0 {
        return Err(AppError::bad_request("recent 必须为正整数").into());
    }
    if recent > 0 && (!start_month.is_empty() || !end_month.is_empty()) {
        return Err(AppError::bad_request("时间范围与笔数筛选不能同时使用").into());
    }
    if !tag.is_empty() {
        let available_tags = get_trade_tags(workspace, ledger_id)?;
        if !available_tags.iter().any(|item| item == tag) {
            return Err(AppError::bad_request("无效的交易标签").into());
        }
    }
    let (from_day, to_day) = normalize_statistics_month_range(start_month, end_month)?;
    statistics(workspace, ledger_id, &from_day, &to_day, recent, tag)
}

/// 结算统计实现。`from_day`/`to_day` 非空时按清仓日期区间筛选，`recent > 0` 时取最近 N 笔，
/// `tag` 非空时先按标签过滤轮次，再与区间/笔数筛选叠加。
fn statistics(
    workspace: &Workspace,
    ledger_id: &str,
    from_day: &str,
    to_day: &str,
    recent: i64,
    tag: &str,
) -> ServiceResult<StockStatisticsDto> {
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
            let trades = db(StockDao::list_trades_by_round(&conn, &round.id))?;
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
    // 稳定排序：closed_at → created_at → id（与原实现一致，保证同值时顺序确定）
    events.sort_by(|left, right| {
        left.round
            .closed_at
            .cmp(&right.round.closed_at)
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
        let date = unix_to_date(event.round.closed_at);
        if include_statistics_event(&date, index, total_events, from_day, to_day, recent) {
            included_total += 1;
        }
    }

    let mut result = StockStatisticsDto {
        principal: account.principal,
        round_count: included_total,
        points: Vec::with_capacity(included_total as usize),
    };
    if included_total == 0 {
        return Ok(result);
    }
    let use_window = !from_day.is_empty() || recent > 0 || !tag.is_empty();

    // 资金事件与结算事件按日期合成同一条时序，保证本金追加/支取在正确时点影响总资产峰值与回撤
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
            date: unix_to_date(event.round.closed_at),
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
    // 总资产曲线：与原实现一致，从「初始本金」起步（`principal_at + cum_pnl - withdrawn_at`）
    let mut equity = principal_at + cum_pnl - withdrawn_at;
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
            equity = principal_at + cum_pnl - withdrawn_at;
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
        let date = unix_to_date(event.round.closed_at);
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
        // 当时总资产 = 当时本金 + 累计已结算盈亏 − 当时累计支取（全量口径）
        equity = principal_at + cum_pnl - withdrawn_at;
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
            point.win_rate = ((wins as f64 / stat_count as f64) * 10_000.0).round() / 100.0;
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
            point.max_drawdown_pct =
                ((drawdown as f64 / principal_at as f64) * 10_000.0).round() / 100.0;
        }
        result.points.push(point);
    }
    Ok(result)
}

/// 判断某笔结算是否落入当前筛选：
/// `recent > 0` 时取排序后最近 N 笔；`from_day` 非空时按清仓日期区间（含首尾日）判断。
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
/// 对照 Go `normalizeStatisticsMonthRange`：两端必须同时提供、格式必须是严格 `YYYY-MM`、
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

/// 返回账本全部「追加本金 / 支取」记录，按日期升序（同日按创建时间、再按 ID）。
fn list_capital_flows(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<Vec<CapitalFlow>> {
    // 原实现按 page_size=100 逐页拉取直到覆盖 total；这里一次取全量，结果集合等价。
    let mut flows: Vec<CapitalFlow> = Vec::new();
    let mut page = 1_i64;
    loop {
        let (records, total) = db(StockDao::query_fund_records(conn, ledger_id, page, 100))?;
        for record in &records {
            push_capital_flow(&mut flows, record);
        }
        // 与原实现一致：`page*100 >= total` 时停止翻页（原实现用 int(total) 比较）
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
            created_at: record.created_at,
            id: record.id.clone(),
        }),
        consts::STOCK_EVENT_WITHDRAW => flows.push(CapitalFlow {
            date: record.record_date.clone(),
            add: 0,
            // amount_change 为负数，取反得到正数支取额
            withdraw: -record.amount_change,
            created_at: record.created_at,
            id: record.id.clone(),
        }),
        _ => {}
    }
}

fn db<T>(result: rusqlite::Result<T>) -> ServiceResult<T> {
    result.map_err(ServiceError::Database)
}

#[cfg(test)]
mod tests {
    use super::*;

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
