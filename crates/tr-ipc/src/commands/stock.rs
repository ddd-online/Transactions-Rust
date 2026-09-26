//! 股票命令。
//!
//! 入参字段名全部是 **snake_case**（`ledger_id` / `stock_code` / `page_size` …），这是固定契约；
//! 路径/查询形态的参数一律搬进 `req`（Tauri 命令只有一个入参），
//! 其中 `code` / `id` / `orderId` 保留大小写原样。
//!
//! **`recent`**：从请求里取字符串再解析，非正整数报
//! `recent 必须为正整数`。这里用 `Option<String>` 保留"显式传了非法值"与"没传"的差异。
//!
//! **价格**：`(price_yuan * 100.0).round() as i64`。**不用** `money::yuan_to_cents`——它的入参是
//! 字符串且小数第三位进位规则不同，会改变边界行为。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{
    StockFundRecordPage, StockNameDto, StockOperationDto, StockOperationRollbackDto,
    StockOperationRollbackPreviewDto, StockOverviewDto, StockPositionDto, StockStatisticsDto,
    StockTradeDto, StockTradeHistoryDetailDto, StockTradeHistoryDto, StockTradeHistorySummaryDto,
    StockTradeImpactDto, StockTradeTagSettingDto,
};
use tr_domain::error::AppError;
use tr_domain::models::StockFeeSetting;
use tr_service::stock::{self, TradeFill};

use crate::error::{ApiError, ApiResult};
use crate::AppState;

use super::require_ledger_id;

/// 价格（元）→ 分，四舍五入到整数分。
fn yuan_to_price_cents(price_yuan: f64) -> i64 {
    (price_yuan * 100.0).round() as i64
}

// ---------- 账户 / 费用设置 ----------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockOverviewRequest {
    pub ledger_id: String,
}

/// 股票账户总览（可用现金 = 本金 + 已实现盈亏 − 累计支取 − 持仓成本）。
#[tauri::command]
pub fn stock_overview(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockOverviewDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::get_overview(&workspace, &req.ledger_id)?)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockAmountDateRequest {
    pub ledger_id: String,
    /// 金额（**分**，整数）
    pub amount: Option<i64>,
    /// 可选的发生日期 `YYYY-MM-DD`
    pub date: String,
}

/// 追加本金（可指定发生日期）。
#[tauri::command]
pub fn stock_principal_add(
    state: State<'_, AppState>,
    req: StockAmountDateRequest,
) -> ApiResult<StockOverviewDto> {
    require_ledger_id(&req.ledger_id)?;
    let amount = req
        .amount
        .ok_or_else(|| ApiError::from(AppError::bad_request("amount is required")))?;
    let workspace = state.workspace()?;
    Ok(stock::add_principal_at_date(
        &workspace,
        &req.ledger_id,
        amount,
        &req.date,
    )?)
}

/// 利息归本（账户利息计入可用现金；本金不变，可指定发生日期）。
#[tauri::command]
pub fn stock_interest_add(
    state: State<'_, AppState>,
    req: StockAmountDateRequest,
) -> ApiResult<StockOverviewDto> {
    require_ledger_id(&req.ledger_id)?;
    let amount = req
        .amount
        .ok_or_else(|| ApiError::from(AppError::bad_request("amount is required")))?;
    let workspace = state.workspace()?;
    Ok(stock::add_interest_at_date(
        &workspace,
        &req.ledger_id,
        amount,
        &req.date,
    )?)
}

/// 从股票账户支取（本金不变；不得超过可用现金）。
#[tauri::command]
pub fn stock_withdraw(
    state: State<'_, AppState>,
    req: StockAmountDateRequest,
) -> ApiResult<StockOverviewDto> {
    require_ledger_id(&req.ledger_id)?;
    let amount = req
        .amount
        .ok_or_else(|| ApiError::from(AppError::bad_request("amount is required")))?;
    let workspace = state.workspace()?;
    Ok(stock::add_withdraw_at_date(
        &workspace,
        &req.ledger_id,
        amount,
        &req.date,
    )?)
}

