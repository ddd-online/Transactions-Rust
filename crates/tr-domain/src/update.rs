//! 应用更新流程的**状态词表与状态机**（纯逻辑，两侧共用）。
//!
//! 「检查 → 下载 → 安装」这条链路的状态只有一个持有者：**应用外壳**（只有它能做 I/O）。
//! 它推进这里的状态机、把 [`UpdateSnapshot`] 发给界面；界面只渲染快照，自己不再拼状态、
//! 不再判断"失败还是最新"。因此本模块里**没有** I/O：既不知道 HTTP，也不知道文件系统。
//!
//! 与 `proxy` 同规格：词表与用户可见文案放领域层，原生（外壳）与 wasm（界面）共用同一份，
//! 两侧不会漂移。字符串取值是**跨进程契约**（事件载荷 / 命令返回），改名即破坏兼容。
//!
//! ## 为什么不是界面侧的七个裸字符串
//!
//! 从前状态词表只以字符串活在渲染函数里，于是"检查失败 ≠ 已是最新"这条**业务规则**
//! 住在页面里（`hasUpdate == false` 且有 `error` 才算失败），而外壳根本不知道
//! "在检查"和"查完了、没新版本"这两档。现在词表是 [`UpdateStatus`]，
//! 合法迁移是 [`UpdateEvent::apply`]：所有状态推导都在这儿，界面只剩 `match`。
//!
//! ## 状态图（合法迁移）
//!
//! ```text
//! idle ──检查开始──▶ checking ──成功且有新版──▶ available ──开始下载──▶ downloading ──完成──▶ downloaded
//!  ▲                    │                          ▲                        │
//!  │                    ├─成功且无新版─▶ no-update  │                        ├─失败─▶ failed
//!  │                    └─失败────────▶ failed ─────┘（重试 / 检查开始）      │
//!  └───────────────────────────────────────────────────取消 / 失败──────────┘
//! ```
//!
//! 读法：`checking` 只能到 `available` / `no-update` / `failed`；`downloading` 能到
//! `downloaded` / `failed` / `available`（取消）；终态 `downloaded` / `no-update` 只在
//! "重新检查"时离开；`failed` 只由"重新检查"离开。
//!
//! 规则是**宽进**的：`apply` 接受任何事件序列，只按"当前状态允不允许"取舍 ——
//! 乱序到达的进度事件会被丢弃，幂等的重复事件不改变结果，状态因此不会倒退。

use serde::{Deserialize, Serialize};

use crate::wire::UpdateCheckResponse;

/// 更新流程的状态词表。
///
/// **序列化取值就是契约**（界面 `match` 的分支、旧版字符串比较的对象）：
/// `idle` / `checking` / `available` / `no-update` / `downloading` / `downloaded` / `failed`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateStatus {
    /// 还没检查过（按钮显示「检查更新」）。
    ///
    /// 界面**本地**用它表示"查不到可更新内容"（见页面的检查分支），外壳不主动发它。
    #[default]
    Idle,
    /// 正在检查。外壳在检查开始时进入它（`update_check` 一进门就 `CheckStarted`），
    /// 界面在"点下去"到"外壳回话"之间也按它渲染 —— 但那一档是**界面自己的在飞标记**，
    /// 不是界面推进状态机（词表与迁移只有外壳一个持有者）。
    Checking,
    /// 查到了更新的版本，可以下载。
    Available,
    /// 查完了，当前已是最新；**与"检查失败"是两回事**（后者是 [`UpdateStatus::Failed`]）。
    NoUpdate,
    /// 正在下载。
    Downloading,
    /// 下载完成，安装包在 `%TEMP%` 里等着安装。
    Downloaded,
    /// 失败：检查失败或下载失败，原因在 [`UpdateSnapshot::error`]。
    Failed,
}

