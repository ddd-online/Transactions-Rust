//! 股票域 DTO。
//!
//! 命名全部为 camelCase。金额单位一律为**分**；
//! 百分比与比率以「小数百分比/倍数」表示（例如 12.5 表示 12.5%）。

use serde::{Deserialize, Serialize};

use crate::consts;
use crate::models::{StockFundRecord, StockPosition, StockTrade};

/// 股票账户总览。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockOverviewDto {
    /// 本金（分）
    pub principal: i64,
    /// 可用现金（账户现金余额）
    #[serde(rename = "availableCash")]
    pub available_cash: i64,
    /// 持仓市值 = Σ（最新价×股数）；行情缺失部分按持仓成本计入
    #[serde(rename = "positionMarketValue")]
    pub position_market_value: i64,
    /// 累计支取（Σ 支取事件金额）
    #[serde(rename = "withdrawnTotal")]
    pub withdrawn_total: i64,
    /// 累计利息归本（Σ 利息归本事件金额）
    #[serde(rename = "interestTotal")]
    pub interest_total: i64,
    /// 总资产 = 可用现金 + 持仓市值
    #[serde(rename = "totalAssets")]
    pub total_assets: i64,
    /// 已实现总盈亏（Σ 卖出净盈亏，分）
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: i64,
    /// 浮动盈亏（分）= Σ（最新价×股数 − 持仓总成本），行情缺失部分为 0
    #[serde(rename = "unrealizedPnl")]
    pub unrealized_pnl: i64,
    /// 本次行情获取失败的持仓数量
    #[serde(rename = "quoteFailedCount")]
    pub quote_failed_count: i64,
    /// 总盈亏占本金百分比（%）
    #[serde(rename = "totalPnlPercent")]
    pub total_pnl_percent: f64,
}

/// 资金变化记录。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFundRecordDto {
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "recordDate")]
    pub record_date: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    #[serde(rename = "eventText")]
    pub event_text: String,
    #[serde(rename = "amountChange")]
    pub amount_change: i64,
    #[serde(rename = "cashBalance")]
    pub cash_balance: i64,
    /// 非卖出事件为 null（该字段不省略，null 会被序列化出来）
    #[serde(rename = "netPnl")]
    pub net_pnl: Option<i64>,
    pub remark: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

impl From<&StockFundRecord> for StockFundRecordDto {
    fn from(record: &StockFundRecord) -> Self {
        Self {
            id: record.id.clone(),
            ledger_id: record.ledger_id.clone(),
            record_date: record.record_date.clone(),
            event_type: record.event_type.clone(),
            event_text: record.event_text.clone(),
            amount_change: record.amount_change,
            cash_balance: record.cash_balance,
            net_pnl: record.net_pnl,
            remark: record.remark.clone(),
            created_at: record.created_at,
        }
    }
}

/// 交易标签设置（可用标签有序列表 + 默认标签）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeTagSettingDto {
    pub tags: Vec<String>,
    /// 默认标签「分析」，始终存在且不可删除
    #[serde(rename = "defaultTag")]
    pub default_tag: String,
}

/// 保存交易标签设置的请求体。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeTagSettingRequest {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    pub tags: Vec<String>,
}

/// 资金变化记录分页结果。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFundRecordPage {
    pub items: Vec<StockFundRecordDto>,
    pub total: i64,
    pub page: i32,
    #[serde(rename = "pageSize")]
    pub page_size: i32,
}

/// 股票持仓。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockPositionDto {
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// 持仓数量（股）
    pub quantity: i64,
    /// 持仓总成本（分，含买入手续费）
    #[serde(rename = "totalCost")]
    pub total_cost: i64,
    /// 该股累计已实现盈亏（分）
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: i64,
    /// 本轮复盘
    pub review: String,
    /// 最新价（分/股），行情获取失败时省略
    #[serde(rename = "latestPrice", skip_serializing_if = "Option::is_none")]
    pub latest_price: Option<i64>,
    /// 昨收价（分/股）
    #[serde(rename = "prevClose", skip_serializing_if = "Option::is_none")]
    pub prev_close: Option<i64>,
    /// 行情时间（Unix 秒）
    #[serde(rename = "quoteTime", skip_serializing_if = "Option::is_none")]
    pub quote_time: Option<i64>,
}

impl From<&StockPosition> for StockPositionDto {
    fn from(position: &StockPosition) -> Self {
        Self {
            id: position.id.clone(),
            ledger_id: position.ledger_id.clone(),
            stock_code: position.stock_code.clone(),
            stock_name: position.stock_name.clone(),
            quantity: position.quantity,
            total_cost: position.total_cost,
            realized_pnl: position.realized_pnl,
            review: position.review.clone(),
            latest_price: None,
            prev_close: None,
            quote_time: None,
        }
    }
}

