//! 股票交易费用与费用分摊。
//!
//! 覆盖范围：取整到分 / 佣金（含最低佣金）/ 买入与卖出费用 / 委托级费用 /
//! 按成交额把委托费用分摊到各笔明细。
//!
//! 本模块是"两侧同一份算法"的落点：服务层与界面层都调用这里，
//! 从而在结构上杜绝前后端各写一份、需要人工保持同步的问题。
//!
//! 取整口径：`f64::round` 是"四舍五入、远离零"；费用金额恒为正，
//! 因此正数上的取整结果不存在歧义。
//!
//! ## 一次委托多笔成交时的计费口径（**按笔取整再相加**）
//!
//! 券商是按**成交笔**收费的，所以每笔成交的费用各自四舍五入到分，再相加：
//!
//! * **佣金**：整笔委托只收一次，按委托总额算，并只在这里用一次最低佣金。
//! * **印花税 / 过户费**：逐笔按成交额算、**逐笔**四舍五入，再求和。
//!
//! 第二条例外于"先求和再取整"，是有意的：那两种费在每笔成交上都会真的收一次，
//! 每笔各自进整到分。用 `¥36.61×100` + `¥36.67×100` 两笔举反例（卖出、沪市）：
//!
//! | 费目 | 逐笔取整再相加（本实现） | 先求和再取整（错的） |
//! |---|---|---|
//! | 印花税 0.05% | `1.83 + 1.83 = 3.66` | `3.664 → 3.66` |
//! | 过户费 0.001% | `0.04 + 0.04 = 0.08` | `0.07328 → 0.07` |
//!
//! 过户费那行两种口径差一分（0.08 vs 0.07），界面上的"预计到手"也就差一分。
//! 回归见 `per_fill_rounding_matches_the_documented_example`。

use crate::models::StockFeeSetting;

/// 一笔交易的费用明细（单位：分）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeeBreakdown {
    /// 佣金
    pub commission: i64,
    /// 印花税（买入恒为 0）
    pub stamp_duty: i64,
    /// 过户费（仅沪市收取，双向）
    pub transfer_fee: i64,
    /// 合计
    pub total: i64,
}

/// 按费率计算费用并四舍五入到分。
pub fn round_to_cents(amount: i64, rate: f64) -> i64 {
    (amount as f64 * rate).round() as i64
}

/// 佣金 = max(金额×费率, 最低佣金)。
pub fn compute_commission(amount: i64, setting: &StockFeeSetting) -> i64 {
    let commission = round_to_cents(amount, setting.commission_rate);
    commission.max(setting.min_commission)
}

/// 过户费（仅沪市收取，买入与卖出双向）。
fn transfer_fee(amount: i64, is_shanghai: bool, setting: &StockFeeSetting) -> i64 {
    if is_shanghai {
        round_to_cents(amount, setting.transfer_fee_rate)
    } else {
        0
    }
}