/// 读取费用设置（不存在时按默认值创建并返回）。
#[tauri::command]
pub fn stock_fee_settings_get(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockFeeSetting> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::get_or_create_fee_setting(
        &workspace,
        &req.ledger_id,
    )?)
}

/// 保存费用设置。`commission_rate` 必填（缺省即 0 → 触发"必须大于 0"的错误）；
/// 其余三项缺省为 0。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockFeeSettingsRequest {
    pub ledger_id: String,
    /// 佣金费率（小数：万2.354 → 0.0002354）
    pub commission_rate: Option<f64>,
    /// 最低佣金（**分**）
    pub min_commission: Option<f64>,
    pub stamp_duty_rate: Option<f64>,
    pub transfer_fee_rate: Option<f64>,
}

/// 保存佣金/最低佣金/印花税/过户费。
#[tauri::command]
pub fn stock_fee_settings_put(
    state: State<'_, AppState>,
    req: StockFeeSettingsRequest,
) -> ApiResult<StockFeeSetting> {
    require_ledger_id(&req.ledger_id)?;
    // `commission_rate` 缺失时直接报 `commission_rate is required`
    let Some(commission_rate) = req.commission_rate else {
        return Err(AppError::bad_request("commission_rate is required").into());
    };
    let workspace = state.workspace()?;
    Ok(stock::save_fee_settings(
        &workspace,
        &req.ledger_id,
        commission_rate,
        req.min_commission.unwrap_or(0.0) as i64,
        req.stamp_duty_rate.unwrap_or(0.0),
        req.transfer_fee_rate.unwrap_or(0.0),
    )?)
}

// ---------- 标签设置 ----------

/// 读取可用交易标签设置。
#[tauri::command]
pub fn stock_tag_settings_get(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockTradeTagSettingDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::get_trade_tags_dto(&workspace, &req.ledger_id)?)
}

/// 保存可用交易标签（「分析」不可删除）。字段名是 `ledger_id` / `tags`；
/// 同时接受界面侧可能用的驼峰 `ledgerId`，语义完全相同。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTagSettingsRequest {
    #[serde(alias = "ledgerId")]
    pub ledger_id: String,
    pub tags: Vec<String>,
}

#[tauri::command]
pub fn stock_tag_settings_put(
    state: State<'_, AppState>,
    req: StockTagSettingsRequest,
) -> ApiResult<StockTradeTagSettingDto> {
    let workspace = state.workspace()?;
    Ok(stock::save_trade_tags(
        &workspace,
        &req.ledger_id,
        &req.tags,
    )?)
}

// ---------- 资金记录 / 持仓 ----------

/// 数值参数：界面可能传数字字符串（`"2"`）也可能直接传数字（`2`）。
///
/// 两种形态都接受，解析结果一致。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum QueryNumber {
    Text(String),
    Integer(i64),
    Float(f64),
}

/// `page` / `page_size` 同时接受数字与数字字符串（`alias` 覆盖 `pageSize` 这种前端驼峰写法）；
/// 非法或缺失时回退默认值。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockFundRecordsRequest {
    pub ledger_id: String,
    pub page: Option<QueryNumber>,
    #[serde(alias = "pageSize")]
    pub page_size: Option<QueryNumber>,
}

/// 解析正整数参数，非法或缺失时返回默认值。
fn parse_positive_int(raw: Option<&QueryNumber>, default: i64) -> i64 {
    match query_number_as_i64(raw) {
        Some(value) if value >= 1 => value,
        _ => default,
    }
}

/// `QueryNumber` → `i64`：数字字符串解析失败、或字符串为空一律得 `None`
/// （空串与非数字串的调用方口径不同，由调用方在拿不到值时自行处理）。
fn query_number_as_i64(raw: Option<&QueryNumber>) -> Option<i64> {
    let raw = raw?;
    match raw {
        QueryNumber::Text(text) => text.parse::<i64>().ok(),
        QueryNumber::Integer(value) => Some(*value),
        QueryNumber::Float(value) => Some(*value as i64),
    }
}

