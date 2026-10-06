//! 股票域的**换算口径**：手 ↔ 股、价格 × 股数 = 成交额（全为整数分）。
//!
//! 从 `tr-service` 里搬来（候选 7 / #37）。收之前这条链在四处各写一遍：
//! `stock/write.rs:103`（`total_lots * 100`）、`:179`（`fill.lots * 100`）、`:675`（`lots * 100`）、
//! `stock.rs:1743`（内联的 `price * lots * 100`）—— 而 `100` 是**A 股的每手股数**，
//! 属于那种"散在代码里、改一次要全文件找"的魔数。
//!
//! 归属说明：这**不是**新的 owner。写入流水线与轮次推导仍归 `stock::write` / `stock`（ADR-0002），
//! 这里只放"手 → 股 → 金额"这类能在 native 上断言的换算，让调用点都走同一份。

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

#[cfg(test)]
mod tests {
    use super::*;

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
