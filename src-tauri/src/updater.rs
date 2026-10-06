//! 应用更新：GitHub Releases 检查 → 流式下载 → SHA256 校验 → 拉起安装包并退出。
//!
//! ## 谁负责什么
//!
//! * **状态词表与合法迁移**在 `tr_domain::update`（纯逻辑，两侧共用，能在 native 上真跑）；
//! * **本模块是状态的唯一持有者**（只有它能做 I/O）：推进状态机、把**完整快照**
//!   （`UpdateSnapshot`）通过事件发给界面，并回答「现在是什么状态」；
//! * **界面只渲染**它读到的快照，自己不再拼状态、不再判断"失败还是最新"。
//!
//! 因此这里只做三件事：**发请求 / 落盘 / 推快照**。任何"什么状态能到什么状态"的判断
//! 都不在这份文件里（连"失败还是最新"也只是把检查的返回交给状态机）。
//!
//! ## 行为清单
//!
//! * 只认 GitHub 的 latest release、跳过 prerelease、取第一个 `.exe` 资产
//! * 检查更新超时 15s（下载另给 1800s），下载地址只允许 GitHub 域名
//! * 下载到 `%TEMP%`，已存在则**先核对 digest 再复用**；流式写入 `<file>.part` 再改名
//! * 用 GitHub 提供的 `asset.digest`（`sha256:...`）校验完整性，缺失则跳过校验
//! * 取消时清理临时文件与已下载文件
//! * 打开安装包后退出应用
//!
//! 事件名只有一份定义（`tr_domain::events`，界面订阅的是同一份常量）。载荷**一律是
//! 完整快照**（从前是"各自带一小块字段，界面再拼"）。改的是载荷，不是名字。
//!
//! **为什么不用 `tauri-plugin-updater`**：本项目的发布管线只上传普通 `.exe` 资产（没有签名与 `latest.json`），
//! 自研路径沿用同一管线、用 `asset.digest` 做完整性校验，无需引入签名密钥管理；
//! 界面契约不变，后续若要切换到插件只需替换本文件与发布脚本。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};

use tr_domain::events::{
    UPDATE_DOWNLOAD_COMPLETE, UPDATE_DOWNLOAD_ERROR, UPDATE_DOWNLOAD_PROGRESS,
};
use tr_domain::update as update_state;
use tr_domain::update::{UpdateEvent, UpdateSnapshot};
use tr_domain::wire::{UpdateCheckResponse, UpdateDownloadRequest, UpdateResponse};
use tr_ipc::ApiResult;

use crate::commands::internal;

/// GitHub 最新 release 接口。
///
/// **必须是本仓库自身**：发布资产与应用内更新一一对应；指向别的仓库会比对到不相干的
/// 版本并下载错误的安装包。
const RELEASE_API: &str =
    "https://api.github.com/repos/ddd-online/Transactions-Rust/releases/latest";
/// 检查更新的超时（15s）。
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// 下载安装包的超时（安装包可达数百 MB，给足时间）。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(1800);

/// 取消标记对应的错误文本（与 `tr_domain::update::CANCELLED` 同值；状态机认它）。
const CANCELLED: &str = update_state::CANCELLED;

