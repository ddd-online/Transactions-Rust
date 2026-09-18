//! 统一错误信封：`{"code": -1, "msg": "...", "status": 500}`。
//!
//! 这段形状是界面与内核之间的硬契约（原 HTTP 版返回的就是 `models.Result`），
//! 字段名与取值都不允许改动。

use serde::Serialize;

use tr_domain::error::AppError;
use tr_service::ServiceError;

/// 失败载荷。`code` 恒为 -1（与原 `Handle` 一致：只有 0 与非 0 两种语义）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub code: i32,
    /// 用户可见文案，同时被界面用于判定"未打开工作空间"
    pub msg: String,
    /// 原 HTTP 状态码语义（400/404/409/500），保留以便界面按类别处理
    pub status: u16,
}

impl ApiError {
    pub fn new(error: AppError) -> Self {
        Self {
            code: -1,
            msg: error.msg,
            status: error.status,
        }
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self::new(error)
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        Self::new(error.into_app_error())
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ApiError {}

/// 命令统一返回类型。
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_shape_is_stable() {
        let payload =
            serde_json::to_value(ApiError::from(AppError::not_found("账本不存在"))).unwrap();
        assert_eq!(payload["code"], -1);
        assert_eq!(payload["msg"], "账本不存在");
        assert_eq!(payload["status"], 404);
    }

    #[test]
    fn service_errors_are_flattened_to_envelope() {
        let err = ApiError::from(ServiceError::from(AppError::workspace_not_opened()));
        assert_eq!(err.msg, "未打开工作空间");
        assert_eq!(err.status, 500);
    }
}
