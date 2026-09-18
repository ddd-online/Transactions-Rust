//! 账本命令。对照 Go `kernel/api/ledger_controller.go`。
//!
//! 入参形状按原 HTTP 语义逐字段对齐：
//! * `GET /ledgers?id=all|uuid1,uuid2` → `ledger_list { id }`
//! * `POST /ledgers { name, description }` → `ledger_create`
//! * `GET /ledgers/:id` → `ledger_get { id }`
//! * `PATCH /ledgers/:id { name, description }` → `ledger_update`
//! * `DELETE /ledgers/:id` → `ledger_delete { id }`
//!
//! 原实现用 `map[string]any` 取值，缺失字段报 `name在请求体中不存在`；
//! 因此这里把 `name` 声明为 `Option<String>` 后手工校验，保持同一文案
//! （若直接声明为 `String`，serde 会在进入函数体前失败，文案就变了）。

use serde::Deserialize;
use tauri::State;

use tr_domain::consts;
use tr_domain::dto::LedgerDto;
use tr_domain::error::AppError;
use tr_service::ledger;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct LedgerListRequest {
    #[serde(default)]
    pub id: String,
}

/// 列出一个、多个或全部账本。
#[tauri::command]
pub fn ledger_list(
    state: State<'_, AppState>,
    req: LedgerListRequest,
) -> ApiResult<Vec<LedgerDto>> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing required query parameter: id",
        )));
    }

    tracing::debug!("IPC ledger_list id={}", req.id);
    let workspace = state.workspace()?;
    let ledgers = if req.id == consts::ALL {
        ledger::list_all_ledger(&workspace)?
    } else {
        let mut collected = Vec::new();
        for raw in req.id.split(',') {
            let id = raw.trim();
            let found = ledger::query_ledger_by_id(&workspace, id).map_err(|error| {
                ApiError::from(AppError::internal(format!("查询账本 {id} 失败: {error}")))
            })?;
            collected.push(found);
        }
        collected
    };

    Ok(ledgers.iter().map(LedgerDto::from).collect())
}

#[derive(Debug, Deserialize)]
pub struct CreateLedgerRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// 新建账本，返回新账本 ID。
#[tauri::command]
pub fn ledger_create(state: State<'_, AppState>, req: CreateLedgerRequest) -> ApiResult<String> {
    let Some(name) = req.name else {
        return Err(ApiError::from(AppError::bad_request(
            "name在请求体中不存在",
        )));
    };
    let workspace = state.workspace()?;
    let id = ledger::create_ledger(&workspace, &name, req.description.as_deref().unwrap_or(""))?;
    Ok(id)
}

#[derive(Debug, Deserialize)]
pub struct LedgerIdRequest {
    pub id: String,
}

/// 查询单个账本；不存在返回 404（与原 `getLedger` 一致）。
#[tauri::command]
pub fn ledger_get(state: State<'_, AppState>, req: LedgerIdRequest) -> ApiResult<LedgerDto> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing ledger id")));
    }
    let workspace = state.workspace()?;
    let found = ledger::query_ledger_by_id(&workspace, &req.id)
        .map_err(|error| ApiError::from(AppError::not_found(error.to_string())))?;
    Ok(LedgerDto::from(&found))
}

#[derive(Debug, Deserialize)]
pub struct UpdateLedgerRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// 修改账本名称与描述。
#[tauri::command]
pub fn ledger_update(state: State<'_, AppState>, req: UpdateLedgerRequest) -> ApiResult<()> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing ledger id")));
    }
    let Some(name) = req.name else {
        return Err(ApiError::from(AppError::bad_request(
            "name在请求体中不存在",
        )));
    };
    let workspace = state.workspace()?;
    ledger::modify_ledger(
        &workspace,
        &req.id,
        &name,
        req.description.as_deref().unwrap_or(""),
    )?;
    Ok(())
}

/// 删除账本及其全部业务数据。
#[tauri::command]
pub fn ledger_delete(state: State<'_, AppState>, req: LedgerIdRequest) -> ApiResult<()> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing ledger id")));
    }
    let workspace = state.workspace()?;
    ledger::delete_ledger_by_id(&workspace, &req.id)?;
    Ok(())
}