impl UpdateStatus {
    /// 状态对应的用户可见文案（与 [`NOTIFY_CHECK_FAILED`] 等通知标题同源）。
    ///
    /// 单元测试遍历所有取值断言"每个状态都有文案"，因为界面的 `match` 用它们渲染 ——
    /// 漏一个就是空白分支。
    pub fn label(self) -> &'static str {
        match self {
            UpdateStatus::Idle => LABEL_IDLE,
            UpdateStatus::Checking => LABEL_CHECKING,
            UpdateStatus::Available => LABEL_AVAILABLE,
            UpdateStatus::NoUpdate => LABEL_NO_UPDATE,
            UpdateStatus::Downloading => LABEL_DOWNLOADING,
            UpdateStatus::Downloaded => LABEL_DOWNLOADED,
            UpdateStatus::Failed => LABEL_FAILED,
        }
    }

    /// 状态不是终态时可以离开它（重新检查 / 下载）。
    ///
    /// 界面据此决定按钮可不可用：下载中与已下载时不让再点「检查更新」
    /// （重复检查会把下载途中的进度冲掉）。
    pub fn allows_new_check(self) -> bool {
        !matches!(self, UpdateStatus::Downloading | UpdateStatus::Downloaded)
    }

    /// 是否要展示发行说明（release notes）：有更新、正在下载、下好了、失败时都保留展示。
    pub fn shows_release_body(self) -> bool {
        matches!(
            self,
            UpdateStatus::Available
                | UpdateStatus::Downloading
                | UpdateStatus::Downloaded
                | UpdateStatus::Failed
        )
    }
}

/// 状态机的事件：外壳与界面都只通过它推进状态。
///
/// 事件**自带完整结果**（检查的返回、下载的成败、进度），因此不需要第二个数据源 ——
/// 界面拿到的快照永远是完整的。
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateEvent {
    /// 开始检查更新。
    CheckStarted,
    /// 检查结束（`update_check` 的返回，成功与失败都在里面）。
    CheckFinished(UpdateCheckResponse),
    /// 外壳拒绝了这次下载请求（地址为空 / 不在白名单）。
    ///
    /// 这是一条**没有进度事件**的失败：从前它由页面自己拼落点
    /// （`UpdateSnapshot::failed_after_download_attempt`），现在落点与"下载中途失败"完全一致 ——
    /// 一个持有者、一条规则。落点保留手上的版本号与发行说明，所以用户仍看得见自己在更新什么。
    DownloadRejected { message: String },
    /// 开始下载（外壳已确认没有别的下载在跑）。
    DownloadStarted { url: String, digest: String },
    /// 下载进度（百分比 0..=100 + 已格式化的速度串）。
    DownloadProgress { percent: u32, speed: String },
    /// 下载完成（安装包已落盘并通过 SHA256 校验）。
    DownloadFinished,
    /// 下载失败（`message` 直接展示给用户）。
    DownloadFailed { message: String },
    /// 用户取消下载：临时文件已清理，回到可下载的状态。
    DownloadCancelled,
}

impl UpdateEvent {
    /// 按当前快照推进状态机，返回新快照；不允许的迁移等于**原样返回**。
    ///
    /// 这是更新流程唯一的规则来源，两条纪律都体现在这里：
    /// * "检查失败 ≠ 已是最新"：`CheckFinished` 带 `error` 一律进 [`UpdateStatus::Failed`]；
    /// * "只有下载中收进度"：`downloading` 之外的进度事件被丢弃，状态不会倒退。
    pub fn apply(self, current: &UpdateSnapshot) -> UpdateSnapshot {
        let mut next = current.clone();
        match self {
            UpdateEvent::CheckStarted => {
                next.status = UpdateStatus::Checking;
                next.error = None;
                next.percent = 0;
                next.speed.clear();
            }
            UpdateEvent::CheckFinished(result) => {
                next.error = result.error.clone();
                if result.error.is_some() {
                    // ⚠ 网络失败不能落到 no-update（界面会显示"已是最新版本"）：
                    next.status = UpdateStatus::Failed;
                    // 上一轮的"可更新"信息一并清掉：这一轮的结论是"没查成功"。
                    next.latest_version.clear();
                    next.download_url.clear();
                    next.digest.clear();
                    next.release_body = result.body.clone();
                } else if result.has_update {
                    next.status = UpdateStatus::Available;
                    next.latest_version = result.latest_version.clone();
                    next.download_url = result.download_url.clone();
                    next.digest = result.digest.clone();
                    next.release_body = result.body.clone();
                } else {
                    next.status = UpdateStatus::NoUpdate;
                    next.latest_version.clear();
                    next.download_url.clear();
                    next.digest.clear();
                    next.release_body = result.body.clone();
                }
                next.percent = 0;
                next.speed.clear();
            }
            UpdateEvent::DownloadStarted { url, digest } => {
                // 没有下载地址就不进下载态：否则界面会停在进度条上等一个永远不来的事件。
                if url.is_empty() {
                    return next;
                }
                next.status = UpdateStatus::Downloading;
                next.download_url = url;
                next.digest = digest;
                next.error = None;
                next.percent = 0;
                next.speed.clear();
            }
            UpdateEvent::DownloadProgress { percent, speed } => {
                if next.status != UpdateStatus::Downloading {
                    // 进度事件乱序/迟到（例如完成事件先到）：丢弃，状态不倒退
                    return next;
                }
                next.percent = percent.min(100);
                next.speed = speed;
            }
            UpdateEvent::DownloadFinished => {
                // 已经不在下载中（重复事件 / 取消之后迟到）→ 保持原状
                if next.status != UpdateStatus::Downloading {
                    return next;
                }
                next.status = UpdateStatus::Downloaded;
                next.percent = 100;
                next.speed.clear();
                next.error = None;
            }
            UpdateEvent::DownloadRejected { message } => {
                next.status = UpdateStatus::Failed;
                next.error = Some(message);
                next.percent = 0;
                next.speed.clear();
            }
            UpdateEvent::DownloadFailed { message } => {
                // 取消之后迟到的失败事件不该翻成 error
                if next.status != UpdateStatus::Downloading {
                    return next;
                }
                next.status = UpdateStatus::Failed;
                next.error = Some(message);
                next.percent = 0;
                next.speed.clear();
            }
            UpdateEvent::DownloadCancelled => {
                next.status = UpdateStatus::Available;
                next.error = None;
                next.percent = 0;
                next.speed.clear();
            }
        }
        next
    }
}

