//! IPC 命令实现。
//!
//! 命令按业务域分模块：`ledger` / `tr` / `category` / `tag` / `template` / `chart` /
//! `key_event` / `diary` / `stock`。
//!
//! 每个命令都用 `#[tauri::command]` 标注、接收 `tauri::State<'_, AppState>`，
//! 由 `src-tauri` 的 `generate_handler![]` 集中注册（命令清单因此只在一处）。

pub mod category;
pub mod chart;
pub mod diary;
pub mod key_event;
pub mod ledger;
pub mod stock;
pub mod tag;
pub mod template;
pub mod tr;

/// 入参契约锁定测试（见文件头说明：字段名改动即破坏契约）。
#[cfg(test)]
mod contract;

pub use category::*;
pub use chart::*;
pub use diary::*;
pub use key_event::*;
pub use ledger::*;
pub use stock::*;
pub use tag::*;
pub use template::*;
pub use tr::*;

use serde::Deserialize;

/// 无参数命令的占位入参。
///
/// 约定：**每个命令都只收一个 `req`**（界面侧的 `call()` 统一发送 `{ req: ... }`），
/// 因此没有入参的命令也声明一个空结构体，避免"有的命令带 req、有的不带"这种不一致。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct EmptyRequest {}
