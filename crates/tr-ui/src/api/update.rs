//! 应用更新命令。对照 `src-tauri/src/updater.rs`（自研实现，**不是** `tauri-plugin-updater`）。
//!
//! | 命令 | 入参 | 返回 |
//! |---|---|---|
//! | `update_check` | **无 `req` 形参** | [`UpdateSnapshot`]（推进后的完整状态） |
//! | `update_download` | [`UpdateDownloadRequest`]（`url` + 可选 `digest`） | [`UpdateSnapshot`] |
//! | `update_download_status` | **无 `req` 形参** | [`UpdateSnapshot`]（**不推进**，只读） |
//! | `update_cancel` | **无 `req` 形参** | [`UpdateSnapshot`] |
//! | `update_install` | **无 `req` 形参** | [`UpdateResponse`]（唯一的"带内成败"形状） |
//!
//! 三个事件（名字在 `tr_domain::events`：外壳发送与界面订阅**共用同一份**），
//! **载荷都是完整快照** [`UpdateSnapshot`]：
//! * [`UPDATE_DOWNLOAD_PROGRESS`] `update:download-progress`
//! * [`UPDATE_DOWNLOAD_COMPLETE`] `update:download-complete`
//! * [`UPDATE_DOWNLOAD_ERROR`] `update:download-error`
//!
//! ## 界面只渲染快照，不推进状态机
//!
//! 状态词表与合法迁移都在 `tr_domain::update`，**外壳是唯一的持有者**：它推进状态机
//! （`update_check` / `update_download` / `update_cancel` 各自在命令体里 `apply`），
//! 命令的返回值与事件载荷都是同一份 [`UpdateSnapshot`]。
//!
//! 界面因此只有"把收到的快照换进去"这一件事：不拼字段、不判断"失败还是最新"、
//! 也不在本地跑一遍状态机。这一条以前不成立 —— 界面自己 `apply(CheckStarted)` +
//! `apply(CheckFinished)`，而外壳那份快照从来没被喂过，于是
//! `update_download_status` 返回的是 `idle`，换页再切回「关于软件」就把检查结果抹掉了。
//!
//! ## 两个容易踩的点
//!
//! 1. 命令**都不 reject 业务失败**：检查失败、下载被拒、下载中途失败都落进快照
//!    （`status == failed` 且有 `error`），界面读快照即可。信封错误（命令没被调用起来）
//!    才走 `IpcError`。
//! 2. `percent` 是 **0..=100 的整数**，`speed` 是**已经格式化好的字符串**（例如 `"2.0 KB/s"`），
//!    不是数字 —— 界面照抄 [`UpdateSnapshot`] 的字段，不要再换算一次。

use tr_domain::commands;
use tr_domain::update::UpdateSnapshot;
use tr_domain::wire::{UpdateDownloadRequest, UpdateResponse};

use crate::ipc::{self, IpcError};

// 事件名只有一份定义（在 `tr_domain::events`）：外壳那边发的是同一个常量。
// 这里再导出一次，是为了让"更新这条链路的事件"跟本模块的命令放在一起看。
pub use tr_domain::events::{
    UPDATE_DOWNLOAD_COMPLETE, UPDATE_DOWNLOAD_ERROR, UPDATE_DOWNLOAD_PROGRESS,
};

/// 检查更新（**无 `req` 形参**）：返回检查之后的完整快照，失败也落在里面。
pub async fn check() -> Result<UpdateSnapshot, IpcError> {
    ipc::call_no_args(commands::UPDATE_CHECK).await
}

/// 下载安装包（同时开始广播 `update:download-progress` 事件）：返回推进后的快照。
///
/// 被拒（地址为空 / 不在白名单）与"已经有一笔在跑"都在快照里体现，不需要界面再解释响应。
pub async fn download(url: &str, digest: &str) -> Result<UpdateSnapshot, IpcError> {
    let digest = if digest.is_empty() {
        None
    } else {
        Some(digest.to_string())
    };
    ipc::call(
        commands::UPDATE_DOWNLOAD,
        UpdateDownloadRequest {
            url: url.to_string(),
            digest,
        },
    )
    .await
}

/// 打开已下载的安装包并退出应用（**无 `req` 形参**）。
///
/// 这一条是唯一"带内成败"的命令：它要么启动安装器（然后进程退出），要么回一句原因。
pub async fn install() -> Result<UpdateResponse, IpcError> {
    ipc::call_no_args(commands::UPDATE_INSTALL).await
}

/// 当前完整状态（**无 `req` 形参**）：界面进入「关于软件」时读它恢复状态与进度。
pub async fn status() -> Result<UpdateSnapshot, IpcError> {
    ipc::call_no_args(commands::UPDATE_DOWNLOAD_STATUS).await
}

/// 取消下载并清理临时文件（**无 `req` 形参**）：返回退回「可下载」之后的快照。
pub async fn cancel() -> Result<UpdateSnapshot, IpcError> {
    ipc::call_no_args(commands::UPDATE_CANCEL).await
}
