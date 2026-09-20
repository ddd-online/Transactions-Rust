//! 分类域命令。
//!
//! 注意入参命名**不统一**（照抄 `crates/tr-ipc/src/commands/category.rs`）：
//! * `category_list` / `category_delete` 用查询参数名 `type`
//! * `category_create` / `category_update_sort` 用请求体字段名 `transactionType`
//! * `ledgerId` 一律 camelCase
//!
//! `type` 为空串或 `"all"` 表示不过滤。

use serde::Serialize;
use tr_domain::dto::{
    CategoryDto, CreateCategoryRequest, InitializeCategoriesResponse, UpdateCategorySortRequest,
};

use crate::ipc::{self, IpcError};

use super::LedgerIdRequest;

/// `"all"`（与 `type`/`transactionType` 的不过滤语义一致）。
pub const ALL: &str = "all";

#[derive(Debug, Serialize)]
struct ListRequest {
    #[serde(rename = "type")]
    transaction_type: String,
    #[serde(rename = "ledgerId")]
    ledger_id: String,
}

#[derive(Debug, Serialize)]
struct DeleteRequest {
    name: String,
    #[serde(rename = "type")]
    transaction_type: String,
    #[serde(rename = "ledgerId")]
    ledger_id: String,
}

/// 查询分类（含每个分类的记录数）。`ledger_id` 为空时后端返回空数组（不报错）。
pub async fn list(transaction_type: &str, ledger_id: &str) -> Result<Vec<CategoryDto>, IpcError> {
    ipc::call(
        "category_list",
        ListRequest {
            transaction_type: transaction_type.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 新建分类。
pub async fn create(ledger_id: &str, name: &str, transaction_type: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "category_create",
        CreateCategoryRequest {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            transaction_type: transaction_type.to_string(),
            sort_order: 0,
        },
    )
    .await
}

/// 删除分类（连带删除该分类下的标签）。
pub async fn delete(name: &str, transaction_type: &str, ledger_id: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "category_delete",
        DeleteRequest {
            name: name.to_string(),
            transaction_type: transaction_type.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 更新分类排序号。
pub async fn update_sort(
    ledger_id: &str,
    name: &str,
    transaction_type: &str,
    sort_order: i32,
) -> Result<(), IpcError> {
    ipc::call_void(
        "category_update_sort",
        UpdateCategorySortRequest {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            transaction_type: transaction_type.to_string(),
            sort_order,
        },
    )
    .await
}

/// 为账本初始化默认分类与标签，返回各自的创建条数。
pub async fn initialize(ledger_id: &str) -> Result<InitializeCategoriesResponse, IpcError> {
    ipc::call(
        "category_initialize",
        LedgerIdRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}
