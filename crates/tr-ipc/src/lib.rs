//! tr-ipc —— 界面与内核之间的唯一通道（Tauri IPC 命令面）。
//!
//! ## 与原 HTTP API 的对应关系
//!
//! 原实现的 68 个 REST 路由在这里变成 68 个 `#[tauri::command]`，一一对应，
//! 命名规则 `<域>_<动作>`（例如 `POST /transactions/query` → `tr_query`）。
//! 命令**统一只收一个 `req` 结构体参数**，字段名与原 JSON body 逐字段一致：
//!
//! ```text
//! invoke('tr_query', { req: { ledgerId: '...', offset: 0, limit: 20, ... } })
//! ```
//!
//! ## 错误信封
//!
//! 命令返回 `Result<T, ApiError>`：
//! * 成功 → JS 侧 promise 直接 resolve 为数据本身（无包装层）
//! * 失败 → promise reject，载荷为 `{"code":-1,"msg":"...","status":500}`
//!
//! 界面侧的 `api::call()` 包装器据此复刻原 `api-client.ts` 的行为：
//! 错误文案为 `"{前缀}: {msg}"`，且 `msg == "未打开工作空间"` 时派发 `workspace-required` 事件。
//! 因此"成功无包装、失败带 code/msg"这一对外语义与原实现一致。

pub mod commands;
pub mod error;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use state::AppState;
