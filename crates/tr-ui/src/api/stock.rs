//! 股票域命令封装。字段名以 `crates/tr-ipc/src/commands/stock.rs` 为准
//! （**唯一权威**，逐字照抄）。
//!
//! | 命令 | 入参 |
//! |---|---|
//! | `stock_fee_settings_get` | `{ ledger_id }` |
//! | `stock_fee_settings_put` | `{ ledger_id, commission_rate, min_commission, stamp_duty_rate, transfer_fee_rate }` |
//! | `stock_tag_settings_get` | `{ ledger_id }` |
//! | `stock_tag_settings_put` | `{ ledger_id, tags }`（同时接受 `ledgerId`） |
//! | `stock_reset` | `{ ledger_id }` |
//!
//! ## 命名
//!
//! 股票域的**请求体是 snake_case**（`ledger_id` / `commission_rate`），
//! 而**响应是 camelCase**（`commissionRate` / `minCommission`）—— 两种混用是既成契约，不要统一。
//!
//! ## 单位
//!
//! * 费率是**小数**：万 2.354 → `0.0002354`；0.05% → `0.0005`。
//!   界面按「万分之 x」展示，`x = rate * 10000`（见 `pages/settings.rs` 的换算注释）。
//! * `min_commission` 是**整数分**（5 元 → `500`）。
//!
//! ## 尚未接入的只读命令
//!
//! `stock_overview` / `stock_positions` / `stock_trades` / `stock_history` 等
//! 在 P6-b 已全部接入（见下方「股票页」一节）。

use serde::Serialize;
use tr_domain::dto::{
    StockFundRecordPage, StockNameDto, StockOperationDto, StockOperationRollbackDto,
    StockOperationRollbackPreviewDto, StockOverviewDto, StockPositionDto, StockStatisticsDto,
    StockTradeDto, StockTradeHistoryDetailDto, StockTradeHistoryDto, StockTradeHistorySummaryDto,
    StockTradeImpactDto, StockTradeTagSettingDto,
};
use tr_domain::models::StockFeeSetting;

use crate::ipc::{self, IpcError};

#[derive(Debug, Serialize)]
struct LedgerIdRequest {
    pub ledger_id: String,
}

#[derive(Debug, Serialize)]
struct FeeSettingsRequest {
    pub ledger_id: String,
    pub commission_rate: f64,
    pub min_commission: f64,
    pub stamp_duty_rate: f64,
    pub transfer_fee_rate: f64,
}

#[derive(Debug, Serialize)]
struct TagSettingsRequest {
    pub ledger_id: String,
    pub tags: Vec<String>,
}

