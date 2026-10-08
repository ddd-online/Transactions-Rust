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
//! ## 收哪些费由**品种**决定（[`Instrument`]）
//!
//! | 品种 | 佣金（双向） | 印花税（仅卖出） | 过户费（双向） |
//! |---|---|---|---|
//! | 沪市股票（60 / 68） | ✅ | ✅ | ✅ |
//! | 深市股票（00 / 30） | ✅ | ✅ | ❌ |
//! | 沪 / 深**场内基金**（ETF / LOF / 封闭式基金 / REITs） | ✅ | ❌ | ❌ |
//!
//! 场内基金只有佣金（最低佣金照收）—— 免印花税、免过户费是交易所/中国结算的规则，
//! 与"哪个市场"无关，所以品种是**一个**判据，不是两个独立的 bool。
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

/// 交易品种：由六位代码判定 —— 它同时决定**收哪些费**与行情接口的市场前缀。
///
/// 场内基金（ETF / LOF / 封闭式基金 / 公募 REITs）与股票的差别只有一处：
/// **不收印花税、不收过户费**，只有佣金（最低佣金照收，买卖双向各一次）。
/// 代码段：沪市基金 `5[0-8]xxxx`、深市基金 `1[5-8]xxxx`；可转债（沪 11xxxx、深 12xxxx）不在支持范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    /// 沪市股票（60 主板 / 68 科创板）：过户费双向
    ShanghaiStock,
    /// 深市股票（00 主板 / 30 创业板）：无过户费
    ShenzhenStock,
    /// 沪市场内基金（50–58）
    ShanghaiFund,
    /// 深市场内基金（15–18）
    ShenzhenFund,
}

impl Instrument {
    /// 由六位代码判定品种；不支持的代码返回 `None`。
    ///
    /// 这是"代码是否合法"的**唯一判据**（[`is_valid_stock_code`] 就是它），也是费用与行情的分岔点。
    pub fn of(stock_code: &str) -> Option<Self> {
        let bytes = stock_code.as_bytes();
        if bytes.len() != 6 || !bytes.iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        match &stock_code[..2] {
            "60" | "68" => Some(Self::ShanghaiStock),
            "00" | "30" => Some(Self::ShenzhenStock),
            "50" | "51" | "52" | "53" | "54" | "55" | "56" | "57" | "58" => {
                Some(Self::ShanghaiFund)
            }
            "15" | "16" | "17" | "18" => Some(Self::ShenzhenFund),
            _ => None,
        }
    }

    /// 行情接口的市场前缀：沪市 `sh`，深市 `sz`。
    pub fn market_prefix(self) -> &'static str {
        match self {
            Self::ShanghaiStock | Self::ShanghaiFund => "sh",
            Self::ShenzhenStock | Self::ShenzhenFund => "sz",
        }
    }

    /// 是不是场内基金（免印花税、免过户费）。
    pub fn is_fund(self) -> bool {
        matches!(self, Self::ShanghaiFund | Self::ShenzhenFund)
    }

    /// 是不是沪市（过户费的唯一判据；场内基金即使在上海也不收）。
    pub fn is_shanghai(self) -> bool {
        matches!(self, Self::ShanghaiStock | Self::ShanghaiFund)
    }

    /// 品种名（界面文案用）。
    pub fn label(self) -> &'static str {
        if self.is_fund() {
            "场内基金"
        } else {
            "股票"
        }
    }
}

/// 一笔交易的费用明细（单位：分）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeeBreakdown {
    /// 佣金
    pub commission: i64,
    /// 印花税（买入恒为 0；场内基金恒为 0）
    pub stamp_duty: i64,
    /// 过户费（仅沪市**股票**收取，双向；场内基金与深市股票恒为 0）
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

/// 过户费（仅沪市**股票**收取，买入与卖出双向；场内基金不收）。
fn transfer_fee(amount: i64, instrument: Instrument, setting: &StockFeeSetting) -> i64 {
    if instrument == Instrument::ShanghaiStock {
        round_to_cents(amount, setting.transfer_fee_rate)
    } else {
        0
    }
}

/// 印花税（仅卖出收取；**场内基金一律免**）。
fn stamp_duty(amount: i64, instrument: Instrument, setting: &StockFeeSetting, is_buy: bool) -> i64 {
    if is_buy || instrument.is_fund() {
        0
    } else {
        round_to_cents(amount, setting.stamp_duty_rate)
    }
}