/// 更新流程的状态（由 Tauri 托管）。
///
/// **快照是唯一的真相**（`state`），进度字段只是它的两个"高频副本"：下载线程每读一块
/// 就要更新一次百分比与速度，走一遍状态机太笨重，所以分开存、取快照时再拼进去。
///
/// 下载是**单例**：`active` 非空表示正有一笔下载在跑（值是 `url|digest`），
/// 同一时间只允许一笔；界面据此恢复进度（见 [`update_download_status`]）。
///
/// 字段内部包一层 `Arc`：下载跑在 `spawn_blocking` 的闭包里（要求 `'static`），
/// 需要把句柄**移动**进去，光有 `&UpdaterState` 是不够的（`clone()` 出来的副本共享同一份状态）。
#[derive(Default, Clone)]
pub struct UpdaterState {
    /// 当前快照（状态 + 可更新信息 + 失败原因）。
    state: Arc<Mutex<UpdateSnapshot>>,
    cancel: Arc<AtomicBool>,
    /// 已下载好的安装包路径（可能已被系统清理，取快照时要核对文件还在不在）。
    downloaded_path: Arc<Mutex<Option<PathBuf>>>,
    /// 正在下载的键（`url|digest`）；`None` = 没有下载在跑
    active: Arc<Mutex<Option<String>>>,
    /// 最近一次上报的进度百分比（下载线程高频写）
    percent: Arc<AtomicU32>,
    /// 最近一次上报的速度文案（下载线程高频写）
    speed: Arc<Mutex<String>>,
}

impl UpdaterState {
    /// 取当前快照（把两个高频进度字段拼进去）：
    ///
    /// * 已经下载好的文件被系统清理掉时**不能再报"已下载"**，否则用户点安装会拿到空文件 ——
    ///   按状态机回到「发现新版本」（没有记下更新信息时退回 `idle`）。
    /// * 下载已结束（取消 / 失败 / 完成）时不带残留的进度。
    fn snapshot(&self) -> UpdateSnapshot {
        let state = self.state.lock().expect("更新状态锁中毒").clone();
        let is_downloading = state.is_downloading();
        let downloaded_file_missing = state.is_downloaded()
            && !self
                .downloaded_path
                .lock()
                .expect("更新状态锁中毒")
                .as_ref()
                .is_some_and(|path| path.exists());
        if downloaded_file_missing {
            return state.back_to_available();
        }
        if !is_downloading {
            return state;
        }
        UpdateSnapshot {
            percent: self.percent.load(Ordering::SeqCst),
            speed: self.speed.lock().expect("更新状态锁中毒").clone(),
            ..state
        }
    }

    /// 按事件推进状态机（这是本模块唯一改状态的地方）。
    fn apply(&self, event: UpdateEvent) -> UpdateSnapshot {
        // 借用一个独立的作用域：`snapshot()` 也要拿这把锁，不能带着借用过去。
        {
            let mut state = self.state.lock().expect("更新状态锁中毒");
            *state = event.apply(&state);
        }
        self.snapshot()
    }

    /// 广播快照：三个事件（进度 / 完成 / 失败）都发同一份完整载荷。
    ///
    /// 发失败（窗口已关等）只记一条日志：状态已经在内存里，界面切回来读得到。
    fn emit_snapshot(&self, app: &AppHandle, event: &str) -> UpdateSnapshot {
        let snapshot = self.snapshot();
        if let Err(error) = app.emit(event, &snapshot) {
            tracing::warn!("update: 广播 {event} 失败: {error}");
        }
        snapshot
    }

    /// 下载线程上报进度（比走一遍状态机便宜，见字段注释）。
    fn report_progress(&self, app: &AppHandle, percent: u32, speed: String) {
        self.percent.store(percent, Ordering::SeqCst);
        *self.speed.lock().expect("更新状态锁中毒") = speed;
        // 不是下载中就不要广播（取消之后迟到的进度会惊动界面）
        if self.state.lock().expect("更新状态锁中毒").is_downloading() {
            self.emit_snapshot(app, UPDATE_DOWNLOAD_PROGRESS);
        }
    }

    /// 记下已下载好的安装包（`None` = 忘掉它；取快照时会核对文件还在不在）。
    fn remember_downloaded(&self, path: Option<PathBuf>) {
        *self.downloaded_path.lock().expect("更新状态锁中毒") = path;
    }

    /// 清掉进度残留（下载结束后调用）。
    fn clear_progress(&self) {
        self.percent.store(0, Ordering::SeqCst);
        self.speed.lock().expect("更新状态锁中毒").clear();
    }
}

