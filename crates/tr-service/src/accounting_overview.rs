//! 记账 · **看板**：把「一期」的结论一次算完（结论式回顾，见 `CONTEXT.md`）。
//!
//! ## 口径（唯一实现，别再在界面侧算一遍）
//!
//! * 日常支出 = 支出 − 固定支出；
//! * 固定支出 = 分类上标了 `is_fixed` 的支出；
//! * 收入 = `income` 合计（记的是税后净额）；
//! * 结余 = 收入 − 支出，**转账不参与**（没有对手方，既不是收入也不是支出）。
//!
//! 于是「日常 + 固定 = 支出」是硬等式，任何一侧改了另一侧跟着改。
//!
//! ## 两处容易写错的地方
//!
//! 1. **同比不是"比上一个整月"**：本期还没走完时，去年只取到**同一个号数**
//!    （10/1–10/16 vs 去年 10/1–10/16）。拿半个月比整月，会得到"少花了 40%"这种假信号。
//! 2. **年视图画的是每月发生额，不是累计**：累计线一年只有一个峰，看不出"哪个月花爆了"。

use chrono::{Datelike, Local, NaiveDate, TimeZone};

use tr_domain::dto::{
    AccountingOverviewDto, AccountingOverviewRequest, OverviewCardsDto, OverviewCompositionDto,
    OverviewPeriodDto, OverviewSeriesDto, OverviewSliceDto,
};
use tr_store::dao::category::CategoryDao;
use tr_store::dao::transaction_record::TransactionRecordDao;
use tr_store::Workspace;

use crate::error::ServiceResult;

/// 构成里最多列几项（其余并成「其他」）。
const TOP_SLICES: usize = 8;

const KIND_MONTH: &str = "month";
const KIND_YEAR: &str = "year";

/// 「其他」这一片的名字（固定文案，界面直接显示）。
const OTHER_LABEL: &str = "其他";

/// 看板取数：一期（一个月或一整年）。
pub fn overview(
    workspace: &Workspace,
    request: &AccountingOverviewRequest,
) -> ServiceResult<AccountingOverviewDto> {
    let conn = workspace.connection();
    let ledger_id = request.ledger_id.as_str();

    let period = resolve_period(request);
    let compare = last_year_window(&period);
    let fixed_categories = CategoryDao::fixed_names(&conn, ledger_id).map_err(db)?;

    // ---- 四张卡：本期三档 + 今年至今的结余 ----
    let stats =
        TransactionRecordDao::query_statistics(&conn, ledger_id, &period.range()).map_err(db)?;
    let fixed_expense = TransactionRecordDao::sum_expense_by_categories(
        &conn,
        ledger_id,
        &period.range(),
        &fixed_categories,
        false,
    )
    .map_err(db)?;
    let last_year_stats =
        TransactionRecordDao::query_statistics(&conn, ledger_id, &compare.range()).map_err(db)?;
    let last_year_fixed = TransactionRecordDao::sum_expense_by_categories(
        &conn,
        ledger_id,
        &compare.range(),
        &fixed_categories,
        false,
    )
    .map_err(db)?;

    // 一期里一条记录都没有 ⇒ 四张卡都是 `None`（界面显示 `—`）。
    // 「花了 ¥0」和「没记账」是两件事，不能长得一样。
    let empty = stats.income == 0 && stats.expense == 0 && stats.transfer == 0;
    let cards = if empty {
        OverviewCardsDto::default()
    } else {
        OverviewCardsDto {
            daily_expense: Some(stats.expense - fixed_expense),
            fixed_expense: Some(fixed_expense),
            income: Some(stats.income),
            balance_ytd: year_to_date_balance(&conn, ledger_id, &period)?,
            // 去年同月同期一条记录都没有 ⇒ `None`，界面显示「无可比数据」
            daily_expense_last_year: (last_year_stats.expense > 0)
                .then_some(last_year_stats.expense - last_year_fixed),
            income_last_year: (last_year_stats.income > 0).then_some(last_year_stats.income),
            fixed_breakdown: slices(
                TransactionRecordDao::sum_expense_by_category(
                    &conn,
                    ledger_id,
                    &period.range(),
                    &fixed_categories,
                    false,
                )
                .map_err(db)?,
                usize::MAX,
            ),
        }
    };

    // ---- 折线：本期 vs 去年同期（同期截断） ----
    let series = build_series(&conn, ledger_id, &period, &compare, &fixed_categories)?;

    // ---- 构成：日常支出（固定项在卡片上有自己的构成） ----
    let by_category = slices(
        TransactionRecordDao::sum_expense_by_category(
            &conn,
            ledger_id,
            &period.range(),
            &fixed_categories,
            true,
        )
        .map_err(db)?,
        TOP_SLICES,
    );
    let by_tag = slices(
        TransactionRecordDao::sum_expense_by_tag(
            &conn,
            ledger_id,
            &period.range(),
            &fixed_categories,
            true,
        )
        .map_err(db)?,
        TOP_SLICES,
    );

    Ok(AccountingOverviewDto {
        period: OverviewPeriodDto {
            kind: period.kind.to_string(),
            key: period.key.clone(),
            label: period.label.clone(),
            is_current: period.is_current,
            through_day: period.through_day,
            year: period.year,
            is_current_year: period.year == Local::now().year(),
        },
        cards,
        series,
        composition: OverviewCompositionDto {
            by_category,
            by_tag,
        },
    })
}

