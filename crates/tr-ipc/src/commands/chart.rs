//! 图表命令。对照 Go `kernel/api/chart_controller.go`。
//!
//! 入参形状按原 HTTP 语义逐字段对齐（图表 DTO 是 camelCase）：
//! * `POST /charts { ledgerId, title, granularity, lines, chartType }` → `chart_create`
//! * `DELETE /charts/:id` → `chart_delete { chartId }`（同时接受 `id`）
//! * `GET /charts?ledgerId=xxx` → `chart_list { ledgerId }`
//! * `PATCH /charts { chartId, title, granularity, lines, chartType, sortOrder }` → `chart_update`
//!
//! 原文案：`missing ledgerId`、`missing chart id`（均 400）；
//! `parse create chart request failed` / `parse update chart request failed` 是 Gin 绑定失败
//! 时的文案，在 Tauri 里请求体反序列化发生在进入命令体之前，无法复用同一文案（见汇报）。

use serde::Deserialize;
use tauri::State;

use tr_domain::dto::{ChartDto, CreateChartRequest, UpdateChartRequest};
use tr_domain::error::AppError;
use tr_service::chart;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// 新建图表，返回完整 DTO（`isPreset` 恒为 false）。
#[tauri::command]
pub fn chart_create(state: State<'_, AppState>, req: CreateChartRequest) -> ApiResult<ChartDto> {
    let workspace = state.workspace()?;
    Ok(chart::create(&workspace, &req)?)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChartIdRequest {
    /// 原路径参数 `:id`；图表 DTO 里的字段名是 `chartId`，两者都接受
    #[serde(rename = "chartId", alias = "id")]
    pub chart_id: String,
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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChartListRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
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
