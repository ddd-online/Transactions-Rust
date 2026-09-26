//! 标签域命令。
//!
//! 与分类域同构，但查询参数名是 `categoryTransactionType`（分类域是 `type`）——照抄
//! `crates/tr-ipc/src/commands/tag.rs`，不要"顺手统一"。

use tr_domain::dto::{CreateTagRequest, TagDto, UpdateTagSortRequest};
use tr_domain::wire::{TagDeleteRequest, TagListRequest};

use crate::ipc::{self, IpcError};

/// 查询标签（含每个标签的记录数）。`ledger_id` 为空时后端返回空数组（不报错）。
pub async fn list(
    category_transaction_type: &str,
    ledger_id: &str,
) -> Result<Vec<TagDto>, IpcError> {
    ipc::call(
        "tag_list",
        TagListRequest {
            category_transaction_type: category_transaction_type.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 新建标签。
pub async fn create(
    ledger_id: &str,
    name: &str,
    category_transaction_type: &str,
) -> Result<(), IpcError> {
    ipc::call_void(
        "tag_create",
        CreateTagRequest {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            category_transaction_type: category_transaction_type.to_string(),
            sort_order: 0,
        },
    )
    .await
}

/// 删除标签（连带清理交易记录上的该标签关联）。
pub async fn delete(
    name: &str,
    category_transaction_type: &str,
    ledger_id: &str,
) -> Result<(), IpcError> {
    ipc::call_void(
        "tag_delete",
        TagDeleteRequest {
            name: name.to_string(),
            category_transaction_type: category_transaction_type.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 更新标签排序号。
pub async fn update_sort(
    ledger_id: &str,
    name: &str,
    category_transaction_type: &str,
    sort_order: i32,
) -> Result<(), IpcError> {
    ipc::call_void(
        "tag_update_sort",
        UpdateTagSortRequest {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            category_transaction_type: category_transaction_type.to_string(),
            sort_order,
        },
    )
    .await
}
