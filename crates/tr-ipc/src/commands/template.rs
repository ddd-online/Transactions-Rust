//! 消费模板命令。对照 Go `kernel/api/transaction_template_controller.go`。
//!
//! 入参形状按原 HTTP 语义逐字段对齐（模板请求体在 Go 里就是 snake_case）：
//! * `POST /templates { template_id?, ledger_id, template_name, transaction_type, category, tags, flags, description, sort_order? }`
//!   → `template_create`
//! * `GET /templates?ledgerId=xxx` → `template_list { ledgerId }`
//! * `DELETE /templates/:id` → `template_delete { id }`（同时接受 `templateId`）
//! * `PATCH /templates/:id/sort { ledgerId, sortOrder }` → `template_update_sort { id, ledgerId, sortOrder }`
//!
//! 原文案：`missing ledgerId`、`missing template id`（均 400）；
//! 模板校验文案（`模板名称不能为空` / `invalid transaction type: xxx` / `分类不能为空`）
//! 由服务层给出，状态码 500（与 Go 的普通 error 兜底一致）。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::TransactionTemplateDto;
use tr_domain::error::AppError;
use tr_service::transaction_template;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// 新建模板，返回新模板 ID。
#[tauri::command]
pub fn template_create(
    state: State<'_, AppState>,
    req: TransactionTemplateDto,
) -> ApiResult<String> {
    let workspace = state.workspace()?;
    let template_id = transaction_template::create(&workspace, &req)?;
    Ok(template_id)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TemplateListRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 列出某账本的全部模板。
#[tauri::command]
pub fn template_list(
    state: State<'_, AppState>,
    req: TemplateListRequest,
) -> ApiResult<Vec<TransactionTemplateDto>> {
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing ledgerId")));
    }

    let workspace = state.workspace()?;
    Ok(transaction_template::list_by_ledger_id(
        &workspace,
        &req.ledger_id,
    )?)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TemplateIdRequest {
    /// 原路径参数 `:id`（同时接受 `templateId`，方便界面侧沿用 DTO 命名）
    #[serde(alias = "templateId")]
    pub id: String,
}

/// 删除模板。
#[tauri::command]
pub fn template_delete(state: State<'_, AppState>, req: TemplateIdRequest) -> ApiResult<()> {
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing template id")));
    }

    let workspace = state.workspace()?;
    transaction_template::delete_by_id(&workspace, &req.id)?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TemplateSortRequest {
    /// 原路径参数 `:id`（同时接受 `templateId`）
    #[serde(alias = "templateId")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

/// 更新模板排序号。
#[tauri::command]
pub fn template_update_sort(state: State<'_, AppState>, req: TemplateSortRequest) -> ApiResult<()> {
    // 原路由 `/templates/:id/sort` 的 `:id` 不可能为空（空 id 匹配不到该路由）；
    // Tauri 命令没有路由层，这里显式挡掉，沿用同一条原文案。
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing template id")));
    }

    let workspace = state.workspace()?;
    transaction_template::update_sort_order(&workspace, &req.id, &req.ledger_id, req.sort_order)?;
    Ok(())
}
