//! 消费模板命令。
//!
//! 入参形状是固定契约（模板请求体是 snake_case）：
//! * `template_create`：`{ template_id?, ledger_id, template_name, transaction_type, category, tags, flags, description, sort_order? }`
//! * `template_list { ledgerId }`
//! * `template_delete { id }`（同时接受 `templateId`）
//! * `template_update_sort { id, ledgerId, sortOrder }`
//!
//! 错误文案：`missing ledgerId`、`missing template id`（均 400）；
//! 模板校验文案（`模板名称不能为空` / `invalid transaction type: xxx` / `分类不能为空`）
//! 由服务层给出，状态码 500（普通 error 的兜底）。

use tauri::State;

use tr_domain::dto::TransactionTemplateDto;
use tr_domain::error::AppError;
use tr_domain::wire::{TemplateIdRequest, TemplateListRequest, TemplateSortRequest};
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

/// 更新模板排序号。
#[tauri::command]
pub fn template_update_sort(state: State<'_, AppState>, req: TemplateSortRequest) -> ApiResult<()> {
    // 空 id 必须在这里显式挡掉（Tauri 命令没有路由层可以替我们过滤），
    // 沿用同一条错误文案。
    if req.id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing template id")));
    }

    let workspace = state.workspace()?;
    transaction_template::update_sort_order(&workspace, &req.id, &req.ledger_id, req.sort_order)?;
    Ok(())
}
