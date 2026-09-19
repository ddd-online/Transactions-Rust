//! 错误处理语义。
//!
//! 四个入口：
//!
//! * [`try_or_fallback`] —— 纯恢复，**不通知**，调用方自行决定是否提示。
//! * [`with_error_handling`] —— 查询模式：通知 + 返回兜底值。
//! * [`with_error_handling_rethrow`] —— 变更模式：通知 + 重新抛出。
//! * [`get_error_message`] —— 取出可读信息。
//!
//! 接收的是**已经构造好的 `Future`**（`ipc::call(...)` 的返回值），而不是惰性 thunk：
//! Rust 的 future 本身就必须先构造再 await，再加一层闭包只会让调用点更啰嗦，没有收益。
//!
//! 通知文案：标题 = `error_prefix`，描述 = `"{error_prefix}: {msg}"`
//! （`msg` 已经是 api 层拼好前缀之后的用户可见文案）。

use std::future::Future;

use crate::ipc::IpcError;
use crate::notify::Notifier;

/// 纯错误恢复：失败时返回兜底值，**不产生任何通知**。
pub async fn try_or_fallback<T, F, Fut>(task: Fut, fallback: F) -> T
where
    Fut: Future<Output = Result<T, IpcError>>,
    F: FnOnce() -> T,
{
    match task.await {
        Ok(value) => value,
        Err(_) => fallback(),
    }
}

/// 查询模式：失败时通知（标题=`error_prefix`，描述=`"{error_prefix}: {msg}"`），
/// 然后返回兜底值，不向上传播错误。
pub async fn with_error_handling<T, F, Fut>(task: Fut, error_prefix: &str, fallback: F) -> T
where
    Fut: Future<Output = Result<T, IpcError>>,
    F: FnOnce() -> T,
{
    match task.await {
        Ok(value) => value,
        Err(error) => {
            notify_error(error_prefix, &error);
            fallback()
        }
    }
}

/// 变更模式：失败时通知并重新抛出，由调用方决定后续行为。
pub async fn with_error_handling_rethrow<T, Fut>(
    task: Fut,
    error_prefix: &str,
) -> Result<T, IpcError>
where
    Fut: Future<Output = Result<T, IpcError>>,
{
    match task.await {
        Ok(value) => Ok(value),
        Err(error) => {
            notify_error(error_prefix, &error);
            Err(error)
        }
    }
}

/// 只通知、不处理（例如事件回调里需要显式提示时）。
pub fn notify_error(error_prefix: &str, error: &IpcError) {
    Notifier::global().error(error_prefix.to_string(), Some(error.prefixed(error_prefix)));
}

/// 任意错误 → 可读字符串。
pub fn get_error_message(error: &IpcError) -> String {
    error.message().to_string()
}
