//! 股票资金流水的**口径**：现金链怎么走、可用现金怎么取、支取超限怎么说。
//!
//! 这些都从 `tr-service` 里搬来（候选 3 / #36），因为从前它们是三份手写的加减加一句硬编码文案，
//! 没有断言守着。搬进来的是**判据**；写入本身的组织归 `stock::write`（ADR-0002 的股票写入聚合，
//! 现金链复算 `recalculate_cash_chain` 就在那里）。
//!
//! 本模块**只放被真正用到的东西**，而且同一件事只留一份实现：
//!
//! * `#36` 搬进来时我还写过一个 `FundEntry` + `latest_index`（"链上哪条最新"），发现
//!   `stock::write::fund_record_after` 早就是同一条规则的实现 —— 那是第二份，删掉；
//! * `#48` 把"链序 + 逐条累加"整条规则收进 [`cash_chain`]，于是那个"谁更新"的谓词、
//!   以及调用方"插入时自己算一次余额"的写法（`cash_after`）一起没有调用点了 —— 一并删除。
//!   现在链上每一条的余额只有 [`cash_chain`] 一个算法，且**按日期排**（见它的注释）。

/// 支取超限时给用户看的整句（**用户可见文案，改动即影响界面/接口**）。
pub fn withdraw_limit_message(available_cash: i64) -> String {
    format!(
        "支取金额不能超过可用现金（{} 元）",
        crate::money::cents_to_yuan(available_cash)
    )
}

/// 追加 / 支取前的可用现金：取现金链**末条**的余额；链还是空的（这个账本从没记过资金）才用当前本金。
///
/// "链末条"= 链序最大的那条（`StockDao::query_latest_fund_record` 的 `ORDER BY`），它的余额
/// 就是 [`cash_chain`] 累加到底的结果 —— 与录入顺序无关。
pub fn cash_before(latest_balance: Option<i64>, principal: i64) -> i64 {
    latest_balance.unwrap_or(principal)
}

/// 现金链上的一条：链序键 + 本次变动。
///
/// 借用记录自己的字段，不另造一份模型 —— 唯一的调用方 `stock::write` 拿的就是资金记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CashChange<'a> {
    /// 资金变化的发生日期（`YYYY-MM-DD`，零填充，按**字典序**比）
    pub record_date: &'a str,
    /// 录入时间（Unix 秒；同一天的按它比）
    pub created_at: i64,
    /// 记录 id（uuid 字符串，字典序；同刻的按它兜底定序）
    pub id: &'a str,
    /// 带符号的变动额（分）：追加本金 / 利息归本是正、支取是负、买入是负、卖出是正
    pub amount_change: i64,
}

