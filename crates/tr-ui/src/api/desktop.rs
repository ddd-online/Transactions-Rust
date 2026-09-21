//! 桌面外壳命令，定义在 `src-tauri/src/commands.rs`。
//!
//! 这些命令不属于业务域（不由 `tr-ipc` 提供），但界面同样需要：
//!
//! | 命令 | 入参 |
//! |---|---|
//! | [`window_control`] | `{ action: minimize\|maximize\|close }` |
//! | [`app_info`] | `{ field }`（name / version / isDev） |
//! | [`asset_url`] | `{ filePath }` |
//! | [`config_get`] | 无参数 |
//! | [`config_set_appearance`] | `{ appearance: light\|dark\|system }` |
//! | [`config_set_close_behavior`] | `{ behavior: quit\|tray\|"" }` |
//! | [`config_set_proxy`] | `{ mode: off\|auto\|manual, url }`（返回归一化后的设置） |
//! | [`config_set_feature`] | `{ feature: accounting\|stock\|keyEvent\|diary, enabled }`（返回落盘后的全部开关） |
//! | [`proxy_detect`] | 无参数（报告当前设置最终会用哪个代理） |
//! | [`workspace_get`] | 无参数 |
//! | [`workspace_set`] | `{ workspaceDir }` |
//! | [`workspace_open`] | `{ workspaceDir }` |
//! | [`dialog_open`] | `{ title?, defaultPath? }` |
//! | [`file_save_image`] | `{ relativePath }` |
//! | [`devtools_get_state`] | 无参数 |
//! | [`devtools_toggle`] | `{ enabled }`（并广播 `devtools:state-changed`） |
//! | [`config_file_path`] | 无参数 |
//!
//! `dialog_open` / `file_save_image` 的返回形状是固定契约
//! （`{ canceled, filePaths, error? }` / `{ success, canceled?, error? }`）。

use serde::{Deserialize, Serialize};

use tr_domain::proxy::ProxySetting;

use crate::ipc::{self, IpcError};

/// 窗口控制动作（最小化 / 最大化 / 关闭）。
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
struct SetFeatureRequest {
    feature: String,
    enabled: bool,
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

/// 功能开关（逐字段照抄 `src-tauri/src/config.rs` 的 `FeatureFlags`）。
///
/// **硬契约**：键名要与外壳一致（`keyEvent` 是 camelCase，其余三个是小写单词）。
/// `Default` = **全开**：外壳漏发这个字段、或老配置里根本没有它时，界面按"没关过任何功能"处理。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FeatureFlags {
    /// 记账（记录 / 分析 / 标签 / 模板）
    pub accounting: bool,
    /// 股票
    pub stock: bool,
    /// 事件
    #[serde(rename = "keyEvent")]
    pub key_event: bool,
    /// 日记
    pub diary: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self::all_enabled()
    }
}

impl FeatureFlags {
    /// 全开（新增功能时的默认值）。
    pub const fn all_enabled() -> Self {
        Self {
            accounting: true,
            stock: true,
            key_event: true,
            diary: true,
        }
    }

    /// 按功能名读取（`shell::Page::feature_key` 用的就是这几个名字）。
    pub fn get(&self, feature: &str) -> Option<bool> {
        Some(match feature {
            "accounting" => self.accounting,
            "stock" => self.stock,
            "keyEvent" => self.key_event,
            "diary" => self.diary,
            _ => return None,
        })
    }
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
    /// 代理设置（`mode` / `url`）。
    ///
    /// 这里**直接复用 `tr_domain::proxy::ProxySetting`**（两侧同一份类型，不是手抄，
    /// 所以不存在字段漂移）；缺省值是 `auto`，后端漏发该字段时也按自动探测处理。
    pub proxy: ProxySetting,
    /// 功能开关（缺省 = 全开，见 [`FeatureFlags`]）。
    pub features: FeatureFlags,
}

/// `proxy_detect` 的返回（逐字段照抄 `commands.rs` 的 `ProxyDetectResponse`）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProxyDetectResponse {
    /// 会用到的代理地址（空串 = 直连）
    pub url: String,
    /// env / system / manual / none
    pub source: String,
    /// 系统是否配置了 PAC 自动配置脚本（本版本不解析）
    pub pac: bool,
    /// 直接展示给用户的说明
    pub message: String,
}

/// `dialog_open` 的返回（固定契约）。
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

/// 保存代理设置（`off` / `auto` / `manual`）。
///
/// 参数**直接复用 `tr_domain::proxy::ProxySetting`**（与后端同一个类型）；
/// 后端会校验并归一化地址，成功时返回可落盘的那份（界面用它回显）。
pub async fn config_set_proxy(setting: ProxySetting) -> Result<ProxySetting, IpcError> {
    ipc::call("config_set_proxy", setting).await
}

/// 设置某个顶级功能是否启用（决定它是否出现在侧边栏）。
///
/// `feature` 只认 `accounting` / `stock` / `keyEvent` / `diary`（与
/// [`crate::shell::Page::feature_key`] 同名字）；返回落盘后的**全部**开关，界面用它回显。
pub async fn config_set_feature(feature: &str, enabled: bool) -> Result<FeatureFlags, IpcError> {
    ipc::call(
        "config_set_feature",
        SetFeatureRequest {
            feature: feature.to_string(),
            enabled,
        },
    )
    .await
}

/// 检测：按当前设置报告最终会用哪个代理（只读，不写配置、不改系统设置）。
pub async fn proxy_detect() -> Result<ProxyDetectResponse, IpcError> {
    ipc::call_no_args("proxy_detect").await
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