/// 界面渲染更新状态所需的**全部**信息（事件载荷与 `update_download_status` 的返回）。
///
/// 事件从前各自只带一小块（`percent` / `filePath` / `message`），界面得把它们拼成状态 ——
/// 那份"拼"的规则正是本模块取代的东西。现在是完整快照：界面只持有"最后一次快照"。
///
/// 字段名是跨进程契约（与 `UpdateCheckResponse` 一样用 camelCase）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSnapshot {
    /// 当前状态。
    pub status: UpdateStatus,
    /// 可更新版本号（`available` / `downloading` / `downloaded` 时非空）。
    #[serde(rename = "latestVersion")]
    pub latest_version: String,
    /// 安装包地址（同上）。
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    /// GitHub asset 的 `digest`（形如 `sha256:...`；下载时用来校验）。
    pub digest: String,
    /// 发行说明（按纯文本渲染；[`UpdateStatus::shows_release_body`] 决定展示与否）。
    #[serde(rename = "releaseBody")]
    pub release_body: String,
    /// 失败原因（`failed` 时非空，直接展示给用户）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 下载进度百分比 0..=100（`downloading` 时有效）。
    pub percent: u32,
    /// 已格式化的下载速度（例如 `2.0 KB/s`；与百分比同源，界面不要再换算）。
    pub speed: String,
}

impl UpdateSnapshot {
    /// 可下载的更新信息（`available` / `downloading` / `downloaded` 时非空）。
    ///
    /// 取消下载、下载失败之后要靠它回到"可更新"状态（见 `UpdateEvent::DownloadCancelled`），
    /// 所以外壳在检查成功时把它记下来。
    pub fn available_info(&self) -> Option<AvailableUpdate> {
        (!self.latest_version.is_empty() && !self.download_url.is_empty()).then(|| {
            AvailableUpdate {
                version: self.latest_version.clone(),
                url: self.download_url.clone(),
                digest: self.digest.clone(),
                body: self.release_body.clone(),
            }
        })
    }

    /// 回到「发现新版本、等待下载」的依据；没有记下来的更新信息时为 `None`（退回 `idle`）。
    pub fn back_to_available(&self) -> UpdateSnapshot {
        match self.available_info() {
            Some(available) => UpdateEvent::CheckFinished(UpdateCheckResponse {
                has_update: true,
                latest_version: available.version,
                download_url: available.url,
                digest: available.digest,
                body: available.body,
                error: None,
            })
            .apply(self),
            None => UpdateSnapshot::default(),
        }
    }

    /// 是否正处在"下载中"。
    pub fn is_downloading(&self) -> bool {
        self.status == UpdateStatus::Downloading
    }

    /// 是否已经有下载好的安装包在等安装。
    pub fn is_downloaded(&self) -> bool {
        self.status == UpdateStatus::Downloaded
    }
}

/// 已发现的更新（检查成功、且版本比当前新）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub url: String,
    pub digest: String,
    pub body: String,
}

