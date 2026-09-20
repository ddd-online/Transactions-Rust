//! 关键事件命令：按年/按日期查询、写入与删除、图片列表/上传/删除。
//!
//! 所有参数并入一个 `req`，命名保持 snake_case（`ledger_id`），
//! 校验文案与顺序是固定契约。

use serde::Deserialize;
use tauri::State;

use tr_domain::error::AppError;
use tr_domain::models::{KeyEvent, KeyEventImage};
use tr_service::key_event;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

use super::{require, require_ledger_id};

#[derive(Debug, Deserialize)]
pub struct YearRequest {
    pub year: String,
    pub ledger_id: String,
}

fn require_year_and_ledger(req: &YearRequest) -> ApiResult<()> {
    require(!req.year.is_empty(), "missing year parameter")?;
    require_ledger_id(&req.ledger_id)
}

/// 某年的全部关键事件。
#[tauri::command]
pub fn key_event_list_by_year(
    state: State<'_, AppState>,
    req: YearRequest,
) -> ApiResult<Vec<KeyEvent>> {
    require_year_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(key_event::query_by_year(
        &workspace,
        &req.ledger_id,
        &req.year,
    )?)
}

/// 某年有事件的日期列表。
#[tauri::command]
pub fn key_event_dates_by_year(
    state: State<'_, AppState>,
    req: YearRequest,
) -> ApiResult<Vec<String>> {
    require_year_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(key_event::query_dates_by_year(
        &workspace,
        &req.ledger_id,
        &req.year,
    )?)
}

#[derive(Debug, Deserialize)]
pub struct KeyEventDateRequest {
    pub date: String,
    pub ledger_id: String,
}

fn require_date_and_ledger(req: &KeyEventDateRequest) -> ApiResult<()> {
    require(!req.date.is_empty(), "missing date parameter")?;
    require_ledger_id(&req.ledger_id)
}

/// 按日期取关键事件（不存在时报错）。
#[tauri::command]
pub fn key_event_get(state: State<'_, AppState>, req: KeyEventDateRequest) -> ApiResult<KeyEvent> {
    require_date_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(key_event::query_by_date(
        &workspace,
        &req.ledger_id,
        &req.date,
    )?)
}

/// 写入关键事件，返回日期。
#[derive(Debug, Deserialize)]
pub struct KeyEventUpsertRequest {
    pub ledger_id: String,
    pub date: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[tauri::command]
pub fn key_event_upsert(
    state: State<'_, AppState>,
    req: KeyEventUpsertRequest,
) -> ApiResult<String> {
    require_ledger_id(&req.ledger_id)?;
    // 只要求 date 字段存在（可以为空串），保留同样的宽松度
    let workspace = state.workspace()?;
    key_event::upsert_key_event(
        &workspace,
        &req.ledger_id,
        &req.date,
        req.title.as_deref().unwrap_or(""),
        req.content.as_deref().unwrap_or(""),
        req.color.as_deref().unwrap_or(""),
    )?;
    Ok(req.date)
}

/// 删除事件及其图片（记录 + 磁盘文件）。
#[tauri::command]
pub fn key_event_delete(state: State<'_, AppState>, req: KeyEventDateRequest) -> ApiResult<()> {
    require_date_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(key_event::delete_by_date(
        &workspace,
        &req.ledger_id,
        &req.date,
    )?)
}

/// 某天的图片列表。
#[tauri::command]
pub fn key_event_images_list(
    state: State<'_, AppState>,
    req: KeyEventDateRequest,
) -> ApiResult<Vec<KeyEventImage>> {
    require_date_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(key_event::list_images(
        &workspace,
        &req.ledger_id,
        &req.date,
    )?)
}

/// 上传一张图片（base64 data URI）。
#[derive(Debug, Deserialize)]
pub struct KeyEventImageAddRequest {
    pub date: String,
    pub ledger_id: String,
    #[serde(default)]
    pub data: Option<String>,
}

#[tauri::command]
pub fn key_event_image_add(
    state: State<'_, AppState>,
    req: KeyEventImageAddRequest,
) -> ApiResult<KeyEventImage> {
    require(!req.date.is_empty(), "missing date parameter")?;
    let Some(data) = req.data.as_deref().filter(|data| !data.is_empty()) else {
        return Err(ApiError::from(AppError::bad_request("invalid image data")));
    };
    require_ledger_id(&req.ledger_id)?;

    let workspace = state.workspace()?;
    Ok(key_event::add_image(
        &workspace,
        &req.ledger_id,
        &req.date,
        data,
    )?)
}

#[derive(Debug, Deserialize)]
pub struct KeyEventImageIdRequest {
    pub id: String,
}

/// 删除一张图片。
#[tauri::command]
pub fn key_event_image_delete(
    state: State<'_, AppState>,
    req: KeyEventImageIdRequest,
) -> ApiResult<()> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing image id parameter",
        )));
    }
    let workspace = state.workspace()?;
    Ok(key_event::delete_image(&workspace, &req.id)?)
}