/// 建立带全局超时的 agent（与 `tr-service::quote` 用同一套 ureq 配置方式）。
///
/// **代理**：显式传入当前设置解析出的 `ureq::Proxy`（`off` / 探测不到时是 `None`）——
/// 不显式给的话 ureq 会退回它默认的"读环境变量"，那样「不使用代理」就形同虚设。
/// `update_check` 与下载都走这一个函数，两条链路不会半生效。
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .proxy(tr_service::proxy::agent_proxy())
        .build()
        .into()
}

/// 检查更新（**外壳自己推进状态机**，返回推进后的完整快照）。
///
/// 失败不抛错：把原因交回状态机（它负责"失败 ≠ 已是最新"），界面读快照即可。
/// 一进门 `CheckStarted`、拿到结果 `CheckFinished` —— 与下载那几条一样，这里是**唯一**
/// 推进"检查"这段状态的地方。从前界面自己 apply 这两步，外壳的状态机只被下载喂过，
/// 于是 `update_download_status` 返回的永远是 idle：检查出「已是最新 / 有新版本」之后
/// 换页再切回「关于软件」会把结果抹掉（实测确认过，见 CHANGELOG 的 0.15.0 一节）。
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> ApiResult<UpdateSnapshot> {
    state.apply(UpdateEvent::CheckStarted);
    let current_version = app.package_info().version.to_string();
    // 阻塞任务自身炸了也要落一档：否则状态机会永远停在 `checking`
    let result =
        match tauri::async_runtime::spawn_blocking(move || check_update(&current_version)).await {
            Ok(response) => response,
            Err(error) => error_response(format!("更新检查任务失败: {error}")),
        };
    state.apply(UpdateEvent::CheckFinished(result));
    Ok(state.snapshot())
}

fn check_update(current_version: &str) -> UpdateCheckResponse {
    let response = agent(CHECK_TIMEOUT)
        .get(RELEASE_API)
        .header("User-Agent", "Transactions-App")
        .header("Accept", "application/vnd.github+json")
        .call();

    let mut response = match response {
        Ok(response) => response,
        Err(error) => return error_response(format!("请求 GitHub API 失败: {error}")),
    };

    let body = match response.body_mut().read_to_string() {
        Ok(body) => body,
        Err(error) => return error_response(format!("读取 GitHub API 响应失败: {error}")),
    };

    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => return error_response("Invalid JSON response".to_string()),
    };

    parse_release(&payload, current_version).unwrap_or_default()
}