/// 现金链：从 `base` 起算，把各条按 **(日期, 创建时间, id) 升序**逐条累加，返回**与入参同序**的余额。
///
/// 这是整条资金链唯一的算法（从前是 `stock::write` 里按**录入顺序**手写的两层循环）。两条要点：
///
/// * 链序是"资金变化**发生**的先后"，不是"**录入**的先后"。倒填的记录（日期更早、录入更晚）
///   因此排到它该在的位置上，它的金额不会被跳过 —— 老口径把"前 i 条（录入序）里链序最大那条"的
///   余额当起点，倒填那条够不着，于是它的金额进不了链，表现为**可用现金虚高**（`#48`）；
/// * 于是链上最后一条（日期最大那条）的余额 = `base + Σ amount_change`，与录入顺序无关；
///   而"可用现金"取的正是它，所以可用现金恒等于"本金 + Σ 非追加本金的变动"。
///
/// `base` 是"第一条记录之前的现金"，由调用方给（`principal − Σ追加本金`：初始本金不进链，
/// 而"追加本金"既改本金又记一条变动，两者不能重复算）。
pub fn cash_chain(base: i64, entries: &[CashChange<'_>]) -> Vec<i64> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    // 稳定排序：键只可能是"链序"这一个含义，同键的保持入参顺序
    order.sort_by(|left, right| {
        let (a, b) = (&entries[*left], &entries[*right]);
        (a.record_date, a.created_at, a.id).cmp(&(b.record_date, b.created_at, b.id))
    });
    let mut balances = vec![base; entries.len()];
    let mut cash = base;
    for index in order {
        cash += entries[index].amount_change;
        balances[index] = cash;
    }
    balances
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 超限那句的**逐字**断言（用户可见文案）。
    #[test]
    fn the_withdraw_limit_message_mentions_the_available_cash_in_yuan() {
        assert_eq!(
            withdraw_limit_message(123_456),
            "支取金额不能超过可用现金（1234.56 元）"
        );
        assert_eq!(
            withdraw_limit_message(500),
            "支取金额不能超过可用现金（5.00 元）"
        );
        assert_eq!(
            withdraw_limit_message(0),
            "支取金额不能超过可用现金（0.00 元）"
        );
    }

    /// 有记录就用记录的余额（哪怕本金已经变了）；没记录才退回本金。
    #[test]
    fn cash_before_falls_back_to_the_principal_only_without_records() {
        assert_eq!(cash_before(Some(12_345), 99_999), 12_345);
        assert_eq!(cash_before(Some(0), 99_999), 0, "余额 0 是余额，不是缺失");
        assert_eq!(cash_before(Some(-500), 99_999), -500);
        assert_eq!(cash_before(None, 99_999), 99_999);
        assert_eq!(cash_before(None, 0), 0, "本金也为 0 时就是 0");
    }

    fn change<'a>(
        record_date: &'a str,
        created_at: i64,
        id: &'a str,
        amount: i64,
    ) -> CashChange<'a> {
        CashChange {
            record_date,
            created_at,
            id,
            amount_change: amount,
        }
    }

    /// 正序：链上每条的余额就是逐条累加，末条 = base + 全部变动。
    #[test]
    fn cash_chain_accumulates_in_date_order() {
        let entries = [
            change("2026-08-01", 100, "a", 10_000),
            change("2026-08-02", 200, "b", -3_000),
            change("2026-08-03", 300, "c", 500),
        ];
        assert_eq!(cash_chain(1_000, &entries), vec![11_000, 8_000, 8_500]);
    }

    /// **`#48` 的回归**：倒填的记录（日期更早、录入更晚）照样进链 ——
    /// 老口径按录入顺序取"前 i 条里链序最大那条"的余额当起点，它的金额就被跳过了。
    #[test]
    fn a_back_dated_record_is_counted_in_the_chain() {
        // 录入顺序与日期顺序**相反**：先录 8-24 的，再补录 7-24 的
        let entries = [
            change("2026-08-24", 1_000, "1", 100_000),
            change("2026-07-24", 2_000, "2", -50_000),
        ];
        let balances = cash_chain(0, &entries);
        assert_eq!(
            balances,
            vec![50_000, -50_000],
            "补录那条排在前（它自己先扣成 -50000），8-24 那条的余额要包含它"
        );

        // 三笔：8-24 建仓（录入最早）、7-24 的历史成交（补录）、8-25 清仓
        let entries = [
            change("2026-08-24", 1_000, "1", 100_000),
            change("2026-07-24", 2_000, "2", -50_000),
            change("2026-08-25", 3_000, "3", 20_000),
        ];
        assert_eq!(cash_chain(0, &entries), vec![50_000, -50_000, 70_000]);
        // 链末条（= 可用现金读的那条）= base + Σ 全部变动，与录入顺序无关
        assert_eq!(
            cash_chain(0, &entries)[2],
            100_000 - 50_000 + 20_000,
            "链末条就是可用现金读的那条"
        );
    }

    /// 同一天比录入时间，同刻比 id（与 `query_latest_fund_record` 的 `ORDER BY` 同序）。
    #[test]
    fn same_day_orders_by_created_at_then_id() {
        let entries = [
            change("2026-08-03", 300, "a", 1_000),
            change("2026-08-03", 100, "z", 2_000),
            change("2026-08-03", 100, "7", 4_000),
        ];
        assert_eq!(cash_chain(0, &entries), vec![7_000, 6_000, 4_000]);
    }

    /// 空链与单条：空链没有余额可言，单条就是 base + 它自己。
    #[test]
    fn empty_and_single_chains() {
        assert!(cash_chain(12_345, &[]).is_empty());
        assert_eq!(
            cash_chain(12_345, &[change("2026-08-01", 1, "a", -500)]),
            vec![11_845]
        );
    }

    /// 起点可以为负、累加也可以为负（倒填的交易排在"追加本金"之前时就会出现），
    /// 不做钳制 —— 它只是链上的一格数字，不是"可用现金"。
    #[test]
    fn balances_may_be_negative() {
        let entries = [
            change("2026-07-01", 100, "a", -1_000_000),
            change("2026-08-01", 200, "b", 1_500_000),
        ];
        assert_eq!(cash_chain(0, &entries), vec![-1_000_000, 500_000]);
    }
}
