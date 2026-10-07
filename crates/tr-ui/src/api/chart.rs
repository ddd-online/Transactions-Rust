//! 图表域命令。
//!
//! 图表 DTO 是 camelCase（`chartId` / `ledgerId` / `chartType` / `isPreset` / `sortOrder`）。
//! `chart_delete` 的入参字段名是 `chartId`（命令同时接受 `id`，这里统一发送 `chartId`）。
//! 拖拽排序走 `chart_update_sort`（只带 `chartId` + `sortOrder`，不碰图表内容）。

use tr_domain::commands;
use tr_domain::dto::{ChartDto, CreateChartRequest, UpdateChartRequest, UpdateChartSortRequest};
use tr_domain::wire::{ChartIdRequest, ChartListRequest};

use crate::ipc::{self, IpcError};

/// 列出某账本的全部图表（后端会先补齐预设图表）。
pub async fn list(ledger_id: &str) -> Result<Vec<ChartDto>, IpcError> {
    ipc::call(
        commands::CHART_LIST,
        ChartListRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 新建图表，返回完整 DTO。
pub async fn create(request: CreateChartRequest) -> Result<ChartDto, IpcError> {
    ipc::call(commands::CHART_CREATE, request).await
}

/// 更新图表，返回更新后的完整 DTO。
pub async fn update(request: UpdateChartRequest) -> Result<ChartDto, IpcError> {
    ipc::call(commands::CHART_UPDATE, request).await
}

/// 删除图表。
pub async fn delete(chart_id: &str) -> Result<(), IpcError> {
    ipc::call_void(
        commands::CHART_DELETE,
        ChartIdRequest {
            chart_id: chart_id.to_string(),
        },
    )
    .await
}

/// 拖动排序：只写 `sortOrder`。
pub async fn update_sort(chart_id: &str, sort_order: i32) -> Result<(), IpcError> {
    ipc::call_void(
        commands::CHART_UPDATE_SORT,
        UpdateChartSortRequest {
            chart_id: chart_id.to_string(),
            sort_order,
        },
    )
    .await
}