// ==================================================================== 期间

/// 解析好的一期。`[start, end]` 是本地时间戳闭区间（记录存的是本地 12:00）。
struct Period {
    kind: &'static str,
    year: i32,
    /// 年视图固定为 1。
    month: u32,
    key: String,
    label: String,
    is_current: bool,
    /// 月视图：这一期走到第几天（已走完的月 / 年视图为 `None`）。
    through_day: Option<i32>,
    start: i64,
    end: i64,
    /// 画几个点：月视图 = 天数，年视图 = 月数。
    points: u32,
}

impl Period {
    fn range(&self) -> Vec<i64> {
        vec![self.start, self.end]
    }
}

fn resolve_period(request: &AccountingOverviewRequest) -> Period {
    let today = Local::now().date_naive();
    let kind = if request.period_kind == KIND_YEAR {
        KIND_YEAR
    } else {
        KIND_MONTH
    };

    let first_day = if kind == KIND_YEAR {
        parse_year_key(&request.period_key).unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("当年 1 月 1 日合法")
        })
    } else {
        parse_month_key(&request.period_key).unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("当月 1 日合法")
        })
    };

    if kind == KIND_YEAR {
        let is_current = first_day.year() == today.year();
        // 当年画到本月；往年画满 12 个月
        let points = if is_current { today.month() } else { 12 };
        let end_date = if is_current {
            today
        } else {
            NaiveDate::from_ymd_opt(first_day.year(), 12, 31).expect("12 月 31 日合法")
        };
        return Period {
            kind,
            year: first_day.year(),
            month: 1,
            key: format!("{}", first_day.year()),
            label: format!("{}年", first_day.year()),
            is_current,
            through_day: None,
            start: local_start(first_day),
            end: local_end(end_date),
            points,
        };
    }

    let is_current = first_day.year() == today.year() && first_day.month() == today.month();
    let days_in = days_in_month(first_day.year(), first_day.month());
    let through_day = is_current.then_some(today.day() as i32);
    // 正在走的那期截到今天，历史月整月
    let points = if is_current { today.day() } else { days_in };
    Period {
        kind,
        year: first_day.year(),
        month: first_day.month(),
        key: format!("{}-{:02}", first_day.year(), first_day.month()),
        label: format!("{}年{}月", first_day.year(), first_day.month()),
        is_current,
        through_day,
        start: local_start(first_day),
        end: local_end(
            NaiveDate::from_ymd_opt(first_day.year(), first_day.month(), points)
                .expect("本期最后一天合法"),
        ),
        points,
    }
}