/// 用户主动取消时后端给出的固定文案（下载线程用它把"取消"与"真失败"分开）。
pub const CANCELLED: &str = "cancelled";
// ------------------------------------------------------------------ 纯函数（原先在外壳里）

/// 版本比较（按点分段数值比较；`v` 前缀可有可无，段数不同按 0 补齐）。
///
/// 留在领域层而不是外壳：它是"要不要提示有更新"的判据，是最该能在 native 上断言的一段。
pub fn is_newer_version(latest: &str, current: &str) -> bool {
    fn parts(version: &str) -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    }

    let (latest, current) = (parts(latest), parts(current));
    for index in 0..latest.len().max(current.len()) {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left > right {
            return true;
        }
        if left < right {
            return false;
        }
    }
    false
}

/// 规范化 GitHub 的 `digest` 字段：`sha256:ABCD…` → 小写十六进制串。
/// 缺失、空串或只有前缀时返回 `None`，表示**跳过校验**。
pub fn normalize_digest(digest: Option<&str>) -> Option<String> {
    digest
        .map(|digest| digest.trim_start_matches("sha256:").to_ascii_lowercase())
        .filter(|digest| !digest.is_empty())
}

/// 下载完成后是否需要放行：没有 digest 就放行，有就必须逐字符相等（忽略大小写与前缀）。
pub fn digest_matches(expected: Option<&str>, actual_hex: &str) -> bool {
    match normalize_digest(expected) {
        Some(expected) => expected == actual_hex.to_ascii_lowercase(),
        None => true,
    }
}

/// 速度格式化（按 1024 进制给出 B/s、KB/s、MB/s）。
///
/// 展示契约：界面**直接显示**这个串，不再换算，所以它算用户可见文案。
pub fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_second / 1_048_576.0)
    } else if bytes_per_second >= 1024.0 {
        format!("{:.1} KB/s", bytes_per_second / 1024.0)
    } else {
        format!("{} B/s", bytes_per_second.round())
    }
}

// ------------------------------------------------------------------ 用户可见文案
//
// 都在这里：界面的 `match` 与通知标题引用它们，单元测试断言"每个状态都有文案"。
// 改动即改文案契约。

/// 还没检查过时那颗按钮。
pub const LABEL_IDLE: &str = "检查更新";
/// 检查进行中。
pub const LABEL_CHECKING: &str = "正在检查更新…";
/// 已是最新版本。
pub const LABEL_NO_UPDATE: &str = "已是最新版本";
/// 状态兜底文案（`available` 那一档的正文由页面自己拼版本号，所以这里是空提示）。
pub const LABEL_AVAILABLE: &str = "发现新版本";
/// 下载中的百分比之外没有额外文案（进度条 + 百分比 + 速度已经说清了）。
pub const LABEL_DOWNLOADING: &str = "";
/// 下载完成。
pub const LABEL_DOWNLOADED: &str = "下载完成";
/// 失败且后端没给原因时的兜底文案。
pub const LABEL_FAILED: &str = "检查失败，请稍后重试";

