//! 服务层统一错误类型：把底层错误（SQLite、文件、网络）收敛为对外的 [`AppError`]。
//!
//! 非 `AppError` 的底层错误一律变成 500 + 原始错误文本，
//! 这样排障信息不会丢失，且前端拿到的 msg 保持稳定。

use tr_domain::error::AppError;
use tr_store::WorkspaceError;

pub type ServiceResult<T> = Result<T, ServiceError>;

#[derive(Debug)]
pub enum ServiceError {
    /// 业务错误（状态码 + 用户可见文案）
    App(AppError),
    /// 数据库错误 → 500
    Database(rusqlite::Error),
    /// 其它底层错误 → 500
    Internal(String),
}

impl ServiceError {
    /// 转为对外错误（命令面直接使用）。
    ///
    /// 底层错误统一经 [`std::fmt::Display`] 收敛，而不是各自拼接，
    /// 这样"查无记录"这类文案只有一处定义（见下面 Display 实现）。
    pub fn into_app_error(self) -> AppError {
        match self {
            ServiceError::App(err) => err,
            other => AppError::internal(other.to_string()),
        }
    }
}

impl From<AppError> for ServiceError {
    fn from(err: AppError) -> Self {
        ServiceError::App(err)
    }
}

impl From<rusqlite::Error> for ServiceError {
    fn from(err: rusqlite::Error) -> Self {
        ServiceError::Database(err)
    }
}

impl From<WorkspaceError> for ServiceError {
    fn from(err: WorkspaceError) -> Self {
        ServiceError::Internal(err.to_string())
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::App(err) => write!(f, "{}", err.msg),
            // "查无记录"的文案必须固定为 `record not found`，
            // 否则错误信息会通过 IPC 原样展示给用户、并造成回归差异。
            ServiceError::Database(err) if tr_store::dao::is_not_found(err) => {
                write!(f, "record not found")
            }
            ServiceError::Database(err) => write!(f, "{err}"),
            ServiceError::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ServiceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_error_keeps_status_and_message() {
        let err = ServiceError::from(AppError::not_found("账本不存在"));
        let app = err.into_app_error();
        assert_eq!(app.status, 404);
        assert_eq!(app.msg, "账本不存在");
    }

    #[test]
    fn database_error_becomes_internal() {
        let err = ServiceError::from(rusqlite::Error::InvalidQuery);
        let app = err.into_app_error();
        assert_eq!(app.status, 500);
        assert!(!app.msg.is_empty());
    }

    #[test]
    fn missing_row_message_is_record_not_found() {
        let err = ServiceError::from(rusqlite::Error::QueryReturnedNoRows);
        assert_eq!(err.to_string(), "record not found");
        assert_eq!(err.into_app_error().msg, "record not found");
    }
}
