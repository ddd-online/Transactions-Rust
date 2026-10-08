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

/// 一轮（**建仓时**）定下来的四项费用参数 —— 本轮之后的加仓 / 减仓 / 清仓都用它，
/// 不再看账本级的系统配置。
///
/// 存在 `tbl_billadm_stock_trade` 的 4 个**可空**列上（`round_commission_rate` /
/// `round_min_commission` / `round_stamp_duty_rate` / `round_transfer_fee_rate`）：
/// 四项都为空 = 老数据（升级前写的成交），按系统配置 [`StockFeeSetting`] 处理。
/// 「这一轮」的判定与 `round_id` 为空的那批成交一致（`tr_domain::stock::current_round_trades`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoundFee {
    /// 佣金费率（万2.354 → 0.0002354）
    #[serde(rename = "commissionRate")]
    pub commission_rate: f64,
    /// 最低佣金（**分**/笔）
    #[serde(rename = "minCommission")]
    pub min_commission: i64,
    /// 印花税率（0 = 本轮不收）
    #[serde(rename = "stampDutyRate")]
    pub stamp_duty_rate: f64,
    /// 过户费率（0 = 本轮不收）
    #[serde(rename = "transferFeeRate")]
    pub transfer_fee_rate: f64,
}

/// 与 [`StockFeeSetting::default`] 同值：没填过本轮设置时，"默认值"就是系统配置的默认值。
impl Default for RoundFee {
    fn default() -> Self {
        let setting = StockFeeSetting::default();
        Self::from(&setting)
    }
}

impl From<&StockFeeSetting> for RoundFee {
    fn from(setting: &StockFeeSetting) -> Self {
        Self {
            commission_rate: setting.commission_rate,
            min_commission: setting.min_commission,
            stamp_duty_rate: setting.stamp_duty_rate,
            transfer_fee_rate: setting.transfer_fee_rate,
        }
    }
}

impl RoundFee {
    /// 折成费用算法要的形态（[`crate::fee`] 只读这四项；`id` / `ledger_id` 留空）。
    pub fn to_setting(&self) -> StockFeeSetting {
        StockFeeSetting {
            commission_rate: self.commission_rate,
            min_commission: self.min_commission,
            stamp_duty_rate: self.stamp_duty_rate,
            transfer_fee_rate: self.transfer_fee_rate,
            ..StockFeeSetting::default()
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
    /// 成交价（**厘**/股，1/1000 元 —— 场内基金的 0.001 元报价靠它）
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
    /// 这一笔所在的**本轮**费用设置（建仓时定下、本轮内不可改）。
    ///
    /// 四项都为 `Some` 才算记录在案；`None` = 老数据（升级前的成交），按账本系统配置处理。
    #[serde(rename = "roundFee", skip_serializing_if = "Option::is_none")]
    pub round_fee: Option<RoundFee>,
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

/// 可回滚的操作记录（每个账本最多保留最新 10 条）。
/// 表 `tbl_billadm_stock_operation`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockOperation {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// [`tr_domain::consts::STOCK_OP_KIND_FUND`] / [`tr_domain::consts::STOCK_OP_KIND_ORDER`]
    #[serde(rename = "kind")]
    pub kind: String,
    /// 操作名（追加本金 / 支取 / 利息归本 / 建仓 / 加仓 / 减仓 / 清仓）—— 固定文案，改动即影响界面
    #[serde(rename = "action")]
    pub action: String,
    /// 展示摘要（金额 / 股票与手数）
    #[serde(rename = "detail")]
    pub detail: String,
    /// 撤销目标：资金类 = 资金记录 id，委托类 = `order_id`
    #[serde(rename = "targetId")]
    pub target_id: String,
    /// 录入时间（Unix 秒，**单调递增**：同一秒内的多条也能定序，与资金记录同一套 idiom）
    #[serde(rename = "createdAt")]
    pub created_at: i64,
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

    /// 本轮快照与系统配置同口径：默认值必须一致，互转不丢项。
    #[test]
    fn round_fee_mirrors_the_system_setting() {
        let defaults = RoundFee::default();
        assert_eq!(defaults, RoundFee::from(&StockFeeSetting::default()));
        assert_eq!(defaults.commission_rate, 0.0002354);
        assert_eq!(defaults.min_commission, 500);

        let round = RoundFee {
            commission_rate: 0.00005,
            min_commission: 0,
            stamp_duty_rate: 0.0,
            transfer_fee_rate: 0.0,
        };
        // 折给算法的那份只带四项（id / ledger 留空）
        let setting = round.to_setting();
        assert_eq!(setting.commission_rate, 0.00005);
        assert_eq!(setting.min_commission, 0);
        assert_eq!(setting.stamp_duty_rate, 0.0);
        assert_eq!(setting.transfer_fee_rate, 0.0);
        assert!(setting.id.is_empty() && setting.ledger_id.is_empty());
        // 往返（to_setting → from）后四项不变
        assert_eq!(RoundFee::from(&setting), round);
    }

    /// 没有快照的成交不序列化 `roundFee`（老数据的载荷保持原样）。
    #[test]
    fn trade_json_omits_round_fee_when_absent() {
        let legacy = serde_json::to_value(StockTrade::default()).unwrap();
        assert!(legacy.get("roundFee").is_none());

        let with_fee = serde_json::to_value(StockTrade {
            round_fee: Some(RoundFee {
                commission_rate: 0.00005,
                min_commission: 0,
                stamp_duty_rate: 0.0,
                transfer_fee_rate: 0.0,
            }),
            ..StockTrade::default()
        })
        .unwrap();
        assert_eq!(with_fee["roundFee"]["commissionRate"], 0.00005);
        assert_eq!(with_fee["roundFee"]["minCommission"], 0);
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