/// 去年同期：同一个号数截止（本期没走完就只取到同一天）。
///
/// 去年二月只有 28 天、本期走到 31 号时，按当月最后一天截断 —— 宁可少比三天，
/// 也不能把"不存在的日子"算成 0。
fn last_year_window(period: &Period) -> Period {
    if period.kind == KIND_YEAR {
        let year = period.year - 1;
        let months = period.points;
        // 当年比到**同一个号数**（今天 10/18 ⇒ 去年 10/18），往年整年比整年。
        // 不这么做的话，10 月同比会拿"今年 18 天"去比"去年 31 天"。
        let day = if period.is_current {
            Local::now().day().clamp(1, days_in_month(year, months))
        } else {
            days_in_month(year, months)
        };
        return Period {
            kind: KIND_YEAR,
            year,
            month: 1,
            key: format!("{year}"),
            label: format!("{year}年"),
            is_current: false,
            through_day: None,
            start: local_start(NaiveDate::from_ymd_opt(year, 1, 1).expect("1 月 1 日合法")),
            end: local_end(NaiveDate::from_ymd_opt(year, months, day).expect("对比截止日合法")),
            points: months,
        };
    }

    let year = period.year - 1;
    let month = period.month;
    let days_in = days_in_month(year, month);
    let day = period
        .through_day
        .unwrap_or(days_in as i32)
        .clamp(1, days_in as i32) as u32;
    Period {
        kind: KIND_MONTH,
        year,
        month,
        key: format!("{year}-{month:02}"),
        label: format!("{year}年{month}月"),
        is_current: false,
        through_day: None,
        start: local_start(NaiveDate::from_ymd_opt(year, month, 1).expect("当月 1 日合法")),
        end: local_end(NaiveDate::from_ymd_opt(year, month, day).expect("合法")),
        points: day,
    }
}

fn parse_month_key(key: &str) -> Option<NaiveDate> {
    let (year, month) = key.split_once('-')?;
    let year: i32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, 1)
}

fn parse_year_key(key: &str) -> Option<NaiveDate> {
    let year: i32 = key.parse().ok()?;
    NaiveDate::from_ymd_opt(year, 1, 1)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let last = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("下月 1 日合法")
        - chrono::Duration::days(1);
    last.day()
}

/// 本地某天的 00:00:00 时间戳（夏令时回拨时取当天最早的那一刻）。
fn local_start(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("00:00 合法");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.timestamp())
        .unwrap_or_else(|| naive.and_utc().timestamp())
}

/// 本地某天的 23:59:59 时间戳（闭区间右端）。
fn local_end(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(23, 59, 59).expect("23:59:59 合法");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.timestamp())
        .unwrap_or_else(|| naive.and_utc().timestamp())
}

/// 今年至今的结余 = 收入 − 支出（转账不参与）。
///
/// 结余给的是**年度**口径而不是本期：半个月的"结余"是伪指标（房租、房贷都记在月初），
/// 只有年度数字才和"今年攒下了吗"对得上。
fn year_to_date_balance(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    period: &Period,
) -> ServiceResult<Option<i64>> {
    let year_start =
        local_start(NaiveDate::from_ymd_opt(period.year, 1, 1).expect("1 月 1 日合法"));
    let stats = TransactionRecordDao::query_statistics(conn, ledger_id, &[year_start, period.end])
        .map_err(db)?;
    if stats.income == 0 && stats.expense == 0 {
        return Ok(None);
    }
    Ok(Some(stats.income - stats.expense))
}

// ==================================================================== 折线

