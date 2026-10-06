//! 成交流水的渲染行：按委托分组、算出父行的汇总（含**加权均价**）。
//!
//! 从 `pages/stock.rs` 搬来（ADR-0001 的第二波）。它是一条有取值的规则：
//!
//! * **分组键** = `orderId`，为空时回落到单笔自己的 `id`（历史数据里可能没有委托号）；
//! * 组内按 `orderSeq` 升序（同一次委托的多笔成交有先后）；
//! * 单笔成交不合并（`is_group = false`，键就是成交 id、价格就是成交价）；
//!   多笔合并成父行（键 `order-{orderId}`，价格是**成交额除股数**的四舍五入）；
//! * 父行的数量 / 金额 / 各项费用是子行之和；`realizedPnl` 全为空则保持 `None`。
//!
//! 前两条从前只活在页面的渲染路径里，改错了要靠截图发现。

use tr_domain::dto::StockTradeDto;

/// 一条渲染行：委托内的多笔成交聚合成父行 + 子行。
#[derive(Debug, Clone, PartialEq)]
pub struct TradeRow {
    /// 行键：单笔 = 成交 id；合并行 = `order-{orderId}`
    pub key: String,
    /// 是不是"合并了多笔成交"的父行
    pub is_group: bool,
    /// 是不是父行下面的子行（由列表拼装时置位）
    pub is_child: bool,
    /// 组内的成交（单笔就是它自己）
    pub trades: Vec<StockTradeDto>,
    pub trade_type: String,
    /// 单笔 = 成交价；合并行 = 加权均价（成交额 / 股数，四舍五入到分）
    pub price: i64,
    pub lots: i64,
    pub amount: i64,
    pub fee: i64,
    pub commission: i64,
    pub stamp_duty: i64,
    pub transfer_fee: i64,
    pub trade_time: i64,
    /// 已实现盈亏：组内全为空则 `None`
    pub realized_pnl: Option<i64>,
}