/// 从 GitHub release JSON 里挑出更新信息（**纯函数**，便于单测）。
///
/// 返回 `None` 表示"无需更新"：预发布、版本不比当前新，或 tag 为空。
/// 注意"有更新但没有 `.exe` 资产"**仍算有更新**（`download_url` 为空串），
/// 这样界面能如实提示"新版本但找不到安装包"。
fn parse_release(
    payload: &serde_json::Value,
    current_version: &str,
) -> Option<UpdateCheckResponse> {
    if payload
        .get("prerelease")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let latest_version = payload
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();

    if latest_version.is_empty()
        || !update_state::is_newer_version(&latest_version, current_version)
    {
        return None;
    }

    // 取**第一个** `browser_download_url` 以 `.exe` 结尾的资产（发布产物里可能还有 zip/sig）
    let asset = payload
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset
                    .get("browser_download_url")
                    .and_then(serde_json::Value::as_str)
                    .map(|url| url.ends_with(".exe"))
                    .unwrap_or(false)
            })
        });

    Some(UpdateCheckResponse {
        has_update: true,
        latest_version,
        download_url: asset
            .and_then(|asset| asset.get("browser_download_url"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        digest: asset
            .and_then(|asset| asset.get("digest"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        body: payload
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        error: None,
    })
}

fn error_response(message: String) -> UpdateCheckResponse {
    tracing::warn!("update:check error: {message}");
    UpdateCheckResponse {
        error: Some(message),
        ..UpdateCheckResponse::default()
    }
}

/// 下载安装包（**外壳自己推进状态机**，返回推进后的完整快照）。
///
/// 下载是**单例**：同一时间只允许一笔。已有下载在跑时**状态机原样不动**、直接把当前
/// 快照还回去（界面继续显示进度，也不会重复触发）。下载期间状态机停在 `downloading`
/// 并广播进度，供界面切回时恢复。
///
/// 两条**没有进度事件**的早退路径（地址为空 / 不在白名单）落 `DownloadRejected`：
/// 与"下载中途失败"同一个落点。界面从前要按 `UpdateSnapshot::failed_after_download_attempt`
/// 自己拼一个落点 —— 那条规则已经并进状态机。
#[tauri::command]
pub async fn update_download(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    req: UpdateDownloadRequest,
) -> ApiResult<UpdateSnapshot> {
    let key = request_key(&req.url, req.digest.as_deref());
    {
        let mut active = state.active.lock().expect("更新状态锁中毒");
        if active.is_some() {
            // 已经有一笔在跑：**不动状态**，把当前快照还回去（界面继续显示进度）
            return Ok(state.snapshot());
        }
        *active = Some(key);
    }
    state.clear_progress();

    run_download(&app, &state, req).await?;
    // 无论成败都要把"正在下载"清掉，否则会永远挡住下一次下载
    *state.active.lock().expect("更新状态锁中毒") = None;
    state.clear_progress();
    Ok(state.snapshot())
}

/// 真正执行下载（单例判断由 [`update_download`] 负责）：**这里只推进状态机，不再回响应体**。
async fn run_download(
    app: &AppHandle,
    state: &State<'_, UpdaterState>,
    req: UpdateDownloadRequest,
) -> ApiResult<()> {
    // URL 白名单：仅允许 GitHub 域名（防止界面被注入后下载任意地址）。
    // 被拒的两条路都**没有进度事件**，所以要显式落一档：`DownloadRejected` 与"下载中途失败"
    // 同一个落点（`failed` + 原因），界面不必再自己拼一个落点。
    let Some(host) = url_host(&req.url) else {
        state.apply(UpdateEvent::DownloadRejected {
            message: "无效的下载地址".to_string(),
        });
        return Ok(());
    };
    if !host.ends_with("github.com") && !host.ends_with("objects.githubusercontent.com") {
        state.apply(UpdateEvent::DownloadRejected {
            message: "下载地址不在白名单内".to_string(),
        });
        return Ok(());
    }

    // 进入下载态（状态机决定 `idle` / `available` 到这里意味着什么）
    state.apply(UpdateEvent::DownloadStarted {
        url: req.url.clone(),
        digest: req.digest.clone().unwrap_or_default(),
    });

    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);

    let app_handle = app.clone();
    let url = req.url.clone();
    let digest = req.digest.clone();
    // 状态句柄：字段都是 `Arc`，clone 一份共享同一份状态，移动进阻塞线程
    let shared_state = (**state).clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_and_verify(&app_handle, &url, digest.as_deref(), &cancel, &shared_state)
    })
    .await
    .map_err(internal)?;

    match result {
        Ok(path) => {
            state.remember_downloaded(Some(path));
            state.apply(UpdateEvent::DownloadFinished);
            state.emit_snapshot(app, UPDATE_DOWNLOAD_COMPLETE);
            Ok(())
        }
        Err(message) if message == CANCELLED => {
            // 用户主动取消：状态回到"可下载"，不广播（发起方就是界面，它自己知道）
            state.apply(UpdateEvent::DownloadCancelled);
            Ok(())
        }
        Err(message) => {
            state.apply(UpdateEvent::DownloadFailed {
                message: message.clone(),
            });
            state.emit_snapshot(app, UPDATE_DOWNLOAD_ERROR);
            Ok(())
        }
    }
}