fn build_series(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    period: &Period,
    compare: &Period,
    fixed_categories: &[String],
) -> ServiceResult<OverviewSeriesDto> {
    if period.kind == KIND_YEAR {
        return Ok(OverviewSeriesDto {
            labels: (1..=period.points).map(|m| format!("{m}月")).collect(),
            current: monthly_values(conn, ledger_id, period, fixed_categories)?,
            last_year: monthly_values(conn, ledger_id, compare, fixed_categories)?,
            mode: "monthly".to_string(),
        });
    }

    let labels: Vec<String> = (1..=period.points).map(|d| d.to_string()).collect();
    let current = daily_cumulative(conn, ledger_id, period, fixed_categories)?;
    let mut last_year = daily_cumulative(conn, ledger_id, compare, fixed_categories)?;
    // 去年二月只有 28 天、今年二月 29 天时，去年那条线要**补齐到同样的长度**：
    // 用最后一天的累计值填尾（累计线不能凭空掉回 0），否则第 29 天会断崖。
    if last_year.len() < labels.len() {
        let tail = last_year.last().copied().unwrap_or(0.0);
        last_year.resize(labels.len(), tail);
    }
    Ok(OverviewSeriesDto {
        labels,
        current,
        last_year,
        mode: "daily".to_string(),
    })
}

/// 日累计（月视图）：第 n 个点 = 1 日到 n 日的和，缺的天补 0。
fn daily_cumulative(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    period: &Period,
    fixed_categories: &[String],
) -> ServiceResult<Vec<f64>> {
    let rows = TransactionRecordDao::sum_expense_buckets(
        conn,
        ledger_id,
        &period.range(),
        "%Y-%m-%d",
        fixed_categories,
        true,
    )
    .map_err(db)?;
    let by_day: std::collections::BTreeMap<String, i64> = rows.into_iter().collect();

    let mut running = 0_i64;
    let mut out = Vec::with_capacity(period.points as usize);
    for day in 1..=period.points {
        running += by_day
            .get(&format!("{}-{:02}-{:02}", period.year, period.month, day))
            .copied()
            .unwrap_or(0);
        out.push(running as f64);
    }
    Ok(out)
}

/// 每月发生额（年视图）：1 月 .. 第 n 月，没数据的月份补 0。
fn monthly_values(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    period: &Period,
    fixed_categories: &[String],
) -> ServiceResult<Vec<f64>> {
    let rows = TransactionRecordDao::sum_expense_buckets(
        conn,
        ledger_id,
        &period.range(),
        "%Y-%m",
        fixed_categories,
        true,
    )
    .map_err(db)?;
    let by_month: std::collections::BTreeMap<String, i64> = rows.into_iter().collect();
    Ok((1..=period.points)
        .map(|month| {
            by_month
                .get(&format!("{}-{month:02}", period.year))
                .copied()
                .unwrap_or(0) as f64
        })
        .collect())
}

// ==================================================================== 构成

/// 金额降序取前 `limit` 项，其余并成「其他」（数量不超过 `limit + 1`）。
fn slices(rows: Vec<(String, i64, i64)>, limit: usize) -> Vec<OverviewSliceDto> {
    if rows.len() <= limit {
        return rows
            .into_iter()
            .map(|(label, amount, count)| OverviewSliceDto {
                label,
                amount,
                count,
                is_other: false,
            })
            .collect();
    }
    let mut out: Vec<OverviewSliceDto> = rows
        .iter()
        .take(limit)
        .map(|(label, amount, count)| OverviewSliceDto {
            label: label.clone(),
            amount: *amount,
            count: *count,
            is_other: false,
        })
        .collect();
    let rest = &rows[limit..];
    out.push(OverviewSliceDto {
        label: OTHER_LABEL.to_string(),
        amount: rest.iter().map(|row| row.1).sum(),
        count: rest.iter().map(|row| row.2).sum(),
        is_other: true,
    });
    out
}