/// 通知标题：检查更新失败。
pub const NOTIFY_CHECK_FAILED: &str = "检查更新失败";
/// 通知标题：下载更新失败。
pub const NOTIFY_DOWNLOAD_FAILED: &str = "下载更新失败";
/// 通知标题：安装更新失败。
pub const NOTIFY_INSTALL_FAILED: &str = "安装更新失败";
/// 通知文案：安装包已拉起，应用即将退出。
pub const NOTIFY_INSTALLING: &str = "正在启动安装程序";

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一份"检查成功、有新版本"的快照（状态机的起点）。
    fn available() -> UpdateSnapshot {
        UpdateEvent::CheckFinished(UpdateCheckResponse {
            has_update: true,
            latest_version: "0.29.0".to_string(),
            download_url: "https://github.com/ddd-online/Transactions-Rust/x.exe".to_string(),
            digest: "sha256:AB".to_string(),
            body: "更新说明".to_string(),
            error: None,
        })
        .apply(&UpdateSnapshot::default())
    }

    /// 下载中的快照。
    fn downloading() -> UpdateSnapshot {
        UpdateEvent::DownloadStarted {
            url: "https://github.com/ddd-online/Transactions-Rust/x.exe".to_string(),
            digest: "sha256:AB".to_string(),
        }
        .apply(&available())
    }

    /// 「检查失败 ≠ 已是最新」：网络失败时必须落到 `failed`，而不是 `no-update`。
    ///
    /// 这条规则从前住在页面的渲染函数里（`hasUpdate == false` 且有 `error`）——
    /// 界面若看漏了就会把"请求超时"显示成"已是最新版本"。
    #[test]
    fn check_failure_is_not_reported_as_up_to_date() {
        let failed = UpdateEvent::CheckFinished(UpdateCheckResponse {
            error: Some("请求 GitHub API 失败: timeout".to_string()),
            ..UpdateCheckResponse::default()
        })
        .apply(&UpdateSnapshot::default());

        assert_eq!(failed.status, UpdateStatus::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some("请求 GitHub API 失败: timeout")
        );

        // 反面：真的查完了、没有新版 → no-update，且不该带 error
        let no_update = UpdateEvent::CheckFinished(UpdateCheckResponse::default())
            .apply(&UpdateSnapshot::default());
        assert_eq!(no_update.status, UpdateStatus::NoUpdate);
        assert!(no_update.error.is_none());
    }

    /// 一次完整的"检查 → 下载 → 完成"：每一步的展示字段都要对得上。
    #[test]
    fn happy_path_walks_from_idle_to_downloaded() {
        let start = UpdateSnapshot::default();
        assert_eq!(start.status, UpdateStatus::Idle);

        let checking = UpdateEvent::CheckStarted.apply(&start);
        assert_eq!(checking.status, UpdateStatus::Checking);
        assert_eq!(checking.status.label(), "正在检查更新…");

        let found = UpdateEvent::CheckFinished(UpdateCheckResponse {
            has_update: true,
            latest_version: "0.29.0".to_string(),
            download_url: "https://github.com/x/y.exe".to_string(),
            digest: "sha256:AB".to_string(),
            body: "更新说明".to_string(),
            error: None,
        })
        .apply(&checking);
        assert_eq!(found.status, UpdateStatus::Available);
        assert_eq!(found.latest_version, "0.29.0");
        assert_eq!(found.release_body, "更新说明");

        let running = UpdateEvent::DownloadStarted {
            url: "https://github.com/x/y.exe".to_string(),
            digest: "sha256:AB".to_string(),
        }
        .apply(&found);
        assert_eq!(running.status, UpdateStatus::Downloading);

        let midway = UpdateEvent::DownloadProgress {
            percent: 42,
            speed: "2.0 KB/s".to_string(),
        }
        .apply(&running);
        assert_eq!((midway.percent, midway.speed.as_str()), (42, "2.0 KB/s"));

        let done = UpdateEvent::DownloadFinished.apply(&midway);
        assert_eq!(done.status, UpdateStatus::Downloaded);
        assert_eq!(done.percent, 100);
        assert!(done.is_downloaded());
        // 下载完成后仍带着版本信息（"重启后还能安装"依赖外壳重新问一次状态）
        assert_eq!(done.latest_version, "0.29.0");
    }

    /// 取消下载：回到「发现新版本」，且临时进度清干净。
    #[test]
    fn cancel_returns_to_available_with_clean_progress() {
        let running = UpdateEvent::DownloadProgress {
            percent: 30,
            speed: "1.0 MB/s".to_string(),
        }
        .apply(&downloading());
        let cancelled = UpdateEvent::DownloadCancelled.apply(&running);

        assert_eq!(cancelled.status, UpdateStatus::Available);
        assert_eq!((cancelled.percent, cancelled.speed.as_str()), (0, ""));
        assert_eq!(cancelled.latest_version, "0.29.0");
        assert_eq!(cancelled.download_url, running.download_url);
        // 可以立刻重新下载（与"取消后从头开始"一致）
        let again = UpdateEvent::DownloadStarted {
            url: running.download_url.clone(),
            digest: running.digest.clone(),
        }
        .apply(&cancelled);
        assert_eq!(again.status, UpdateStatus::Downloading);
        assert_eq!(again.percent, 0);
    }

    /// 进度事件与完成事件**乱序**到达（或者轮询到的旧快照随后又被事件覆盖）：
    /// 结果只取决于"最后一个能生效的事件"，状态不会倒退。
    #[test]
    fn late_progress_does_not_roll_back_a_finished_download() {
        let done = UpdateEvent::DownloadFinished.apply(&downloading());
        let late = UpdateEvent::DownloadProgress {
            percent: 90,
            speed: "9.0 MB/s".to_string(),
        }
        .apply(&done);

        assert_eq!(
            late.status,
            UpdateStatus::Downloaded,
            "迟到的进度不能把状态拉回下载中"
        );
        assert_eq!(late.percent, 100, "完成后的百分比仍是 100");
        assert!(late.speed.is_empty());
    }

    /// 重复事件幂等：同一个事件应用两次与一次等价。
    #[test]
    fn repeated_events_are_idempotent() {
        let running = downloading();
        let once = UpdateEvent::DownloadProgress {
            percent: 50,
            speed: "1.5 MB/s".to_string(),
        }
        .apply(&running);
        let twice = UpdateEvent::DownloadProgress {
            percent: 50,
            speed: "1.5 MB/s".to_string(),
        }
        .apply(&once);
        assert_eq!(once, twice);

        let done = UpdateEvent::DownloadFinished.apply(&once);
        assert_eq!(UpdateEvent::DownloadFinished.apply(&done), done);

        let failed = UpdateEvent::DownloadFailed {
            message: "下载中断: broken pipe".to_string(),
        }
        .apply(&downloading());
        assert_eq!(
            UpdateEvent::DownloadFailed {
                message: "下载中断: broken pipe".to_string()
            }
            .apply(&failed),
            failed
        );

        let cancelled = UpdateEvent::DownloadCancelled.apply(&running);
        assert_eq!(UpdateEvent::DownloadCancelled.apply(&cancelled), cancelled);
        assert_eq!(cancelled.status, UpdateStatus::Available);
    }

    /// 失败事件只在下载中生效：取消之后迟到的失败事件不能把界面翻成 error。
    #[test]
    fn failure_is_ignored_once_the_download_is_gone() {
        let after_cancel = UpdateEvent::DownloadCancelled.apply(&downloading());
        let late = UpdateEvent::DownloadFailed {
            message: "下载中断".to_string(),
        }
        .apply(&after_cancel);
        assert_eq!(late.status, UpdateStatus::Available);
        assert!(late.error.is_none());
    }

    /// 下载失败：留在可重试的位置（版本信息还在，"立即更新"能再点一次）。
    #[test]
    fn download_failure_keeps_the_update_retryable() {
        let failed = UpdateEvent::DownloadFailed {
            message: "下载文件校验失败（SHA256 不匹配）".to_string(),
        }
        .apply(&downloading());

        assert_eq!(failed.status, UpdateStatus::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some("下载文件校验失败（SHA256 不匹配）")
        );
        assert_eq!(failed.percent, 0);
        assert!(failed.speed.is_empty());
        assert_eq!(
            failed.available_info(),
            Some(AvailableUpdate {
                version: "0.29.0".to_string(),
                url: downloading().download_url,
                digest: "sha256:AB".to_string(),
                body: "更新说明".to_string(),
            })
        );
    }

    /// 下载地址为空时不进"下载中"：否则界面会等一个永远不来的进度事件。
    #[test]
    fn download_started_without_a_url_stays_put() {
        let found = available();
        let stuck = UpdateEvent::DownloadStarted {
            url: String::new(),
            digest: String::new(),
        }
        .apply(&found);
        assert_eq!(stuck.status, UpdateStatus::Available);
    }

    /// 外壳拒绝下载（地址为空 / 不在白名单）→ 落 `failed`，且**保留**手上的版本号与说明：
    /// 用户还能看清自己在更新什么，也能直接再点一次（`failed` 允许重新检查 / 重试）。
    #[test]
    fn rejected_download_request_lands_in_failed_and_keeps_what_it_knows() {
        let found = available();
        let rejected = UpdateEvent::DownloadRejected {
            message: "下载地址不在白名单内".to_string(),
        }
        .apply(&found);
        assert_eq!(rejected.status, UpdateStatus::Failed);
        assert_eq!(rejected.error.as_deref(), Some("下载地址不在白名单内"));
        assert_eq!(rejected.latest_version, "0.29.0");
        assert_eq!(rejected.release_body, found.release_body);
        assert!(rejected.status.shows_release_body());

        // 连版本号都没有时同样是 `failed`：仍然可点（不是把用户丢在一个不能重试的界面里）
        let nothing_known = UpdateEvent::DownloadRejected {
            message: "无效的下载地址".to_string(),
        }
        .apply(&UpdateSnapshot::default());
        assert_eq!(nothing_known.status, UpdateStatus::Failed);
        assert!(nothing_known.status.allows_new_check());
    }

    /// 下载完成后重启进程（界面拿到的是一份新快照）：已下载的安装包仍可安装。
    #[test]
    fn a_fresh_snapshot_can_report_downloaded() {
        // 外壳重建快照：状态 downloaded，且版本信息还能从检查结果里补回来
        let restored = UpdateSnapshot {
            status: UpdateStatus::Downloaded,
            percent: 100,
            ..available()
        };
        assert!(restored.is_downloaded());
        assert!(!restored.is_downloading());
        // 界面只看快照，不再自己判断"文件还在不在"（外壳发之前已经核对过文件）
        assert_eq!(restored.status.label(), "下载完成");
    }

    /// 下载中不允许再开一次检查（会把进度冲掉）；终态里 `downloaded` 也不允许。
    #[test]
    fn new_checks_are_refused_while_a_download_is_in_flight() {
        assert!(UpdateStatus::Idle.allows_new_check());
        assert!(UpdateStatus::NoUpdate.allows_new_check());
        assert!(UpdateStatus::Failed.allows_new_check());
        assert!(UpdateStatus::Available.allows_new_check());
        assert!(!UpdateStatus::Downloading.allows_new_check());
        assert!(!UpdateStatus::Downloaded.allows_new_check());
    }

    /// 每个状态都有用户可见文案（界面 `match` 的每个分支都靠它；`downloading` 那档
    /// 正文由进度条与百分比承担、`available` 那档正文是页面拼的版本号，这两档允许是空串）。
    #[test]
    fn every_status_has_its_own_label() {
        let all = [
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            UpdateStatus::Available,
            UpdateStatus::NoUpdate,
            UpdateStatus::Downloading,
            UpdateStatus::Downloaded,
            UpdateStatus::Failed,
        ];
        for status in all {
            let label = status.label();
            let expected = match status {
                UpdateStatus::Idle => LABEL_IDLE,
                UpdateStatus::Checking => LABEL_CHECKING,
                UpdateStatus::Available => LABEL_AVAILABLE,
                UpdateStatus::NoUpdate => LABEL_NO_UPDATE,
                UpdateStatus::Downloading => LABEL_DOWNLOADING,
                UpdateStatus::Downloaded => LABEL_DOWNLOADED,
                UpdateStatus::Failed => LABEL_FAILED,
            };
            assert_eq!(label, expected, "{status:?} 的文案漂了");
            // 有正文的那几档不能是空串（空串 = 界面上会留一段空白）
            if status != UpdateStatus::Downloading {
                assert!(!label.is_empty(), "{status:?} 的文案不能是空串");
            }
        }
        // 按钮与通知文案不能为空
        for text in [
            LABEL_IDLE,
            LABEL_CHECKING,
            LABEL_NO_UPDATE,
            LABEL_AVAILABLE,
            LABEL_DOWNLOADED,
            LABEL_FAILED,
            NOTIFY_CHECK_FAILED,
            NOTIFY_DOWNLOAD_FAILED,
            NOTIFY_INSTALL_FAILED,
            NOTIFY_INSTALLING,
        ] {
            assert!(!text.is_empty());
        }
        // 三条通知各有各的名字（否则用户分不清是检查、下载还是安装挂了）
        assert_ne!(NOTIFY_CHECK_FAILED, NOTIFY_DOWNLOAD_FAILED);
        assert_ne!(NOTIFY_DOWNLOAD_FAILED, NOTIFY_INSTALL_FAILED);
        assert_ne!(NOTIFY_CHECK_FAILED, NOTIFY_INSTALL_FAILED);
    }

    /// 发行说明的展示条件（界面原来自己写 `matches!`，现在归状态）。
    #[test]
    fn release_notes_show_only_where_they_matter() {
        assert!(!UpdateStatus::Idle.shows_release_body());
        assert!(!UpdateStatus::Checking.shows_release_body());
        assert!(!UpdateStatus::NoUpdate.shows_release_body());
        assert!(UpdateStatus::Available.shows_release_body());
        assert!(UpdateStatus::Downloading.shows_release_body());
        assert!(UpdateStatus::Downloaded.shows_release_body());
        assert!(UpdateStatus::Failed.shows_release_body());
    }

    /// 状态词表就是字符串契约：`no-update` 是连字符（不是 `no_update`）。
    #[test]
    fn status_words_keep_the_documented_spelling() {
        let word = |status: UpdateStatus| serde_json::to_string(&status).unwrap();
        assert_eq!(word(UpdateStatus::Idle), "\"idle\"");
        assert_eq!(word(UpdateStatus::Checking), "\"checking\"");
        assert_eq!(word(UpdateStatus::Available), "\"available\"");
        assert_eq!(word(UpdateStatus::NoUpdate), "\"no-update\"");
        assert_eq!(word(UpdateStatus::Downloading), "\"downloading\"");
        assert_eq!(word(UpdateStatus::Downloaded), "\"downloaded\"");
        assert_eq!(word(UpdateStatus::Failed), "\"failed\"");
        // 缺省状态是 idle（`#[serde(default)]` 的兜底）
        assert_eq!(UpdateStatus::default(), UpdateStatus::Idle);
    }

    /// 快照的 JSON 形状是跨进程契约：字段名与"缺省值"都不能漂。
    #[test]
    fn snapshot_keeps_the_documented_wire_shape() {
        let snapshot = UpdateEvent::DownloadProgress {
            percent: 42,
            speed: "2.0 KB/s".to_string(),
        }
        .apply(&downloading());
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();

        assert_eq!(value["status"], "downloading");
        assert_eq!(value["latestVersion"], "0.29.0");
        assert_eq!(
            value["downloadUrl"],
            "https://github.com/ddd-online/Transactions-Rust/x.exe"
        );
        assert_eq!(value["digest"], "sha256:AB");
        assert_eq!(value["releaseBody"], "更新说明");
        assert_eq!(value["percent"], 42);
        assert_eq!(value["speed"], "2.0 KB/s");
        assert!(value.get("error").is_none(), "成功时不该带 error 字段");

        // 缺字段的载荷按默认值反序列化（旧版本进程 / 手写测试数据）
        let partial: UpdateSnapshot =
            serde_json::from_str(r#"{"status":"failed","error":"坏了"}"#).unwrap();
        assert_eq!(partial.status, UpdateStatus::Failed);
        assert_eq!(partial.error.as_deref(), Some("坏了"));
        assert_eq!((partial.percent, partial.speed.as_str()), (0, ""));
    }

    #[test]
    fn version_comparison_orders_by_numeric_segments() {
        assert!(is_newer_version("0.29.0", "0.28.0"));
        assert!(is_newer_version("v0.29.0", "0.28.0"));
        assert!(is_newer_version("1.0.0", "0.28.0"));
        assert!(is_newer_version("0.28.1", "0.28.0"));
        assert!(!is_newer_version("0.28.0", "0.28.0"));
        assert!(!is_newer_version("0.1.9", "0.2.0"));
        // 段数不同按 0 补齐
        assert!(is_newer_version("0.28.0.1", "0.28.0"));
        assert!(!is_newer_version("0.28", "0.28.0"));
    }

    /// digest 规范化：GitHub 返回大写十六进制 + `sha256:` 前缀。
    #[test]
    fn normalize_digest_strips_prefix_and_lowercases() {
        assert_eq!(
            normalize_digest(Some("sha256:ABCDEF")),
            Some("abcdef".to_string())
        );
        // 没有前缀也照收（按原样比较）
        assert_eq!(normalize_digest(Some("AbCdEf")), Some("abcdef".to_string()));
        // 缺失 / 空串 / 只有前缀 → 跳过校验
        assert_eq!(normalize_digest(None), None);
        assert_eq!(normalize_digest(Some("")), None);
        assert_eq!(normalize_digest(Some("sha256:")), None);
    }

    /// 校验语义：没有 digest 就放行；有就必须相等（大小写不敏感）。
    #[test]
    fn digest_matches_only_when_equal_or_absent() {
        assert!(digest_matches(None, "abc123"));
        assert!(digest_matches(Some(""), "abc123"));
        assert!(digest_matches(Some("sha256:ABC123"), "abc123"));
        assert!(digest_matches(Some("abc123"), "ABC123"));
        assert!(!digest_matches(Some("sha256:abc123"), "abc124"));
        assert!(!digest_matches(Some("sha256:abc123"), ""));
    }

    /// 速度文案：界面直接展示它，所以格式是用户可见契约（例如 `2.0 KB/s`）。
    #[test]
    fn speed_formatting_is_stable() {
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(3.0 * 1_048_576.0), "3.0 MB/s");
    }
}