/// 一笔成交的费用（**逐笔取整**）：佣金只按费率算，**不套最低佣金**。
///
/// 最低佣金是**委托级**的门槛（一次委托只收一次），套在单笔成交上会让多笔委托
/// 被收成 N 份最低佣金。委托级佣金见 [`compute_order_fee`]。
fn compute_fill_fee(
    amount: i64,
    is_shanghai: bool,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> FeeBreakdown {
    let commission = round_to_cents(amount, setting.commission_rate);
    let stamp_duty = if is_buy {
        0
    } else {
        round_to_cents(amount, setting.stamp_duty_rate)
    };
    let transfer_fee = transfer_fee(amount, is_shanghai, setting);
    FeeBreakdown {
        commission,
        stamp_duty,
        transfer_fee,
        total: commission + stamp_duty + transfer_fee,
    }
}

/// 委托级费用：佣金按**委托总额**收一次（含最低佣金），印花税与过户费**逐笔**算好再相加。
///
/// `fills` 是各笔成交的成交额（`价格 × 股数`，单位分）；`total_amount` 应当等于它们之和
/// （单独传是为了让调用方不必重复求和，两者不一致时以 `fills` 为准）。
pub fn compute_order_fee(
    fills: &[i64],
    is_shanghai: bool,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> FeeBreakdown {
    let per_fill = compute_fill_fees(fills, is_shanghai, setting, is_buy);
    let total_amount: i64 = fills.iter().sum();

    FeeBreakdown {
        // 佣金：按委托总额、只收一次；最低佣金也只在这里生效
        commission: compute_commission(total_amount, setting),
        // 这两项是"逐笔取整后求和"，不是"求和后取整"（见模块头的对照表）
        stamp_duty: per_fill.iter().map(|fee| fee.stamp_duty).sum(),
        transfer_fee: per_fill.iter().map(|fee| fee.transfer_fee).sum(),
        total: 0, // 下面统一按三项之和重算
    }
    .with_total()
}

impl FeeBreakdown {
    /// 把 `total` 重算成三项之和（构造时用它兜住"合计与明细不一致"）。
    fn with_total(mut self) -> Self {
        self.total = self.commission + self.stamp_duty + self.transfer_fee;
        self
    }
}

/// 把委托费用拆到各笔成交（**就是逐笔各算各的**，与 [`compute_order_fee`] 同一口径）。
///
/// 这样落在每笔成交上的费用，求和后与委托级合计**一模一样**：
/// `compute_order_fee` 的印花税/过户费本来就是各笔之和，佣金则按成交额比例分摊
/// （末笔吸收取整余数，保证 Σ分摊 = 委托佣金）。
///
/// ⚠ 调用方要传**用同一份 `fills` 算出来的** `fee`，否则两份口径会对不上 ——
/// 资金记录用 `fee.total`、每笔成交用分摊值，两者不一致就是"钱对不上账"。
pub fn allocate_order_fee(
    fee: FeeBreakdown,
    amounts: &[i64],
    is_shanghai: bool,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> Vec<FeeBreakdown> {
    if amounts.is_empty() {
        return Vec::new();
    }
    let mut per_fill = compute_fill_fees(amounts, is_shanghai, setting, is_buy);
    // 佣金改为按成交额比例分摊委托级那一笔（最低佣金只在委托级生效过）
    let commissions = allocate_by_amount(fee.commission, amounts);
    for (index, fill) in per_fill.iter_mut().enumerate() {
        fill.commission = commissions[index];
        fill.total = fill.commission + fill.stamp_duty + fill.transfer_fee;
    }
    per_fill
}

/// 多笔成交的**逐笔**费用（各笔自己四舍五入，不套最低佣金）。
///
/// 给"按笔展示费用明细"这类用途；委托级合计用 [`compute_order_fee`]。
pub fn compute_fill_fees(
    fills: &[i64],
    is_shanghai: bool,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> Vec<FeeBreakdown> {
    fills
        .iter()
        .map(|amount| compute_fill_fee(*amount, is_shanghai, setting, is_buy))
        .collect()
}

/// 按权重比例拆分一个金额，末项吸收取整余数。
pub fn allocate_by_amount(total: i64, weights: &[i64]) -> Vec<i64> {
    let mut result = vec![0_i64; weights.len()];
    if weights.is_empty() {
        return result;
    }

    let sum: i64 = weights.iter().sum();
    if sum <= 0 {
        // 权重不可用时全额落到末项
        result[weights.len() - 1] = total;
        return result;
    }

    let mut allocated = 0_i64;
    for (i, weight) in weights.iter().enumerate().take(weights.len() - 1) {
        let value = (total as f64 * *weight as f64 / sum as f64).round() as i64;
        result[i] = value;
        allocated += value;
    }
    result[weights.len() - 1] = total - allocated;
    result
}

/// 沪市代码判定：60（主板）/ 68（科创板）开头，过户费双向收取。
pub fn is_shanghai_code(stock_code: &str) -> bool {
    stock_code.starts_with("60") || stock_code.starts_with("68")
}

/// A 股六位代码校验（沪 60/68、深 00/30）。行情抓取与股票名查询共用。
pub fn is_valid_stock_code(stock_code: &str) -> bool {
    let bytes = stock_code.as_bytes();
    if bytes.len() != 6 || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    stock_code.starts_with("60")
        || stock_code.starts_with("68")
        || stock_code.starts_with("00")
        || stock_code.starts_with("30")
}

/// 行情接口使用的市场前缀：沪市 `sh`，其余 `sz`。
pub fn market_prefix(stock_code: &str) -> &'static str {
    if stock_code.starts_with("60") || stock_code.starts_with("68") {
        "sh"
    } else {
        "sz"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting() -> StockFeeSetting {
        StockFeeSetting {
            commission_rate: 0.0002354,
            min_commission: 500,
            stamp_duty_rate: 0.0005,
            transfer_fee_rate: 0.00001,
            ..StockFeeSetting::default()
        }
    }

    #[test]
    fn commission_respects_minimum() {
        // 小额：按费率算出的佣金低于最低佣金，取最低佣金 5 元
        assert_eq!(compute_commission(10_000, &setting()), 500);
        // 大额：100 万元 × 万2.354 = 235.4 元 → 23540 分
        assert_eq!(compute_commission(100_000_000, &setting()), 23_540);
    }

    #[test]
    fn buy_fee_charges_transfer_only_in_shanghai() {
        let s = setting();
        // 单笔委托（一笔成交）
        let sh = compute_order_fee(&[100_000_000], true, &s, true);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 0);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 24_540);

        let sz = compute_order_fee(&[100_000_000], false, &s, true);
        assert_eq!(sz.transfer_fee, 0);
        assert_eq!(sz.total, 23_540);
    }

    #[test]
    fn sell_fee_charges_stamp_duty() {
        let s = setting();
        let sh = compute_order_fee(&[100_000_000], true, &s, false);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 50_000);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 74_540);
    }

    /// 用户报的例子：一次委托两笔成交（各 1 手），印花税与过户费**逐笔**取整再相加。
    ///
    /// 36.61×100 = 3661.00 元、36.67×100 = 3667.00 元（共 7328.00），卖出、沪市。
    #[test]
    fn per_fill_rounding_matches_the_documented_example() {
        let s = setting();
        let fills = [366_100_i64, 366_700];
        let fee = compute_order_fee(&fills, true, &s, false);

        // 佣金：7328.00 × 0.02354% = 1.725 元 → 低于最低佣金 → 5.00（一次委托只收一次）
        assert_eq!(fee.commission, 500);
        // 印花税：1.8305 → 1.83；1.8335 → 1.83 ⇒ 3.66
        //（若先求和再取整：7328 × 0.05% = 3.664 → 3.66，这次恰好一样）
        assert_eq!(fee.stamp_duty, 366);
        // 过户费：0.03661 → 0.04；0.03667 → 0.04 ⇒ **0.08**
        //（先求和再取整只有 0.07328 → 0.07 —— 这正是本次修掉的一分钱）
        assert_eq!(fee.transfer_fee, 8);
        assert_eq!(fee.total, 874, "5.00 + 3.66 + 0.08");

        // 逐笔分摊之和必须与委托级合计一致（否则资金记录与各笔成交会对不上账）
        let allocated = allocate_order_fee(fee, &fills, true, &s, false);
        assert_eq!(allocated.len(), 2);
        assert_eq!(allocated.iter().map(|f| f.commission).sum::<i64>(), 500);
        assert_eq!(allocated.iter().map(|f| f.stamp_duty).sum::<i64>(), 366);
        assert_eq!(allocated.iter().map(|f| f.transfer_fee).sum::<i64>(), 8);
        assert_eq!(allocated.iter().map(|f| f.total).sum::<i64>(), fee.total);
        // 两笔成交额几乎相同 → 过户费各 0.04
        assert_eq!(allocated[0].transfer_fee, 4);
        assert_eq!(allocated[1].transfer_fee, 4);

        // 预计到手 = 7328.00 - 8.74 = 7319.26
        let net = fills.iter().sum::<i64>() - fee.total;
        assert_eq!(net, 731_926);
    }

    /// 对比"先求和再取整"：证明两条口径确实会差（否则上面那条单测就白写了）。
    #[test]
    fn whole_order_rounding_would_lose_a_cent_on_transfer_fee() {
        let s = setting();
        let fills = [366_100_i64, 366_700];
        let per_fill = compute_order_fee(&fills, true, &s, false).transfer_fee;
        let whole_order = round_to_cents(fills.iter().sum::<i64>(), s.transfer_fee_rate);
        assert_eq!(per_fill, 8);
        assert_eq!(whole_order, 7);
    }

    #[test]
    fn order_fee_is_charged_once_per_order_not_per_fill() {
        let s = setting();
        // 三笔小额成交：每笔单独计费都会触发 5 元最低佣金
        let amounts = [1_000_000_i64, 1_000_000, 1_000_000];
        let order_fee = compute_order_fee(&amounts, true, &s, true);
        let allocated = allocate_order_fee(order_fee, &amounts, true, &s, true);

        assert_eq!(allocated.len(), 3);
        // 委托级佣金 = 300 万 × 万2.354 = 706 分，高于最低佣金 500 分
        assert_eq!(order_fee.commission, 706);
        assert_eq!(
            allocated.iter().map(|f| f.commission).sum::<i64>(),
            order_fee.commission,
            "分摊之和必须等于委托级费用"
        );
        assert_eq!(
            allocated.iter().map(|f| f.total).sum::<i64>(),
            order_fee.total
        );
        // 单笔分摊额低于"按笔单独计收"的最低佣金 → 证明最低佣金只在委托级收一次
        assert_eq!(compute_commission(1_000_000, &s), 500);
        assert!(allocated[0].commission < 500);
        assert!(
            order_fee.commission < 3 * 500,
            "按笔计收会是 1500 分，按委托计收只有 706 分"
        );
    }

    #[test]
    fn order_fee_allocation_puts_rounding_remainder_on_last_fill() {
        let s = setting();
        let amounts = [30_000_000_i64, 30_000_000, 40_000_000];
        let order_fee = compute_order_fee(&amounts, true, &s, true);
        let allocated = allocate_order_fee(order_fee, &amounts, true, &s, true);

        // 100 万 × 万2.354 = 235.4 元 = 23540 分，前两笔各 7062 分，末笔吃余数
        assert_eq!(order_fee.commission, 23_540);
        assert_eq!(allocated[0].commission, 7_062);
        assert_eq!(allocated[1].commission, 7_062);
        assert_eq!(allocated[2].commission, 23_540 - 7_062 - 7_062);
        assert_eq!(
            allocated.iter().map(|f| f.stamp_duty).sum::<i64>(),
            order_fee.stamp_duty
        );
        assert_eq!(
            allocated.iter().map(|f| f.transfer_fee).sum::<i64>(),
            order_fee.transfer_fee
        );
    }

    #[test]
    fn allocate_by_amount_puts_remainder_on_last_item() {
        assert_eq!(allocate_by_amount(100, &[1, 1, 1]), vec![33, 33, 34]);
        assert_eq!(allocate_by_amount(10, &[0, 0]), vec![0, 10]);
        assert_eq!(allocate_by_amount(7, &[]), Vec::<i64>::new());
        assert_eq!(allocate_by_amount(5, &[3]), vec![5]);
    }

    #[test]
    fn shanghai_and_code_checks() {
        assert!(is_shanghai_code("600519"));
        assert!(is_shanghai_code("688981"));
        assert!(!is_shanghai_code("000001"));
        assert!(is_valid_stock_code("600519"));
        assert!(is_valid_stock_code("000001"));
        assert!(is_valid_stock_code("300750"));
        assert!(!is_valid_stock_code("12345"));
        assert!(!is_valid_stock_code("830799")); // 北交所不在支持范围
        assert_eq!(market_prefix("600519"), "sh");
        assert_eq!(market_prefix("000001"), "sz");
    }
}