fn db(error: rusqlite::Error) -> crate::ServiceError {
    tracing::error!("看板取数失败: {}", error);
    crate::ServiceError::from(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::models::Category;
    use tr_store::dao::category::CategoryDao;

    /// 把 `YYYY-MM-DD` 写成"本地 12:00"的 Unix 秒（与真实写入路径一致）。
    fn at(ymd: &str) -> i64 {
        let date = NaiveDate::parse_from_str(ymd, "%Y-%m-%d").expect("日期合法");
        let naive = date.and_hms_opt(12, 0, 0).expect("12:00 合法");
        Local
            .from_local_datetime(&naive)
            .earliest()
            .expect("本地时间存在")
            .timestamp()
    }

    fn record(ws: &Workspace, id: &str, ymd: &str, kind: &str, category: &str, price: i64) {
        ws.connection()
            .execute(
                "INSERT INTO tbl_billadm_transaction_record \
                 (transaction_id, ledger_id, price, transaction_type, category, description, flags, \
                  key_event_date, transaction_at, created_at, updated_at) \
                 VALUES (?1, 'l1', ?2, ?3, ?4, '-', NULL, '', ?5, ?5, ?5)",
                rusqlite::params![id, price, kind, category, at(ymd)],
            )
            .unwrap();
    }

    fn tag(ws: &Workspace, id: &str, name: &str) {
        ws.connection()
            .execute(
                "INSERT INTO tbl_billadm_transaction_record_tag (ledger_id, transaction_id, tag) \
                 VALUES ('l1', ?1, ?2)",
                rusqlite::params![id, name],
            )
            .unwrap();
    }

    fn mark_fixed(ws: &Workspace, name: &str) {
        let conn = ws.connection();
        CategoryDao::create(
            &conn,
            &Category {
                ledger_id: "l1".to_string(),
                name: name.to_string(),
                transaction_type: "expense".to_string(),
                sort_order: 0,
                is_fixed: false,
            },
        )
        .unwrap();
        CategoryDao::set_fixed(&conn, "l1", name, "expense", true).unwrap();
    }

    fn ask(ws: &Workspace, kind: &str, key: &str) -> AccountingOverviewDto {
        overview(
            ws,
            &AccountingOverviewRequest {
                ledger_id: "l1".to_string(),
                period_kind: kind.to_string(),
                period_key: key.to_string(),
            },
        )
        .unwrap()
    }

    /// 口径的硬等式：日常 + 固定 = 支出；转账三张卡都不沾；结余是年度口径。
    #[test]
    fn daily_plus_fixed_equals_expense_and_transfer_is_not_money_out() {
        let (ws, dir) = crate::test_support::workspace("overview-caliber");
        mark_fixed(&ws, "房产房贷");
        record(&ws, "i1", "2025-03-10", "income", "工资奖金", 100_000);
        record(&ws, "f1", "2025-03-01", "expense", "房产房贷", 50_000);
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);
        record(&ws, "d2", "2025-03-06", "expense", "购物消费", 3_000);
        record(&ws, "t1", "2025-03-10", "transfer", "五险一金", 1_000);

        let reply = ask(&ws, "month", "2025-03");
        let cards = &reply.cards;
        assert_eq!(cards.daily_expense, Some(5_000));
        assert_eq!(cards.fixed_expense, Some(50_000));
        assert_eq!(
            cards.daily_expense.unwrap() + cards.fixed_expense.unwrap(),
            55_000,
            "日常 + 固定必须等于支出合计"
        );
        assert_eq!(cards.income, Some(100_000));
        // 结余 = 100000 − 55000；那 1000 的转账不参与（工资记的是净额）
        assert_eq!(cards.balance_ytd, Some(45_000));
        // 固定项自己有一份构成
        assert_eq!(cards.fixed_breakdown.len(), 1);
        assert_eq!(cards.fixed_breakdown[0].label, "房产房贷");
        // 构成只算日常：转账、固定都不在里头
        let labels: Vec<&str> = reply
            .composition
            .by_category
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, vec!["购物消费", "餐饮美食"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **一个分类都没标「固定」时**：固定支出是 0、日常支出等于全部支出。
    ///
    /// 这是最容易写错的一处：空集合若被当成"不过滤"，固定支出会等于整期支出、
    /// 日常支出变成 0 —— 而「日常 + 固定 = 支出」仍然成立，等式型断言发现不了。
    #[test]
    fn no_fixed_category_means_zero_fixed_not_everything() {
        let (ws, dir) = crate::test_support::workspace("overview-no-fixed");
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);
        record(&ws, "d2", "2025-03-06", "expense", "购物消费", 3_000);

        let reply = ask(&ws, "month", "2025-03");
        assert_eq!(reply.cards.fixed_expense, Some(0), "没标固定就只能算出 0");
        assert_eq!(reply.cards.daily_expense, Some(5_000), "全部支出都是日常");
        // 固定项自己的构成也必须是空的
        assert!(reply.cards.fixed_breakdown.is_empty());
        // 构成里两项都在（固定档没把整期的支出吞掉）
        assert_eq!(reply.composition.by_category.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 去年同月一条记录都没有 ⇒ 同比为 `None`（界面显示「无可比数据」，不是 0）。
    #[test]
    fn missing_last_year_is_none_not_zero() {
        let (ws, dir) = crate::test_support::workspace("overview-no-compare");
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);

        let reply = ask(&ws, "month", "2025-03");
        assert_eq!(reply.cards.daily_expense_last_year, None);
        assert_eq!(reply.cards.income_last_year, None);
        assert_eq!(reply.series.last_year.len(), reply.series.current.len());
        assert!(reply.series.last_year.iter().all(|value| *value == 0.0));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 一期里没有任何记录 ⇒ 四张卡全是 `None`（「花了 0」和「没记账」不是一回事）。
    #[test]
    fn an_empty_period_has_no_cards_at_all() {
        let (ws, dir) = crate::test_support::workspace("overview-empty");
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);

        let reply = ask(&ws, "month", "2019-07");
        let cards = &reply.cards;
        assert_eq!(cards.daily_expense, None);
        assert_eq!(cards.fixed_expense, None);
        assert_eq!(cards.income, None);
        assert_eq!(cards.balance_ytd, None);
        assert!(reply.composition.by_category.is_empty());
        assert!(reply.composition.by_tag.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 月视图是**累计**：第 n 个点 = 1..n 的和，没记的日子不跳变。
    #[test]
    fn month_series_accumulates_day_by_day() {
        let (ws, dir) = crate::test_support::workspace("overview-cumulative");
        record(&ws, "d1", "2025-03-01", "expense", "餐饮美食", 1_000);
        record(&ws, "d2", "2025-03-03", "expense", "餐饮美食", 2_000);
        record(&ws, "d3", "2025-03-31", "expense", "餐饮美食", 4_000);

        let reply = ask(&ws, "month", "2025-03");
        assert_eq!(reply.period.through_day, None, "历史月不截断");
        assert_eq!(reply.series.mode, "daily");
        assert_eq!(reply.series.labels.len(), 31);
        let current = &reply.series.current;
        assert_eq!(current[0], 1_000.0);
        assert_eq!(current[1], 1_000.0, "3/2 没花钱，累计不动");
        assert_eq!(current[2], 3_000.0);
        assert_eq!(current[30], 7_000.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 年视图画**每月发生额**，不是累计（累计线一年只有一个峰，看不出哪个月花爆了）。
    #[test]
    fn year_series_is_monthly_not_cumulative() {
        let (ws, dir) = crate::test_support::workspace("overview-year");
        record(&ws, "d1", "2025-01-10", "expense", "餐饮美食", 1_000);
        record(&ws, "d2", "2025-03-10", "expense", "餐饮美食", 3_000);

        let reply = ask(&ws, "year", "2025");
        assert_eq!(reply.period.kind, "year");
        assert_eq!(reply.series.mode, "monthly");
        assert_eq!(reply.series.labels.len(), 12);
        assert_eq!(reply.series.labels[0], "1月");
        assert_eq!(reply.series.current[0], 1_000.0);
        assert_eq!(reply.series.current[1], 0.0);
        assert_eq!(
            reply.series.current[2], 3_000.0,
            "3 月就是 3 月，不叠加上 1 月"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 分类多于 8 项时，尾巴并成「其他」，合计不变。
    #[test]
    fn composition_merges_the_tail_into_other() {
        let (ws, dir) = crate::test_support::workspace("overview-top-n");
        for index in 0..10 {
            record(
                &ws,
                &format!("d{index}"),
                "2025-03-05",
                "expense",
                &format!("分类{index}"),
                1_000,
            );
        }

        let reply = ask(&ws, "month", "2025-03");
        let slices = &reply.composition.by_category;
        assert_eq!(slices.len(), TOP_SLICES + 1);
        assert_eq!(slices.last().unwrap().label, OTHER_LABEL);
        let total: i64 = slices.iter().map(|item| item.amount).sum();
        assert_eq!(total, 10_000, "合并只能换标签，不能改合计");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 标签档：一笔记录挂两个标签就各算一次（所以它那档的合计不等于支出合计）。
    #[test]
    fn tag_composition_counts_a_record_once_per_tag() {
        let (ws, dir) = crate::test_support::workspace("overview-tags");
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 1_000);
        tag(&ws, "d1", "三餐");
        tag(&ws, "d1", "外卖");
        record(&ws, "d2", "2025-03-06", "expense", "餐饮美食", 500);
        tag(&ws, "d2", "三餐");

        let reply = ask(&ws, "month", "2025-03");
        let by_tag = &reply.composition.by_tag;
        assert_eq!(by_tag.len(), 2);
        assert_eq!(by_tag[0].label, "三餐");
        assert_eq!(by_tag[0].amount, 1_500);
        assert_eq!(by_tag[1].label, "外卖");
        assert_eq!(by_tag[1].amount, 1_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 同期截断：本期走到 16 号，去年也只比到 16 号 —— 半个月不能比整月。
    #[test]
    fn comparison_stops_at_the_same_day_of_last_year() {
        let period = Period {
            kind: KIND_MONTH,
            year: 2026,
            month: 10,
            key: "2026-10".to_string(),
            label: "2026年10月".to_string(),
            is_current: true,
            through_day: Some(16),
            start: local_start(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()),
            end: local_end(NaiveDate::from_ymd_opt(2026, 10, 16).unwrap()),
            points: 16,
        };

        let compare = last_year_window(&period);
        assert_eq!(compare.year, 2025);
        assert_eq!(compare.month, 10);
        assert_eq!(compare.points, 16);
        assert_eq!(
            compare.end,
            local_end(NaiveDate::from_ymd_opt(2025, 10, 16).unwrap())
        );
    }

    /// 去年二月只有 28 天：本期走到 31 号时，去年截到月末，不能凭空"补三天 0"。
    #[test]
    fn comparison_clamps_to_a_shorter_month() {
        let period = Period {
            kind: KIND_MONTH,
            year: 2024,
            month: 2,
            key: "2024-02".to_string(),
            label: "2024年2月".to_string(),
            is_current: false,
            through_day: None,
            start: local_start(NaiveDate::from_ymd_opt(2024, 2, 1).unwrap()),
            end: local_end(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()),
            points: 29,
        };

        let compare = last_year_window(&period);
        assert_eq!(compare.year, 2023);
        assert_eq!(compare.points, 28, "2023 年 2 月只有 28 天");
        assert_eq!(
            compare.end,
            local_end(NaiveDate::from_ymd_opt(2023, 2, 28).unwrap())
        );
    }

    /// 没填期间 → 落在今天所在的月；填了非法值也回落当期（不报错、不猜）。
    #[test]
    fn missing_or_garbage_period_falls_back_to_now() {
        let (ws, dir) = crate::test_support::workspace("overview-fallback");
        let reply = ask(&ws, "month", "");
        let today = Local::now().date_naive();
        assert_eq!(
            reply.period.key,
            format!("{}-{:02}", today.year(), today.month())
        );
        assert!(reply.period.through_day.is_some(), "当期要按同期截断");

        let reply = ask(&ws, "month", "不是日期");
        assert_eq!(
            reply.period.key,
            format!("{}-{:02}", today.year(), today.month())
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
