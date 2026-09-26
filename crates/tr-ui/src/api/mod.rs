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
//! ## 入参 / 返回类型
//!
//! 请求与响应类型**全部来自 `tr_domain`**（`tr_domain::wire` 是请求/响应，
//! `tr_domain::dto` 是业务 DTO）：界面与内核共用同一份定义，字段名不可能漂移，
//! 也不需要"逐字照抄"或跨 crate 比对。命名本来就不统一（camelCase 与 snake_case 混用），
//! 那是既成契约，各类型的 `#[serde(rename)]` 逐字保留。
//!
//! ## 错误
//!
//! 所有函数返回 `Result<T, IpcError>`，**不**在这里做通知。
//! 是否提示、提示什么前缀，交给调用方用 [`crate::error_handler`] 决定
//! （命令封装与错误提示解耦：是否提示、用什么前缀由调用方决定）。

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
