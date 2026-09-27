//! 待办域命令封装。字段名以 `crates/tr-ipc/src/commands/todo.rs` 为准（**唯一权威**）。
//!
//! | 命令 | 入参 |
//! |---|---|
//! | `todo_cards` | `{ ledger_id }` → 卡片 + 卡下进行中的事项（进度记录内嵌） |
//! | `todo_history` | `{ ledger_id }` → 已完成的事项（含主题名与进度记录） |
//! | `todo_card_create` | `{ ledger_id, title }` |
//! | `todo_card_delete` | `{ id }` |
//! | `todo_item_create` | `{ ledger_id, card_id, title, start_date, due_date, urgency, importance }` |
//! | `todo_item_update` | `{ ledger_id, id, …同上… }` |
//! | `todo_item_status` | `{ ledger_id, id, status }`（`doing` / `done`） |
//! | `todo_item_delete` | `{ id }` |
//! | `todo_progress_add` | `{ ledger_id, item_id, content }` |
//! | `todo_progress_delete` | `{ id }` |

use tr_domain::dto::{TodoCardDto, TodoHistoryDto, TodoItemDto, TodoProgressDto};
use tr_domain::wire::{
    IdRequest, LedgerIdRequest, TodoCardCreateRequest, TodoItemCreateRequest,
    TodoItemStatusRequest, TodoItemUpdateRequest, TodoProgressCreateRequest,
};

use crate::ipc::{self, IpcError};

/// 卡片视图（卡片 + 卡下进行中的事项）。
pub async fn cards(ledger_id: &str) -> Result<Vec<TodoCardDto>, IpcError> {
    ipc::call(
        "todo_cards",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 历史（已完成的事项）。
pub async fn history(ledger_id: &str) -> Result<Vec<TodoHistoryDto>, IpcError> {
    ipc::call(
        "todo_history",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 新建卡片（主题）。
pub async fn card_create(ledger_id: &str, title: &str) -> Result<TodoCardDto, IpcError> {
    ipc::call(
        "todo_card_create",
        TodoCardCreateRequest {
            ledger_id: ledger_id.to_string(),
            title: title.to_string(),
        },
    )
    .await
}

/// 删卡片（连它的事项与进度记录一起删）。
pub async fn card_delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("todo_card_delete", IdRequest { id: id.to_string() }).await
}

/// 新建事项。日期是日粒度 `YYYY-MM-DD`（空串 = 没填）。
#[allow(clippy::too_many_arguments)]
pub async fn item_create(
    ledger_id: &str,
    card_id: &str,
    title: &str,
    start_date: &str,
    due_date: &str,
    urgency: i32,
    importance: i32,
) -> Result<TodoItemDto, IpcError> {
    ipc::call(
        "todo_item_create",
        TodoItemCreateRequest {
            ledger_id: ledger_id.to_string(),
            card_id: card_id.to_string(),
            title: title.to_string(),
            start_date: start_date.to_string(),
            due_date: due_date.to_string(),
            urgency,
            importance,
        },
    )
    .await
}

/// 编辑事项（不动状态）。
#[allow(clippy::too_many_arguments)]
pub async fn item_update(
    ledger_id: &str,
    id: &str,
    title: &str,
    start_date: &str,
    due_date: &str,
    urgency: i32,
    importance: i32,
) -> Result<TodoItemDto, IpcError> {
    ipc::call(
        "todo_item_update",
        TodoItemUpdateRequest {
            ledger_id: ledger_id.to_string(),
            id: id.to_string(),
            title: title.to_string(),
            start_date: start_date.to_string(),
            due_date: due_date.to_string(),
            urgency,
            importance,
        },
    )
    .await
}

/// 改状态（`done` 会进历史，`doing` 会退回卡片）。
pub async fn item_status(ledger_id: &str, id: &str, status: &str) -> Result<TodoItemDto, IpcError> {
    ipc::call(
        "todo_item_status",
        TodoItemStatusRequest {
            ledger_id: ledger_id.to_string(),
            id: id.to_string(),
            status: status.to_string(),
        },
    )
    .await
}

/// 删事项（连它的进度记录一起删）。
pub async fn item_delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("todo_item_delete", IdRequest { id: id.to_string() }).await
}

/// 追加一条进度记录。
pub async fn progress_add(
    ledger_id: &str,
    item_id: &str,
    content: &str,
) -> Result<TodoProgressDto, IpcError> {
    ipc::call(
        "todo_progress_add",
        TodoProgressCreateRequest {
            ledger_id: ledger_id.to_string(),
            item_id: item_id.to_string(),
            content: content.to_string(),
        },
    )
    .await
}

/// 删一条进度记录。
pub async fn progress_delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("todo_progress_delete", IdRequest { id: id.to_string() }).await
}