/// 股票名称查询结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockNameDto {
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
}

/// 外部行情（金额单位：分）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockQuoteDto {
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    /// 最新价（分/股）
    #[serde(rename = "latestPrice")]
    pub latest_price: i64,
    /// 昨收价（分/股）
    #[serde(rename = "prevClose")]
    pub prev_close: i64,
    /// 行情时间（Unix 秒）
    #[serde(rename = "quoteTime")]
    pub quote_time: i64,
}

/// 股票交易记录。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeDto {
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    #[serde(rename = "tradeType")]
    pub trade_type: String,
    #[serde(rename = "roundId")]
    pub round_id: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "orderSeq")]
    pub order_seq: i64,
    pub price: i64,
    pub lots: i64,
    pub shares: i64,
    pub amount: i64,
    pub fee: i64,
    pub commission: i64,
    #[serde(rename = "stampDuty")]
    pub stamp_duty: i64,
    #[serde(rename = "transferFee")]
    pub transfer_fee: i64,
    /// 非卖出为 null（该字段不省略）
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: Option<i64>,
    #[serde(rename = "tradeTime")]
    pub trade_time: i64,
    pub remark: String,
}

impl From<&StockTrade> for StockTradeDto {
    fn from(trade: &StockTrade) -> Self {
        Self {
            id: trade.id.clone(),
            ledger_id: trade.ledger_id.clone(),
            stock_code: trade.stock_code.clone(),
            stock_name: trade.stock_name.clone(),
            trade_type: trade.trade_type.clone(),
            round_id: trade.round_id.clone(),
            order_id: trade.order_id.clone(),
            order_seq: trade.order_seq,
            price: trade.price,
            lots: trade.lots,
            shares: trade.shares,
            amount: trade.amount,
            fee: trade.fee,
            commission: trade.commission,
            stamp_duty: trade.stamp_duty,
            transfer_fee: trade.transfer_fee,
            realized_pnl: trade.realized_pnl,
            trade_time: trade.trade_time,
            remark: trade.remark.clone(),
        }
    }
}

/// 编辑/删除交易后会失效的轮次（该轮复盘随之丢失）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeImpactRoundDto {
    #[serde(rename = "roundId")]
    pub round_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    #[serde(rename = "roundNo")]
    pub round_no: i64,
    pub tag: String,
    #[serde(rename = "hasReview")]
    pub has_review: bool,
}

/// 交易编辑/删除前的影响预演结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeImpactDto {
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// 该股变动后的持仓股数
    #[serde(rename = "positionAfter")]
    pub position_after: i64,
    /// 变动后的可用现金（分）
    #[serde(rename = "cashAfter")]
    pub cash_after: i64,
    /// 会因此失效的轮次
    #[serde(rename = "removedRounds")]
    pub removed_rounds: Vec<StockTradeImpactRoundDto>,
}

/// 股票交易历史集合（左栏列表项）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeHistoryDto {
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// 已完成轮次数
    #[serde(rename = "roundCount")]
    pub round_count: i64,
    /// 该股累计已实现盈亏（分）
    #[serde(rename = "totalPnl")]
    pub total_pnl: i64,
    /// 累计盈亏率（%，相对全部建仓成本）
    #[serde(rename = "totalPnlRate")]
    pub total_pnl_rate: f64,
    #[serde(rename = "lastClosedAt")]
    pub last_closed_at: i64,
    #[serde(rename = "latestPrice", skip_serializing_if = "Option::is_none")]
    pub latest_price: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 一次完整轮次：从建仓到清仓的全部交易 + 本轮盈亏。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeRoundDto {
    pub id: String,
    #[serde(rename = "historyId")]
    pub history_id: String,
    #[serde(rename = "roundNo")]
    pub round_no: i64,
    #[serde(rename = "openedAt")]
    pub opened_at: i64,
    #[serde(rename = "closedAt")]
    pub closed_at: i64,
    pub tag: String,
    pub review: String,
    /// 本轮盈亏（分）
    pub pnl: i64,
    /// 本轮盈亏率（%）
    #[serde(rename = "pnlRate")]
    pub pnl_rate: f64,
    #[serde(rename = "tradeCount")]
    pub trade_count: i64,
    pub trades: Vec<StockTradeDto>,
}

