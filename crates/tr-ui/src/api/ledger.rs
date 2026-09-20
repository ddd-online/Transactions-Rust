//! 账本域命令。
//!
//! | 命令 | 入参 |
//! |---|---|
//! | `ledger_list` | `{ id }`（`all` 或账本 id） |
//! | `ledger_create` | `{ name, description }` |
//! | `ledger_get` | `{ id }` |
//! | `ledger_update` | `{ id, name, description }` |
//! | `ledger_delete` | `{ id }` |

use serde::Serialize;
use tr_domain::dto::LedgerDto;

use crate::ipc::{self, IpcError};
use crate::store::ALL;

use super::IdRequest;

#[derive(Debug, Serialize)]
struct CreateRequest {
    name: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct UpdateRequest {
    id: String,
    name: String,
    description: String,
}

/// 查询全部账本（`id = "all"`）。
pub async fn list_all() -> Result<Vec<LedgerDto>, IpcError> {
    list(ALL).await
}

/// 按 `id` 查询（`"all"` 或逗号分隔的 id 列表）。
pub async fn list(id: &str) -> Result<Vec<LedgerDto>, IpcError> {
    ipc::call("ledger_list", IdRequest { id: id.to_string() }).await
}

/// 新建账本，返回新账本 id。
pub async fn create(name: &str, description: &str) -> Result<String, IpcError> {
    ipc::call(
        "ledger_create",
        CreateRequest {
            name: name.to_string(),
            description: description.to_string(),
        },
    )
    .await
}

/// 查询单个账本。
pub async fn get(id: &str) -> Result<LedgerDto, IpcError> {
    ipc::call("ledger_get", IdRequest { id: id.to_string() }).await
}

/// 修改账本名称与描述。
pub async fn update(id: &str, name: &str, description: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "ledger_update",
        UpdateRequest {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
        },
    )
    .await
}

/// 删除账本及其全部业务数据。
pub async fn delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("ledger_delete", IdRequest { id: id.to_string() }).await
}
