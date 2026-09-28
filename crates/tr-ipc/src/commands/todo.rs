//! 待办命令。
//!
//! 入参字段名全部是 **snake_case**（`ledger_id` / `card_id` / `start_date` …），这是固定契约；
//! 删除类复用 `IdRequest`（只有 id）—— 界面上的行本来就来自当前账本，删除不需要再带账本。
//!
//! `urgency` / `importance` 是 -5..=5 的整数，越界由服务层报 400（文案在那儿）。

use tauri::State;

use tr_domain::dto::{TodoCardDto, TodoHistoryDto, TodoItemDto, TodoProgressDto};
use tr_domain::wire::{
    IdRequest, LedgerIdRequest, TodoCardCreateRequest, TodoCardSortRequest, TodoItemCreateRequest,
    TodoItemStatusRequest, TodoItemUpdateRequest, TodoProgressCreateRequest,
    TodoProgressDoneRequest,
};
use tr_service::todo;

use crate::error::ApiResult;
use crate::AppState;

use super::require_ledger_id;

/// 卡片视图：卡片 + 卡下进行中的事项（进度记录内嵌）。
#[tauri::command]
pub fn todo_cards(state: State<'_, AppState>, req: LedgerIdRequest) -> ApiResult<Vec<TodoCardDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::list_cards(&workspace, &req.ledger_id)?)
}

/// 历史：已完成的事项（含主题名与进度记录）。
#[tauri::command]
pub fn todo_history(
    state: State<'_, AppState>,
    req: LedgerIdRequest,
) -> ApiResult<Vec<TodoHistoryDto>> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::list_history(&workspace, &req.ledger_id)?)
}

/// 新建卡片（主题）。
#[tauri::command]
pub fn todo_card_create(
    state: State<'_, AppState>,
    req: TodoCardCreateRequest,
) -> ApiResult<TodoCardDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::create_card(&workspace, &req.ledger_id, &req.title)?)
}

/// 删卡片（连它的事项与进度记录一起删）。
#[tauri::command]
pub fn todo_card_delete(state: State<'_, AppState>, req: IdRequest) -> ApiResult<()> {
    super::require(!req.id.is_empty(), "id is required")?;
    let workspace = state.workspace()?;
    todo::delete_card(&workspace, &req.id)?;
    Ok(())
}

/// 拖动排序：把一张卡片挪到 `sort_order`。
#[tauri::command]
pub fn todo_card_sort(state: State<'_, AppState>, req: TodoCardSortRequest) -> ApiResult<()> {
    super::require(!req.id.is_empty(), "id is required")?;
    let workspace = state.workspace()?;
    todo::update_card_sort(&workspace, &req.id, req.sort_order)?;
    Ok(())
}

/// 在卡片下新建事项。
#[tauri::command]
pub fn todo_item_create(
    state: State<'_, AppState>,
    req: TodoItemCreateRequest,
) -> ApiResult<TodoItemDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::create_item(
        &workspace,
        &req.ledger_id,
        &req.card_id,
        &req.title,
        &req.start_date,
        &req.due_date,
        req.urgency,
        req.importance,
    )?)
}

/// 编辑事项（不动状态）。
#[tauri::command]
pub fn todo_item_update(
    state: State<'_, AppState>,
    req: TodoItemUpdateRequest,
) -> ApiResult<TodoItemDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::update_item(
        &workspace,
        &req.ledger_id,
        &req.id,
        &req.title,
        &req.start_date,
        &req.due_date,
        req.urgency,
        req.importance,
    )?)
}

/// 改状态：`doing` / `done`。
#[tauri::command]
pub fn todo_item_status(
    state: State<'_, AppState>,
    req: TodoItemStatusRequest,
) -> ApiResult<TodoItemDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::set_item_status(
        &workspace,
        &req.ledger_id,
        &req.id,
        &req.status,
    )?)
}

/// 删事项（连它的进度记录一起删）。
#[tauri::command]
pub fn todo_item_delete(state: State<'_, AppState>, req: IdRequest) -> ApiResult<()> {
    super::require(!req.id.is_empty(), "id is required")?;
    let workspace = state.workspace()?;
    todo::delete_item(&workspace, &req.id)?;
    Ok(())
}

/// 追加一条进度记录。
#[tauri::command]
pub fn todo_progress_add(
    state: State<'_, AppState>,
    req: TodoProgressCreateRequest,
) -> ApiResult<TodoProgressDto> {
    require_ledger_id(&req.ledger_id)?;
    let workspace = state.workspace()?;
    Ok(todo::add_progress(
        &workspace,
        &req.ledger_id,
        &req.item_id,
        &req.content,
    )?)
}

/// 删一条进度记录。
#[tauri::command]
pub fn todo_progress_delete(state: State<'_, AppState>, req: IdRequest) -> ApiResult<()> {
    super::require(!req.id.is_empty(), "id is required")?;
    let workspace = state.workspace()?;
    todo::delete_progress(&workspace, &req.id)?;
    Ok(())
}

/// 给一条进度记录打勾 / 取消打勾。
#[tauri::command]
pub fn todo_progress_done(
    state: State<'_, AppState>,
    req: TodoProgressDoneRequest,
) -> ApiResult<()> {
    super::require(!req.id.is_empty(), "id is required")?;
    let workspace = state.workspace()?;
    todo::set_progress_done(&workspace, &req.id, req.done)?;
    Ok(())
}