/// 单只股票的交易历史详情（右栏）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeHistoryDetailDto {
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    #[serde(rename = "roundCount")]
    pub round_count: i64,
    #[serde(rename = "totalPnl")]
    pub total_pnl: i64,
    #[serde(rename = "totalPnlRate")]
    pub total_pnl_rate: f64,
    /// 盈利轮数
    #[serde(rename = "winCount")]
    pub win_count: i64,
    /// 亏损轮数
    #[serde(rename = "lossCount")]
    pub loss_count: i64,
    #[serde(rename = "lastClosedAt")]
    pub last_closed_at: i64,
    pub rounds: Vec<StockTradeRoundDto>,
}

/// 交易历史总览。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeHistorySummaryDto {
    /// 已清仓股票数
    #[serde(rename = "stockCount")]
    pub stock_count: i64,
    #[serde(rename = "roundCount")]
    pub round_count: i64,
    #[serde(rename = "winCount")]
    pub win_count: i64,
    #[serde(rename = "lossCount")]
    pub loss_count: i64,
    #[serde(rename = "totalPnl")]
    pub total_pnl: i64,
    #[serde(rename = "totalPnlRate")]
    pub total_pnl_rate: f64,
}

/// 一条可回滚的操作记录（列表用；`kind` 与 `targetId` 是内部字段，不对外暴露）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockOperationDto {
    #[serde(rename = "id")]
    pub id: String,
    /// 操作名（固定文案：追加本金 / 支取 / 利息归本 / 建仓 / 加仓 / 减仓 / 清仓）
    #[serde(rename = "action")]
    pub action: String,
    /// 展示摘要（如 `¥5,000.00`、`贵州茅台 600519 · 3 手 · ¥30,000.00`）
    #[serde(rename = "detail")]
    pub detail: String,
    /// 录入时间（Unix 秒）
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// 回滚预演：要撤销的那次操作 + 它会带来的影响。
///
/// 「失效轮次」沿用编辑/删除成交那一套（[`StockTradeImpactRoundDto`]）：
/// 回滚清仓/减仓会让该轮次不再成立，轮次上的复盘与标签随之丢失 —— 确认框要先把这件事说清楚。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockOperationRollbackPreviewDto {
    /// 要撤销的操作名（无记录时为空串）
    #[serde(rename = "action")]
    pub action: String,
    #[serde(rename = "detail")]
    pub detail: String,
    /// 会因此失效的轮次（复盘随之丢失）；资金类操作恒为空
    #[serde(rename = "removedRounds")]
    pub removed_rounds: Vec<StockTradeImpactRoundDto>,
}

/// 回滚结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockOperationRollbackDto {
    /// 被撤销的操作名
    #[serde(rename = "action")]
    pub action: String,
    /// 撤销目标已被别的改动删除 → 只把记录弹掉，没有实际回滚
    #[serde(rename = "skipped")]
    pub skipped: bool,
}

/// 交易统计总览：本金 + 逐笔结算统计点。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockStatisticsDto {
    /// 当前本金（分）
    pub principal: i64,
    /// 已结算笔数（全部已完成轮次）
    #[serde(rename = "roundCount")]
    pub round_count: i64,
    /// 第 1 笔起的统计点（无结算时为空）
    pub points: Vec<StockStatisticsPointDto>,
}

/// 一个结算统计点：截至第 N 笔清仓的累计口径指标。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockStatisticsPointDto {
    /// 全局结算序号（第 N 笔）
    pub sequence: i64,
    /// 本统计点的结算时间（该笔清仓时间，Unix 秒）
    #[serde(rename = "closedAt")]
    pub closed_at: i64,
    #[serde(rename = "stockCode")]
    pub stock_code: String,
    #[serde(rename = "stockName")]
    pub stock_name: String,
    /// 该股第几轮（该股自己的轮次序号）
    #[serde(rename = "stockRoundNo")]
    pub stock_round_no: i64,
    pub tag: String,
    /// 本笔盈亏（分）
    pub pnl: i64,
    /// 本笔盈亏率（%）
    #[serde(rename = "pnlRate")]
    pub pnl_rate: f64,
    /// 本笔包含的成交笔数
    #[serde(rename = "tradeCount")]
    pub trade_count: i64,
    /// 累计盈亏（分）
    #[serde(rename = "totalPnl")]
    pub total_pnl: i64,
    #[serde(rename = "winCount")]
    pub win_count: i64,
    #[serde(rename = "lossCount")]
    pub loss_count: i64,
    /// 胜率（%）
    #[serde(rename = "winRate")]
    pub win_rate: f64,
    /// 平均盈利（分）
    #[serde(rename = "avgWin")]
    pub avg_win: i64,
    /// 平均亏损（分，正数）
    #[serde(rename = "avgLoss")]
    pub avg_loss: i64,
    /// 实际盈亏比（平均盈利 ÷ 平均亏损），尚无亏损样本时为 null
    #[serde(rename = "pnlRatio")]
    pub pnl_ratio: Option<f64>,
    /// 期望值（分/笔）
    pub expectancy: i64,
    /// 最大回撤（分）
    #[serde(rename = "maxDrawdown")]
    pub max_drawdown: i64,
    /// 最大回撤占当时本金比例（%）
    #[serde(rename = "maxDrawdownPct")]
    pub max_drawdown_pct: f64,
}