/// 读取费用设置（不存在时后端按默认值创建并返回）。
pub async fn fee_settings_get(ledger_id: &str) -> Result<StockFeeSetting, IpcError> {
    ipc::call(
        "stock_fee_settings_get",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 保存费用设置。`commission_rate` 必填；`min_commission` 是**分**。
pub async fn fee_settings_put(
    ledger_id: &str,
    commission_rate: f64,
    min_commission: i64,
    stamp_duty_rate: f64,
    transfer_fee_rate: f64,
) -> Result<StockFeeSetting, IpcError> {
    ipc::call(
        "stock_fee_settings_put",
        FeeSettingsRequest {
            ledger_id: ledger_id.to_string(),
            commission_rate,
            // 后端该字段虽然是 `Option<f64>`，但语义是"分"，整数照传即可
            min_commission: min_commission as f64,
            stamp_duty_rate,
            transfer_fee_rate,
        },
    )
    .await
}

/// 读取可用交易标签（`defaultTag` 恒为「分析」）。
pub async fn tag_settings_get(ledger_id: &str) -> Result<StockTradeTagSettingDto, IpcError> {
    ipc::call(
        "stock_tag_settings_get",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 保存可用交易标签（「分析」不可删除，由后端过滤）。
pub async fn tag_settings_put(
    ledger_id: &str,
    tags: Vec<String>,
) -> Result<StockTradeTagSettingDto, IpcError> {
    ipc::call(
        "stock_tag_settings_put",
        TagSettingsRequest {
            ledger_id: ledger_id.to_string(),
            tags,
        },
    )
    .await
}

/// 清空指定账本的全部股票数据（后端返回 `true`）。
pub async fn reset(ledger_id: &str) -> Result<bool, IpcError> {
    ipc::call(
        "stock_reset",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

// ================================================================ 股票页
//
// P6-b 接入。字段名逐字照抄 `crates/tr-ipc/src/commands/stock.rs`：
// 请求全 snake_case，响应全 camelCase（DTO 里已 rename）。

#[derive(Debug, Serialize)]
struct AmountRequest {
    pub ledger_id: String,
    /// 金额（**分**）
    pub amount: i64,
    /// 发生日期 `YYYY-MM-DD`（空串表示今天）
    pub date: String,
}

/// `stock_fund_records` 的分页参数。
///
/// 后端同时接受数字与数字字符串（`QueryNumber`），这里发数字：
/// `serde_wasm_bindgen` 对 `Option<i64>` 的 `None` 会写成 `undefined`，
/// 后端的 `#[serde(default)]` + `Option` 会把它当"没传"（与不传字段等价）。
#[derive(Debug, Serialize)]
struct FundRecordsRequest {
    pub ledger_id: String,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PositionReviewRequest {
    pub ledger_id: String,
    pub code: String,
    pub review: String,
}

#[derive(Debug, Serialize)]
struct TradesRequest {
    pub ledger_id: String,
    pub stock_code: String,
}

/// 一笔委托内的一笔成交明细（价格单位：**元**，后端负责 ×100）。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TradeFillInput {
    pub price: f64,
    pub lots: i64,
}

impl TradeFillInput {
    pub fn new(price: f64, lots: i64) -> Self {
        Self { price, lots }
    }
}

#[derive(Debug, Serialize)]
struct TradeCreateRequest {
    pub ledger_id: String,
    pub stock_code: String,
    pub stock_name: String,
    pub trade_type: String,
    pub trade_time: i64,
    pub remark: String,
    pub tag: String,
    pub fills: Vec<TradeFillInput>,
}

#[derive(Debug, Serialize)]
struct TradeUpdateRequest {
    pub ledger_id: String,
    pub id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: i64,
    pub trade_time: i64,
}

#[derive(Debug, Serialize)]
struct OrderDeleteRequest {
    pub ledger_id: String,
    /// 后端字段名就是 `order_id`（snake_case）
    pub order_id: String,
}

#[derive(Debug, Serialize)]
struct TradeImpactRequest {
    pub ledger_id: String,
    /// `update_trade` | `delete_order`
    pub action: String,
    pub trade_id: String,
    pub order_id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: i64,
    pub trade_time: i64,
}

#[derive(Debug, Serialize)]
struct RoundReviewRequest {
    pub ledger_id: String,
    pub id: String,
    pub review: String,
}

#[derive(Debug, Serialize)]
struct RoundTagRequest {
    pub ledger_id: String,
    pub id: String,
    pub tag: String,
}

#[derive(Debug, Serialize)]
struct StatisticsRequest {
    pub ledger_id: String,
    pub start_month: String,
    pub end_month: String,
    /// 只发正整数；`None` 表示不筛选（后端对非法值会报 `recent 必须为正整数`）
    pub recent: Option<i64>,
    pub tag: String,
}

#[derive(Debug, Serialize)]
struct StockNameRequest {
    pub stock_code: String,
}

/// 账户总览（本金 / 可用现金 / 持仓市值 / 总资产 / 已实现 / 浮动盈亏 / 行情失败数）。
pub async fn overview(ledger_id: &str) -> Result<StockOverviewDto, IpcError> {
    ipc::call(
        "stock_overview",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 追加本金（**分**，可指定发生日期）。
pub async fn principal_add(
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> Result<StockOverviewDto, IpcError> {
    ipc::call(
        "stock_principal_add",
        AmountRequest {
            ledger_id: ledger_id.to_string(),
            amount,
            date: date.to_string(),
        },
    )
    .await
}

/// 利息归本（**分**，可指定发生日期）。本金不变，只增加可用现金与「累计利息归本」。
pub async fn interest_add(
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> Result<StockOverviewDto, IpcError> {
    ipc::call(
        "stock_interest_add",
        AmountRequest {
            ledger_id: ledger_id.to_string(),
            amount,
            date: date.to_string(),
        },
    )
    .await
}

/// 从股票账户支取（**分**，不得超过可用现金）。
pub async fn withdraw(
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> Result<StockOverviewDto, IpcError> {
    ipc::call(
        "stock_withdraw",
        AmountRequest {
            ledger_id: ledger_id.to_string(),
            amount,
            date: date.to_string(),
        },
    )
    .await
}

/// 资金变化记录分页（默认第 1 页、每页 10 条）。
pub async fn fund_records(
    ledger_id: &str,
    page: i64,
    page_size: i64,
) -> Result<StockFundRecordPage, IpcError> {
    ipc::call(
        "stock_fund_records",
        FundRecordsRequest {
            ledger_id: ledger_id.to_string(),
            page: Some(page),
            page_size: Some(page_size),
        },
    )
    .await
}

/// 持仓列表（只含未清仓股票，后端会挂行情；行情失败时 `latestPrice` 为 `None`）。
pub async fn positions(ledger_id: &str) -> Result<Vec<StockPositionDto>, IpcError> {
    ipc::call(
        "stock_positions",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 保存持仓的「本轮复盘」（500 字以内），返回更新后的持仓。
pub async fn position_review(
    ledger_id: &str,
    code: &str,
    review: &str,
) -> Result<StockPositionDto, IpcError> {
    ipc::call(
        "stock_position_review",
        PositionReviewRequest {
            ledger_id: ledger_id.to_string(),
            code: code.to_string(),
            review: review.to_string(),
        },
    )
    .await
}

/// 某股交易列表（持仓中只返回本轮）。
pub async fn trades(ledger_id: &str, stock_code: &str) -> Result<Vec<StockTradeDto>, IpcError> {
    ipc::call(
        "stock_trades",
        TradesRequest {
            ledger_id: ledger_id.to_string(),
            stock_code: stock_code.to_string(),
        },
    )
    .await
}

/// 记录一笔委托（可含多笔成交明细），返回成交明细数组。
pub async fn trade_create(
    ledger_id: &str,
    stock_code: &str,
    stock_name: &str,
    trade_type: &str,
    trade_time: i64,
    tag: &str,
    fills: Vec<TradeFillInput>,
) -> Result<Vec<StockTradeDto>, IpcError> {
    ipc::call(
        "stock_trade_create",
        TradeCreateRequest {
            ledger_id: ledger_id.to_string(),
            stock_code: stock_code.to_string(),
            stock_name: stock_name.to_string(),
            trade_type: trade_type.to_string(),
            trade_time,
            // 下单弹窗没有备注字段，恒发空串
            remark: String::new(),
            tag: tag.to_string(),
            fills,
        },
    )
    .await
}

/// 编辑一笔成交（按当前费用设置重算整笔委托）。
pub async fn trade_update(
    ledger_id: &str,
    id: &str,
    price_yuan: f64,
    lots: i64,
    trade_time: i64,
) -> Result<StockTradeDto, IpcError> {
    ipc::call(
        "stock_trade_update",
        TradeUpdateRequest {
            ledger_id: ledger_id.to_string(),
            id: id.to_string(),
            price: price_yuan,
            lots,
            trade_time,
        },
    )
    .await
}

/// 删除整笔委托。
pub async fn trade_order_delete(ledger_id: &str, order_id: &str) -> Result<bool, IpcError> {
    ipc::call(
        "stock_trade_order_delete",
        OrderDeleteRequest {
            ledger_id: ledger_id.to_string(),
            order_id: order_id.to_string(),
        },
    )
    .await
}

/// 预演编辑/删除的影响（**不落库**）。
#[allow(clippy::too_many_arguments)]
pub async fn trade_impact(
    ledger_id: &str,
    action: &str,
    trade_id: &str,
    order_id: &str,
    price_yuan: f64,
    lots: i64,
    trade_time: i64,
) -> Result<StockTradeImpactDto, IpcError> {
    ipc::call(
        "stock_trade_impact",
        TradeImpactRequest {
            ledger_id: ledger_id.to_string(),
            action: action.to_string(),
            trade_id: trade_id.to_string(),
            order_id: order_id.to_string(),
            price: price_yuan,
            lots,
            trade_time,
        },
    )
    .await
}

/// 交易历史集合列表（左栏）。
pub async fn history(ledger_id: &str) -> Result<Vec<StockTradeHistoryDto>, IpcError> {
    ipc::call(
        "stock_history",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 单只股票历史详情（右栏）。
pub async fn history_detail(
    ledger_id: &str,
    stock_code: &str,
) -> Result<StockTradeHistoryDetailDto, IpcError> {
    ipc::call(
        "stock_history_detail",
        TradesRequest {
            ledger_id: ledger_id.to_string(),
            stock_code: stock_code.to_string(),
        },
    )
    .await
}

/// 全部股票的交易历史总览。
pub async fn history_summary(ledger_id: &str) -> Result<StockTradeHistorySummaryDto, IpcError> {
    ipc::call(
        "stock_history_summary",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 保存某轮次的交易复盘，返回整份详情。
pub async fn round_review(
    ledger_id: &str,
    id: &str,
    review: &str,
) -> Result<StockTradeHistoryDetailDto, IpcError> {
    ipc::call(
        "stock_round_review",
        RoundReviewRequest {
            ledger_id: ledger_id.to_string(),
            id: id.to_string(),
            review: review.to_string(),
        },
    )
    .await
}

/// 保存某轮次的交易标签，返回整份详情。
pub async fn round_tag(
    ledger_id: &str,
    id: &str,
    tag: &str,
) -> Result<StockTradeHistoryDetailDto, IpcError> {
    ipc::call(
        "stock_round_tag",
        RoundTagRequest {
            ledger_id: ledger_id.to_string(),
            id: id.to_string(),
            tag: tag.to_string(),
        },
    )
    .await
}

/// 逐笔结算统计。
///
/// `recent` 传 `None` 表示"不按最近 N 笔筛选"（**不要传 0**，
/// 后端对 `<= 0` 会返回 `recent 必须为正整数`）。
pub async fn statistics(
    ledger_id: &str,
    start_month: &str,
    end_month: &str,
    recent: Option<i64>,
    tag: &str,
) -> Result<StockStatisticsDto, IpcError> {
    ipc::call(
        "stock_statistics",
        StatisticsRequest {
            ledger_id: ledger_id.to_string(),
            start_month: start_month.to_string(),
            end_month: end_month.to_string(),
            recent: recent.filter(|value| *value > 0),
            tag: tag.to_string(),
        },
    )
    .await
}

/// 查询股票名称（优先本地交易记录，未命中走外部行情；**不需要 ledger_id**）。
pub async fn stock_name(stock_code: &str) -> Result<StockNameDto, IpcError> {
    ipc::call(
        "stock_name",
        StockNameRequest {
            stock_code: stock_code.to_string(),
        },
    )
    .await
}

/// 操作记录列表（最新的在前，最多 10 条）。
pub async fn operation_list(ledger_id: &str) -> Result<Vec<StockOperationDto>, IpcError> {
    ipc::call(
        "stock_operation_list",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 回滚预演（**不落库**）：要撤销哪一次操作、会不会让某些轮次失效。
pub async fn operation_preview(
    ledger_id: &str,
) -> Result<StockOperationRollbackPreviewDto, IpcError> {
    ipc::call(
        "stock_operation_preview",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 回滚最新一次操作。
pub async fn operation_rollback(ledger_id: &str) -> Result<StockOperationRollbackDto, IpcError> {
    ipc::call(
        "stock_operation_rollback",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}
