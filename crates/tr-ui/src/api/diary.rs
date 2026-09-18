//! 日记域命令。对照原 `app/src/backend/api/diary.ts` 与 `diary_controller.go`。
//!
//! | 原路由 | 命令 | 入参 |
//! |---|---|---|
//! | `GET /diary/dates` | `diary_list_dates` | `{}`（空请求体） |
//! | `GET /diary/:date` | `diary_get` | `{ date }` |
//! | `PUT /diary/:date` | `diary_upsert` | [`DiaryUpsertRequest`]（`date` + `content`/`mood`） |
//! | `DELETE /diary/:date` | `diary_delete` | `{ date }` |
//! | `POST /diary/import/scan` | `diary_import_scan` | `{ directory }` |
//! | `POST /diary/import/file` | `diary_import_file` | `{ path, date }` |
//! | `POST /diary/export` | `diary_export` | [`DiaryExportRequest`]（`directory` + `year`/`month`） |
//!
//! 日记是**工作空间级**数据，不与账本绑定（因此入参里没有 `ledgerId`）。

use serde::Serialize;
use tr_domain::dto::{
    DiaryExportRequest, DiaryExportResult, DiaryScanResponse, DiaryUpsertRequest,
};
use tr_domain::models::{DiaryDateItem, DiaryEntry};

use crate::ipc::{self, IpcError};

#[derive(Debug, Serialize)]
struct EmptyRequest {}

#[derive(Debug, Serialize)]
struct DateRequest {
    date: String,
}

#[derive(Debug, Serialize)]
struct ScanRequest {
    directory: String,
}

#[derive(Debug, Serialize)]
struct ImportFileRequest {
    path: String,
    date: String,
}

/// 有日记的日期列表（倒序，含字数与心情）。
pub async fn list_dates() -> Result<Vec<DiaryDateItem>, IpcError> {
    ipc::call("diary_list_dates", EmptyRequest {}).await
}

/// 取某天日记（不存在时报错）。
pub async fn get(date: &str) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_get",
        DateRequest {
            date: date.to_string(),
        },
    )
    .await
}

/// 保存日记，返回写入后的条目。
pub async fn upsert(date: &str, content: &str, mood: &str) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_upsert",
        DiaryUpsertRequest {
            date: date.to_string(),
            content: Some(content.to_string()),
            mood: Some(mood.to_string()),
        },
    )
    .await
}

/// 删除日记。
pub async fn delete(date: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "diary_delete",
        DateRequest {
            date: date.to_string(),
        },
    )
    .await
}

/// 扫描目录里的日记文件。
pub async fn import_scan(directory: &str) -> Result<DiaryScanResponse, IpcError> {
    ipc::call(
        "diary_import_scan",
        ScanRequest {
            directory: directory.to_string(),
        },
    )
    .await
}

/// 导入单个文件（后端自动识别 UTF-8/UTF-16/GBK）。
pub async fn import_file(path: &str, date: &str) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_import_file",
        ImportFileRequest {
            path: path.to_string(),
            date: date.to_string(),
        },
    )
    .await
}

/// 导出到目录：`year`/`month` 为 `None` 表示不限。
pub async fn export(
    directory: &str,
    year: Option<i64>,
    month: Option<i64>,
) -> Result<DiaryExportResult, IpcError> {
    ipc::call(
        "diary_export",
        DiaryExportRequest {
            directory: directory.to_string(),
            year,
            month,
        },
    )
    .await
}
