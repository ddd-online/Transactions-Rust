//! 应用更新：GitHub Releases 检查 → 流式下载 → SHA256 校验 → 拉起安装包并退出。
//!
//! 本实现的行为清单：
//! * 只认 GitHub 的 latest release、跳过 prerelease、取第一个 `.exe` 资产
//! * 检查更新超时 15s（下载另给 1800s），下载地址只允许 GitHub 域名
//! * 下载到 `%TEMP%`，已存在则**先核对 digest 再复用**；流式写入 `<file>.part` 再改名
//! * 用 GitHub 提供的 `asset.digest`（`sha256:...`）校验完整性，缺失则跳过校验
//! * 取消时清理临时文件与已下载文件
//! * 打开安装包后退出应用
//!
//! 事件名：`update:download-progress|complete|error`。
//!
//! **为什么不用 `tauri-plugin-updater`**：本项目的发布管线只上传普通 `.exe` 资产（没有签名与 `latest.json`），
//! 自研路径沿用同一管线、用 `asset.digest` 做完整性校验，无需引入签名密钥管理；
//! 界面契约不变，后续若要切换到插件只需替换本文件与发布脚本。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};

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

const EVENT_PROGRESS: &str = "update:download-progress";
const EVENT_COMPLETE: &str = "update:download-complete";
const EVENT_ERROR: &str = "update:download-error";

/// 更新流程的可变状态（由 Tauri 托管）。
#[derive(Default)]
pub struct UpdaterState {
    cancel: Arc<AtomicBool>,
    downloaded_path: Mutex<Option<PathBuf>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateCheckResponse {
    #[serde(rename = "hasUpdate")]
    pub has_update: bool,
    #[serde(rename = "latestVersion")]
    pub latest_version: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    /// 形如 `sha256:...`（GitHub release asset 的 digest）
    #[serde(rename = "digest")]
    pub digest: String,
    #[serde(rename = "body")]
    pub body: String,
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdaterResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl UpdaterResponse {
    fn ok() -> Self {
        Self {
            success: true,
            error: None,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateDownloadRequest {
    pub url: String,
    #[serde(default)]
    pub digest: Option<String>,
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

/// 检查更新。失败不抛错，而是把原因放进 `error` 字段（界面按"无更新 + 错误提示"处理）。
#[tauri::command]
pub async fn update_check(app: AppHandle) -> ApiResult<UpdateCheckResponse> {
    let current_version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || check_update(&current_version))
        .await
        .map_err(internal)
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

    if latest_version.is_empty() || !is_newer_version(&latest_version, current_version) {
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

/// 版本比较（按点分段数值比较）。
fn is_newer_version(latest: &str, current: &str) -> bool {
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

/// 下载并校验安装包（阻塞；由命令层放到工作线程执行）。
#[tauri::command]
pub async fn update_download(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    req: UpdateDownloadRequest,
) -> ApiResult<UpdaterResponse> {
    // URL 白名单：仅允许 GitHub 域名（防止界面被注入后下载任意地址）
    let Some(host) = url_host(&req.url) else {
        return Ok(UpdaterResponse::failed("无效的下载地址"));
    };
    if !host.ends_with("github.com") && !host.ends_with("objects.githubusercontent.com") {
        return Ok(UpdaterResponse::failed("下载地址不在白名单内"));
    }

    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);

    let app_handle = app.clone();
    let url = req.url.clone();
    let digest = req.digest.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_and_verify(&app_handle, &url, digest.as_deref(), &cancel)
    })
    .await
    .map_err(internal)?;

    match result {
        Ok(path) => {
            *state.downloaded_path.lock().expect("更新状态锁中毒") = Some(path.clone());
            let _ = app.emit(
                EVENT_COMPLETE,
                serde_json::json!({ "filePath": path.to_string_lossy() }),
            );
            Ok(UpdaterResponse::ok())
        }
        Err(message) if message == CANCELLED => Ok(UpdaterResponse::failed("cancelled")),
        Err(message) => {
            let _ = app.emit(EVENT_ERROR, serde_json::json!({ "message": message }));
            Ok(UpdaterResponse::failed(message))
        }
    }
}

/// 用户主动取消：置位取消标记并清理临时文件。
#[tauri::command]
pub fn update_cancel(state: State<'_, UpdaterState>) -> ApiResult<()> {
    state.cancel.store(true, Ordering::SeqCst);
    if let Some(path) = state.downloaded_path.lock().expect("更新状态锁中毒").take() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(part_path_of(&path));
    }
    Ok(())
}

/// 打开已下载的安装包并退出应用（给安装器留出启动时间后再退出）。
#[tauri::command]
pub fn update_install(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> ApiResult<UpdaterResponse> {
    let path = state
        .downloaded_path
        .lock()
        .expect("更新状态锁中毒")
        .clone();

    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(UpdaterResponse::failed("安装文件不存在"));
    };

    use tauri_plugin_opener::OpenerExt;
    if let Err(error) = app
        .opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
    {
        tracing::error!("update:install error: {error}");
        return Ok(UpdaterResponse::failed(error.to_string()));
    }

    // 给安装器一点启动时间后退出：安装器需要独占替换程序文件
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        handle.exit(0);
    });
    Ok(UpdaterResponse::ok())
}

/// 取消标记对应的错误文本（与界面约定一致）。
const CANCELLED: &str = "cancelled";

/// 流式下载 + SHA256 校验；返回最终文件路径。
fn download_and_verify(
    app: &AppHandle,
    url: &str,
    expected_digest: Option<&str>,
    cancel: &AtomicBool,
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
            format_speed(downloaded as f64 / elapsed)
        } else {
            "0 B/s".to_string()
        };
        let _ = app.emit(
            EVENT_PROGRESS,
            serde_json::json!({
                "percent": percent,
                "downloaded": downloaded,
                "total": total,
                "speed": speed,
            }),
        );
    }

    file.flush()
        .map_err(|error| format!("刷新文件失败: {error}"))?;
    drop(file);

    if !digest_matches(expected_digest, &format!("{:x}", hasher.finalize())) {
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
    if normalize_digest(expected_digest).is_none() {
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
    digest_matches(expected_digest, &format!("{:x}", hasher.finalize()))
}

/// 规范化 GitHub 的 `digest` 字段：`sha256:ABCD…` → 小写十六进制串。
/// 缺失、空串或只有前缀时返回 `None`，表示**跳过校验**。
fn normalize_digest(digest: Option<&str>) -> Option<String> {
    digest
        .map(|digest| digest.trim_start_matches("sha256:").to_ascii_lowercase())
        .filter(|digest| !digest.is_empty())
}

/// 下载完成后是否需要放行：没有 digest 就放行，有就必须逐字符相等（忽略大小写与前缀）。
fn digest_matches(expected: Option<&str>, actual_hex: &str) -> bool {
    match normalize_digest(expected) {
        Some(expected) => expected == actual_hex.to_ascii_lowercase(),
        None => true,
    }
}

/// 速度格式化（按 1024 进制给出 B/s、KB/s、MB/s）。
fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_second / 1_048_576.0)
    } else if bytes_per_second >= 1024.0 {
        format!("{:.1} KB/s", bytes_per_second / 1024.0)
    } else {
        format!("{} B/s", bytes_per_second.round())
    }
}

/// 取 URL 的主机名（去掉 userinfo 与端口），无法解析时返回 `None`。
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
    fn speed_formatting_is_stable() {
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(3.0 * 1_048_576.0), "3.0 MB/s");
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
}
