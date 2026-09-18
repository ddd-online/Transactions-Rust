//! 命令封装（按业务域分模块）。
//!
//! 对照原 `app/src/backend/api/*.ts` 的 10 个模块，一一对应：
//!
//! | 本模块 | 原文件 | 命令前缀 |
//! |---|---|---|
//! | [`ledger`] | `ledger.ts` | `ledger_*` |
//! | [`tr`] | `tr.ts` | `tr_*` |
//! | [`category`] | `category.ts` | `category_*` |
//! | [`tag`] | `tag.ts` | `tag_*` |
//! | [`template`] | `template.ts` | `template_*` |
//! | [`chart`] | `chart.ts` | `chart_*` |
//! | [`key_event`] | `key-event.ts` | `key_event_*` |
//! | [`diary`] | `diary.ts` | `diary_*` |
//! | [`stock`] | `stock.ts` | `stock_*`（设置页要用的费用/标签/重置已在 P6-a 接入） |
//! | [`desktop`] | `workspace.ts` + `electronAPI` | `window_control` / `workspace_*` / `config_*` / `dialog_open` / `devtools_*` |
//! | [`update`] | `electronAPI` 的 `update:*` | `update_check` / `update_download` / `update_install` / `update_cancel` |
//!
//! ## 字段命名的硬约束
//!
//! 请求结构的字段名**逐字照抄 `crates/tr-ipc/src/commands/*.rs` 里的 `*Request`**
//! （该目录是唯一权威，本层只做搬运，不做归一化）。原实现的命名本来就不统一：
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
//! （与原实现把 api-client 与 notification 解耦的设计一致）。

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

pub use crate::ipc::IpcError;
