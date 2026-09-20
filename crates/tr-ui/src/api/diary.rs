//! 日记域命令。
//!
//! | 命令 | 入参 |
//! |---|---|
//! | `diary_list_dates` | `{ ledger_id }` |
//! | `diary_get` | `{ date, ledger_id }` |
//! | `diary_upsert` | [`DiaryUpsertRequest`]（`ledger_id` + `date` + `content`/`mood`） |
//! | `diary_delete` | `{ date, ledger_id }` |
//! | `diary_import_scan` | `{ directory }`（纯文件系统，**不带账本**） |
//! | `diary_import_file` | `{ path, date, ledger_id }` |
//! | `diary_export` | [`DiaryExportRequest`]（`ledger_id` + `directory` + `year`/`month`） |
//!
//! 日记**按账本隔离**：除扫描之外的入参都带 `ledger_id`（snake_case，与 IPC 侧一致 ——
//! 这里刻意**不用** `api/mod.rs` 里那个 camelCase 的 `LedgerIdRequest`，否则字段名对不上）。
//! 导出只导指定账本，导入落到指定账本。

use serde::Serialize;
use tr_domain::dto::{
    DiaryExportRequest, DiaryExportResult, DiaryScanResponse, DiaryUpsertRequest,
};
use tr_domain::models::{DiaryDateItem, DiaryEntry};

use crate::ipc::{self, IpcError};

#[derive(Debug, Serialize)]
struct LedgerRequest {
    ledger_id: String,
}

#[derive(Debug, Serialize)]
struct DateRequest {
    date: String,
    ledger_id: String,
}

#[derive(Debug, Serialize)]
struct ScanRequest {
    directory: String,
}

#[derive(Debug, Serialize)]
struct ImportFileRequest {
    path: String,
    date: String,
    ledger_id: String,
}

/// 某账本有日记的日期列表（倒序，含字数与心情）。
pub async fn list_dates(ledger_id: &str) -> Result<Vec<DiaryDateItem>, IpcError> {
    ipc::call(
        "diary_list_dates",
        LedgerRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 取某账本某天日记（不存在时报错）。
pub async fn get(date: &str, ledger_id: &str) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_get",
        DateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 保存某账本某天日记，返回写入后的条目。
pub async fn upsert(
    date: &str,
    content: &str,
    mood: &str,
    ledger_id: &str,
) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_upsert",
        DiaryUpsertRequest {
            ledger_id: ledger_id.to_string(),
            date: date.to_string(),
            content: Some(content.to_string()),
            mood: Some(mood.to_string()),
        },
    )
    .await
}

/// 删除某账本某天的日记。
pub async fn delete(date: &str, ledger_id: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "diary_delete",
        DateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
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

/// 导入单个文件到指定账本（后端自动识别 UTF-8/UTF-16/GBK）。
pub async fn import_file(path: &str, date: &str, ledger_id: &str) -> Result<DiaryEntry, IpcError> {
    ipc::call(
        "diary_import_file",
        ImportFileRequest {
            path: path.to_string(),
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 把指定账本的日记导出到目录：`year`/`month` 为 `None` 表示不限。
pub async fn export(
    directory: &str,
    year: Option<i64>,
    month: Option<i64>,
    ledger_id: &str,
) -> Result<DiaryExportResult, IpcError> {
    ipc::call(
        "diary_export",
        DiaryExportRequest {
            ledger_id: ledger_id.to_string(),
            directory: directory.to_string(),
            year,
            month,
        },
    )
    .await
}
