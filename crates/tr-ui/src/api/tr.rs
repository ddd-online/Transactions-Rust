//! 消费记录域命令。
//!
//! 入参形状以 `crates/tr-ipc/src/commands/tr.rs` 为准：
//!
//! | 命令 | 入参形状 |
//! |---|---|
//! | `tr_query` | [`TrQueryCondition`]（`ledgerId` / `offset` / `limit` / `tsRange` / `items` / `sortFields`） |
//! | `tr_chart_data` | [`ChartQueryRequest`]（`ledgerId` / `tsRange` / `granularity` / `lines`） |
//! | `tr_create` | [`TransactionRecordDto`] |
//! | `tr_batch_create` | `Vec<TransactionRecordDto>`（命令的 `req` 就是数组本身） |
//! | `tr_delete` | `{ id }` |
//! | `tr_link` | `{ transaction_id, date }`（**snake_case**） |
//! | `tr_unlink` | `{ transaction_id }`（**snake_case**） |
//! | `tr_linked_by_date` | `{ date, ledger_id }`（**snake_case**） |

use serde::Serialize;
use tr_domain::dto::{
    ChartQueryRequest, ChartQueryResponse, TrQueryCondition, TrQueryResult, TransactionRecordDto,
};

use crate::ipc::{self, IpcError};

#[derive(Debug, Serialize)]
struct IdRequest {
    id: String,
}

#[derive(Debug, Serialize)]
struct LinkRequest {
    transaction_id: String,
    date: String,
}

#[derive(Debug, Serialize)]
struct UnlinkRequest {
    transaction_id: String,
}

#[derive(Debug, Serialize)]
struct LinkedByDateRequest {
    date: String,
    ledger_id: String,
}

/// 构造一份"第 `page` 页、每页 `page_size` 条"的默认查询条件。
///
/// `offset = (page - 1) * page_size`，`limit = page_size`；
/// `page` 从 1 开始（与 `tr_query_result.page` 的语义一致）。
pub fn default_condition(ledger_id: &str, page: i32, page_size: i32) -> TrQueryCondition {
    let page = page.max(1);
    let page_size = page_size.max(1);
    TrQueryCondition {
        ledger_id: ledger_id.to_string(),
        offset: i64::from(page - 1) * i64::from(page_size),
        limit: i64::from(page_size),
        ts_range: Vec::new(),
        items: Vec::new(),
        sort_fields: Vec::new(),
    }
}

/// 条件查询。条件项之间 OR，项内 AND；`trStatistics` 覆盖**全部**命中记录而非当前页。
pub async fn query(condition: TrQueryCondition) -> Result<TrQueryResult, IpcError> {
    ipc::call("tr_query", condition).await
}

/// 图表逐曲线分桶数据。
pub async fn chart_data(request: ChartQueryRequest) -> Result<ChartQueryResponse, IpcError> {
    ipc::call("tr_chart_data", request).await
}

/// 新建一条记录，返回记录 id。
pub async fn create(record: TransactionRecordDto) -> Result<String, IpcError> {
    ipc::call("tr_create", record).await
}

/// 批量新建，返回成功条数（错误文案形如 `record 1: ...`）。
pub async fn batch_create(records: Vec<TransactionRecordDto>) -> Result<i32, IpcError> {
    ipc::call("tr_batch_create", records).await
}

/// 删除记录及其标签关联。
pub async fn delete(id: &str) -> Result<(), IpcError> {
    ipc::call_void("tr_delete", IdRequest { id: id.to_string() }).await
}

/// 关联到关键事件，返回日期。
pub async fn link(transaction_id: &str, date: &str) -> Result<String, IpcError> {
    ipc::call(
        "tr_link",
        LinkRequest {
            transaction_id: transaction_id.to_string(),
            date: date.to_string(),
        },
    )
    .await
}

/// 解除关联，返回记录 id。
pub async fn unlink(transaction_id: &str) -> Result<String, IpcError> {
    ipc::call(
        "tr_unlink",
        UnlinkRequest {
            transaction_id: transaction_id.to_string(),
        },
    )
    .await
}

/// 某天已关联的记录（含标签）。
pub async fn linked_by_date(
    date: &str,
    ledger_id: &str,
) -> Result<Vec<TransactionRecordDto>, IpcError> {
    ipc::call(
        "tr_linked_by_date",
        LinkedByDateRequest {
            date: date.to_string(),
            ledger_id: ledger_id.to_string(),
        },
    )
    .await
}
