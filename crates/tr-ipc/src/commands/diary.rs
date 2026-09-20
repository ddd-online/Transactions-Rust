//! 日记命令：日期列表、单日读写、导入扫描与导入、导出。
//!
//! **日记按账本隔离**：除 `diary_import_scan`（纯文件系统、不碰数据库）之外的命令都要求
//! `ledger_id`，缺失时报既有的 `ledger_id is required`。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{
    DiaryExportRequest, DiaryExportResult, DiaryScanResponse, DiaryUpsertRequest,
};
use tr_domain::models::DiaryEntry;
use tr_service::diary;

use crate::error::ApiResult;
use crate::AppState;

use super::{require, require_ledger_id};

/// 日期列表（倒序）。
#[tauri::command]
pub fn diary_list_dates(
    state: State<'_, AppState>,
    req: DiaryLedgerRequest,
) -> ApiResult<Vec<tr_domain::models::DiaryDateItem>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(diary::list_dates(&workspace, &req.ledger_id)?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryLedgerRequest {
    pub ledger_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DiaryDateRequest {
    pub date: String,
    pub ledger_id: String,
}

/// `date` 必填（空串报 `missing date parameter`），账本必填。
fn require_date_and_ledger(req: &DiaryDateRequest) -> ApiResult<()> {
    require_date_parameter(&req.date)?;
    require_ledger_id(&req.ledger_id)
}

/// `date` 必填（空串报 `missing date parameter`）。
fn require_date_parameter(date: &str) -> ApiResult<()> {
    require(!date.is_empty(), "missing date parameter")
}

/// `directory` 必填（空串报 `directory is required`）。
fn require_directory(directory: &str) -> ApiResult<()> {
    require(!directory.is_empty(), "directory is required")
}

/// 取某账本某天日记（不存在时报错）。
#[tauri::command]
pub fn diary_get(state: State<'_, AppState>, req: DiaryDateRequest) -> ApiResult<DiaryEntry> {
    require_date_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(diary::get_by_date(&workspace, &req.ledger_id, &req.date)?)
}

/// 保存日记，返回写入后的条目。
#[tauri::command]
pub fn diary_upsert(state: State<'_, AppState>, req: DiaryUpsertRequest) -> ApiResult<DiaryEntry> {
    require_date_parameter(&req.date)?;
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(diary::upsert(
        &workspace,
        &req.ledger_id,
        &req.date,
        req.content.as_deref().unwrap_or(""),
        req.mood.as_deref().unwrap_or(""),
    )?)
}

/// 删除某账本某天的日记。
#[tauri::command]
pub fn diary_delete(state: State<'_, AppState>, req: DiaryDateRequest) -> ApiResult<()> {
    require_date_and_ledger(&req)?;
    let workspace = state.workspace()?;
    Ok(diary::delete_by_date(
        &workspace,
        &req.ledger_id,
        &req.date,
    )?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryScanRequest {
    pub directory: String,
}

/// 扫描目录里的日记文件（不碰数据库，因此不需要账本）。
#[tauri::command]
pub fn diary_import_scan(req: DiaryScanRequest) -> ApiResult<DiaryScanResponse> {
    require_directory(&req.directory)?;
    Ok(diary::scan_directory(&req.directory)?)
}

#[derive(Debug, Deserialize)]
pub struct DiaryImportFileRequest {
    pub path: String,
    pub date: String,
    pub ledger_id: String,
}

/// 导入单个文件（自动识别 UTF-8/UTF-16/GBK）到指定账本。
#[tauri::command]
pub fn diary_import_file(
    state: State<'_, AppState>,
    req: DiaryImportFileRequest,
) -> ApiResult<DiaryEntry> {
    require(
        !req.path.is_empty() && !req.date.is_empty(),
        "path and date are required",
    )?;
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(diary::import_file(
        &workspace,
        &req.ledger_id,
        &req.path,
        &req.date,
    )?)
}

/// 导出**指定账本**的日记到目录（`year`/`month` 为 0 表示不限）。
///
/// 参数校验：负数、`month > 12`、以及"只给 month 不给 year"都报
/// `invalid year/month range`。
#[tauri::command]
pub fn diary_export(
    state: State<'_, AppState>,
    req: DiaryExportRequest,
) -> ApiResult<DiaryExportResult> {
    require_directory(&req.directory)?;
    require_ledger_id(&req.ledger_id)?;

    let year = req.year.unwrap_or(0);
    let month = req.month.unwrap_or(0);
    require(
        year >= 0 && (0..=12).contains(&month) && !(year == 0 && month != 0),
        "invalid year/month range",
    )?;

    let workspace = state.workspace()?;
    Ok(diary::export_to_directory(
        &workspace,
        &req.ledger_id,
        &req.directory,
        year,
        month,
    )?)
}