/// 资金变化记录分页。
#[tauri::command]
pub fn stock_fund_records(
    state: State<'_, AppState>,
    req: StockFundRecordsRequest,
) -> ApiResult<StockFundRecordPage> {
    require_ledger_id(&req.ledger_id)?;
    let page = parse_positive_int(req.page.as_ref(), 1);
    let page_size = parse_positive_int(req.page_size.as_ref(), 10);
    let workspace = state.workspace()?;
    Ok(stock::list_fund_records(
        &workspace,
        &req.ledger_id,
        page,
        page_size,
    )?)
}

/// 持仓列表（只含未清仓股票，挂载行情）。
///
/// 涉及网络行情，用 `spawn_blocking` 包住同步服务调用，避免阻塞 Tauri 主线程。
#[tauri::command]
pub async fn stock_positions(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<Vec<StockPositionDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    tauri::async_runtime::spawn_blocking(move || {
        stock::list_positions(&workspace, &req.ledger_id).map_err(ApiError::from)
    })
    .await
    .map_err(|error| ApiError::from(AppError::internal(error.to_string())))?
}

/// 保存持仓中的「本轮复盘」（500 字以内）。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockPositionReviewRequest {
    pub ledger_id: String,
    /// 股票代码
    pub code: String,
    pub review: String,
}

#[tauri::command]
pub fn stock_position_review(
    state: State<'_, AppState>,
    req: StockPositionReviewRequest,
) -> ApiResult<StockPositionDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::update_position_review(
        &workspace,
        &req.ledger_id,
        &req.code,
        &req.review,
    )?)
}

// ---------- 交易 ----------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTradesRequest {
    pub ledger_id: String,
    pub stock_code: String,
}

/// 某股交易列表（持仓中只返回本轮）。
#[tauri::command]
pub fn stock_trades(
    state: State<'_, AppState>,
    req: StockTradesRequest,
) -> ApiResult<Vec<StockTradeDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::list_trades(
        &workspace,
        &req.ledger_id,
        &req.stock_code,
    )?)
}

/// 一笔委托内的一笔成交明细（价格单位：**元**）。
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct TradeFillRequest {
    pub price: f64,
    pub lots: f64,
}

/// 一笔委托（可含多笔成交明细），返回成交明细数组。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTradeCreateRequest {
    pub ledger_id: String,
    pub stock_code: String,
    pub stock_name: String,
    pub trade_type: String,
    pub trade_time: f64,
    pub remark: String,
    pub tag: String,
    /// 成交明细；缺省时回退到单笔 `price` / `lots`（兼容旧调用）
    pub fills: Vec<TradeFillRequest>,
    /// 兼容旧调用的单笔价格（**元**）
    pub price: f64,
    pub lots: f64,
}

/// 解析成交明细：优先 `fills` 数组，缺省回退到单笔 `price`/`lots`。
///
/// `fills` 元素不是对象时报 `成交明细格式错误`；价格一律四舍五入到分。
fn parse_trade_fills(req: &StockTradeCreateRequest) -> Result<Vec<TradeFill>, ApiError> {
    if !req.fills.is_empty() {
        return Ok(req
            .fills
            .iter()
            .map(|fill| TradeFill {
                price_cents: yuan_to_price_cents(fill.price),
                lots: fill.lots as i64,
            })
            .collect());
    }
    Ok(vec![TradeFill {
        price_cents: yuan_to_price_cents(req.price),
        lots: req.lots as i64,
    }])
}

#[tauri::command]
pub fn stock_trade_create(
    state: State<'_, AppState>,
    req: StockTradeCreateRequest,
) -> ApiResult<Vec<StockTradeDto>> {
    require_ledger_id(&req.ledger_id)?;
    if req.stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    let fills = parse_trade_fills(&req)?;
    let workspace = state.workspace()?;
    Ok(stock::create_trade_order(
        &workspace,
        &req.ledger_id,
        &req.stock_code,
        &req.stock_name,
        &req.trade_type,
        &fills,
        req.trade_time as i64,
        &req.remark,
        &req.tag,
    )?)
}