/// 当前更新状态：界面（重新）进入「关于软件」时调用它**取一份完整快照**。
///
/// 这就是"补状态"，与事件走同一个状态机、同一份字段 —— 所以"进页面时读到的"
/// 与"事件推过来的"永远一致，二者谁先到都不影响结果。
#[tauri::command]
pub fn update_download_status(state: State<'_, UpdaterState>) -> ApiResult<UpdateSnapshot> {
    Ok(state.snapshot())
}

/// 下载去重键：同一个地址 + 同一份 digest 视为同一次下载。
fn request_key(url: &str, digest: Option<&str>) -> String {
    format!("{url}|{}", digest.unwrap_or_default())
}

/// 用户主动取消：置位取消标记、清理临时文件，并把状态退回「可下载」。
///
/// 只有真的有一笔在跑时才动状态：拿到取消按钮的用户必然在下载中，但幂等一点更安全
/// （重复调用不会把 `downloaded` 打回 `available`）。
#[tauri::command]
pub fn update_cancel(state: State<'_, UpdaterState>) -> ApiResult<UpdateSnapshot> {
    state.cancel.store(true, Ordering::SeqCst);
    if state.active.lock().expect("更新状态锁中毒").is_some() {
        state.apply(UpdateEvent::DownloadCancelled);
    }
    if let Some(path) = state.downloaded_path.lock().expect("更新状态锁中毒").take() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(part_path_of(&path));
    }
    Ok(state.snapshot())
}

/// 打开已下载的安装包并退出应用（给安装器留出启动时间后再退出）。
#[tauri::command]
pub fn update_install(app: AppHandle, state: State<'_, UpdaterState>) -> ApiResult<UpdateResponse> {
    let path = state
        .downloaded_path
        .lock()
        .expect("更新状态锁中毒")
        .clone();

    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(UpdateResponse::failed("安装文件不存在"));
    };

    use tauri_plugin_opener::OpenerExt;
    if let Err(error) = app
        .opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
    {
        tracing::error!("update:install error: {error}");
        return Ok(UpdateResponse::failed(error.to_string()));
    }

    // 给安装器一点启动时间后退出：安装器需要独占替换程序文件
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        handle.exit(0);
    });
    Ok(UpdateResponse::ok())
}

/// 流式下载 + SHA256 校验；返回最终文件路径。
///
/// `state` 是共享（`Arc` 克隆）的状态句柄：进度要**同时**落进快照并广播，
/// 不带它就没有"切回页面立刻看到进度"这条能力。
fn download_and_verify(
    app: &AppHandle,
    url: &str,
    expected_digest: Option<&str>,
    cancel: &AtomicBool,
    state: &UpdaterState,
) -> Result<PathBuf, String> {
    let file_name = url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("transactions-update.exe");
    let target = std::env::temp_dir().join(file_name);

    // 复用 `%TEMP%` 里的旧文件之前**必须核对 digest**：文件名只带版本号
    // （`Transactions-x64-v{版本}.exe`），而同一个 tag 的资产是允许被重新上传的
    // （真实事故：v0.2.0 的资产曾经就是 0.1.0 的安装包，修好后重新上传 —— 若这里直接复用，
    // 用户点「立即更新」装上的还是那份旧包，现象就是"更新完界面还是旧版"）。
    // 不匹配（或读不出来）就删掉重下，绝不把旧字节当新版装出去。
    if target.exists() {
        if file_digest_matches(&target, expected_digest) {
            return Ok(target);
        }
        tracing::warn!("update:download 复用的缓存文件校验不通过，已删除并重新下载");
        let _ = std::fs::remove_file(&target);
    }

    let part = part_path_of(&target);

    let mut response = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .header("User-Agent", "Transactions-App")
        .call()
        .map_err(|error| format!("下载失败: {error}"))?;

    let total: u64 = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);

    let mut reader = response.body_mut().as_reader();
    let mut file =
        std::fs::File::create(&part).map_err(|error| format!("创建临时文件失败: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let started = Instant::now();

    loop {
        if cancel.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err(CANCELLED.to_string());
        }

        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("下载中断: {error}"))?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|error| format!("写入失败: {error}"))?;
        hasher.update(&buffer[..read]);
        downloaded += read as u64;

        let percent = downloaded
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0) as u32;
        let elapsed = started.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            update_state::format_speed(downloaded as f64 / elapsed)
        } else {
            "0 B/s".to_string()
        };
        // 进度同时落进状态并广播完整快照：界面（重新）进入「关于软件」时
        // 靠 `update_download_status` 立刻回显，不必等下一个事件（大文件时事件间隔可能很久）
        state.report_progress(app, percent, speed);
    }

    file.flush()
        .map_err(|error| format!("刷新文件失败: {error}"))?;
    drop(file);

    if !update_state::digest_matches(expected_digest, &format!("{:x}", hasher.finalize())) {
        let _ = std::fs::remove_file(&part);
        return Err("下载文件校验失败（SHA256 不匹配）".to_string());
    }
    std::fs::rename(&part, &target).map_err(|error| format!("重命名失败: {error}"))?;
    Ok(target)
}

