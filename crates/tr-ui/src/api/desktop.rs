//! 桌面外壳命令。对照原 `electronAPI`（`electron/src/preload.js`）与
//! `src-tauri/src/commands.rs`。
//!
//! 这些命令不属于业务域（不由 `tr-ipc` 提供），但界面同样需要：
//!
//! | 命令 | 原实现 | 入参 |
//! |---|---|---|
//! | [`window_control`] | `window-control` | `{ action: minimize\|maximize\|close }` |
//! | [`app_info`] | `app`（field: name/version/isDev） | `{ field }` |
//! | [`asset_url`] | `imageUrl.ts` | `{ filePath }` |
//! | [`config_get`] | `config:get-*` | 无参数 |
//! | [`config_set_appearance`] | 外观设置 | `{ appearance: light\|dark\|system }` |
//! | [`config_set_close_behavior`] | 关闭行为 | `{ behavior: quit\|tray\|"" }` |
//! | [`workspace_get`] | `workspace:get` | 无参数 |
//! | [`workspace_set`] | `workspace:set` | `{ workspaceDir }` |
//! | [`workspace_open`] | `POST /workspace` | `{ workspaceDir }` |
//! | [`dialog_open`] | `dialog:open` | `{ title?, defaultPath? }` |
//! | [`file_save_image`] | `file:save` | `{ relativePath }` |
//! | [`devtools_get_state`] | `devtools:get-state` | 无参数 |
//! | [`devtools_toggle`] | `devtools:toggle` | `{ enabled }`（并广播 `devtools:state-changed`） |
//! | [`config_file_path`] | —— | 无参数 |
//!
//! `dialog_open` / `file_save_image` 的返回形状与原实现一致
//! （`{ canceled, filePaths, error? }` / `{ success, canceled?, error? }`）。

use serde::{Deserialize, Serialize};

use crate::ipc::{self, IpcError};

/// 窗口控制动作（与原 `window-control` 的三个命令一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowAction {
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Serialize)]
struct WindowControlRequest {
    action: WindowAction,
}

#[derive(Debug, Serialize)]
struct AppInfoRequest {
    field: String,
}

#[derive(Debug, Serialize)]
struct AssetUrlRequest {
    #[serde(rename = "filePath")]
    file_path: String,
}

#[derive(Debug, Serialize)]
struct AppearanceRequest {
    appearance: String,
}

#[derive(Debug, Serialize)]
struct CloseBehaviorRequest {
    behavior: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceDirRequest {
    #[serde(rename = "workspaceDir")]
    workspace_dir: String,
}

#[derive(Debug, Default, Serialize)]
struct DialogOpenRequest {
    title: String,
    #[serde(rename = "defaultPath")]
    default_path: String,
}

#[derive(Debug, Serialize)]
struct FileSaveRequest {
    #[serde(rename = "relativePath")]
    relative_path: String,
}

#[derive(Debug, Serialize)]
struct DevToolsToggleRequest {
    enabled: bool,
}

/// `config_get` 的返回（逐字段照抄 `commands.rs` 的 `ConfigSnapshot`）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConfigSnapshot {
    #[serde(rename = "workspaceDir")]
    pub workspace_dir: String,
    #[serde(rename = "closeBehavior")]
    pub close_behavior: String,
    /// light / dark / system
    pub appearance: String,
    #[serde(rename = "configPath")]
    pub config_path: String,
    #[serde(rename = "isDev")]
    pub is_dev: bool,
}

/// `dialog_open` 的返回（与原 `dialog:open` 一致）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DialogOpenResponse {
    pub canceled: bool,
    #[serde(rename = "filePaths")]
    pub file_paths: Vec<String>,
    pub error: Option<String>,
}

impl DialogOpenResponse {
    /// 取用户选中的第一个目录（取消或异常时为空）。
    pub fn first_path(&self) -> Option<&str> {
        self.file_paths.first().map(String::as_str)
    }
}