/// 编辑一笔成交（按当前费用设置重算整笔委托）。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTradeUpdateRequest {
    pub ledger_id: String,
    /// 成交记录 ID
    pub id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: f64,
    pub trade_time: f64,
}

#[tauri::command]
pub fn stock_trade_update(
    state: State<'_, AppState>,
    req: StockTradeUpdateRequest,
) -> ApiResult<StockTradeDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::update_trade_fill(
        &workspace,
        &req.ledger_id,
        &req.id,
        yuan_to_price_cents(req.price),
        req.lots as i64,
        req.trade_time as i64,
    )?)
}

/// 删除整笔委托。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTradeOrderDeleteRequest {
    pub ledger_id: String,
    /// 委托 ID（保持 `orderId` 的大小写）
    #[serde(alias = "order_id")]
    pub order_id: String,
}

#[tauri::command]
pub fn stock_trade_order_delete(
    state: State<'_, AppState>,
    req: StockTradeOrderDeleteRequest,
) -> ApiResult<bool> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    stock::delete_trade_order(&workspace, &req.ledger_id, &req.order_id)?;
    Ok(true)
}

/// 预演编辑/删除的影响（**不落库**）。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockTradeImpactRequest {
    pub ledger_id: String,
    /// `update_trade` | `delete_order`
    pub action: String,
    pub trade_id: String,
    pub order_id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: f64,
    pub trade_time: f64,
}

#[tauri::command]
pub fn stock_trade_impact(
    state: State<'_, AppState>,
    req: StockTradeImpactRequest,
) -> ApiResult<StockTradeImpactDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::preview_trade_change(
        &workspace,
        &req.ledger_id,
        &req.action,
        &req.trade_id,
        &req.order_id,
        yuan_to_price_cents(req.price),
        req.lots as i64,
        req.trade_time as i64,
    )?)
}

// ---------- 历史 / 轮次 ----------

/// 交易历史集合列表（左栏）。涉及行情，走 `spawn_blocking`。
#[tauri::command]
pub async fn stock_history(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<Vec<StockTradeHistoryDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    tauri::async_runtime::spawn_blocking(move || {
        stock::list_trade_histories(&workspace, &req.ledger_id).map_err(ApiError::from)
    })
    .await
    .map_err(|error| ApiError::from(AppError::internal(error.to_string())))?
}

/// 单只股票历史详情（右栏）。
#[tauri::command]
pub fn stock_history_detail(
    state: State<'_, AppState>,
    req: StockTradesRequest,
) -> ApiResult<StockTradeHistoryDetailDto> {
    require_ledger_id(&req.ledger_id)?;
    if req.stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    let workspace = state.workspace()?;
    Ok(stock::get_trade_history_detail(
        &workspace,
        &req.ledger_id,
        &req.stock_code,
    )?)
}

/// 全部股票的交易历史总览。
#[tauri::command]
pub fn stock_history_summary(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockTradeHistorySummaryDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::get_trade_history_summary(
        &workspace,
        &req.ledger_id,
    )?)
}

/// 保存某轮次的交易复盘（500 字以内）。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockRoundReviewRequest {
    pub ledger_id: String,
    /// 轮次 ID
    pub id: String,
    pub review: String,
}

#[tauri::command]
pub fn stock_round_review(
    state: State<'_, AppState>,
    req: StockRoundReviewRequest,
) -> ApiResult<StockTradeHistoryDetailDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::update_round_review(
        &workspace,
        &req.ledger_id,
        &req.id,
        &req.review,
    )?)
}

/// 保存某轮次的交易标签。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockRoundTagRequest {
    pub ledger_id: String,
    /// 轮次 ID
    pub id: String,
    pub tag: String,
}

#[tauri::command]
pub fn stock_round_tag(
    state: State<'_, AppState>,
    req: StockRoundTagRequest,
) -> ApiResult<StockTradeHistoryDetailDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(stock::update_round_tag(
        &workspace,
        &req.ledger_id,
        &req.id,
        &req.tag,
    )?)
}

// ---------- 统计 / 股票名 / 重置 ----------