/// 按 `orderId` 分组（键 = `orderId || id`，组内按 `orderSeq` 升序）。
pub fn group_trades(trades: &[StockTradeDto]) -> Vec<TradeRow> {
    let mut groups: Vec<(String, Vec<StockTradeDto>)> = Vec::new();
    for trade in trades {
        let key = if trade.order_id.is_empty() {
            trade.id.clone()
        } else {
            trade.order_id.clone()
        };
        match groups.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, items)) => items.push(trade.clone()),
            None => groups.push((key, vec![trade.clone()])),
        }
    }

    groups
        .into_iter()
        .map(|(key, mut items)| {
            items.sort_by_key(|trade| trade.order_seq);
            let first = items.first().cloned().unwrap_or_default();
            let shares: i64 = items.iter().map(|trade| trade.shares).sum();
            let amount: i64 = items.iter().map(|trade| trade.amount).sum();
            let fee: i64 = items.iter().map(|trade| trade.fee).sum();
            let commission: i64 = items.iter().map(|trade| trade.commission).sum();
            let stamp_duty: i64 = items.iter().map(|trade| trade.stamp_duty).sum();
            let transfer_fee: i64 = items.iter().map(|trade| trade.transfer_fee).sum();
            let lots: i64 = items.iter().map(|trade| trade.lots).sum();
            let pnl_values: Vec<i64> = items
                .iter()
                .filter_map(|trade| trade.realized_pnl)
                .collect();
            let realized_pnl = if pnl_values.is_empty() {
                None
            } else {
                Some(pnl_values.iter().sum())
            };
            let price = if shares > 0 {
                ((amount as f64) / (shares as f64)).round() as i64
            } else {
                0
            };
            let single = items.len() == 1;
            TradeRow {
                key: if single {
                    first.id.clone()
                } else {
                    format!("order-{key}")
                },
                is_group: !single,
                is_child: false,
                trades: items,
                trade_type: first.trade_type.clone(),
                price: if single { first.price } else { price },
                lots,
                amount,
                fee,
                commission,
                stamp_duty,
                transfer_fee,
                trade_time: first.trade_time,
                realized_pnl,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(id: &str, order_id: &str, order_seq: i64, shares: i64, price: i64) -> StockTradeDto {
        StockTradeDto {
            id: id.to_string(),
            order_id: order_id.to_string(),
            order_seq,
            shares,
            price,
            lots: shares / 100,
            amount: shares * price,
            trade_type: "open".to_string(),
            trade_time: 1_700_000_000 + order_seq,
            ..StockTradeDto::default()
        }
    }

    #[test]
    fn a_single_fill_is_not_grouped() {
        let rows = group_trades(&[fill("t1", "o1", 0, 100, 1234)]);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(!row.is_group);
        assert!(!row.is_child);
        assert_eq!(row.key, "t1", "单笔的键是成交 id");
        assert_eq!(row.price, 1234, "单笔价格就是成交价");
        assert_eq!(row.trades.len(), 1);
        assert_eq!(row.amount, 123_400);
        assert_eq!(row.realized_pnl, None);
    }

    /// 一次委托多笔成交：合并成父行，价格是**成交额 / 股数**的加权均价（四舍五入）。
    #[test]
    fn multiple_fills_of_one_order_merge_into_a_weighted_average_row() {
        let mut a = fill("t1", "o1", 1, 100, 1234);
        a.fee = 5;
        a.commission = 5;
        a.stamp_duty = 0;
        a.transfer_fee = 0;
        a.realized_pnl = Some(10);
        let mut b = fill("t2", "o1", 0, 300, 1250);
        b.fee = 7;
        b.commission = 6;
        b.stamp_duty = 1;
        b.transfer_fee = 0;
        b.realized_pnl = Some(20);

        let rows = group_trades(&[a, b]);
        assert_eq!(rows.len(), 1, "同一个 orderId 只出一行");
        let row = &rows[0];
        assert!(row.is_group);
        assert_eq!(row.key, "order-o1");
        assert_eq!(row.trades.len(), 2);
        assert_eq!(
            row.trades[0].id, "t2",
            "组内按 orderSeq 升序（b 的 seq 更小）"
        );
        assert_eq!(row.trades[1].id, "t1");
        assert_eq!(row.lots, 4, "手数也是求和");
        assert_eq!(row.amount, 100 * 1234 + 300 * 1250);
        assert_eq!(row.fee, 12);
        assert_eq!(row.commission, 11);
        assert_eq!(row.stamp_duty, 1);
        assert_eq!(row.transfer_fee, 0);
        assert_eq!(
            row.price,
            ((100 * 1234 + 300 * 1250) as f64 / 400.0).round() as i64,
            "加权均价"
        );
        assert_eq!(row.realized_pnl, Some(30), "已实现盈亏求和");
        assert_eq!(row.trade_time, 1_700_000_000, "取组内 seq 最小的那笔");
    }

    /// 分组的顺序 = 首次出现的顺序（页面按成交列表的既有顺序渲染）。
    #[test]
    fn groups_keep_their_first_appearance_order() {
        let rows = group_trades(&[
            fill("t1", "o1", 0, 100, 1000),
            fill("t2", "o2", 0, 100, 2000),
            fill("t3", "o1", 1, 100, 1000),
        ]);
        let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["order-o1", "t2"],
            "o1 有两笔（合并行），o2 只有一笔（单笔行键是成交 id）"
        );
    }

    /// 没有委托号（历史数据）时按成交 id 各自成行。
    #[test]
    fn fills_without_an_order_id_stay_apart() {
        let rows = group_trades(&[fill("t1", "", 0, 100, 1000), fill("t2", "", 0, 200, 1000)]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| !row.is_group));
        assert_eq!(rows[0].key, "t1");
        assert_eq!(rows[1].key, "t2");
    }

    /// 只有卖出才有已实现盈亏：全为空 → `None`（页面据此不画那一列）。
    #[test]
    fn realized_pnl_stays_none_unless_some_fill_has_it() {
        let rows = group_trades(&[fill("t1", "o1", 0, 100, 1000)]);
        assert_eq!(rows[0].realized_pnl, None);
    }

    #[test]
    fn an_empty_list_yields_no_rows() {
        assert!(group_trades(&[]).is_empty());
    }
}
