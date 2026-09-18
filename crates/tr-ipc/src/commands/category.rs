//! 分类命令。对照 Go `kernel/api/category_controller.go`。
//!
//! 入参形状按原 HTTP 语义逐字段对齐（字段名都以 serde rename 写成原查询/路径/请求体里的名字）：
//! * `GET /categories?type=all|income|expense|transfer&ledgerId=xxx` → `category_list { type, ledgerId }`
//! * `POST /categories { ledgerId, name, transactionType, sortOrder }` → `category_create`
//!   （请求体直接用 [`CreateCategoryRequest`]，字段名与 Go 的 `dto.CreateCategoryRequest` 相同）
//! * `DELETE /categories/:name?type=...&ledgerId=...` → `category_delete { name, type, ledgerId }`
//! * `PATCH /categories/:name/sort { ledgerId, name, transactionType, sortOrder }` → `category_update_sort`
//!   （原实现从路径取 `name`、从请求体取其余字段；Rust 只有一份 `name`）
//! * `POST /categories/initialize { ledgerId }` → `category_initialize { ledgerId }`
//!
//! 兼容性说明：分类的删除/列表在原实现里用查询参数名 `type`，而创建/排序的请求体字段名是
//! `transactionType`。为避免界面侧两套叫法出错，`type` 字段同时接受 `transactionType`（serde alias），
//! 语义完全相同。
//!
//! 原文案：`missing required parameters`（缺参数，400）、`缺少 ledgerId 参数`（400）、
//! `invalid request: ...`（Gin 绑定失败，400）——最后一条在 Tauri 里由 serde 在进入命令体之前
//! 处理，无法复用同一文案（见汇报）。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{
    CategoryDto, CreateCategoryRequest, InitializeCategoriesResponse, UpdateCategorySortRequest,
};
use tr_domain::error::AppError;
use tr_service::category;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CategoryListRequest {
    /// 原查询参数 `type`（空字符串与 `all` 等价：不过滤）
    #[serde(rename = "type", alias = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 查询分类并补齐每个分类的记录数。`ledgerId` 为空时返回空数组（与原实现一致，不报错）。
#[tauri::command]
pub fn category_list(
    state: State<'_, AppState>,
    req: CategoryListRequest,
) -> ApiResult<Vec<CategoryDto>> {
    if req.ledger_id.is_empty() {
        return Ok(Vec::new());
    }

    let workspace = state.workspace()?;
    let categories = category::query_category(&workspace, &req.ledger_id, &req.transaction_type)?;

    let names: Vec<String> = categories
        .iter()
        .map(|category| category.name.clone())
        .collect();
    let counts = category::count_records_by_categories(&workspace, &req.ledger_id, &names)?;

    Ok(categories
        .iter()
        .map(|category| {
            let mut dto = CategoryDto::from(category);
            dto.record_count = counts.get(&category.name).copied().unwrap_or(0) as i32;
            dto
        })
        .collect())
}

/// 新建分类（返回 `()`，与原接口返回 nil 一致）。
#[tauri::command]
pub fn category_create(state: State<'_, AppState>, req: CreateCategoryRequest) -> ApiResult<()> {
    let workspace = state.workspace()?;
    category::create_category(&workspace, &req.ledger_id, &req.name, &req.transaction_type)?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CategoryDeleteRequest {
    /// 原路径参数 `:name`
    pub name: String,
    /// 原查询参数 `type`（同时接受请求体字段名 `transactionType`）
    #[serde(rename = "type", alias = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 删除分类（连带删除该分类下的标签）。
#[tauri::command]
pub fn category_delete(state: State<'_, AppState>, req: CategoryDeleteRequest) -> ApiResult<()> {
    if req.name.is_empty() || req.transaction_type.is_empty() || req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "missing required parameters",
        )));
    }

    let workspace = state.workspace()?;
    category::delete_category(&workspace, &req.ledger_id, &req.name, &req.transaction_type)?;
    Ok(())
}

/// 更新分类排序号。
#[tauri::command]
pub fn category_update_sort(
    state: State<'_, AppState>,
    req: UpdateCategorySortRequest,
) -> ApiResult<()> {
    let workspace = state.workspace()?;
    category::update_category_sort(
        &workspace,
        &req.ledger_id,
        &req.name,
        &req.transaction_type,
        req.sort_order,
    )?;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct InitializeCategoriesRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 为账本初始化默认分类与标签，返回 `{ categories, tags }`。
#[tauri::command]
pub fn category_initialize(
    state: State<'_, AppState>,
    req: InitializeCategoriesRequest,
) -> ApiResult<InitializeCategoriesResponse> {
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("缺少 ledgerId 参数")));
    }

    let workspace = state.workspace()?;
    let (categories, tags) = category::initialize_categories(&workspace, &req.ledger_id)?;
    Ok(InitializeCategoriesResponse { categories, tags })
}
