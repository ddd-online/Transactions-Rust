//! 股票资金流水的**时序口径**：现金链上"哪一条是最新"的唯一判据。
//!
//! 现金链的做法是：每条资金记录都带一个"当时的余额"，新的一条按**前一条的余额**往下算。
//! 于是"前一条是谁"就决定了可用现金对不对 —— 而这条判据（`(日期, 创建时间, ID)` 最大者胜）
//! 从前只活在 `tr-service` 的注释里（见 AGENTS.md「时间戳语义」一节），没有任何断言守着。
//!
//! 这里先把**排序**钉住，因为它是"已知坑"的来源：**倒填**的资金记录（日期比已有的早、但录入得晚）
//! 不是"最新那条"，于是它的金额进不了链 —— 表现为可用现金虚高。用户真实数据上正好虚高过一笔建仓的钱。
//! AGENTS.md 把这条记为"已知、未修"；本模块的职责是**让它可断言**（修不修是另一件事，得单独动口径）。
//!
//! 每种事件对余额的作用（`+追加本金` / `−支取` / 买卖的现金流）**还没搬进来**：入库时金额的符号约定
//! 要先在 `tr-service` 那边核实（追加本金存正数？支取存正数还是负数？），确认后再补一个
//! `cash_delta(event, amount)` —— 先猜符号等于制造第二份口径。

/// 一条资金流水在现金链上的样子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundEntry {
    /// `YYYY-MM-DD`（**本地日**，见 AGENTS.md 的时间戳语义）
    pub record_date: String,
    /// 创建时刻（Unix 秒）
    pub created_at: i64,
    /// 行 id（同一 (日期, 创建时间) 下的兜底次序）
    pub id: i64,
}

impl FundEntry {
    pub fn new(record_date: impl Into<String>, created_at: i64, id: i64) -> Self {
        Self {
            record_date: record_date.into(),
            created_at,
            id,
        }
    }

    /// 现金链的时序键：(日期, 创建时间, ID)。日期是零填充的 `YYYY-MM-DD`，所以字符串序就是时间序。
    pub fn order_key(&self) -> (&str, i64, i64) {
        (self.record_date.as_str(), self.created_at, self.id)
    }
}

/// 链上"最新一条"的下标（`(日期, 创建时间, ID)` 最大者胜）。
///
/// 空链返回 `None`（= 还没有过资金记录，可用现金从 0 起）。
pub fn latest_index(entries: &[FundEntry]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.order_key().cmp(&right.order_key()))
        .map(|(index, _)| index)
}

/// 追加 / 支取前的可用现金：取现金链**末条**的余额；链还是空的（这个账本从没记过资金）才用当前本金。
///
/// 这条"没有记录就退回本金"的口径在 `tr-service/src/stock.rs` 里写了三遍
/// （追加本金 359 / 支取 444 / 利息归本 529 附近），三处的 `match` 都是同一件事：
/// `Ok(latest) => latest.cash_balance`、`Err(NotFound) => account.principal`。
pub fn cash_before(latest_balance: Option<i64>, principal: i64) -> i64 {
    latest_balance.unwrap_or(principal)
}

/// 支取超限时给用户看的整句（**用户可见文案，改动即影响界面/接口**）。
///
/// 口径：支取金额不能超过可用现金（可用现金由 [`cash_before`] 给出）。这句话从前硬编码在
/// `tr-service` 的 `AppError::bad_request(format!(...))` 里 —— 搬到这儿是为了让它可断言
/// （含分 → 元的展示口径：`money::cents_to_yuan`）。
pub fn withdraw_limit_message(available_cash: i64) -> String {
    format!(
        "支取金额不能超过可用现金（{} 元）",
        crate::money::cents_to_yuan(available_cash)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有记录就用记录的余额（哪怕本金已经变了）；没记录才退回本金。
    #[test]
    fn cash_before_falls_back_to_the_principal_only_without_records() {
        assert_eq!(cash_before(Some(12_345), 99_999), 12_345);
        assert_eq!(cash_before(Some(0), 99_999), 0, "余额 0 是余额，不是缺失");
        assert_eq!(cash_before(Some(-500), 99_999), -500);
        assert_eq!(cash_before(None, 99_999), 99_999);
        assert_eq!(cash_before(None, 0), 0, "本金也为 0 时就是 0");
    }

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

    fn entry(date: &str, created_at: i64, id: i64) -> FundEntry {
        FundEntry::new(date, created_at, id)
    }

    #[test]
    fn an_empty_chain_has_no_latest() {
        assert_eq!(latest_index(&[]), None);
    }

    #[test]
    fn the_latest_is_by_date_then_created_at_then_id() {
        let chain = vec![
            entry("2026-08-01", 100, 1),
            entry("2026-08-03", 50, 2),
            entry("2026-08-01", 200, 3),
        ];
        // 日期最大的那条（08-03）胜，哪怕它的创建时间最早、id 也不是最大
        assert_eq!(latest_index(&chain), Some(1));

        // 同日再比创建时间
        let same_day = vec![entry("2026-08-03", 100, 1), entry("2026-08-03", 300, 2)];
        assert_eq!(latest_index(&same_day), Some(1));

        // 同日同刻再比 id
        let same_instant = vec![entry("2026-08-03", 100, 7), entry("2026-08-03", 100, 9)];
        assert_eq!(latest_index(&same_instant), Some(1));
    }

    /// **把已知坑钉住**：倒填的记录（日期更早、录入更晚）不是"最新那条"，于是它的金额进不了链 ——
    /// 这就是"可用现金虚高"的来源。修它等于改现金链口径，得单独一个提交（见 #36）。
    #[test]
    fn a_back_dated_record_is_not_the_latest_even_though_it_was_entered_later() {
        let chain = vec![
            entry("2026-08-24", 1_000, 1), // 今天的建仓：日期新
            entry("2026-07-24", 2_000, 2), // 补录的上月成交：录入更晚，但日期更早
        ];
        assert_eq!(
            latest_index(&chain),
            Some(0),
            "现金链只认日期最大的那条，补录那条被跳过（现状）"
        );
    }

    /// 顺序不影响答案（判据是"取最大"，不是"最后一条"）。
    #[test]
    fn the_answer_does_not_depend_on_the_input_order() {
        let forward = vec![entry("2026-08-01", 100, 1), entry("2026-08-02", 100, 2)];
        let backward = vec![entry("2026-08-02", 100, 2), entry("2026-08-01", 100, 1)];
        let pick =
            |chain: &[FundEntry]| latest_index(chain).map(|index| chain[index].record_date.clone());
        assert_eq!(pick(&forward).as_deref(), Some("2026-08-02"));
        assert_eq!(pick(&backward).as_deref(), Some("2026-08-02"));
    }
}
