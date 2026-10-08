//! 股票域的**换算口径**：手 ↔ 股、价格 × 股数 = 成交额（全为整数分）。
//!
//! 从 `tr-service` 里搬来（候选 7 / #37）。收之前这条链在四处各写一遍：
//! `stock/write.rs:103`（`total_lots * 100`）、`:179`（`fill.lots * 100`）、`:675`（`lots * 100`）、
//! `stock.rs:1743`（内联的 `price * lots * 100`）—— 而 `100` 是**A 股的每手股数**，
//! 属于那种"散在代码里、改一次要全文件找"的魔数。
//!
//! 归属说明：这**不是**新的 owner。写入流水线与轮次推导仍归 `stock::write` / `stock`（ADR-0002），
//! 这里只放"手 → 股 → 金额"与"当前轮次的切法"这类能在 native 上断言的算法，让调用点都走同一份。

use crate::consts;
use crate::models::{RoundFee, StockTrade};

/// 每手股数（A 股口径）。
pub const SHARES_PER_LOT: i64 = 100;

/// 手数 → 股数。
pub fn shares_of(lots: i64) -> i64 {
    lots * SHARES_PER_LOT
}

/// 成交额（分）：单价（**厘**/股）× 股数 → 分。
///
/// **价格是厘、金额是分**（见 [`crate::money`]）：`amount = round(price_milli × shares / 10)`。
/// 整手（100 股）× 整数厘一定是整数分（`4389 × 100 = 438900 → 43890 分 = 438.90 元`），
/// 只有零股才真的需要这一次四舍五入。价格恒为正，所以这里按 `+5` 做"四舍五入、远离零"。
pub fn amount_of(price_milli: i64, shares: i64) -> i64 {
    (price_milli.saturating_mul(shares) + 5) / 10
}

/// 成交额（分）：单价（**厘**/股）× 手数（内部按 [`SHARES_PER_LOT`] 折股）。
pub fn amount_of_lots(price_milli: i64, lots: i64) -> i64 {
    amount_of(price_milli, shares_of(lots))
}

/// 从**按时间升序**的成交流里切出"当前在建轮次"（最近一次清仓之后的那几笔）。
///
/// 从 `tr-service/src/stock.rs` 搬来（候选 7 / #37）。规则三条，都是能被断言的：
///
/// * 建仓 / 加仓：股数累加，这笔属于当前轮；
/// * 减仓：股数减少；**没减到 0 就仍算这一轮**的成交；
/// * 清仓（或减到 0）：那一笔**不属于**它结束的那一轮之后的部分 —— 实现上是"清空已收集的成交"，
///   所以清仓那笔本身也不留在结果里；
/// * 其它成交类型忽略。
///
/// 用途：详情区显示的"当前轮次"与按轮次聚合的统计都读它。
pub fn current_round_trades(trades: &[StockTrade]) -> Vec<StockTrade> {
    let mut result: Vec<StockTrade> = Vec::new();
    let mut shares = 0_i64;
    for trade in trades {
        match trade.trade_type.as_str() {
            consts::STOCK_TRADE_OPEN | consts::STOCK_TRADE_ADD => {
                shares += trade.shares;
                result.push(trade.clone());
            }
            consts::STOCK_TRADE_REDUCE | consts::STOCK_TRADE_CLOSE => {
                shares -= trade.shares;
                if shares > 0 {
                    result.push(trade.clone());
                } else {
                    shares = 0;
                    result.clear();
                }
            }
            _ => {}
        }
    }
    result
}

/// 轮次号的分配规则：已完成的轮次数 + 序号（从 0 起）+ 1。
///
/// 从前这条 `+ 1` 写在两处（`stock/write.rs` 的 `close_round`、`stock.rs` 的重放路径），
/// 各自拼一次 —— 轮次号错一个，详情区、统计与复盘都会对不上（候选 7 / #37）。
pub fn next_round_no(completed_rounds: i64, index: i64) -> i64 {
    completed_rounds + index + 1
}
/// 当前轮次的**资金口径**（与界面「资金变动」列同一套算法）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoundFlow {
    /// 本轮买入成本合计（成交额 + 费用）
    pub buy_cost: i64,
    /// 本轮资金变动合计：买入为 `−(成交额 + 费用)`、卖出为 `成交额 − 费用`
    pub cash_flow: i64,
}

/// 买入方向（建仓 / 加仓）。
pub fn is_buy(trade_type: &str) -> bool {
    matches!(
        trade_type,
        consts::STOCK_TRADE_OPEN | consts::STOCK_TRADE_ADD
    )
}

/// 卖出方向（减仓 / 清仓）。
pub fn is_sell(trade_type: &str) -> bool {
    matches!(
        trade_type,
        consts::STOCK_TRADE_REDUCE | consts::STOCK_TRADE_CLOSE
    )
}

