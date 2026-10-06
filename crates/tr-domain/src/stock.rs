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
use crate::models::StockTrade;

/// 每手股数（A 股口径）。
pub const SHARES_PER_LOT: i64 = 100;

/// 手数 → 股数。
pub fn shares_of(lots: i64) -> i64 {
    lots * SHARES_PER_LOT
}

/// 成交额（分）：单价（分/股）× 股数。
pub fn amount_of(price: i64, shares: i64) -> i64 {
    price * shares
}

/// 成交额（分）：单价（分/股）× 手数（内部按 [`SHARES_PER_LOT`] 折股）。
pub fn amount_of_lots(price: i64, lots: i64) -> i64 {
    amount_of(price, shares_of(lots))
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
        // 贵州茅台 1700.00 元/股 = 170000 分/股，1 手 = 100 股 → 17,000,000 分 = 170,000.00 元
        assert_eq!(amount_of(170_000, shares_of(1)), 17_000_000);
        assert_eq!(amount_of(1_700, shares_of(10)), 1_700_000);
        assert_eq!(amount_of(0, shares_of(5)), 0);
        assert_eq!(amount_of(170_000, 0), 0);
    }

    /// 两个入口给同一个答案（`amount_of_lots` 就是"先折股再乘"的糖）。
    #[test]
    fn the_two_amount_entries_agree() {
        for (price, lots) in [(170_000_i64, 1_i64), (1_700, 10), (36_61, 2), (0, 7)] {
            assert_eq!(
                amount_of_lots(price, lots),
                amount_of(price, shares_of(lots)),
                "price={price} lots={lots}"
            );
        }
    }

    /// 分到分的整数运算：不引入浮点（金额恒为整数分是硬契约）。
    #[test]
    fn the_arithmetic_stays_in_integer_cents() {
        // 36.61 元/股 × 200 股 = 7322.00 元
        assert_eq!(amount_of_lots(3_661, 2), 732_200);
    }
}
