//! 股票交易模型。
//!
//! 与核心记账模型不同，这些结构体的 JSON 名称是 **camelCase**，
//! 数据库中仍是 snake_case 列名；列映射在 DAO 层显式书写。

use serde::{Deserialize, Serialize};

/// 股票账户（每个账本一个）。表 `tbl_billadm_stock_account`。
/// 本金以整数分存储。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockAccount {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 本金（分）
    #[serde(rename = "principal")]
    pub principal: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 交易费用设置（每个账本一份）。费率以小数存储（万2.354 → 0.0002354）。
/// 表 `tbl_billadm_stock_fee_setting`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFeeSetting {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 佣金费率（万2.354）
    #[serde(rename = "commissionRate")]
    pub commission_rate: f64,
    /// 最低佣金（分/笔）
    #[serde(rename = "minCommission")]
    pub min_commission: i64,
    /// 印花税率（卖出收取，0.05%）
    #[serde(rename = "stampDutyRate")]
    pub stamp_duty_rate: f64,
    /// 过户费率（买卖双向，仅沪市，0.001%）
    #[serde(rename = "transferFeeRate")]
    pub transfer_fee_rate: f64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// `Default` 必须与数据库列默认值一致（万2.354 / 5 元 / 0.05% / 0.001%），
/// 否则"新建费用设置"的返回值会与数据库列默认值不同。
impl Default for StockFeeSetting {
    fn default() -> Self {
        Self {
            id: String::new(),
            ledger_id: String::new(),
            commission_rate: 0.0002354,
            min_commission: 500,
            stamp_duty_rate: 0.0005,
            transfer_fee_rate: 0.00001,
            created_at: 0,
            updated_at: 0,
        }
    }
}

/// 资金变化记录：现金余额链条的来源，当前现金 = 末条记录余额。
/// 表 `tbl_billadm_stock_fund_record`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFundRecord {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "recordDate")]
    pub record_date: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    #[serde(rename = "eventText")]
    pub event_text: String,
    /// 金额变化（分，带符号）
    #[serde(rename = "amountChange")]
    pub amount_change: i64,
    /// 现金余额（分）
    #[serde(rename = "cashBalance")]
    pub cash_balance: i64,
    /// 卖出净盈亏（分），非卖出事件为空
    #[serde(rename = "netPnl")]
    pub net_pnl: Option<i64>,
    #[serde(rename = "remark")]
    pub remark: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// 股票持仓（每笔买卖实时维护，卖出时按总成本比例结转已实现盈亏）。
/// 表 `tbl_billadm_stock_position`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockPosition {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// 持仓数量（股）
    #[serde(rename = "quantity")]
    pub quantity: i64,
    /// 持仓总成本（分）
    #[serde(rename = "totalCost")]
    pub total_cost: i64,
    /// 已实现盈亏（分，该股累计）
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: i64,
    /// 本轮复盘（持仓期间先写，清仓归档到轮次）
    #[serde(rename = "review")]
    pub review: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 股票交易记录（建仓/加仓/减仓/清仓）。单位均为分；`Shares = Lots × 100`。
/// `OrderID`/`OrderSeq` 标识成交所属的委托：一笔委托可包含多笔成交明细。
/// 表 `tbl_billadm_stock_trade`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTrade {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// open / add / reduce / close
    #[serde(rename = "tradeType")]
    pub trade_type: String,
    /// 所属轮次 ID（清仓时挂接到交易历史）
    #[serde(rename = "roundId")]
    pub round_id: String,
    /// 所属委托 ID（同委托多笔成交共用）
    #[serde(rename = "orderId")]
    pub order_id: String,
    /// 委托内第几笔成交（从 1 起）
    #[serde(rename = "orderSeq")]
    pub order_seq: i64,
    /// 成交价（分/股）
    #[serde(rename = "price")]
    pub price: i64,
    /// 手数
    #[serde(rename = "lots")]
    pub lots: i64,
    /// 股数（手数×100）
    #[serde(rename = "shares")]
    pub shares: i64,
    /// 成交金额（分）
    #[serde(rename = "amount")]
    pub amount: i64,
    /// 交易费用（分）
    #[serde(rename = "fee")]
    pub fee: i64,
    #[serde(rename = "commission")]
    pub commission: i64,
    #[serde(rename = "stampDuty")]
    pub stamp_duty: i64,
    #[serde(rename = "transferFee")]
    pub transfer_fee: i64,
    /// 卖出净盈亏（分），仅减仓/清仓非空
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: Option<i64>,
    /// 成交时间（Unix 秒）
    #[serde(rename = "tradeTime")]
    pub trade_time: i64,
    #[serde(rename = "remark")]
    pub remark: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// 股票交易历史集合（每只股票一条）。表 `tbl_billadm_stock_trade_history`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeHistory {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 一次完整的「建仓 → 清仓」轮次。表 `tbl_billadm_stock_trade_round`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeRound {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "historyId")]
    pub history_id: String,
    /// 轮次序号（该股从 1 起）
    #[serde(rename = "roundNo")]
    pub round_no: i64,
    /// 本轮首次建仓时间（Unix 秒）
    #[serde(rename = "openedAt")]
    pub opened_at: i64,
    /// 本轮清仓时间（Unix 秒）
    #[serde(rename = "closedAt")]
    pub closed_at: i64,
    /// 交易标签（分析/打板/尾盘/追涨/蓄力）
    #[serde(rename = "tag")]
    pub tag: String,
    /// 本轮交易复盘
    #[serde(rename = "review")]
    pub review: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// 股票交易标签设置（每个账本一份）。`tags` 是可用标签的有序 JSON 数组，
/// 不参与序列化（对外只通过 DTO 暴露）。
/// 表 `tbl_billadm_stock_trade_tag_setting`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeTagSetting {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(skip)]
    pub tags: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_setting_defaults_match_database_defaults() {
        let setting = StockFeeSetting::default();
        assert_eq!(setting.commission_rate, 0.0002354);
        assert_eq!(setting.min_commission, 500);
        assert_eq!(setting.stamp_duty_rate, 0.0005);
        assert_eq!(setting.transfer_fee_rate, 0.00001);
    }

    #[test]
    fn stock_json_names_are_camel_case_like_go() {
        let trade = serde_json::to_value(StockTrade {
            ledger_id: "l1".into(),
            stock_code: "600519".into(),
            order_id: "o1".into(),
            order_seq: 2,
            trade_time: 99,
            ..StockTrade::default()
        })
        .unwrap();
        assert_eq!(trade["ledgerId"], "l1");
        assert_eq!(trade["stockCode"], "600519");
        assert_eq!(trade["orderId"], "o1");
        assert_eq!(trade["orderSeq"], 2);
        assert_eq!(trade["tradeTime"], 99);
        // 空的可空盈亏字段序列化为 null
        assert!(trade["realizedPnl"].is_null());
    }

    #[test]
    fn tag_setting_tags_field_is_not_exposed() {
        let value = serde_json::to_value(StockTradeTagSetting {
            tags: r#"["分析"]"#.into(),
            ..StockTradeTagSetting::default()
        })
        .unwrap();
        assert!(value.get("tags").is_none());
    }
}
