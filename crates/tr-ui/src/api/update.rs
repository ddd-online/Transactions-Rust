//! 应用更新命令。对照 `src-tauri/src/updater.rs`（自研实现，**不是** `tauri-plugin-updater`）。
//!
//! | 命令 | 入参 | 返回 |
//! |---|---|---|
//! | `update_check` | **无 `req` 形参** | [`UpdateCheckResponse`] |
//! | `update_download` | [`UpdateDownloadRequest`]（`url` + 可选 `digest`） | [`UpdateResponse`] |
//! | `update_install` | **无 `req` 形参** | [`UpdateResponse`] |
//! | `update_cancel` | **无 `req` 形参** | `()` |
//! | `update_download_status` | **无 `req` 形参** | [`UpdateDownloadStatus`]（恢复下载状态用） |
//!
//! 三个事件（名字与后端逐字一致）：
//! * [`EVENT_DOWNLOAD_PROGRESS`] `update:download-progress` → [`UpdateProgress`]
//! * [`EVENT_DOWNLOAD_COMPLETE`] `update:download-complete` → `{ filePath }`
//! * [`EVENT_DOWNLOAD_ERROR`] `update:download-error` → `{ message }`
//!
//! ## 两个容易踩的点
//!
//! 1. `update_check` **不会 reject**：网络失败时它 resolve 出 `error` 字段，
//!    界面必须把「`hasUpdate == false` + 有 `error`」当成"检查失败"而不是"已是最新"。
//! 2. `update_download` / `update_install` 也**不会 reject**：失败时 resolve 出
//!    `{ success: false, error }`（`update_download` 被取消时 `error == "cancelled"`）。
//!    只有 `update_cancel` 是纯 `()` 返回。
//!
//! `percent` 是 **0..=100 的整数**，`speed` 是**已经格式化好的字符串**（例如 `"2.0 KB/s"`），
//! 不是数字 —— 界面对照 `UpdateProgress` 的字段类型照抄，不要再除一次。

use serde::{Deserialize, Serialize};

use crate::ipc::{self, IpcError};

/// 事件名：下载进度。
pub const EVENT_DOWNLOAD_PROGRESS: &str = "update:download-progress";
/// 事件名：下载完成（载荷 `{ filePath }`）。
pub const EVENT_DOWNLOAD_COMPLETE: &str = "update:download-complete";
/// 事件名：下载失败（载荷 `{ message }`）。
pub const EVENT_DOWNLOAD_ERROR: &str = "update:download-error";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct UpdateCheckResponse {
    #[serde(rename = "hasUpdate")]
    pub has_update: bool,
    #[serde(rename = "latestVersion")]
    pub latest_version: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    /// 形如 `sha256:...`（GitHub release asset 的 digest）
    pub digest: String,
    /// release notes（Markdown；本轮按纯文本渲染）
    pub body: String,
    /// 检查失败原因（网络/解析问题），成功时为 `None`
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateResponse {
    pub success: bool,
    pub error: Option<String>,
}

impl UpdateResponse {
    /// 是否为"用户主动取消"（后端用固定文案 `cancelled` 表示）。
    pub fn is_cancelled(&self) -> bool {
        self.error.as_deref() == Some("cancelled")
    }
}

/// `update:download-progress` 的载荷。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateProgress {
    /// 0..=100（整数；`total` 未知时为 0）
    pub percent: u32,
    /// 已下载字节
    pub downloaded: i64,
    /// 总字节（未知时 0）
    pub total: i64,
    /// 已格式化的速度串，例如 `"2.0 KB/s"`
    pub speed: String,
}

/// `update:download-complete` 的载荷。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateComplete {
    #[serde(rename = "filePath")]
    pub file_path: String,
}

/// `update:download-error` 的载荷。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateError {
    pub message: String,
}

/// `update_download_status` 的返回：界面（重新）进入「关于软件」时恢复下载状态。
///
/// 下载是**单例**（一次只有一笔，跑在外壳的线程里），界面进来时先问一次当前状态，
/// 这样切换页面回来、甚至重开界面，都能接着显示进度而不是回到"未下载"。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateDownloadStatus {
    /// 是否有下载正在跑
    pub active: bool,
    /// 已经下载好、等待安装
    pub downloaded: bool,
    /// 最近一次上报的百分比
    pub percent: u32,
    /// 最近一次上报的速度串
    pub speed: String,
}

#[derive(Debug, Serialize)]
struct DownloadRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
}

/// 检查更新（**无 `req` 形参**）。
pub async fn check() -> Result<UpdateCheckResponse, IpcError> {
    ipc::call_no_args("update_check").await
}

/// 下载安装包（同时发 `update:download-progress` 事件）。
pub async fn download(url: &str, digest: &str) -> Result<UpdateResponse, IpcError> {
    let digest = if digest.is_empty() {
        None
    } else {
        Some(digest.to_string())
    };
    ipc::call(
        "update_download",
        DownloadRequest {
            url: url.to_string(),
            digest,
        },
    )
    .await
}

/// 打开已下载的安装包并退出应用（**无 `req` 形参**）。
pub async fn install() -> Result<UpdateResponse, IpcError> {
    ipc::call_no_args("update_install").await
}

/// 当前下载状态（**无 `req` 形参**）：界面进入「关于软件」时用它恢复进度。
pub async fn download_status() -> Result<UpdateDownloadStatus, IpcError> {
    ipc::call_no_args("update_download_status").await
}

/// 取消下载并清理临时文件（**无 `req` 形参**）。
pub async fn cancel() -> Result<(), IpcError> {
    ipc::call_void_no_args("update_cancel").await
}