/// `file_save_image` 的返回。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FileSaveResponse {
    pub success: bool,
    pub canceled: Option<bool>,
    pub error: Option<String>,
}

/// 最小化 / 最大化（还原）/ 关闭窗口。
pub async fn window_control(action: WindowAction) -> Result<(), IpcError> {
    ipc::call_void("window_control", WindowControlRequest { action }).await
}

/// 读取应用信息（`name` / `version` / `isDev`）。
pub async fn app_info(field: &str) -> Result<String, IpcError> {
    ipc::call(
        "app_info",
        AppInfoRequest {
            field: field.to_string(),
        },
    )
    .await
}

/// 工作空间资产相对路径 → `<img src>` 可用 URL（`trasset://`）。
pub async fn asset_url(file_path: &str) -> Result<String, IpcError> {
    ipc::call(
        "asset_url",
        AssetUrlRequest {
            file_path: file_path.to_string(),
        },
    )
    .await
}

/// 读取配置快照。
pub async fn config_get() -> Result<ConfigSnapshot, IpcError> {
    ipc::call_no_args("config_get").await
}

/// 设置外观（light / dark / system）。
pub async fn config_set_appearance(appearance: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "config_set_appearance",
        AppearanceRequest {
            appearance: appearance.to_string(),
        },
    )
    .await
}

/// 设置关闭行为（quit / tray / 空串表示首次询问）。
pub async fn config_set_close_behavior(behavior: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "config_set_close_behavior",
        CloseBehaviorRequest {
            behavior: behavior.to_string(),
        },
    )
    .await
}

/// 读取已保存的工作空间目录（空串表示尚未选择）。
pub async fn workspace_get() -> Result<String, IpcError> {
    ipc::call_no_args("workspace_get").await
}

/// 只记录工作空间目录，不打开数据库。
pub async fn workspace_set(workspace_dir: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "workspace_set",
        WorkspaceDirRequest {
            workspace_dir: workspace_dir.to_string(),
        },
    )
    .await
}

/// 打开工作空间（打开数据库 + 切换日志目录 + 记住目录 + 广播 `workspace-changed`）。
pub async fn workspace_open(workspace_dir: &str) -> Result<(), IpcError> {
    ipc::call_void(
        "workspace_open",
        WorkspaceDirRequest {
            workspace_dir: workspace_dir.to_string(),
        },
    )
    .await
}

/// 弹出目录选择对话框。
pub async fn dialog_open(title: &str, default_path: &str) -> Result<DialogOpenResponse, IpcError> {
    ipc::call(
        "dialog_open",
        DialogOpenRequest {
            title: title.to_string(),
            default_path: default_path.to_string(),
        },
    )
    .await
}

/// 把工作空间资产里的图片另存到用户选择的位置。
pub async fn file_save_image(relative_path: &str) -> Result<FileSaveResponse, IpcError> {
    ipc::call(
        "file_save_image",
        FileSaveRequest {
            relative_path: relative_path.to_string(),
        },
    )
    .await
}

// ---------------------------------------------------------------- DevTools

/// 事件名：DevTools 开合状态变化（载荷是 `bool`）。
///
/// 与 `src-tauri/src/commands.rs` 的 `EVENT_DEVTOOLS_STATE_CHANGED` 逐字一致。
pub const EVENT_DEVTOOLS_STATE_CHANGED: &str = "devtools:state-changed";

/// 读取 DevTools 当前是否打开（**无 `req` 形参**）。
pub async fn devtools_get_state() -> Result<bool, IpcError> {
    ipc::call_no_args("devtools_get_state").await
}

/// 开合 DevTools，返回操作后的真实状态（后端同时广播 `devtools:state-changed`）。
pub async fn devtools_toggle(enabled: bool) -> Result<bool, IpcError> {
    ipc::call("devtools_toggle", DevToolsToggleRequest { enabled }).await
}

/// 配置文件绝对路径（**无 `req` 形参**）。
pub async fn config_file_path() -> Result<String, IpcError> {
    ipc::call_no_args("config_file_path").await
}
