//! 日记命令。对照 Go `kernel/api/diary_controller.go`。
//!
//! | 原路由 | 命令 |
//! |---|---|
//! | `GET /diary/dates` | `diary_list_dates` |
//! | `GET /diary/:date` | `diary_get` |
//! | `PUT /diary/:date` | `diary_upsert` |
//! | `DELETE /diary/:date` | `diary_delete` |
//! | `POST /diary/import/scan` | `diary_import_scan` |
//! | `POST /diary/import/file` | `diary_import_file` |
//! | `POST /diary/export` | `diary_export` |

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{
    DiaryExportRequest, DiaryExportResult, DiaryScanResponse, DiaryUpsertRequest,
};
use tr_domain::error::AppError;
use tr_domain::models::DiaryEntry;
use tr_service::diary;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// `GET /diary/dates`：日期列表（倒序）。
#[tauri::command]
pub fn diary_list_dates(
    state: State<'_, AppState>,
    _req: crate::commands::EmptyRequest,
) -> ApiResult<Vec<tr_domain::models::DiaryDateItem>> {
    let workspace = state.workspace()?;
    Ok(diary::list_dates(&workspace)?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryDateRequest {
    pub date: String,
}

/// `GET /diary/:date`：取某天日记（不存在时报错，与原实现一致）。
#[tauri::command]
pub fn diary_get(state: State<'_, AppState>, req: DiaryDateRequest) -> ApiResult<DiaryEntry> {
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing date parameter",
        )));
    }
    let workspace = state.workspace()?;
    Ok(diary::get_by_date(&workspace, &req.date)?)
}

/// `PUT /diary/:date`：保存日记，返回写入后的条目。
#[tauri::command]
pub fn diary_upsert(state: State<'_, AppState>, req: DiaryUpsertRequest) -> ApiResult<DiaryEntry> {
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing date parameter",
        )));
    }
    let workspace = state.workspace()?;
    Ok(diary::upsert(
        &workspace,
        &req.date,
        req.content.as_deref().unwrap_or(""),
        req.mood.as_deref().unwrap_or(""),
    )?)
}

/// `DELETE /diary/:date`：删除日记。
#[tauri::command]
pub fn diary_delete(state: State<'_, AppState>, req: DiaryDateRequest) -> ApiResult<()> {
    if req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing date parameter",
        )));
    }
    let workspace = state.workspace()?;
    Ok(diary::delete_by_date(&workspace, &req.date)?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryScanRequest {
    pub directory: String,
}

/// `POST /diary/import/scan`：扫描目录里的日记文件。
#[tauri::command]
pub fn diary_import_scan(req: DiaryScanRequest) -> ApiResult<DiaryScanResponse> {
    if req.directory.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "directory is required",
        )));
    }
    Ok(diary::scan_directory(&req.directory)?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryImportFileRequest {
    pub path: String,
    pub date: String,
}

/// `POST /diary/import/file`：导入单个文件（自动识别 UTF-8/UTF-16/GBK）。
#[tauri::command]
pub fn diary_import_file(
    state: State<'_, AppState>,
    req: DiaryImportFileRequest,
) -> ApiResult<DiaryEntry> {
    if req.path.is_empty() || req.date.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "path and date are required",
        )));
    }
    let workspace = state.workspace()?;
    Ok(diary::import_file(&workspace, &req.path, &req.date)?)
}

/// `POST /diary/export`：导出到目录（`year`/`month` 为 0 表示不限）。
///
/// 参数校验与原实现逐条一致：负数、`month > 12`、以及"只给 month 不给 year"都报
/// `invalid year/month range`。
#[tauri::command]
pub fn diary_export(
    state: State<'_, AppState>,
    req: DiaryExportRequest,
) -> ApiResult<DiaryExportResult> {
    if req.directory.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "directory is required",
        )));
    }

    let year = req.year.unwrap_or(0);
    let month = req.month.unwrap_or(0);
    if year < 0 || !(0..=12).contains(&month) || (year == 0 && month != 0) {
        return Err(ApiError::from(AppError::bad_request(
            "invalid year/month range",
        )));
    }

    let workspace = state.workspace()?;
    Ok(diary::export_to_directory(
        &workspace,
        &req.directory,
        year,
        month,
    )?)
}
