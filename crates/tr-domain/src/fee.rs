//! 股票交易费用与费用分摊。
//!
//! 对照实现：原后端 `kernel/service/stock_service.go` 的 `roundToCents` / `computeCommission` /
//! `ComputeBuyFee` / `ComputeSellFee` / `ComputeOrderFee` / `AllocateOrderFee` / `allocateByAmount`，
//! 以及原前端重复实现同一算法的 `app/src/backend/stockFee.ts`。
//!
//! 本模块是"两侧同一份算法"的落点：后端服务层与界面层都调用这里，
//! 从而在结构上杜绝了原实现里前后端各写一份、需要人工保持同步的问题。
//!
//! 取整口径：Go `math.Round` 与 Rust `f64::round` 都是"四舍五入、远离零"，
//! 与原前端的 `Math.round`（负数时朝 +∞ 取整）在正数费用上等价——费用金额恒为正。

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

/// 买入费用 = 佣金 + 过户费（仅沪市）。
pub fn compute_buy_fee(amount: i64, is_shanghai: bool, setting: &StockFeeSetting) -> FeeBreakdown {
    let commission = compute_commission(amount, setting);
    let transfer_fee = if is_shanghai {
        round_to_cents(amount, setting.transfer_fee_rate)
    } else {
        0
    };
    FeeBreakdown {
        commission,
        stamp_duty: 0,
        transfer_fee,
        total: commission + transfer_fee,
    }
}

/// 卖出费用 = 佣金 + 印花税 + 过户费（仅沪市）。印花税仅卖出时收取。
pub fn compute_sell_fee(amount: i64, is_shanghai: bool, setting: &StockFeeSetting) -> FeeBreakdown {
    let commission = compute_commission(amount, setting);
    let stamp_duty = round_to_cents(amount, setting.stamp_duty_rate);
    let transfer_fee = if is_shanghai {
        round_to_cents(amount, setting.transfer_fee_rate)
    } else {
        0
    };
    FeeBreakdown {
        commission,
        stamp_duty,
        transfer_fee,
        total: commission + stamp_duty + transfer_fee,
    }
}

/// 按委托成交总额计算一次费用：最低佣金按「委托」收取，而不是按每笔成交。
pub fn compute_order_fee(
    total_amount: i64,
    is_shanghai: bool,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> FeeBreakdown {
    if is_buy {
        compute_buy_fee(total_amount, is_shanghai, setting)
    } else {
        compute_sell_fee(total_amount, is_shanghai, setting)
    }
}

/// 把委托级费用按各笔成交金额比例分摊到明细。
/// 每个费用项单独分摊、四舍五入到分，最后一笔吸收余数，保证 Σ分摊 = 委托费用。
pub fn allocate_order_fee(fee: FeeBreakdown, amounts: &[i64]) -> Vec<FeeBreakdown> {
    let commissions = allocate_by_amount(fee.commission, amounts);
    let stamp_duties = allocate_by_amount(fee.stamp_duty, amounts);
    let transfer_fees = allocate_by_amount(fee.transfer_fee, amounts);

    (0..amounts.len())
        .map(|i| FeeBreakdown {
            commission: commissions[i],
            stamp_duty: stamp_duties[i],
            transfer_fee: transfer_fees[i],
            total: commissions[i] + stamp_duties[i] + transfer_fees[i],
        })
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
        // 权重不可用时全额落到末项（与原实现一致）
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

/// 行情接口使用的市场前缀（沪 `sh`，其余 `sz`），对应 Go `fetchTencentQuotes` 的判定。
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
        let sh = compute_buy_fee(100_000_000, true, &s);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 0);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 24_540);

        let sz = compute_buy_fee(100_000_000, false, &s);
        assert_eq!(sz.transfer_fee, 0);
        assert_eq!(sz.total, 23_540);
    }

    #[test]
    fn sell_fee_charges_stamp_duty() {
        let s = setting();
        let sh = compute_sell_fee(100_000_000, true, &s);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 50_000);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 74_540);
    }

    #[test]
    fn order_fee_is_charged_once_per_order_not_per_fill() {
        let s = setting();
        // 三笔小额成交：每笔单独计费都会触发 5 元最低佣金
        let amounts = [1_000_000_i64, 1_000_000, 1_000_000];
        let order_fee = compute_order_fee(3_000_000, true, &s, true);
        let allocated = allocate_order_fee(order_fee, &amounts);

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
        let order_fee = compute_order_fee(100_000_000, true, &s, true);
        let allocated = allocate_order_fee(order_fee, &amounts);

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
        assert!(!is_valid_stock_code("830799")); // 北交所不在支持范围（与原实现一致）
        assert_eq!(market_prefix("600519"), "sh");
        assert_eq!(market_prefix("000001"), "sz");
    }
}
