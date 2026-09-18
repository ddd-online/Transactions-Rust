//! 错误处理语义。
//!
//! 对照原 `app/src/backend/errorHandler.ts` 的三个导出：
//!
//! * `tryOrFallback(fn, fallback)` —— 纯恢复，**不通知**，调用方自行决定是否提示。
//! * `withErrorHandling(fn, { errorPrefix, fallback })` —— 查询模式：通知 + 返回兜底值。
//! * `withErrorHandling(fn, { errorPrefix, rethrow: true })` —— 变更模式：通知 + 重新抛出。
//! * `getErrorMessage(error)` —— 取出可读信息。
//!
//! 与 TS 版的差异（有意为之）：TS 版接收 `() => Promise<T>` 惰性 thunk，Rust 版接收
//! **已经构造好的 `Future`**（`ipc::call(...)` 的返回值）。Rust 的 future 本身就必须
//! 先构造再 await，再加上惰性求值语义，只会让调用点多一层闭包，没有实际收益。
//!
//! 通知文案与 TS 版逐字一致：标题 = `errorPrefix`，描述 = `"{errorPrefix}: {msg}"`
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

/// 等价原 `getErrorMessage(error)`：任意错误 → 可读字符串。
pub fn get_error_message(error: &IpcError) -> String {
    error.message().to_string()
}
