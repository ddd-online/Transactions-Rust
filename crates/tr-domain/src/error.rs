//! 应用错误。
//!
//! 语义保持一致：`status` 决定失败形态（400/404/409/500），`msg` 是**直接展示给用户的文案**，
//! 因此这里的字符串既是 API 契约也是 UI 文案，改动即为破坏性变更。

use std::fmt;

/// 带 HTTP 语义状态码的业务错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub status: u16,
    pub msg: String,
}

impl AppError {
    pub fn new(status: u16, msg: impl Into<String>) -> Self {
        Self {
            status,
            msg: msg.into(),
        }
    }

    /// 400
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(400, msg)
    }

    /// 404
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(404, msg)
    }

    /// 409
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(409, msg)
    }

    /// 500
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(500, msg)
    }

    /// 未打开工作空间（500）。命令层统一返回这个错误，
    /// 前端据此派发 `workspace-required` 事件触发工作空间选择流程。
    pub fn workspace_not_opened() -> Self {
        Self::internal(ERR_WORKSPACE_NOT_OPENED)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for AppError {}

/// 未打开工作空间的固定文案（前端按字符串判定，**不可修改**）。
pub const ERR_WORKSPACE_NOT_OPENED: &str = "未打开工作空间";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_and_message_are_preserved() {
        assert_eq!(AppError::bad_request("x").status, 400);
        assert_eq!(AppError::not_found("x").status, 404);
        assert_eq!(AppError::conflict("x").status, 409);
        assert_eq!(AppError::internal("x").status, 500);
        assert_eq!(AppError::workspace_not_opened().msg, "未打开工作空间");
        assert_eq!(AppError::bad_request("缺少参数").to_string(), "缺少参数");
    }
}
