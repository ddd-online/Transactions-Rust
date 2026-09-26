//! 图表命令。
//!
//! 入参形状是固定契约（图表 DTO 是 camelCase）：
//! * `chart_create`：`{ ledgerId, title, granularity, lines, chartType }`
//! * `chart_delete { chartId }`（同时接受 `id`）
//! * `chart_list { ledgerId }`
//! * `chart_update`：`{ chartId, title, granularity, lines, chartType, sortOrder }`
//!
//! 错误文案：`missing ledgerId`、`missing chart id`（均 400）；
//! `parse create chart request failed` / `parse update chart request failed` 是请求体绑定失败
//! 时的文案，在 Tauri 里请求体反序列化发生在进入命令体之前，无法复用同一文案（见汇报）。

use tauri::State;

use tr_domain::dto::{ChartDto, CreateChartRequest, UpdateChartRequest};
use tr_domain::error::AppError;
use tr_domain::wire::{ChartIdRequest, ChartListRequest};
use tr_service::chart;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// 新建图表，返回完整 DTO（`isPreset` 恒为 false）。
#[tauri::command]
pub fn chart_create(state: State<'_, AppState>, req: CreateChartRequest) -> ApiResult<ChartDto> {
    let workspace = state.workspace()?;
    Ok(chart::create(&workspace, &req)?)
}

/// 删除图表。
#[tauri::command]
pub fn chart_delete(state: State<'_, AppState>, req: ChartIdRequest) -> ApiResult<()> {
    if req.chart_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing chart id")));
    }

    let workspace = state.workspace()?;
    chart::delete_by_id(&workspace, &req.chart_id)?;
    Ok(())
}

/// 列出某账本的全部图表（会先补齐预设图表，失败只告警）。
#[tauri::command]
pub fn chart_list(state: State<'_, AppState>, req: ChartListRequest) -> ApiResult<Vec<ChartDto>> {
    if req.ledger_id.is_empty() {
        return Err(ApiError::from(AppError::bad_request("missing ledgerId")));
    }

    let workspace = state.workspace()?;
    Ok(chart::list_by_ledger_id(&workspace, &req.ledger_id)?)
}

/// 更新图表，返回更新后的完整 DTO。
#[tauri::command]
pub fn chart_update(state: State<'_, AppState>, req: UpdateChartRequest) -> ApiResult<ChartDto> {
    let workspace = state.workspace()?;
    Ok(chart::update(&workspace, &req)?)
}