/// 把当前轮次（最近一次清仓之后）的成交折成一组金额。
///
/// 从 `tr-service/src/stock.rs` 搬来（候选 7 / #37）。两条口径从前只写在注释里、没有断言守着：
///
/// * **买入**（建仓 / 加仓）：`buy_cost += 成交额 + 费用`，同时 `cash_flow −= 成交额 + 费用`
///   （钱从现金里出去）；
/// * **卖出**（减仓 / 清仓）：只 `cash_flow += 成交额 − 费用`；**`buy_cost` 不回落** ——
///   它是"这一轮为买入花了多少"，不是当前持仓成本。
pub fn round_flow(trades_asc: &[StockTrade]) -> RoundFlow {
    let mut flow = RoundFlow::default();
    for trade in current_round_trades(trades_asc) {
        if is_buy(&trade.trade_type) {
            let cost = trade.amount + trade.fee;
            flow.buy_cost += cost;
            flow.cash_flow -= cost;
        } else if is_sell(&trade.trade_type) {
            flow.cash_flow += trade.amount - trade.fee;
        }
    }
    flow
}

/// 当前轮次（最近一次清仓之后）**建仓时定下的费用设置**；没记录（老数据）返回 `None`
/// ⇒ 调用方按账本系统配置处理。
///
/// 「本轮的第一笔」由 [`current_round_trades`] 的切分规则给出（清仓那笔本身不属于之后的部分），
/// 所以清仓之后这里不会再把上一轮的快照带出来。同一轮内每笔成交都写着同一份快照
/// （建仓时写入，加仓 / 减仓 / 清仓原样带上），因此取首笔即可。
pub fn current_round_fee(trades_asc: &[StockTrade]) -> Option<RoundFee> {
    current_round_trades(trades_asc)
        .first()
        .and_then(|trade| trade.round_fee.clone())
}

