//! 图表域命令。对照原 `app/src/backend/api/chart.ts` 与 `chart_controller.go`。
//!
//! 图表 DTO 是 camelCase（`chartId` / `ledgerId` / `chartType` / `isPreset` / `sortOrder`）。
//! `chart_delete` 的入参字段名是 `chartId`（命令同时接受 `id`，这里统一发送 `chartId`）。

use serde::Serialize;
use tr_domain::dto::{ChartDto, CreateChartRequest, UpdateChartRequest};

use crate::ipc::{self, IpcError};

#[derive(Debug, Serialize)]
struct ListRequest {
    #[serde(rename = "ledgerId")]
    ledger_id: String,
}

#[derive(Debug, Serialize)]
struct IdRequest {
    #[serde(rename = "chartId")]
    chart_id: String,
}

/// 列出某账本的全部图表（后端会先补齐预设图表）。
pub async fn list(ledger_id: &str) -> Result<Vec<ChartDto>, IpcError> {
    ipc::call(
        "chart_list",
        ListRequest {
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}

/// 新建图表，返回完整 DTO。
pub async fn create(request: CreateChartRequest) -> Result<ChartDto, IpcError> {
    ipc::call("chart_create", request).await
}

/// 更新图表，返回更新后的完整 DTO。
pub async fn update(request: UpdateChartRequest) -> Result<ChartDto, IpcError> {
    ipc::call("chart_update", request).await
}

/// 删除图表。
pub async fn delete(chart_id: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "chart_delete",
        IdRequest {
            chart_id: chart_id.to_string(),
        },
    )
    .await
}
