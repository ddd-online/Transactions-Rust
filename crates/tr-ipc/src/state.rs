//! 命令上下文：Tauri 托管状态，供所有业务命令读取。
//!
//! 与原 Go 版 `api.Handlers` 持有的 `WsManager` 一致：进程内单例，
//! 再次打开工作空间会替换上一个。桌面外壳自己的状态（窗口、配置、托盘）
//! 不放在这里，由 `src-tauri` 单独托管。

use std::sync::Arc;

use tr_domain::error::AppError;
use tr_store::{Workspace, WsManager};

pub struct AppState {
    pub ws: Arc<WsManager>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            ws: Arc::new(WsManager::new()),
        }
    }

    /// 取当前打开的工作空间；未打开时返回与原实现完全相同的错误文案
    /// （界面据此派发 `workspace-required` 事件）。
    pub fn workspace(&self) -> Result<Arc<Workspace>, AppError> {
        self.ws
            .opened_workspace()
            .ok_or_else(AppError::workspace_not_opened)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_workspace_yields_the_shared_error_message() {
        let state = AppState::new();
        let err = state.workspace().unwrap_err();
        assert_eq!(err.msg, "未打开工作空间");
        assert_eq!(err.status, 500);
    }
}