/// 由一轮交易推导本轮盈亏与盈亏率（不存储冗余派生值）。
///
/// 买入成本 = Σ(成交金额 + 费用)；卖出净额 = Σ(成交金额 − 费用)；盈亏 = 卖出净额 − 买入成本。
/// 盈亏率按买入成本计算，四舍五入到小数点后两位（`round(x * 10000) / 100`）。
pub fn round_pnl(trades: &[StockTrade]) -> (i64, f64, i64) {
    let mut pnl = 0_i64;
    let mut buy_cost = 0_i64;
    for trade in trades {
        match trade.trade_type.as_str() {
            consts::STOCK_TRADE_OPEN | consts::STOCK_TRADE_ADD => {
                buy_cost += trade.amount + trade.fee;
            }
            consts::STOCK_TRADE_REDUCE | consts::STOCK_TRADE_CLOSE => {
                pnl += trade.amount - trade.fee;
            }
            _ => {}
        }
    }
    pnl -= buy_cost;
    let pnl_rate = if buy_cost > 0 {
        ((pnl as f64 / buy_cost as f64) * 10_000.0).round() / 100.0
    } else {
        0.0
    };
    (pnl, pnl_rate, buy_cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(trade_type: &str, amount: i64, fee: i64) -> StockTrade {
        StockTrade {
            trade_type: trade_type.to_string(),
            amount,
            fee,
            ..StockTrade::default()
        }
    }

    #[test]
    fn round_pnl_subtracts_buy_cost_including_fees() {
        // 建仓 10000 分（费用 5）+ 清仓 11000 分（费用 6）
        let trades = vec![
            trade(consts::STOCK_TRADE_OPEN, 10_000, 5),
            trade(consts::STOCK_TRADE_CLOSE, 11_000, 6),
        ];
        let (pnl, rate, buy_cost) = round_pnl(&trades);
        assert_eq!(buy_cost, 10_005);
        assert_eq!(pnl, 11_000 - 6 - 10_005);
        assert_eq!(rate, ((pnl as f64 / 10_005.0) * 10_000.0).round() / 100.0);
    }

    #[test]
    fn round_pnl_handles_reduce_as_sell() {
        let trades = vec![
            trade(consts::STOCK_TRADE_OPEN, 10_000, 0),
            trade(consts::STOCK_TRADE_ADD, 5_000, 0),
            trade(consts::STOCK_TRADE_REDUCE, 6_000, 0),
            trade(consts::STOCK_TRADE_CLOSE, 10_000, 0),
        ];
        let (pnl, _rate, buy_cost) = round_pnl(&trades);
        assert_eq!(buy_cost, 15_000);
        assert_eq!(pnl, 16_000 - 15_000);
    }

    #[test]
    fn round_pnl_rate_is_zero_without_buy_cost() {
        let trades = vec![trade(consts::STOCK_TRADE_CLOSE, 1_000, 0)];
        let (pnl, rate, buy_cost) = round_pnl(&trades);
        assert_eq!(buy_cost, 0);
        assert_eq!(pnl, 1_000);
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn overview_dto_uses_camel_case_keys() {
        let value = serde_json::to_value(StockOverviewDto::default()).unwrap();
        assert!(value.get("availableCash").is_some());
        assert!(value.get("totalPnlPercent").is_some());
        assert!(value.get("interestTotal").is_some());
    }

    #[test]
    fn position_dto_omits_missing_quote_fields() {
        let value = serde_json::to_value(StockPositionDto::default()).unwrap();
        assert!(value.get("latestPrice").is_none());
        assert!(value.get("prevClose").is_none());
        assert!(value.get("quoteTime").is_none());
    }

    #[test]
    fn statistics_point_serializes_null_pnl_ratio() {
        let value = serde_json::to_value(StockStatisticsPointDto::default()).unwrap();
        assert!(value["pnlRatio"].is_null());
        assert!(value.get("maxDrawdownPct").is_some());
    }

    #[test]
    fn fund_record_keeps_nullable_net_pnl() {
        let value = serde_json::to_value(StockFundRecordDto::default()).unwrap();
        assert!(value["netPnl"].is_null());
    }
}