/// 浮动盈亏率（%）= 浮动盈亏 ÷ **持仓成本** × 100。
///
/// 持仓成本 = 本轮净投入 = `−`[`RoundFlow::cash_flow`]：买入的钱出去、卖出的钱回来，
/// "还压在这只股票上"的就是两者的净额。**不是**本轮买入总额（`buy_cost`，减仓的回款不冲抵它
/// ⇒ 分母偏大、亏损率被摊薄）——用户真实数据上：市值 3790.00 + 本轮资金变动 −4808.28 = 浮动盈亏
/// −1018.28，率是 −21.18%，而按买入总额 16776.16 算出来只有 −6.07%。
///
/// 净投入 ≤ 0（卖出回款已经超过买入支出）时没有可算的比率 ⇒ `None`（界面显示 `—`）。
/// 买入总额仍是**已结算轮次**盈亏率的分母（[`crate::dto::round_pnl`]），两者口径不同、别互换。
pub fn floating_pnl_rate(pnl: i64, cash_flow: i64) -> Option<f64> {
    let cost = -cash_flow;
    if cost <= 0 {
        return None;
    }
    Some((pnl as f64 / cost as f64 * 10_000.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(trade_type: &str, shares: i64) -> StockTrade {
        StockTrade {
            trade_type: trade_type.to_string(),
            shares,
            ..StockTrade::default()
        }
    }

    /// 带金额与费用的成交（资金口径用）。
    fn money_trade(trade_type: &str, shares: i64, amount: i64, fee: i64) -> StockTrade {
        StockTrade {
            trade_type: trade_type.to_string(),
            shares,
            amount,
            fee,
            ..StockTrade::default()
        }
    }

    #[test]
    fn a_buy_takes_money_out_of_cash_and_adds_to_the_buy_cost() {
        let flow = round_flow(&[money_trade(consts::STOCK_TRADE_OPEN, 100, 1_000_000, 500)]);
        assert_eq!(flow.buy_cost, 1_000_500, "买入成本 = 成交额 + 费用");
        assert_eq!(flow.cash_flow, -1_000_500, "买入是负数");
    }

    #[test]
    fn a_sell_only_moves_the_cash_flow_and_keeps_the_buy_cost() {
        let flow = round_flow(&[
            money_trade(consts::STOCK_TRADE_OPEN, 100, 1_000_000, 500),
            money_trade(consts::STOCK_TRADE_CLOSE, 100, 1_100_000, 600),
        ]);
        // 清仓把轮次收尾 → 当前轮为空（见 current_round_trades 的口径）
        assert_eq!(flow, RoundFlow::default(), "清仓之后当前轮没有成交");
    }

    /// 减仓（没清完）留一轮里：买入成本保留，卖出只加现金。
    #[test]
    fn a_partial_round_mixes_both_directions() {
        let flow = round_flow(&[
            money_trade(consts::STOCK_TRADE_OPEN, 200, 2_000_000, 1_000),
            money_trade(consts::STOCK_TRADE_REDUCE, 100, 1_200_000, 700),
        ]);
        assert_eq!(flow.buy_cost, 2_001_000);
        assert_eq!(flow.cash_flow, -2_001_000 + (1_200_000 - 700));
    }

    #[test]
    fn an_empty_round_has_a_zero_flow() {
        assert_eq!(round_flow(&[]), RoundFlow::default());
    }

    /// 本轮的费用设置 = 本轮第一笔（建仓单）上的快照。
    #[test]
    fn the_round_fee_comes_from_the_opening_trade() {
        let snapshot = RoundFee {
            commission_rate: 0.00005,
            min_commission: 0,
            stamp_duty_rate: 0.0,
            transfer_fee_rate: 0.0,
        };
        let opening = StockTrade {
            round_fee: Some(snapshot.clone()),
            ..trade(consts::STOCK_TRADE_OPEN, 100)
        };
        // 加仓那一笔也写着同一份（服务层写入时原样带上），取首笔即可
        let adding = StockTrade {
            round_fee: Some(snapshot.clone()),
            ..trade(consts::STOCK_TRADE_ADD, 100)
        };
        assert_eq!(
            current_round_fee(&[opening.clone(), adding]),
            Some(snapshot.clone())
        );
        // 没有记录（老数据）→ None，调用方按系统配置处理
        assert_eq!(
            current_round_fee(&[trade(consts::STOCK_TRADE_OPEN, 100)]),
            None
        );
        assert_eq!(current_round_fee(&[]), None);
    }

    /// 清仓之后，上一轮的快照不能再当"本轮"用。
    #[test]
    fn a_closed_round_does_not_leak_its_fee() {
        let snapshot = RoundFee {
            commission_rate: 0.0003,
            min_commission: 500,
            stamp_duty_rate: 0.0005,
            transfer_fee_rate: 0.00001,
        };
        let closed = vec![
            StockTrade {
                round_fee: Some(snapshot),
                ..trade(consts::STOCK_TRADE_OPEN, 100)
            },
            trade(consts::STOCK_TRADE_CLOSE, 100),
        ];
        assert_eq!(current_round_fee(&closed), None, "清仓后本轮为空");
    }

    #[test]
    fn round_numbers_start_from_one_after_the_completed_ones() {
        assert_eq!(next_round_no(0, 0), 1);
        assert_eq!(next_round_no(3, 0), 4);
        assert_eq!(next_round_no(3, 1), 5, "重放时按序号往后排");
    }
    #[test]
    fn open_and_add_stay_in_the_current_round() {
        let round = current_round_trades(&[
            trade(consts::STOCK_TRADE_OPEN, 100),
            trade(consts::STOCK_TRADE_ADD, 50),
        ]);
        assert_eq!(round.len(), 2);
        assert_eq!(round[1].shares, 50);
    }

    /// 减仓没减到 0：仍算这一轮的成交（这正是"减仓不算新轮次"那条口径）。
    #[test]
    fn a_partial_reduce_is_still_part_of_the_round() {
        let round = current_round_trades(&[
            trade(consts::STOCK_TRADE_OPEN, 100),
            trade(consts::STOCK_TRADE_REDUCE, 40),
        ]);
        assert_eq!(round.len(), 2);
    }

    /// 清仓把轮次收尾：结果清空，**清仓那笔本身也不留下**。
    #[test]
    fn a_full_close_clears_the_round_and_does_not_keep_the_close_itself() {
        let round = current_round_trades(&[
            trade(consts::STOCK_TRADE_OPEN, 100),
            trade(consts::STOCK_TRADE_CLOSE, 100),
        ]);
        assert!(round.is_empty());
    }

    /// 清仓之后的新建仓才是新的一轮。
    #[test]
    fn the_next_open_starts_a_new_round() {
        let round = current_round_trades(&[
            trade(consts::STOCK_TRADE_OPEN, 100),
            trade(consts::STOCK_TRADE_CLOSE, 100),
            trade(consts::STOCK_TRADE_OPEN, 200),
            trade(consts::STOCK_TRADE_ADD, 100),
        ]);
        assert_eq!(round.len(), 2);
        assert_eq!(round[0].shares, 200, "只留清仓之后那两笔");
    }

    /// 超出持仓的减仓（数据订正/回滚可能造出来）按"归零"处理，不会把股数带成负数。
    #[test]
    fn an_over_reduce_is_clamped_to_zero() {
        let round = current_round_trades(&[
            trade(consts::STOCK_TRADE_OPEN, 100),
            trade(consts::STOCK_TRADE_REDUCE, 300),
        ]);
        assert!(round.is_empty());
    }

    #[test]
    fn unknown_trade_types_are_ignored() {
        let round =
            current_round_trades(&[trade("whatever", 100), trade(consts::STOCK_TRADE_OPEN, 100)]);
        assert_eq!(round.len(), 1);
    }

    #[test]
    fn an_empty_stream_has_no_current_round() {
        assert!(current_round_trades(&[]).is_empty());
    }

    #[test]
    fn a_lot_is_a_hundred_shares() {
        assert_eq!(SHARES_PER_LOT, 100);
        assert_eq!(shares_of(0), 0);
        assert_eq!(shares_of(1), 100);
        assert_eq!(shares_of(3), 300);
        assert_eq!(shares_of(-2), -200, "允许负数（回滚/订正路径会用到）");
    }

    #[test]
    fn the_amount_is_price_times_shares() {
        // 贵州茅台 1700.00 元/股 = 1,700,000 厘/股，1 手 = 100 股 → 17,000,000 分 = 170,000.00 元
        assert_eq!(amount_of(1_700_000, shares_of(1)), 17_000_000);
        assert_eq!(amount_of(1_700_000, shares_of(10)), 170_000_000);
        assert_eq!(amount_of(0, shares_of(5)), 0);
        assert_eq!(amount_of(1_700_000, 0), 0);
    }

    /// 场内基金的 0.001 元报价：厘装得下，成交额仍是整数分。
    #[test]
    fn a_fund_price_with_milli_digits_lands_on_whole_cents() {
        // 沪深300ETF 4.389 元/份 = 4389 厘 → 1 手（100 份）= 438.90 元 = 43,890 分
        assert_eq!(amount_of_lots(4_389, 1), 43_890);
        assert_eq!(amount_of_lots(4_389, 3), 131_670);
        // 零股要四舍五入到分：3.053 元/份 × 7 份 = 21.371 元 → 21.37 元
        assert_eq!(amount_of(3_053, 7), 2_137);
        // 恰好半数（…5 厘·股）向上进：0.0005 元 = 5 厘，1 份 → 0.5 分 → 1 分
        assert_eq!(amount_of(5, 1), 1);
        assert_eq!(amount_of(4, 1), 0);
    }

    /// 两个入口给同一个答案（`amount_of_lots` 就是"先折股再乘"的糖）。
    #[test]
    fn the_two_amount_entries_agree() {
        for (price_milli, lots) in [(1_700_000_i64, 1_i64), (1_700_000, 10), (4_389, 3), (0, 7)] {
            assert_eq!(
                amount_of_lots(price_milli, lots),
                amount_of(price_milli, shares_of(lots)),
                "price_milli={price_milli} lots={lots}"
            );
        }
    }

    /// 分到分的整数运算：不引入浮点（金额恒为整数分是硬契约）。
    #[test]
    fn the_arithmetic_stays_in_integer_cents() {
        // 36.61 元/股 = 36,610 厘 × 200 股 = 7322.00 元
        assert_eq!(amount_of_lots(36_610, 2), 732_200);
    }

    /// 用户真实数据（博敏电子 603936，2026-10-08）：市值 3790.00 + 本轮资金变动 −4808.28
    /// = 浮动盈亏 −1018.28，率按净投入算 −21.18%。
    ///
    /// 修前分母是**本轮买入总额** 16776.16（四次建仓/加仓的成交额 + 费用，减仓回款不冲抵），
    /// 于是同一个 −1018.28 只显示成 −6.07% —— 亏损率被摊薄了三倍多（用户报的缺陷）。
    #[test]
    fn floating_rate_divides_by_the_net_invested_capital() {
        assert_eq!(floating_pnl_rate(-101_828, -480_828), Some(-21.18));
        // 老分母（本轮买入总额 16776.16 分，正数）算出来只有 −6.07%：同一个 −1018.28，
        // 亏损率被摊薄三倍多 —— 两个分母给出的数必须不是一个量级，这条顺手钉住"改的不是无关紧要的位"。
        let buy_cost = 1_677_616_f64;
        assert_eq!(
            ((-101_828_f64 / buy_cost * 10_000.0).round() / 100.0),
            -6.07
        );
    }

    #[test]
    fn floating_rate_keeps_the_sign_of_a_profit() {
        // 净投入 500.00，浮动盈亏 +100.00 → +20%
        assert_eq!(floating_pnl_rate(10_000, -50_000), Some(20.0));
        // 四舍五入到两位小数（与 round_pnl 的盈亏率同一口径）
        assert!(matches!(floating_pnl_rate(1, -3), Some(rate) if (rate - 33.33).abs() < 1e-9));
    }

    /// 卖出回款已超过买入支出（净投入 ≤ 0）时没有可算的比率：界面显示 `—`，不显示一个"负分母"的率。
    #[test]
    fn floating_rate_is_none_when_the_round_took_out_more_than_it_put_in() {
        assert_eq!(floating_pnl_rate(10_000, 0), None);
        assert_eq!(floating_pnl_rate(10_000, 50_000), None);
    }
}
