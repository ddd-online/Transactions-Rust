//! 错误处理语义。
//!
//! 两个入口：
//!
//! * [`notify_error`] —— 只通知、不处理，调用方自行决定后续行为。
//! * [`get_error_message`] —— 取出可读信息。
//!
//! 通知文案：标题 = `error_prefix`，描述 = `"{error_prefix}: {msg}"`
//! （`msg` 已经是 api 层拼好前缀之后的用户可见文案）。

use crate::ipc::IpcError;
use crate::notify::Notifier;

/// 只通知、不处理（例如事件回调里需要显式提示时）。
pub fn notify_error(error_prefix: &str, error: &IpcError) {
    Notifier::global().error(error_prefix.to_string(), Some(error.prefixed(error_prefix)));
}

/// 任意错误 → 可读字符串。
pub fn get_error_message(error: &IpcError) -> String {
    error.message().to_string()
}