/// 一笔成交的费用（**逐笔取整**）：佣金只按费率算，**不套最低佣金**。
///
/// 最低佣金是**委托级**的门槛（一次委托只收一次），套在单笔成交上会让多笔委托
/// 被收成 N 份最低佣金。委托级佣金见 [`compute_order_fee`]。
fn compute_fill_fee(
    amount: i64,
    instrument: Instrument,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> FeeBreakdown {
    let commission = round_to_cents(amount, setting.commission_rate);
    let breakdown = FeeBreakdown {
        commission,
        stamp_duty: stamp_duty(amount, instrument, setting, is_buy),
        transfer_fee: transfer_fee(amount, instrument, setting),
        total: 0,
    };
    breakdown.with_total()
}

/// 委托级费用：佣金按**委托总额**收一次（含最低佣金），印花税与过户费**逐笔**算好再相加。
///
/// `fills` 是各笔成交的成交额（`价格 × 股数`，单位分）；`total_amount` 应当等于它们之和
/// （单独传是为了让调用方不必重复求和，两者不一致时以 `fills` 为准）。
pub fn compute_order_fee(
    fills: &[i64],
    instrument: Instrument,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> FeeBreakdown {
    let per_fill = compute_fill_fees(fills, instrument, setting, is_buy);
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
    instrument: Instrument,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> Vec<FeeBreakdown> {
    if amounts.is_empty() {
        return Vec::new();
    }
    let mut per_fill = compute_fill_fees(amounts, instrument, setting, is_buy);
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
    instrument: Instrument,
    setting: &StockFeeSetting,
    is_buy: bool,
) -> Vec<FeeBreakdown> {
    fills
        .iter()
        .map(|amount| compute_fill_fee(*amount, instrument, setting, is_buy))
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

/// A 股六位代码校验（股票：沪 60/68、深 00/30；场内基金：沪 5[0-8]、深 1[5-8]）。
///
/// 行情抓取、股票名查询与下单校验共用**这一个**判据（[`Instrument::of`]）——
/// 多一处"我也判断一下代码"，就多一处"ETF 在界面能填、在行情那儿被拒"的分叉。
pub fn is_valid_stock_code(stock_code: &str) -> bool {
    Instrument::of(stock_code).is_some()
}

/// 行情接口使用的市场前缀：沪市 `sh`，深市 `sz`；非法代码回落到 `sz`（调用方多半会先校验）。
pub fn market_prefix(stock_code: &str) -> &'static str {
    Instrument::of(stock_code)
        .map(Instrument::market_prefix)
        .unwrap_or("sz")
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
        let sh = compute_order_fee(&[100_000_000], Instrument::ShanghaiStock, &s, true);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 0);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 24_540);

        let sz = compute_order_fee(&[100_000_000], Instrument::ShenzhenStock, &s, true);
        assert_eq!(sz.transfer_fee, 0);
        assert_eq!(sz.total, 23_540);
    }

    #[test]
    fn sell_fee_charges_stamp_duty() {
        let s = setting();
        let sh = compute_order_fee(&[100_000_000], Instrument::ShanghaiStock, &s, false);
        assert_eq!(sh.commission, 23_540);
        assert_eq!(sh.stamp_duty, 50_000);
        assert_eq!(sh.transfer_fee, 1_000);
        assert_eq!(sh.total, 74_540);
    }

    /// 场内基金（ETF / LOF / REITs）：**只有佣金** —— 免印花税、免过户费，买卖都一样。
    ///
    /// 沪市的基金最容易写错：它"在沪市"，但过户费与市场无关（只对沪市**股票**收）。
    #[test]
    fn a_fund_is_charged_commission_only() {
        let s = setting();
        for instrument in [Instrument::ShanghaiFund, Instrument::ShenzhenFund] {
            let buy = compute_order_fee(&[100_000_000], instrument, &s, true);
            assert_eq!(buy.commission, 23_540, "{instrument:?} 买入佣金照收");
            assert_eq!(buy.stamp_duty, 0, "{instrument:?} 买入无印花税");
            assert_eq!(buy.transfer_fee, 0, "{instrument:?} 买入无过户费");
            assert_eq!(buy.total, 23_540);

            let sell = compute_order_fee(&[100_000_000], instrument, &s, false);
            assert_eq!(sell.stamp_duty, 0, "{instrument:?} 卖出也免印花税");
            assert_eq!(sell.transfer_fee, 0, "{instrument:?} 卖出也免过户费");
            assert_eq!(sell.total, 23_540, "{instrument:?} 卖出只有佣金");
        }

        // 最低佣金照收：小额委托仍是 5.00 元（基金不豁免佣金门槛）
        let small = compute_order_fee(&[100_000], Instrument::ShanghaiFund, &s, false);
        assert_eq!(small.commission, 500);
        assert_eq!(small.total, 500);
    }

    /// 逐笔口径在基金上同样成立：多笔成交的分摊之和 == 委托级合计（此时就只有佣金）。
    #[test]
    fn a_fund_order_splits_commission_across_fills() {
        let s = setting();
        let amounts = [3_000_000_i64, 3_000_000];
        let order = compute_order_fee(&amounts, Instrument::ShenzhenFund, &s, false);
        assert_eq!(order.stamp_duty, 0);
        assert_eq!(order.transfer_fee, 0);
        let allocated = allocate_order_fee(order, &amounts, Instrument::ShenzhenFund, &s, false);
        assert_eq!(
            allocated.iter().map(|fee| fee.total).sum::<i64>(),
            order.total
        );
        assert!(allocated.iter().all(|fee| fee.stamp_duty == 0));
        assert!(allocated.iter().all(|fee| fee.transfer_fee == 0));
    }

    /// 用户报的例子：一次委托两笔成交（各 1 手），印花税与过户费**逐笔**取整再相加。
    ///
    /// 36.61×100 = 3661.00 元、36.67×100 = 3667.00 元（共 7328.00），卖出、沪市。
    #[test]
    fn per_fill_rounding_matches_the_documented_example() {
        let s = setting();
        let fills = [366_100_i64, 366_700];
        let fee = compute_order_fee(&fills, Instrument::ShanghaiStock, &s, false);

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
        let allocated = allocate_order_fee(fee, &fills, Instrument::ShanghaiStock, &s, false);
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
        let per_fill = compute_order_fee(&fills, Instrument::ShanghaiStock, &s, false).transfer_fee;
        let whole_order = round_to_cents(fills.iter().sum::<i64>(), s.transfer_fee_rate);
        assert_eq!(per_fill, 8);
        assert_eq!(whole_order, 7);
    }

    #[test]
    fn order_fee_is_charged_once_per_order_not_per_fill() {
        let s = setting();
        // 三笔小额成交：每笔单独计费都会触发 5 元最低佣金
        let amounts = [1_000_000_i64, 1_000_000, 1_000_000];
        let order_fee = compute_order_fee(&amounts, Instrument::ShanghaiStock, &s, true);
        let allocated =
            allocate_order_fee(order_fee, &amounts, Instrument::ShanghaiStock, &s, true);

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
        let order_fee = compute_order_fee(&amounts, Instrument::ShanghaiStock, &s, true);
        let allocated =
            allocate_order_fee(order_fee, &amounts, Instrument::ShanghaiStock, &s, true);

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
    fn instruments_and_code_checks() {
        // 股票：沪 60/68、深 00/30
        assert_eq!(Instrument::of("600519"), Some(Instrument::ShanghaiStock));
        assert_eq!(Instrument::of("688981"), Some(Instrument::ShanghaiStock));
        assert_eq!(Instrument::of("000001"), Some(Instrument::ShenzhenStock));
        assert_eq!(Instrument::of("300750"), Some(Instrument::ShenzhenStock));
        // 场内基金：沪 5[0-8]（ETF / LOF / 封闭式基金 / REITs）、深 1[5-8]
        for code in [
            "510300", "588000", "501050", "508000", "161725", "159915", "180101",
        ] {
            let instrument =
                Instrument::of(code).unwrap_or_else(|| panic!("{code} 应被认作场内基金"));
            assert!(instrument.is_fund(), "{code} 应当是场内基金");
            assert_eq!(instrument.label(), "场内基金");
        }
        assert_eq!(Instrument::of("510300"), Some(Instrument::ShanghaiFund));
        assert_eq!(Instrument::of("159915"), Some(Instrument::ShenzhenFund));
        // 不支持：位数不对 / 非数字 / 北交所 / 可转债 / 港美股
        for code in [
            "12345", "60051a", "830799", "113050", "123456", "00700", "AAPL",
        ] {
            assert_eq!(Instrument::of(code), None, "{code} 不该被认作支持的品种");
            assert!(!is_valid_stock_code(code), "{code} 不该通过代码校验");
        }
        assert!(is_valid_stock_code("600519"));
        assert!(is_valid_stock_code("000001"));
        assert!(is_valid_stock_code("300750"));
        assert!(is_valid_stock_code("510300"), "ETF 必须能填进下单弹窗");
        assert!(is_valid_stock_code("159915"));

        // 市场前缀：沪市（股票与基金）sh，深市 sz
        assert_eq!(market_prefix("600519"), "sh");
        assert_eq!(market_prefix("000001"), "sz");
        assert_eq!(market_prefix("510300"), "sh");
        assert_eq!(market_prefix("588000"), "sh");
        assert_eq!(market_prefix("159915"), "sz");
        assert_eq!(market_prefix("161725"), "sz");
        // 非法代码回落到 sz（与从前一致，调用方会先校验）
        assert_eq!(market_prefix("113050"), "sz");
    }

    /// `is_shanghai` 是过户费的判据，但**基金即使在上海也不收** —— 两个问题别混成一个。
    #[test]
    fn shanghai_market_and_transfer_fee_are_not_the_same_question() {
        let s = setting();
        assert!(Instrument::ShanghaiFund.is_shanghai());
        assert!(Instrument::ShanghaiFund.is_fund());
        assert_eq!(
            compute_order_fee(&[100_000_000], Instrument::ShanghaiFund, &s, false).transfer_fee,
            0,
            "沪市场内基金不收过户费"
        );
        assert!(Instrument::ShanghaiStock.is_shanghai());
        assert!(!Instrument::ShanghaiStock.is_fund());
        assert_eq!(
            compute_order_fee(&[100_000_000], Instrument::ShanghaiStock, &s, false).transfer_fee,
            1_000,
            "沪市股票收过户费"
        );
    }
}
