//! 账本域命令。
//!
//! | 命令 | 入参 |
//! |---|---|
//! | `ledger_list` | `{ id }`（`all` 或账本 id） |
//! | `ledger_create` | `{ name, description }` |
//! | `ledger_get` | `{ id }` |
//! | `ledger_update` | `{ id, name, description }` |
//! | `ledger_delete` | `{ id }` |

use tr_domain::commands;
use tr_domain::dto::LedgerDto;
use tr_domain::wire::{CreateLedgerRequest, IdRequest, LedgerListRequest, UpdateLedgerRequest};

use crate::ipc::{self, IpcError};
use crate::store::ALL;

/// 查询全部账本（`id = "all"`）。
pub async fn list_all() -> Result<Vec<LedgerDto>, IpcError> {
    list(ALL).await
}

/// 按 `id` 查询（`"all"` 或逗号分隔的 id 列表）。
pub async fn list(id: &str) -> Result<Vec<LedgerDto>, IpcError> {
    ipc::call(
        commands::LEDGER_LIST,
        LedgerListRequest { id: id.to_string() },
    )
    .await
}

/// 新建账本，返回新账本 id。
pub async fn create(name: &str, description: &str) -> Result<String, IpcError> {
    ipc::call(
        commands::LEDGER_CREATE,
        CreateLedgerRequest {
            name: Some(name.to_string()),
            description: Some(description.to_string()),
        },
    )
    .await
}

/// 查询单个账本。
pub async fn get(id: &str) -> Result<LedgerDto, IpcError> {
    ipc::call(commands::LEDGER_GET, IdRequest { id: id.to_string() }).await
}

/// 修改账本名称与描述。
pub async fn update(id: &str, name: &str, description: &str) -> Result<(), IpcError> {
    ipc::call_void(
        commands::LEDGER_UPDATE,
        UpdateLedgerRequest {
            id: id.to_string(),
            name: Some(name.to_string()),
            description: Some(description.to_string()),
        },
    )
    .await
}

/// 删除账本及其全部业务数据。
pub async fn delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void(commands::LEDGER_DELETE, IdRequest { id: id.to_string() }).await
}
