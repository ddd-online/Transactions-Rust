//! 消费模板域命令。
//!
//! 模板请求体在 `tr-ipc` 里是 **snake_case**（`template_id` / `template_name` /
//! `transaction_type` / `sort_order`），但列表/排序的查询参数是 camelCase
//! （`ledgerId` / `sortOrder`）—— 两种混用是既成契约。

use tr_domain::dto::TransactionTemplateDto;
use tr_domain::wire::{TemplateIdRequest, TemplateListRequest, TemplateSortRequest};

use crate::ipc::{self, IpcError};

/// 新建模板，返回新模板 id。
pub async fn create(template: TransactionTemplateDto) -> Result<String, IpcError> {
    ipc::call("template_create", template).await
}

/// 列出某账本的全部模板。
pub async fn list(ledger_id: &str) -> Result<Vec<TransactionTemplateDto>, IpcError> {
    ipc::call(
        "template_list",
        TemplateListRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 删除模板。
pub async fn delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("template_delete", TemplateIdRequest { id: id.to_string() }).await
}

/// 更新模板排序号。
pub async fn update_sort(id: &str, ledger_id: &str, sort_order: i32) -> Result<(), IpcError> {
    ipc::call_void(
        "template_update_sort",
        TemplateSortRequest {
            id: id.to_string(),
            ledger_id: ledger_id.to_string(),
            sort_order,
        },
    )
    .await
}
