//! 命令封装（按业务域分模块）。
//!
//! | 本模块 | 命令前缀 |
//! |---|---|
//! | [`ledger`] | `ledger_*` |
//! | [`tr`] | `tr_*` |
//! | [`category`] | `category_*` |
//! | [`tag`] | `tag_*` |
//! | [`template`] | `template_*` |
//! | [`chart`] | `chart_*` |
//! | [`key_event`] | `key_event_*` |
//! | [`diary`] | `diary_*` |
//! | [`stock`] | `stock_*`（设置页要用的费用/标签/重置也已接入） |
//! | [`desktop`] | `window_control` / `workspace_*` / `config_*` / `dialog_open` / `devtools_*` |
//! | [`update`] | `update_check` / `update_download` / `update_install` / `update_cancel` |
//!
//! ## 字段命名的硬约束
//!
//! 请求结构的字段名**逐字照抄 `crates/tr-ipc/src/commands/*.rs` 里的 `*Request`**
//! （该目录是唯一权威，本层只做搬运，不做归一化）。命名本来就不统一：
//!
//! * camelCase：`ledgerId`、`transactionType`、`categoryTransactionType`、`tsRange`、`sortFields`
//! * snake_case：`ledger_id`、`category_transaction_type`、`transaction_id`、`sort_order`
//! * 查询参数式：`category_list { type, ledgerId }`、`tr_linked_by_date { date, ledger_id }`
//!
//! 响应类型直接复用 `tr_domain::dto` 的 DTO（界面与内核共享同一份定义，
//! 避免两侧字段漂移）；`window_control` 等桌面外壳命令的响应结构在本层声明。
//!
//! ## 错误
//!
//! 所有函数返回 `Result<T, IpcError>`，**不**在这里做通知。
//! 是否提示、提示什么前缀，交给调用方用 [`crate::error_handler`] 决定
//! （命令封装与错误提示解耦：是否提示、用什么前缀由调用方决定）。

use serde::Serialize;

pub mod category;
pub mod chart;
pub mod desktop;
pub mod diary;
pub mod key_event;
pub mod ledger;
pub mod stock;
pub mod tag;
pub mod template;
pub mod tr;
pub mod update;

// ---------------------------------------------------------------- 同形请求体（本层共享）
//
// 这几个形状在多个域里逐字重复（`{ id }` 5 份、`{ ledgerId }` 3 份），
// 合到本模块各留一份。**字段名与 serde 重命名逐字不变**——它们是与 `tr-ipc` 的硬契约。
// 只在 tr-ui 内部共享：界面侧的请求结构体仍然是"手抄 tr-ipc"的副本，不能跨 crate 共用。

/// 只带一个 `id` 字段的请求（`ledger_list` / `ledger_get` / `ledger_delete` /
/// `tr_delete` / `template_delete` / `key_event_image_delete`）。
#[derive(Debug, Serialize)]
pub(super) struct IdRequest {
    id: String,
}

/// 只带一个 `ledgerId`（camelCase）字段的请求
/// （`chart_list` / `category_initialize` / `template_list`）。
#[derive(Debug, Serialize)]
pub(super) struct LedgerIdRequest {
    #[serde(rename = "ledgerId")]
    ledger_id: String,
}

pub use crate::ipc::IpcError;