/// 统计的筛选参数：`start_month` / `end_month` / `recent` / `tag`。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockStatisticsRequest {
    pub ledger_id: String,
    pub start_month: String,
    pub end_month: String,
    /// 非法值报 `recent 必须为正整数`
    pub recent: Option<QueryNumber>,
    pub tag: String,
}

/// 逐笔结算统计（可按月份区间 / 最近 N 笔 / 标签筛选）。
#[tauri::command]
pub fn stock_statistics(
    state: State<'_, AppState>,
    req: StockStatisticsRequest,
) -> ApiResult<StockStatisticsDto> {
    require_ledger_id(&req.ledger_id)?;
    let mut recent = 0_i64;
    if let Some(raw) = req.recent.as_ref() {
        // 空串按"没传"处理（保持宽松）；非数字串或 <= 0 一律报错
        let parsed = query_number_as_i64(Some(raw));
        match parsed {
            None => {}
            Some(value) if value > 0 => recent = value,
            // 解析失败或 <= 0 一律报 `recent 必须为正整数`
            _ => return Err(AppError::bad_request("recent 必须为正整数").into()),
        }
    }
    let workspace = state.workspace()?;
    Ok(tr_service::stock_statistics::get_statistics_range(
        &workspace,
        &req.ledger_id,
        &req.start_month,
        &req.end_month,
        recent,
        &req.tag,
    )?)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StockNameRequest {
    pub stock_code: String,
}

/// 查询股票名称（优先本地交易记录，未命中走外部行情接口）。涉及网络，走 `spawn_blocking`。
#[tauri::command]
pub async fn stock_name(
    state: State<'_, AppState>,
    req: StockNameRequest,
) -> ApiResult<StockNameDto> {
    if req.stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    // `stock_code` 为空才报错，**不**要求 ledger_id
    let workspace = state.workspace()?;
    tauri::async_runtime::spawn_blocking(move || {
        stock::lookup_stock_name(&workspace, &req.stock_code).map_err(ApiError::from)
    })
    .await
    .map_err(|error| ApiError::from(AppError::internal(error.to_string())))?
}

/// 清空指定账本的全部股票交易数据。
#[tauri::command]
pub fn stock_reset(state: State<'_, AppState>, req: StockOverviewRequest) -> ApiResult<bool> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    stock::reset_data(&workspace, &req.ledger_id)?;
    Ok(true)
}

// ---------- 操作记录 / 回滚 ----------
//
// 三条命令共用 `StockOverviewRequest`（只需要 ledger_id），界面侧对应 `api::stock` 的
// `LedgerIdRequest` —— 字段集一致，契约审计逐字段比得过。

/// 某账本的操作记录（最新的在前，最多 10 条）。
#[tauri::command]
pub fn stock_operation_list(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<Vec<StockOperationDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(tr_service::stock::list_operations(
        &workspace,
        &req.ledger_id,
    )?)
}

/// 回滚预演：要撤销哪一次操作、会不会让某些轮次失效（复盘随之丢失）。**不落库**。
#[tauri::command]
pub fn stock_operation_preview(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockOperationRollbackPreviewDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(tr_service::stock::preview_rollback(
        &workspace,
        &req.ledger_id,
    )?)
}

/// 回滚最新一次操作（撤销后把它从记录里弹掉）。
#[tauri::command]
pub fn stock_operation_rollback(
    state: State<'_, AppState>,
    req: StockOverviewRequest,
) -> ApiResult<StockOperationRollbackDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(tr_service::stock::rollback_latest(
        &workspace,
        &req.ledger_id,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_positive_int_fallback_rules() {
        // 缺失 / 空串 / 非法 / < 1 → 默认值
        assert_eq!(parse_positive_int(None, 10), 10);
        assert_eq!(
            parse_positive_int(Some(&QueryNumber::Text(String::new())), 10),
            10
        );
        assert_eq!(
            parse_positive_int(Some(&QueryNumber::Text("abc".into())), 10),
            10
        );
        assert_eq!(
            parse_positive_int(Some(&QueryNumber::Text("0".into())), 10),
            10
        );
        assert_eq!(
            parse_positive_int(Some(&QueryNumber::Text("-3".into())), 10),
            10
        );
        // 合法：字符串与数字两种形态都接受
        assert_eq!(
            parse_positive_int(Some(&QueryNumber::Text("2".into())), 10),
            2
        );
        assert_eq!(parse_positive_int(Some(&QueryNumber::Integer(7)), 10), 7);
        assert_eq!(parse_positive_int(Some(&QueryNumber::Float(3.0)), 10), 3);
    }

    #[test]
    fn request_bodies_deserialize_with_documented_field_names() {
        // 全部是 snake_case 键：ledger_id / price / lots / trade_time
        let body: StockTradeCreateRequest = serde_json::from_str(
            r#"{"ledger_id":"l1","stock_code":"605258","stock_name":"协和电子",
                 "trade_type":"open","trade_time":1700000000,
                 "fills":[{"price":38.06,"lots":2}]}"#,
        )
        .unwrap();
        assert_eq!(body.ledger_id, "l1");
        assert_eq!(body.fills.len(), 1);
        assert_eq!(body.fills[0].price, 38.06);
        // 价格（元）→ 分：四舍五入到整数分
        let fills = parse_trade_fills(&body).unwrap();
        assert_eq!(fills[0].price_cents, 3806);
        assert_eq!(fills[0].lots, 2);

        // 兼容旧调用：没有 fills 时回退到单笔 price / lots
        let legacy: StockTradeCreateRequest = serde_json::from_str(
            r#"{"ledger_id":"l1","stock_code":"600000","stock_name":"浦发银行",
                 "trade_type":"open","price":10.005,"lots":10}"#,
        )
        .unwrap();
        let fills = parse_trade_fills(&legacy).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price_cents, 1001, "10.005 元四舍五入到 1001 分");
        assert_eq!(fills[0].lots, 10);

        // 缺参等价于零值
        let empty: StockTradeCreateRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.stock_code, "");
        assert!(empty.fills.is_empty());
    }

    #[test]
    fn page_parameters_accept_query_string_and_numbers() {
        let query: StockFundRecordsRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","page":"2","page_size":"50"}"#).unwrap();
        assert_eq!(parse_positive_int(query.page.as_ref(), 1), 2);
        assert_eq!(parse_positive_int(query.page_size.as_ref(), 10), 50);

        let numeric: StockFundRecordsRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","page":3,"pageSize":20}"#).unwrap();
        assert_eq!(parse_positive_int(numeric.page.as_ref(), 1), 3);
        assert_eq!(parse_positive_int(numeric.page_size.as_ref(), 10), 20);
    }

    #[test]
    fn statistics_request_parses_recent_shapes() {
        let text: StockStatisticsRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","recent":"5","start_month":"2023-01"}"#)
                .unwrap();
        assert_eq!(text.start_month, "2023-01");
        match text.recent {
            Some(QueryNumber::Text(ref value)) => assert_eq!(value, "5"),
            other => panic!("意外的解析结果: {other:?}"),
        }

        let numeric: StockStatisticsRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","recent":5}"#).unwrap();
        match numeric.recent {
            Some(QueryNumber::Integer(value)) => assert_eq!(value, 5),
            other => panic!("意外的解析结果: {other:?}"),
        }
    }

    #[test]
    fn price_conversion_rounds_to_cents() {
        assert_eq!(yuan_to_price_cents(10.0), 1000);
        assert_eq!(yuan_to_price_cents(38.06), 3806);
        assert_eq!(yuan_to_price_cents(36.61), 3661);
        // 浮点边界：10.005 在 f64 里略小于 10.005，因此四舍五入得到 1001 分
        assert_eq!(yuan_to_price_cents(10.005), 1001);
        assert_eq!(yuan_to_price_cents(0.005), 1);
    }

    #[test]
    fn missing_ledger_id_reports_the_documented_message() {
        let error = require_ledger_id("").unwrap_err();
        assert_eq!(error.msg, "ledger_id is required");
        assert_eq!(error.status, 400);
        assert!(require_ledger_id("l1").is_ok());
    }
}
