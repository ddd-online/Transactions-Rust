//! 关键事件命令。对照 Go `kernel/api/key_event_controller.go`。
//!
//! | 原路由 | 命令 |
//! |---|---|
//! | `GET /key-events/year/:year` | `key_event_list_by_year` |
//! | `GET /key-events/dates/:year` | `key_event_dates_by_year` |
//! | `GET /key-events/:date` | `key_event_get` |
//! | `POST /key-events` | `key_event_upsert` |
//! | `DELETE /key-events/:date` | `key_event_delete` |
//! | `GET /key-events/:date/images` | `key_event_images_list` |
//! | `POST /key-events/:date/images` | `key_event_image_add` |
//! | `DELETE /key-event-images/:id` | `key_event_image_delete` |
//!
//! 原实现的 path/query 参数全部并入 `req`，命名保持 snake_case（`ledger_id`），
//! 校验文案与顺序也与控制器一致。

use serde::Deserialize;
use tauri::State;

use tr_domain::error::AppError;
use tr_domain::models::{KeyEvent, KeyEventImage};
use tr_service::key_event;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct YearRequest {
    pub year: String,
    pub ledger_id: String,
}

fn require_year_and_ledger(req: &YearRequest) -> ApiResult<()> {
    if req.year.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing year parameter",
        )));
    }
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "ledger_id is required",
        )));
    }
    Ok(())
}

/// `GET /key-events/year/:year`：某年的全部关键事件。
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

/// `GET /key-events/dates/:year`：某年有事件的日期列表。
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
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing date parameter",
        )));
    }
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "ledger_id is required",
        )));
    }
    Ok(())
}

/// `GET /key-events/:date`：按日期取关键事件（不存在时报错）。
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

/// `POST /key-events`：写入关键事件，返回日期（与原实现返回 `date` 一致）。
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
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "ledger_id is required",
        )));
    }
    // 原实现只要求 date 字段存在（可以为空串），据此保留同样的宽松度
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

/// `DELETE /key-events/:date`：删除事件及其图片（记录 + 磁盘文件）。
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

/// `GET /key-events/:date/images`：某天的图片列表。
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

/// `POST /key-events/:date/images`：上传一张图片（base64 data URI）。
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
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing date parameter",
        )));
    }
    let Some(data) = req.data.as_deref().filter(|data| !data.is_empty()) else {
        return Err(ApiError::from(AppError::bad_request("invalid image data")));
    };
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "ledger_id is required",
        )));
    }

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

/// `DELETE /key-event-images/:id`：删除一张图片。
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
