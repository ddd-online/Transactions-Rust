//! 应用错误。
//!
//! 语义保持一致：`status` 决定失败形态（400/404/409/500），`msg` 是**直接展示给用户的文案**，
//! 因此这里的字符串既是 API 契约也是 UI 文案，改动即为破坏性变更。
//!
//! `code` 是**"哪一类失败"**（候选 10 / #40）：从前信封里恒为 `-1`，界面只能靠比 `msg` 文案来认
//! "未打开工作空间"这类情形。默认 `-1`（普通业务失败），特殊的几类各有专属码。

use std::fmt;

/// `code` 的默认值：普通业务失败（信封里一直以来的取值）。
pub const ERR_CODE_DEFAULT: i32 = -1;

/// "未打开工作空间"的专属码（界面据此弹工作空间选择，不必再比文案）。
pub const ERR_CODE_WORKSPACE_NOT_OPENED: i32 = -2;

/// 带 HTTP 语义状态码的业务错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub status: u16,
    /// 失败的种类（见模块文档）：默认 [`ERR_CODE_DEFAULT`]
    pub code: i32,
    pub msg: String,
}

impl AppError {
    pub fn new(status: u16, msg: impl Into<String>) -> Self {
        Self {
            status,
            code: ERR_CODE_DEFAULT,
            msg: msg.into(),
        }
    }

    /// 换一个 `code`（构造出来之后再标种类，避免每个构造器都多一个参数）。
    pub fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
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
    ///
    /// **带专属 `code`**：界面因此不必比 `msg` 文案（文案是用户可见的、随时可能改）。
    pub fn workspace_not_opened() -> Self {
        Self::internal(ERR_WORKSPACE_NOT_OPENED).with_code(ERR_CODE_WORKSPACE_NOT_OPENED)
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

    /// `code` 表达"哪一类失败"：普通构造器一律默认码，"未打开工作空间"有专属码
    /// （界面据此弹选择屏，而不是比 `msg` 文案 —— 候选 10 / #40）。
    #[test]
    fn only_workspace_not_opened_carries_a_special_code() {
        assert_eq!(AppError::bad_request("x").code, ERR_CODE_DEFAULT);
        assert_eq!(AppError::not_found("x").code, ERR_CODE_DEFAULT);
        assert_eq!(AppError::conflict("x").code, ERR_CODE_DEFAULT);
        assert_eq!(AppError::internal("x").code, ERR_CODE_DEFAULT);
        let required = AppError::workspace_not_opened();
        assert_eq!(required.code, ERR_CODE_WORKSPACE_NOT_OPENED);
        assert_ne!(ERR_CODE_WORKSPACE_NOT_OPENED, ERR_CODE_DEFAULT);
        assert_eq!(required.status, 500, "状态码语义不变");
        assert_eq!(
            required.msg, ERR_WORKSPACE_NOT_OPENED,
            "文案不变（只用于展示）"
        );
    }

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
