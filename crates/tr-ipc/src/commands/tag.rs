//! 标签命令。对照 Go `kernel/api/tag_controller.go`。
//!
//! 入参形状按原 HTTP 语义逐字段对齐：
//! * `GET /tags?categoryTransactionType=xxx&ledgerId=xxx` → `tag_list { categoryTransactionType, ledgerId }`
//! * `POST /tags { ledgerId, name, categoryTransactionType, sortOrder }` → `tag_create`
//! * `DELETE /tags/:name?categoryTransactionType=...&ledgerId=...` → `tag_delete { name, categoryTransactionType, ledgerId }`
//! * `PATCH /tags/:name/sort { ledgerId, name, categoryTransactionType, sortOrder }` → `tag_update_sort`
//!
//! 缺参数时原文案为 `missing required parameters`（400），与原控制器逐字一致。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{CreateTagRequest, TagDto, UpdateTagSortRequest};
use tr_domain::error::AppError;
use tr_service::tag;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TagListRequest {
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 查询标签并补齐每个标签的记录数。`ledgerId` 为空时返回空数组（与原实现一致，不报错）。
#[tauri::command]
pub fn tag_list(state: State<'_, AppState>, req: TagListRequest) -> ApiResult<Vec<TagDto>> {
    if req.ledger_id.is_empty() {
        return Ok(Vec::new());
    }

    let workspace = state.workspace()?;
    let tags = tag::query_tags(&workspace, &req.ledger_id, &req.category_transaction_type)?;

    let names: Vec<String> = tags.iter().map(|tag| tag.name.clone()).collect();
    let counts = tag::count_records_by_tags(&workspace, &req.ledger_id, &names)?;

    Ok(tags
        .iter()
        .map(|tag| {
            let mut dto = TagDto::from(tag);
            dto.record_count = counts.get(&tag.name).copied().unwrap_or(0) as i32;
            dto
        })
        .collect())
}

/// 新建标签（返回 `()`，与原接口返回 nil 一致）。
#[tauri::command]
pub fn tag_create(state: State<'_, AppState>, req: CreateTagRequest) -> ApiResult<()> {
    let workspace = state.workspace()?;
    tag::create_tag(
        &workspace,
        &req.ledger_id,
        &req.name,
        &req.category_transaction_type,
    )?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TagDeleteRequest {
    /// 原路径参数 `:name`
    pub name: String,
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 删除标签（连带清理交易记录上的该标签关联）。
#[tauri::command]
pub fn tag_delete(state: State<'_, AppState>, req: TagDeleteRequest) -> ApiResult<()> {
    if req.name.is_empty() || req.category_transaction_type.is_empty() || req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing required parameters",
        )));
    }

    let workspace = state.workspace()?;
    tag::delete_tag(
        &workspace,
        &req.ledger_id,
        &req.name,
        &req.category_transaction_type,
    )?;
    Ok(())
}

/// 更新标签排序号。
#[tauri::command]
pub fn tag_update_sort(state: State<'_, AppState>, req: UpdateTagSortRequest) -> ApiResult<()> {
    let workspace = state.workspace()?;
    tag::update_tag_sort(
        &workspace,
        &req.ledger_id,
        &req.name,
        &req.category_transaction_type,
        req.sort_order,
    )?;
    Ok(())
}
