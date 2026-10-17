//! 记账 · **看板**：把「一期」的结论一次算完（结论式回顾，见 `CONTEXT.md`）。
//!
//! ## 口径（唯一实现，别再在界面侧算一遍）
//!
//! * 支出 = `expense` 合计（全部支出，分类不参与切分）；
//! * 收入 = `income` 合计（记的是税后净额）；
//! * 结余 = 收入 − 支出，**转账不参与**（没有对手方，既不是收入也不是支出）。
//!
//! 支出构成是支出的下钻：合计等于本期支出，不再有"排除固定"这一层。
//!
//! ## 结余为什么是年度口径
//!
//! 半个月的结余是伪指标（房租、房贷都记在月初），所以 `balance_ytd` 永远从当年 1 月 1 日
//! 算到这一期的终点 —— 选「月」时它也不是当月的结余，界面因此不给它配任何说明文字
//! （说明会和顶部的期间控件打架）。

use chrono::{Datelike, Local, NaiveDate, TimeZone};

use tr_domain::dto::{
    AccountingOverviewDto, AccountingOverviewRequest, OverviewCardsDto, OverviewPeriodDto,
    OverviewSliceDto,
};
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

    // ---- 收支卡：本期两档 + 今年至今的结余 ----
    let stats =
        TransactionRecordDao::query_statistics(&conn, ledger_id, &period.range()).map_err(db)?;

    // 一期里一条记录都没有 ⇒ 三个数都是 `None`（界面显示 `—`）。
    // 「花了 ¥0」和「没记账」是两件事，不能长得一样。
    let empty = stats.income == 0 && stats.expense == 0 && stats.transfer == 0;
    let cards = if empty {
        OverviewCardsDto::default()
    } else {
        OverviewCardsDto {
            expense: Some(stats.expense),
            income: Some(stats.income),
            balance_ytd: year_to_date_balance(&conn, ledger_id, &period)?,
        }
    };

    // ---- 构成：本期支出按分类的下钻（全部支出，不分类别） ----
    let categories = slices(
        TransactionRecordDao::sum_expense_by_category(&conn, ledger_id, &period.range())
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
        },
        cards,
        categories,
    })
}

// ==================================================================== 期间

/// 解析好的一期。`[start, end]` 是本地时间戳闭区间（记录存的是本地 12:00）。
struct Period {
    kind: &'static str,
    year: i32,
    key: String,
    label: String,
    is_current: bool,
    /// 月视图：这一期走到第几天（已走完的月 / 年视图为 `None`）。
    through_day: Option<i32>,
    start: i64,
    end: i64,
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
        let end_date = if is_current {
            today
        } else {
            NaiveDate::from_ymd_opt(first_day.year(), 12, 31).expect("12 月 31 日合法")
        };
        return Period {
            kind,
            year: first_day.year(),
            key: format!("{}", first_day.year()),
            label: format!("{}年", first_day.year()),
            is_current,
            through_day: None,
            start: local_start(first_day),
            end: local_end(end_date),
        };
    }

    let is_current = first_day.year() == today.year() && first_day.month() == today.month();
    let days_in = days_in_month(first_day.year(), first_day.month());
    let through_day = is_current.then_some(today.day() as i32);
    // 正在走的那期截到今天，历史月整月
    let last_day = if is_current { today.day() } else { days_in };
    Period {
        kind,
        year: first_day.year(),
        key: format!("{}-{:02}", first_day.year(), first_day.month()),
        label: format!("{}年{}月", first_day.year(), first_day.month()),
        is_current,
        through_day,
        start: local_start(first_day),
        end: local_end(
            NaiveDate::from_ymd_opt(first_day.year(), first_day.month(), last_day)
                .expect("本期最后一天合法"),
        ),
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

    /// 口径：支出算**全部**支出（分类不参与切分）；转账哪一档都不沾；结余是年度口径。
    #[test]
    fn expense_counts_every_category_and_transfer_is_not_money_out() {
        let (ws, dir) = crate::test_support::workspace("overview-caliber");
        record(&ws, "i1", "2025-03-10", "income", "工资奖金", 100_000);
        record(&ws, "f1", "2025-03-01", "expense", "房产房贷", 50_000);
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);
        record(&ws, "d2", "2025-03-06", "expense", "购物消费", 3_000);
        record(&ws, "t1", "2025-03-10", "transfer", "五险一金", 1_000);

        let reply = ask(&ws, "month", "2025-03");
        let cards = &reply.cards;
        assert_eq!(cards.expense, Some(55_000), "支出不分固定/日常，全算");
        assert_eq!(cards.income, Some(100_000));
        // 结余 = 100000 − 55000；那 1000 的转账不参与（工资记的是净额）
        assert_eq!(cards.balance_ytd, Some(45_000));

        // 构成的每个分类都在，合计正好等于卡上的支出
        let labels: Vec<&str> = reply
            .categories
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, vec!["房产房贷", "购物消费", "餐饮美食"]);
        let total: i64 = reply.categories.iter().map(|s| s.amount).sum();
        assert_eq!(Some(total), cards.expense, "构成是支出的下钻，合计必须相等");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 一期里没有任何记录 ⇒ 三个数全是 `None`（「花了 0」和「没记账」不是一回事）。
    #[test]
    fn an_empty_period_has_no_cards_at_all() {
        let (ws, dir) = crate::test_support::workspace("overview-empty");
        record(&ws, "d1", "2025-03-05", "expense", "餐饮美食", 2_000);

        let reply = ask(&ws, "month", "2019-07");
        let cards = &reply.cards;
        assert_eq!(cards.expense, None);
        assert_eq!(cards.income, None);
        assert_eq!(cards.balance_ytd, None);
        assert!(reply.categories.is_empty());

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
        let slices = &reply.categories;
        assert_eq!(slices.len(), TOP_SLICES + 1);
        assert_eq!(slices.last().unwrap().label, OTHER_LABEL);
        let total: i64 = slices.iter().map(|item| item.amount).sum();
        assert_eq!(total, 10_000, "合并只能换标签，不能改合计");

        std::fs::remove_dir_all(&dir).ok();
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
