//! 统一错误信封：`{"code": -1, "msg": "...", "status": 500}`。
//!
//! 这段形状是界面与内核之间的硬契约，字段名与取值都不允许改动。

use serde::Serialize;

use tr_domain::error::AppError;
use tr_service::ServiceError;

/// 失败载荷。`code` 是**失败的种类**（`-1` = 普通业务失败，见 `tr_domain::error` 的模块文档），
/// 界面按它分支而不是按文案（候选 10 / #40）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub code: i32,
    /// 用户可见文案（**只用于展示**；判定走 `code`）
    pub msg: String,
    /// HTTP 状态码语义（400/404/409/500），保留以便界面按类别处理
    pub status: u16,
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self {
            code: error.code,
            msg: error.msg,
            status: error.status,
        }
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        Self::from(error.into_app_error())
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

    /// 信封把"哪一类失败"透出去（界面据此分支，而不是比 `msg` 文案 —— 候选 10 / #40）。
    #[test]
    fn the_envelope_carries_the_failure_kind() {
        let required = ApiError::from(AppError::workspace_not_opened());
        assert_eq!(
            required.code,
            tr_domain::error::ERR_CODE_WORKSPACE_NOT_OPENED
        );
        assert_eq!(required.code, -2, "取值是契约（界面按它分支）");

        let plain = serde_json::to_value(ApiError::from(AppError::bad_request("x"))).unwrap();
        assert_eq!(plain["code"], -1, "普通业务失败仍是默认码");
    }

    #[test]
    fn service_errors_are_flattened_to_envelope() {
        let err = ApiError::from(ServiceError::from(AppError::workspace_not_opened()));
        assert_eq!(err.msg, "未打开工作空间");
        assert_eq!(err.status, 500);
    }
}
