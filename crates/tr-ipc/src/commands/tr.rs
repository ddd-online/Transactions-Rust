//! 消费记录命令。
//!
//! 入参形状是固定契约（注意 `link`/`unlink` 用的是 **snake_case**
//! ——请求体里取 `transaction_id`；其余对象用 DTO 的原始字段名）。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{
    ChartQueryRequest, ChartQueryResponse, TrQueryCondition, TrQueryResult, TransactionRecordDto,
};
use tr_domain::error::AppError;
use tr_service::transaction_record;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

use super::require_ledger_id;

/// 条件查询（条件项之间 OR，项内 AND）。
#[tauri::command]
pub fn tr_query(state: State<'_, AppState>, req: TrQueryCondition) -> ApiResult<TrQueryResult> {
    tracing::debug!(
        "IPC tr_query ledger={} offset={} limit={} items={}",
        req.ledger_id,
        req.offset,
        req.limit,
        req.items.len()
    );
    let workspace = state.workspace()?;
    Ok(transaction_record::query_trs_on_condition(
        &workspace, &req,
    )?)
}

/// 图表逐曲线分桶数据。
#[tauri::command]
pub fn tr_chart_data(
    state: State<'_, AppState>,
    req: ChartQueryRequest,
) -> ApiResult<ChartQueryResponse> {
    let workspace = state.workspace()?;
    Ok(transaction_record::query_trs_for_chart(&workspace, &req)?)
}

/// 新建一条记录，返回记录 ID。
#[tauri::command]
pub fn tr_create(state: State<'_, AppState>, req: TransactionRecordDto) -> ApiResult<String> {
    req.validate()?;
    let workspace = state.workspace()?;
    Ok(transaction_record::create_tr(&workspace, &req)?)
}

/// 批量新建，返回成功条数。
///
/// 逐条校验，错误文案是 `record %d: %s`（下标从 1 起）。
#[tauri::command]
pub fn tr_batch_create(
    state: State<'_, AppState>,
    req: Vec<TransactionRecordDto>,
) -> ApiResult<i32> {
    for (index, dto) in req.iter().enumerate() {
        if let Err(error) = dto.validate() {
            return Err(ApiError::from(AppError::bad_request(format!(
                "record {}: {}",
                index + 1,
                error.msg
            ))));
        }
    }
    let workspace = state.workspace()?;
    Ok(transaction_record::batch_create_tr(&workspace, &req)?)
}

#[derive(Debug, Deserialize)]
pub struct TransactionIdRequest {
    pub id: String,
}

/// 删除记录及其标签关联。
#[tauri::command]
pub fn tr_delete(state: State<'_, AppState>, req: TransactionIdRequest) -> ApiResult<()> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing transaction id",
        )));
    }
    let workspace = state.workspace()?;
    Ok(transaction_record::delete_tr_by_id(&workspace, &req.id)?)
}

#[derive(Debug, Deserialize)]
pub struct LinkRequest {
    pub transaction_id: String,
    pub date: String,
}

/// 关联到关键事件，返回日期。
#[tauri::command]
pub fn tr_link(state: State<'_, AppState>, req: LinkRequest) -> ApiResult<String> {
    if req.transaction_id.is_empty() || req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "transaction_id and date are required",
        )));
    }
    let workspace = state.workspace()?;
    transaction_record::link_to_key_event(&workspace, &req.transaction_id, &req.date)?;
    Ok(req.date)
}

#[derive(Debug, Deserialize)]
pub struct UnlinkRequest {
    pub transaction_id: String,
}

/// 解除关联，返回记录 ID。
#[tauri::command]
pub fn tr_unlink(state: State<'_, AppState>, req: UnlinkRequest) -> ApiResult<String> {
    if req.transaction_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "transaction_id is required",
        )));
    }
    let workspace = state.workspace()?;
    transaction_record::unlink_from_key_event(&workspace, &req.transaction_id)?;
    Ok(req.transaction_id)
}

#[derive(Debug, Deserialize)]
pub struct LinkedByDateRequest {
    pub date: String,
    pub ledger_id: String,
}

/// 某天已关联的记录（含标签）。
#[tauri::command]
pub fn tr_linked_by_date(
    state: State<'_, AppState>,
    req: LinkedByDateRequest,
) -> ApiResult<Vec<TransactionRecordDto>> {
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request("date is required")));
    }
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(transaction_record::query_linked_by_date(
        &workspace,
        &req.ledger_id,
        &req.date,
    )?)
}
