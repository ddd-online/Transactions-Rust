//! 应用更新命令。对照 `src-tauri/src/updater.rs`（自研实现，**不是** `tauri-plugin-updater`）。
//!
//! | 命令 | 入参 | 返回 |
//! |---|---|---|
//! | `update_check` | **无 `req` 形参** | [`UpdateCheckResponse`] |
//! | `update_download` | [`UpdateDownloadRequest`]（`url` + 可选 `digest`） | [`UpdateResponse`] |
//! | `update_install` | **无 `req` 形参** | [`UpdateResponse`] |
//! | `update_cancel` | **无 `req` 形参** | `()` |
//! | `update_download_status` | **无 `req` 形参** | [`UpdateSnapshot`]（当前完整状态） |
//!
//! 四个事件（名字与后端逐字一致），**载荷都是完整快照** [`UpdateSnapshot`]：
//! * [`EVENT_DOWNLOAD_PROGRESS`] `update:download-progress`
//! * [`EVENT_DOWNLOAD_COMPLETE`] `update:download-complete`
//! * [`EVENT_DOWNLOAD_ERROR`] `update:download-error`
//!
//! ## 界面侧只持有"最后一次快照"
//!
//! 状态词表与合法迁移都在 `tr_domain::update`（外壳是唯一持有者，它推进状态机、发快照）：
//! 界面**不拼状态、不判断"失败还是最新"**，只把收到的那份 [`UpdateSnapshot`] 渲染出来。
//! 事件与 `update_download_status` 走同一个状态机、同一份字段 —— 因此"进页面时读到的"
//! 与"事件推过来的"永远一致，谁先到都不影响结果（"补状态"不是特例，就是常规读法）。
//!
//! ## 两个容易踩的点
//!
//! 1. `update_check` **不会 reject**：网络失败时它 resolve 出 `error` 字段。**不要**把
//!    "没有新版本"当成失败（反之也不行）—— 判据在状态机里，界面读 `status` 即可。
//! 2. `update_download` / `update_install` 也**不会 reject**：失败时 resolve 出
//!    `{ success: false, error }`（`update_download` 被取消时 `error == "cancelled"`、
//!    已有一笔在跑时 `error == "already_downloading"`）。
//!    只有 `update_cancel` 是纯 `()` 返回。
//!
//! `percent` 是 **0..=100 的整数**，`speed` 是**已经格式化好的字符串**（例如 `"2.0 KB/s"`），
//! 不是数字 —— 界面照抄 [`UpdateSnapshot`] 的字段，不要再换算一次。

use tr_domain::update::{UpdateEvent, UpdateSnapshot};
use tr_domain::wire::{UpdateCheckResponse, UpdateDownloadRequest, UpdateResponse};

use crate::ipc::{self, IpcError};

/// 事件名：下载进度（载荷是完整快照）。
pub const EVENT_DOWNLOAD_PROGRESS: &str = "update:download-progress";
/// 事件名：下载完成（载荷是完整快照）。
pub const EVENT_DOWNLOAD_COMPLETE: &str = "update:download-complete";
/// 事件名：下载失败（载荷是完整快照）。
pub const EVENT_DOWNLOAD_ERROR: &str = "update:download-error";

/// 检查更新（**无 `req` 形参**）：成功与失败都在返回里，交给状态机判定。
pub async fn check() -> Result<UpdateCheckResponse, IpcError> {
    ipc::call_no_args("update_check").await
}

/// 下载安装包（同时开始广播 `update:download-progress` 事件）。
pub async fn download(url: &str, digest: &str) -> Result<UpdateResponse, IpcError> {
    let digest = if digest.is_empty() {
        None
    } else {
        Some(digest.to_string())
    };
    ipc::call(
        "update_download",
        UpdateDownloadRequest {
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

/// 当前完整状态（**无 `req` 形参**）：界面进入「关于软件」时读它恢复状态与进度。
pub async fn status() -> Result<UpdateSnapshot, IpcError> {
    ipc::call_no_args("update_download_status").await
}

/// 取消下载并清理临时文件（**无 `req` 形参**）。
pub async fn cancel() -> Result<(), IpcError> {
    ipc::call_void_no_args("update_cancel").await
}

/// 下载请求被外壳拒绝时的三种结果（界面据此决定落哪个状态）。
///
/// **是纯函数**：状态机也有一份同样口径的规则（[`UpdateSnapshot::failed_after_download_attempt`]），
/// 这里只把"响应说了什么"翻译成"状态机该收哪个事件"，避免界面自己发明一套词。
pub enum DownloadOutcome {
    /// 下载成功收尾（正常路径由 `update:download-complete` 事件先到）。
    Finished,
    /// 外壳报告已经有一笔在跑：**别动状态**，让进度事件继续驱动界面。
    AlreadyRunning,
    /// 用户取消：回到"可下载"。
    Cancelled,
    /// 真的失败了（含"地址不在白名单"这类没有进度事件的早退）。
    Failed(String),
}

/// 把 `update_download` 的返回翻译成 [`DownloadOutcome`]。
pub fn download_outcome(response: &UpdateResponse) -> DownloadOutcome {
    if response.success {
        return DownloadOutcome::Finished;
    }
    if response.is_cancelled() {
        return DownloadOutcome::Cancelled;
    }
    if response.error.as_deref() == Some(tr_domain::update::ALREADY_DOWNLOADING) {
        return DownloadOutcome::AlreadyRunning;
    }
    DownloadOutcome::Failed(
        response
            .error
            .clone()
            .unwrap_or_else(|| "下载失败".to_string()),
    )
}

/// 把响应落成状态机的下一步：返回 `None` 表示"什么都不做"（[`DownloadOutcome::AlreadyRunning`]）。
pub fn download_followup(
    response: &UpdateResponse,
    current: &UpdateSnapshot,
) -> Option<UpdateSnapshot> {
    match download_outcome(response) {
        DownloadOutcome::AlreadyRunning => None,
        DownloadOutcome::Finished => {
            // 事件通常已经把它置成 downloaded；万一事件还没到，也不能停在"下载中"
            if current.is_downloading() {
                Some(UpdateEvent::DownloadFinished.apply(current))
            } else {
                None
            }
        }
        DownloadOutcome::Cancelled => Some(UpdateEvent::DownloadCancelled.apply(current)),
        DownloadOutcome::Failed(message) => Some(current.failed_after_download_attempt(&message)),
    }
}