fn part_path_of(target: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.part", target.display()))
}

/// 复用已下载文件前的校验：有期望 digest 就必须一致，没有（release 未提供 digest）则放行。
/// 文件读不出来（权限 / 被占用）按"不可复用"处理，调用方会删掉它重新下载。
fn file_digest_matches(path: &std::path::Path, expected_digest: Option<&str>) -> bool {
    if update_state::normalize_digest(expected_digest).is_none() {
        return true;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return false,
        }
    }
    update_state::digest_matches(expected_digest, &format!("{:x}", hasher.finalize()))
}

/// 取 URL 的主机名（去掉 userinfo 与端口），无法解析时返回 `None`。
///
/// 留在外壳侧（而不是 `tr_domain::update`）：它是**下载地址白名单**这条安全规则的判据，
/// 只有外壳发得出请求。
fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::update::UpdateStatus;

    /// 取一份完整快照（与命令体同一套逻辑，避免测试里也抄一份）。
    fn snapshot_of(state: &UpdaterState) -> UpdateSnapshot {
        state.snapshot()
    }

    /// 快照查询：**这是"切回「关于软件」能恢复进度"的判据**。
    ///
    /// 四种情况都要能如实报出来：什么都没发生、正在下载（带最近一次进度）、
    /// 已下载但文件被清理掉了（此时不能再报"已下载"，否则用户点安装会拿到空文件）、
    /// 下载失败之后（原因要跟着快照走）。
    #[test]
    fn snapshot_reports_progress_and_forgets_a_missing_file() {
        let state = UpdaterState::default();

        // 什么都没发生：idle，没有进度残留
        let idle = snapshot_of(&state);
        assert_eq!(idle.status, UpdateStatus::Idle);
        assert_eq!((idle.percent, idle.speed.as_str()), (0, ""));

        // 查到有更新 → 开始下载：状态是 downloading，并带上最近一次上报的进度
        state.apply(UpdateEvent::CheckFinished(UpdateCheckResponse {
            has_update: true,
            latest_version: "0.29.0".to_string(),
            download_url: "https://github.com/x.exe".to_string(),
            digest: "sha256:AB".to_string(),
            body: String::new(),
            error: None,
        }));
        state.apply(UpdateEvent::DownloadStarted {
            url: "https://github.com/x.exe".to_string(),
            digest: "sha256:AB".to_string(),
        });
        state.percent.store(42, Ordering::SeqCst);
        *state.speed.lock().unwrap() = "2.0 MB/s".to_string();
        let running = snapshot_of(&state);
        assert!(running.is_downloading());
        assert_eq!((running.percent, running.speed.as_str()), (42, "2.0 MB/s"));
        assert_eq!(running.latest_version, "0.29.0");

        // 下载完成且文件还在 → downloaded
        let file = std::env::temp_dir().join(format!(
            "tr-updater-status-{}-{}.exe",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&file, b"payload").unwrap();
        state.remember_downloaded(Some(file.clone()));
        state.apply(UpdateEvent::DownloadFinished);
        state.clear_progress();
        let done = snapshot_of(&state);
        assert!(done.is_downloaded(), "文件在、应报已下载");
        assert_eq!(done.percent, 100);

        // 文件被清理掉（`%TEMP%` 会被系统清理）→ 不能再报已下载
        std::fs::remove_file(&file).unwrap();
        let gone = snapshot_of(&state);
        assert!(!gone.is_downloaded(), "文件不在、不能报已下载");
        assert_eq!(
            gone.status,
            UpdateStatus::Available,
            "文件没了要退回「发现新版本」（版本信息还在，可以重新下载）"
        );
        assert_eq!(gone.latest_version, "0.29.0");

        // 失败原因跟着快照走（界面直接展示它，不再自己拼）
        let state = UpdaterState::default();
        state.apply(UpdateEvent::CheckFinished(UpdateCheckResponse {
            error: Some("请求 GitHub API 失败: timeout".to_string()),
            ..UpdateCheckResponse::default()
        }));
        let failed = snapshot_of(&state);
        assert_eq!(failed.status, UpdateStatus::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some("请求 GitHub API 失败: timeout")
        );
    }

    /// 去重键：同一个下载（同 URL + 同 digest）必须得到同一个键。
    #[test]
    fn request_key_identifies_the_same_download() {
        assert_eq!(
            request_key("https://github.com/a.exe", Some("sha256:AB")),
            request_key("https://github.com/a.exe", Some("sha256:AB"))
        );
        // digest 缺失（release 未提供）也要稳定
        assert_eq!(
            request_key("https://github.com/a.exe", None),
            request_key("https://github.com/a.exe", None)
        );
        // 不同版本是不同的下载
        assert_ne!(
            request_key("https://github.com/a.exe", Some("sha256:AB")),
            request_key("https://github.com/a.exe", Some("sha256:CD"))
        );
        assert_ne!(
            request_key("https://github.com/a.exe", None),
            request_key("https://github.com/b.exe", None)
        );
    }

    #[test]
    fn url_host_parses_and_strips_userinfo_and_port() {
        assert_eq!(
            url_host(
                "https://github.com/ddd-online/Transactions-Rust/releases/download/v0.1.0/a.exe"
            ),
            Some("github.com".to_string())
        );
        assert_eq!(
            url_host("https://objects.githubusercontent.com:443/x.exe"),
            Some("objects.githubusercontent.com".to_string())
        );
        assert_eq!(
            url_host("https://user:pass@evil.com/x.exe"),
            Some("evil.com".to_string())
        );
        assert_eq!(url_host("not a url"), None);
        assert_eq!(url_host("ftp://github.com/x"), None);
    }

    #[test]
    fn part_path_appends_suffix() {
        let target = PathBuf::from("C:\\tmp\\a.exe");
        assert!(part_path_of(&target)
            .to_string_lossy()
            .ends_with("a.exe.part"));
    }

    /// 复用 `%TEMP%` 缓存前的 digest 校验（真实事故的回归）：
    /// v0.2.0 的资产曾经就是 0.1.0 的安装包，而缓存文件名只带版本号 ——
    /// 修好发布后重新上传的资产与缓存里的旧字节同名不同内容，**不复核就会把旧包再装一遍**。
    #[test]
    fn cached_file_is_reused_only_when_digest_matches() {
        let path =
            std::env::temp_dir().join(format!("tr-updater-cache-{}.bin", std::process::id()));
        std::fs::write(&path, b"bogus-package").expect("写测试缓存文件");
        let actual = format!("{:x}", Sha256::digest(b"bogus-package"));

        // 一致（前缀与大小写都不敏感）→ 复用
        assert!(file_digest_matches(
            &path,
            Some(&format!("sha256:{}", actual.to_uppercase()))
        ));
        // 不一致 → 不复用（调用方会删掉它重新下载）
        let wrong = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!file_digest_matches(&path, Some(wrong)));
        // 没有期望 digest → 放行（与下载完成后的校验语义一致）
        assert!(file_digest_matches(&path, None));
        assert!(file_digest_matches(&path, Some("")));
        assert!(file_digest_matches(&path, Some("sha256:")));
        // 读不出来（文件不存在）→ 不复用
        assert!(!file_digest_matches(
            &path.with_extension("missing"),
            Some(&actual)
        ));

        let _ = std::fs::remove_file(&path);
    }

    fn release(tag: &str, prerelease: bool, assets: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "prerelease": prerelease,
            "body": "更新说明",
            "assets": assets,
        })
    }

    /// 有更新：tag 的 `v` 前缀去掉、取第一个 `.exe` 资产的 url 与 digest、body 一并带出。
    #[test]
    fn parse_release_picks_first_exe_asset() {
        let payload = release(
            "v0.29.0",
            false,
            serde_json::json!([
                // 非 .exe 的资产必须被跳过（发布里常有 zip / sig）
                { "browser_download_url": "https://github.com/x/releases/download/v0.29.0/app.zip" },
                { "browser_download_url": "https://github.com/x/releases/download/v0.29.0/notes.txt" },
                {
                    "browser_download_url": "https://github.com/x/releases/download/v0.29.0/Transactions-x64-v0.29.0.exe",
                    "digest": "sha256:ABCDEF0123456789"
                },
                { "browser_download_url": "https://github.com/x/releases/download/v0.29.0/other.exe" }
            ]),
        );
        let response = parse_release(&payload, "0.28.0").expect("应判定为有更新");
        assert!(response.has_update);
        assert_eq!(response.latest_version, "0.29.0");
        assert_eq!(
            response.download_url,
            "https://github.com/x/releases/download/v0.29.0/Transactions-x64-v0.29.0.exe"
        );
        assert_eq!(response.digest, "sha256:ABCDEF0123456789");
        assert_eq!(response.body, "更新说明");
        assert!(response.error.is_none());
    }

    /// 预发布、版本不更新、tag 为空 → 都当"无更新"（返回 None，界面不提示）。
    #[test]
    fn parse_release_reports_no_update_when_not_newer_or_prerelease() {
        let assets = serde_json::json!([]);
        assert!(parse_release(&release("v0.29.0", true, assets.clone()), "0.28.0").is_none());
        assert!(parse_release(&release("v0.28.0", false, assets.clone()), "0.28.0").is_none());
        assert!(parse_release(&release("v0.1.0", false, assets.clone()), "0.2.0").is_none());
        assert!(parse_release(&release("", false, assets.clone()), "0.28.0").is_none());
        // 没有 prerelease 字段时按"正式版"处理
        let no_flag = serde_json::json!({ "tag_name": "v0.29.0", "assets": [] });
        assert!(parse_release(&no_flag, "0.28.0").is_some());
    }

    /// 有更新但没有 `.exe` 资产：仍算有更新，只是下载地址为空。
    #[test]
    fn parse_release_keeps_update_without_exe_asset() {
        let payload = release(
            "0.29.0",
            false,
            serde_json::json!([{ "browser_download_url": "https://github.com/x/app.zip" }]),
        );
        let response = parse_release(&payload, "0.28.0").expect("仍应提示有更新");
        assert!(response.has_update);
        assert_eq!(response.download_url, "");
        assert_eq!(response.digest, "");
        // body 缺失时是空串，不能是 null
        let without_body = serde_json::json!({ "tag_name": "0.29.0", "assets": [] });
        assert_eq!(parse_release(&without_body, "0.28.0").unwrap().body, "");
    }
}
