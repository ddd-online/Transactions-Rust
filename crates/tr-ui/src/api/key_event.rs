//! 事件域命令。
//!
//! 事件/图片模型是 camelCase（`ledgerId` / `createdAt` / `filePath`），
//! 但**请求入参一律 snake_case**（`ledger_id`）—— 照抄 `tr-ipc/src/commands/key_event.rs`。
//!
//! `key_event_upsert` 是"有则更新、无则插入"，返回日期；`title` / `content` / `color`
//! 可选（缺省按空串处理）。

use tr_domain::models::{KeyEvent, KeyEventImage};
use tr_domain::wire::{
    IdRequest, KeyEventDateRequest, KeyEventImageAddRequest, KeyEventUpsertRequest, YearRequest,
};

use crate::ipc::{self, IpcError};

/// 某年的全部事件。
pub async fn list_by_year(year: &str, ledger_id: &str) -> Result<Vec<KeyEvent>, IpcError> {
    ipc::call(
        "key_event_list_by_year",
        YearRequest {
            year: year.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 某年有事件的日期列表（用于日历打点）。
pub async fn dates_by_year(year: &str, ledger_id: &str) -> Result<Vec<String>, IpcError> {
    ipc::call(
        "key_event_dates_by_year",
        YearRequest {
            year: year.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 按日期取事件（不存在时报错）。
pub async fn get(date: &str, ledger_id: &str) -> Result<KeyEvent, IpcError> {
    ipc::call(
        "key_event_get",
        KeyEventDateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 写入事件，返回日期。
pub async fn upsert(
    ledger_id: &str,
    date: &str,
    title: &str,
    content: &str,
    color: &str,
) -> Result<String, IpcError> {
    ipc::call(
        "key_event_upsert",
        KeyEventUpsertRequest {
            ledger_id: ledger_id.to_string(),
            date: date.to_string(),
            title: Some(title.to_string()),
            content: Some(content.to_string()),
            color: Some(color.to_string()),
        },
    )
    .await
}

/// 删除事件及其图片（记录 + 磁盘文件）。
pub async fn delete(date: &str, ledger_id: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "key_event_delete",
        KeyEventDateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 某天的图片列表。
pub async fn images_list(date: &str, ledger_id: &str) -> Result<Vec<KeyEventImage>, IpcError> {
    ipc::call(
        "key_event_images_list",
        KeyEventDateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 上传一张图片（base64 data URI）。
pub async fn image_add(date: &str, ledger_id: &str, data: &str) -> Result<KeyEventImage, IpcError> {
    ipc::call(
        "key_event_image_add",
        KeyEventImageAddRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
            data: Some(data.to_string()),
        },
    )
    .await
}

/// 删除一张图片。
pub async fn image_delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("key_event_image_delete", IdRequest { id: id.to_string() }).await
}
