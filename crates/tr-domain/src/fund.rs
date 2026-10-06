//! 股票资金流水的**口径**：现金链上"谁更新"、余额怎么走、支取超限怎么说。
//!
//! 这些都从 `tr-service` 里搬来（候选 3 / #36），因为从前它们是三份手写的加减加一句硬编码文案，
//! 没有断言守着。搬进来的是**判据**；写入本身的组织归 `stock::write`（ADR-0002 的股票写入聚合，
//! 现金链复算 `recalculate_cash_chain` 就在那里）。
//!
//! 本模块**只放被真正用到的东西**：一开始我还搬进来一个 `FundEntry` + `latest_index`（"链上哪条最新"），
//! 后来发现 `stock::write::fund_record_after` 早就是这条规则的实现、而且真的在用 —— 那就是第二份实现，
//! 于是删掉了自己那份，改成让那边**委托**到 [`is_newer`]。判据只有一处，才是这次搬家的目的。

/// 支取超限时给用户看的整句（**用户可见文案，改动即影响界面/接口**）。
pub fn withdraw_limit_message(available_cash: i64) -> String {
    format!(
        "支取金额不能超过可用现金（{} 元）",
        crate::money::cents_to_yuan(available_cash)
    )
}

/// 追加 / 支取前的可用现金：取现金链**末条**的余额；链还是空的（这个账本从没记过资金）才用当前本金。
pub fn cash_before(latest_balance: Option<i64>, principal: i64) -> i64 {
    latest_balance.unwrap_or(principal)
}

/// 现金链上的一步：新余额 = 上一条余额 + 本次变动。
///
/// 四种资金事件共用这一条：入库时 `amount_change` 已经带符号（追加本金与利息归本是 `+amount`、
/// 支取是 `-amount`），所以余额算式不需要按事件分支。别在别处再写 `prev - amount` 这类分叉。
pub fn cash_after(prev_cash: i64, amount_change: i64) -> i64 {
    prev_cash + amount_change
}

/// 现金链的时序判据：`(记录日期, 创建时间, id)` 三者依次比较，`a` 是否比 `b` 更**新**。
///
/// 日期是零填充的 `YYYY-MM-DD`、id 是 uuid 字符串，两者都按**字典序**比（这就是既有行为）。这条规则决定"复算现金链时，某一条的
/// 余额接在哪一条后面"—— 而它也是那个**已知坑**的来源：**倒填**的资金记录（日期更早、录入更晚）
/// 不是"更新"的那条，于是它的金额进不了链，表现为可用现金虚高（AGENTS.md 有记，用户真实数据上
/// 正好虚高过一笔建仓的钱）。修它等于改现金链口径，得单独一个提交。
pub fn is_newer(a: (&str, i64, &str), b: (&str, i64, &str)) -> bool {
    (a.0, a.1, a.2) > (b.0, b.1, b.2)
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

    /// `amount_change` 已带符号，所以"支取"就是"加一个负数"。
    #[test]
    fn cash_after_adds_a_signed_change() {
        assert_eq!(cash_after(10_000, 5_000), 15_000, "追加本金");
        assert_eq!(
            cash_after(10_000, -4_000),
            6_000,
            "支取（符号在 amount_change 里）"
        );
        assert_eq!(cash_after(10_000, 0), 10_000);
        assert_eq!(cash_after(0, -500), -500);
    }

    #[test]
    fn newer_is_by_date_then_created_at_then_id() {
        assert!(
            is_newer(("2026-08-03", 0, "a"), ("2026-08-01", 999, "z")),
            "日期优先"
        );
        assert!(
            is_newer(("2026-08-03", 300, "a"), ("2026-08-03", 100, "z")),
            "同日比创建时间"
        );
        assert!(
            is_newer(("2026-08-03", 100, "9"), ("2026-08-03", 100, "7")),
            "同刻比 id"
        );
        assert!(
            !is_newer(("2026-08-03", 100, "7"), ("2026-08-03", 100, "7")),
            "相等不算更新"
        );
    }

    /// **把已知坑钉住**：补录的记录（日期更早、录入更晚）不是"更新"的那条，所以现金链会跳过它。
    #[test]
    fn a_back_dated_record_is_not_newer_even_though_it_was_entered_later() {
        let today = ("2026-08-24", 1_000, "1");
        let back_dated = ("2026-07-24", 2_000, "2");
        assert!(is_newer(today, back_dated), "日期大的那条才算更新");
        assert!(
            !is_newer(back_dated, today),
            "补录那条不是更新 —— 它的金额进不了链（现状）"
        );
    }
}
