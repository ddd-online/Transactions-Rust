//! 桌面外壳命令：窗口控制、配置读写、文件夹/保存对话框、DevTools，以及工作空间打开编排。
//!
//! 与业务命令（`tr-ipc`）的分工：凡是需要窗口、配置、日志、对话框的编排动作放在这里；
//! 纯数据操作（账本/交易/股票…）放在 `tr-ipc`。两者都由 `main.rs` 的
//! `generate_handler![]` 统一注册，命令清单因此集中在一处。
//!
//! 命名与入参形状遵循同一约定（命令名 snake_case、入参统一一个 `req` 结构体），
//! 界面侧的封装见 `crates/tr-ui/src/api/desktop.rs`：
//! * `window_control`：窗口控制（action: minimize / maximize / close）
//! * `dialog_open`：选择文件或目录
//! * `file_save_image`：把工作空间资产另存到用户选择的位置
//! * `app_info`：应用信息（field: name / version / isDev）
//! * `config_*`：配置读写（含 `config_set_proxy` / `proxy_detect`：代理设置与系统代理探测）
//! * `devtools_*`：开发者工具状态与开合

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use tr_domain::error::AppError;
use tr_domain::proxy::ProxySetting;
use tr_domain::wire::{
    AppInfoRequest, AssetUrlRequest, ConfigSnapshot, DevToolsToggleRequest, DialogOpenRequest,
    DialogOpenResponse, FeatureFlags, FileSaveRequest, FileSaveResponse, ProxyDetectResponse,
    SetAppearanceRequest, SetCloseBehaviorRequest, SetFeatureRequest, SetKeyEventLinkedOpenRequest,
    WindowControlRequest, WorkspaceDirRequest,
};
use tr_ipc::{ApiError, ApiResult, AppState};

use crate::assets;
use crate::config::{
    home_dir, ConfigStore, APPEARANCE_SYSTEM, CLOSE_BEHAVIOR_QUIT, CLOSE_BEHAVIOR_TRAY,
};
use crate::logging::LogSinks;
use crate::shell;

/// 桌面外壳自己的托管状态（窗口/配置/日志）。
pub struct DesktopState {
    pub config: ConfigStore,
    pub logs: LogSinks,
    /// 是否开发构建（debug 构建为 true，配置文件名带 -dev）
    pub is_dev: bool,
}

impl DesktopState {
    pub fn new(is_dev: bool) -> Self {
        let config = ConfigStore::load(is_dev);
        let app_dir = shell::app_directory();
        Self {
            config,
            logs: LogSinks::new(&app_dir),
            is_dev,
        }
    }
}

/// 事件名：工作空间切换成功后广播，界面据此重新挂载当前页面。
pub const EVENT_WORKSPACE_CHANGED: &str = "workspace-changed";
/// 事件名：DevTools 开合状态变化。
pub const EVENT_DEVTOOLS_STATE_CHANGED: &str = "devtools:state-changed";

/// 把底层错误收敛为 500 信封（命令面与更新器共用同一套转换）。
pub(crate) fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::from(AppError::internal(error.to_string()))
}

/// 注册一次文件 / 目录选择回调，并在异步运行时的工作线程上等待结果。
///
/// 用回调 + 阻塞等待而不是对话框插件的阻塞 API：阻塞 API 在主线程会死锁。
async fn pick_path<F>(register: F) -> ApiResult<Option<PathBuf>>
where
    F: FnOnce(Box<dyn FnOnce(Option<tauri_plugin_dialog::FilePath>) + Send>),
{
    let (tx, rx) = std::sync::mpsc::channel();
    register(Box::new(move |picked| {
        let _ = tx.send(picked);
    }));

    Ok(
        tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
            .await
            .map_err(internal)?
            .and_then(|file_path| file_path.into_path().ok()),
    )
}

// ---------------------------------------------------------------- 窗口控制

/// 自绘标题栏的三个按钮。关闭走 [`shell::request_close`]（含"关闭行为"处理）。
#[tauri::command]
pub fn window_control(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    req: WindowControlRequest,
) -> ApiResult<()> {
    match req.action.as_str() {
        "minimize" => {
            window.minimize().map_err(internal)?;
        }
        "maximize" => {
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize().map_err(internal)?;
            } else {
                window.maximize().map_err(internal)?;
            }
        }
        "close" => shell::request_close(&app, &window, &state),
        other => {
            return Err(ApiError::from(AppError::bad_request(format!(
                "未知的窗口控制命令: {other}"
            ))))
        }
    }
    Ok(())
}

// ------------------------------------------------------------ 应用信息

#[tauri::command]
pub fn app_info(
    app: AppHandle,
    state: State<'_, DesktopState>,
    req: AppInfoRequest,
) -> ApiResult<String> {
    let value = match req.field.as_str() {
        "name" => app.package_info().name.clone(),
        "version" => app.package_info().version.to_string(),
        "isDev" => state.is_dev.to_string(),
        _ => String::new(),
    };
    Ok(value)
}

// ------------------------------------------------------------ 资产 URL

/// 把数据库里的相对路径转成 `<img src>` 可用 URL。
#[tauri::command]
pub fn asset_url(req: AssetUrlRequest) -> ApiResult<String> {
    Ok(assets::asset_url(&req.file_path))
}

// ------------------------------------------------------------ 配置读写

#[tauri::command]
pub fn config_get(state: State<'_, DesktopState>) -> ApiResult<ConfigSnapshot> {
    // 界面启动的第一个调用；打一条 info 便于排障（"窗口空白/PII 没反应"时先看这里有没有出现）
    let config = state.config.snapshot();
    tracing::info!(
        "IPC config_get: workspaceDir={:?} appearance={:?} proxy={}",
        config.workspace_dir,
        config.appearance,
        config.proxy.mode
    );
    Ok(ConfigSnapshot {
        workspace_dir: config.workspace_dir,
        close_behavior: config.close_behavior,
        appearance: if config.appearance.is_empty() {
            APPEARANCE_SYSTEM.to_string()
        } else {
            config.appearance
        },
        config_path: state.config.path().to_string_lossy().to_string(),
        is_dev: state.is_dev,
        proxy: config.proxy,
        features: config.features,
        key_event_linked_open: config.key_event_linked_open,
    })
}

/// 关闭行为只接受 quit / tray / 空（空表示首次询问）。
#[tauri::command]
pub fn config_set_close_behavior(
    state: State<'_, DesktopState>,
    req: SetCloseBehaviorRequest,
) -> ApiResult<()> {
    if !matches!(
        req.behavior.as_str(),
        CLOSE_BEHAVIOR_QUIT | CLOSE_BEHAVIOR_TRAY | ""
    ) {
        return Err(ApiError::from(AppError::bad_request("无效的关闭行为")));
    }
    state
        .config
        .update(|config| config.close_behavior = req.behavior);
    Ok(())
}

/// 外观：持久化 + 应用到所有窗口（驱动 `prefers-color-scheme`）。
#[tauri::command]
pub fn config_set_appearance(
    app: AppHandle,
    state: State<'_, DesktopState>,
    req: SetAppearanceRequest,
) -> ApiResult<()> {
    let theme = match req.appearance.as_str() {
        "light" | "dark" | "system" => shell::theme_of(&req.appearance),
        _ => return Err(ApiError::from(AppError::bad_request("无效的外观设置"))),
    };

    state
        .config
        .update(|config| config.appearance = req.appearance.clone());

    for (_, window) in app.webview_windows() {
        let _ = window.set_theme(theme);
    }
    Ok(())
}

// ------------------------------------------------------------ 功能开关

/// 功能开关：某个顶级功能是否在侧边栏出现。
///
/// 只写配置，**不改界面**：侧边栏读的是界面侧的共享状态（`store::AppStores::enabled_features`），
/// 由界面在调用成功后自己更新，所以这里返回落盘后的完整开关集合，界面拿它回显。
/// 「设置」不在可关闭之列（`FeatureFlags` 里根本没有它），关掉它就没有界面能再打开它了。
#[tauri::command]
pub fn config_set_feature(
    state: State<'_, DesktopState>,
    req: SetFeatureRequest,
) -> ApiResult<FeatureFlags> {
    let snapshot = state.config.snapshot();
    // `get` 先拦一次未知名：`set` 对未知名返回 false 也能挡住，但先查一次能让
    // "这个功能名到底认不认"只有一个判据（`FeatureFlags::get`）。
    if snapshot.features.get(&req.feature).is_none() {
        return Err(ApiError::from(AppError::bad_request("无效的功能开关")));
    }
    let features = state.config.update(|config| {
        config.features.set(&req.feature, req.enabled);
        config.features.clone()
    });
    tracing::info!(
        "IPC config_set_feature: {}={} → accounting={} stock={} keyEvent={} diary={}",
        req.feature,
        req.enabled,
        features.accounting,
        features.stock,
        features.key_event,
        features.diary
    );
    Ok(features)
}

// ------------------------------------------------------------ 事件页右栏偏好

/// 事件页右栏（关联交易列表）展开还是收起：写配置并返回落盘后的值。
///
/// 与功能开关同一套路：外壳只负责落盘，界面调成功后用返回值回显 —— 于是**换页**和
/// **重启**都保持用户上一次的选择（偏好是会话无关的）。
#[tauri::command]
pub fn config_set_key_event_linked_open(
    state: State<'_, DesktopState>,
    req: SetKeyEventLinkedOpenRequest,
) -> ApiResult<bool> {
    let open = state.config.update(|config| {
        config.key_event_linked_open = req.open;
        config.key_event_linked_open
    });
    tracing::info!("IPC config_set_key_event_linked_open: open={open}");
    Ok(open)
}

// ------------------------------------------------------------ 代理设置
/// 校验 + 归一化（**纯函数**，单测直接断言用户看到的文案）。
///
/// 地址只支持 `http://`：见 `tr_domain::proxy::normalize_http_proxy`。
fn validate_proxy_setting(request: &ProxySetting) -> ApiResult<ProxySetting> {
    ProxySetting::validated(&request.mode, &request.url)
        .map_err(|message| ApiError::from(AppError::bad_request(message)))
}

/// 保存代理设置：校验 → 落盘 → **立即推给 tr-service 的进程槽位**（行情随即生效）。
///
/// 返回归一化后的设置（界面用它回显，例如把 `127.0.0.1:7890` 显示成 `http://127.0.0.1:7890`）。
#[tauri::command]
pub fn config_set_proxy(
    state: State<'_, DesktopState>,
    req: ProxySetting,
) -> ApiResult<ProxySetting> {
    let setting = validate_proxy_setting(&req)?;
    state.config.update(|config| config.proxy = setting.clone());
    tr_service::proxy::set(setting.clone());
    tracing::info!(
        "IPC config_set_proxy: mode={} url={:?}",
        setting.mode,
        setting.url
    );
    Ok(setting)
}

/// 「检测」按钮：按**当前设置**报告最终会用哪个代理（只读：不写配置、不改系统设置）。
#[tauri::command]
pub fn proxy_detect(state: State<'_, DesktopState>) -> ApiResult<ProxyDetectResponse> {
    let resolved = tr_service::proxy::resolve(&state.config.snapshot().proxy);
    Ok(ProxyDetectResponse {
        url: resolved.url.unwrap_or_default(),
        source: resolved.source.to_string(),
        pac: resolved.pac,
        message: resolved.detail,
    })
}

// ------------------------------------------------------------ 工作空间

#[tauri::command]
pub fn workspace_get(state: State<'_, DesktopState>) -> ApiResult<String> {
    Ok(state.config.snapshot().workspace_dir)
}

/// 只记录目录（等价 `workspace:set`），不打开数据库。
#[tauri::command]
pub fn workspace_set(state: State<'_, DesktopState>, req: WorkspaceDirRequest) -> ApiResult<()> {
    state
        .config
        .update(|config| config.workspace_dir = req.workspace_dir);
    Ok(())
}

/// 打开工作空间。
///
/// 一次完成三件事：打开数据库（含格式校验）、把日志切到工作空间目录、记住目录。
/// 失败时返回既定的错误文案（含"该工作空间不是最新格式…"的升级提示）。
///
/// 首次启动时这个命令是**从初始化窗口**发出的：成功后必须立刻切到主窗口
/// （界面可以在切换后再发一次 `workspace:init` 兜底）。
/// 这里由外壳自己完成切换，界面不需要额外调用——初始化窗口只加载与主窗口相同的
/// `index.html`，"还没配置工作空间"时它展示选择目录的引导，因此切换是外壳的职责。
#[tauri::command]
pub fn workspace_open(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    ipc_state: State<'_, AppState>,
    req: WorkspaceDirRequest,
) -> ApiResult<()> {
    let raw = req.workspace_dir.trim();
    if raw.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "工作目录路径不能为空",
        )));
    }

    let directory = PathBuf::from(raw);
    let opened = match ipc_state.ws.open_workspace(&directory) {
        Ok(opened) => opened,
        Err(error) => {
            tracing::warn!("打开工作空间失败: {} —— {}", raw, error);
            return Err(internal(error.to_string()));
        }
    };

    state.logs.set_workspace(Some(opened.directory()));
    state
        .config
        .update(|config| config.workspace_dir = raw.to_string());
    tracing::info!("工作空间已打开: {}", raw);

    // 旧版本按 UTC 取股票资金记录的日期（东区比委托日期早一天，见 tr_service::stock::unix_to_date），
    // 打开工作空间时做一次**可证明的**订正：只有日期确实比对应委托的本地日期早一天的记录才触发重放。
    // **失败不影响打开**（整个订正在一个事务里，失败即回滚、数据保持原样）：它只是一次数据订正，
    // 不该让用户连工作空间都打不开。
    if let Ok(workspace) = ipc_state.workspace() {
        match tr_service::stock::repair_legacy_trade_fund_dates(&workspace) {
            Ok(0) => {}
            Ok(count) => tracing::info!("已订正 {count} 个账本的股票资金记录日期"),
            Err(error) => tracing::warn!("订正股票资金记录日期失败（工作空间照常打开）: {error}"),
        }
    }

    let _ = app.emit(EVENT_WORKSPACE_CHANGED, raw.to_string());

    // 从初始化窗口发起的"选目录"：打开成功后立刻换成主窗口。
    if window.label() == shell::INIT_WINDOW {
        transition_from_init(&app, &window);
    }
    Ok(())
}

/// 初始化窗口 → 主窗口的切换：创建并显示主窗口，然后销毁初始化窗口。
///
/// 先显示主窗口，再销毁初始化窗口；用 `destroy()` 而不是 `close()`：
/// 初始化窗口不该走"关闭行为（最小化到托盘）"那套逻辑。
/// 幂等：当前窗口不是初始化窗口、或初始化窗口已销毁时是空操作。
///
/// **必须推迟到本次 IPC 回调之外**（后台线程 + `run_on_main_thread`）：
/// `workspace_open` 是同步命令，Tauri 在主线程上、且**是在 WebView2 的
/// `WebMessageReceived` 回调里**执行它；在这个回调里紧接着
/// `WebviewWindowBuilder::build()` 会建出一个**空壳主窗口** —— 实测现象：
/// 窗口可见但是纯白、没有任何 `Chrome_WidgetWin_1` 渲染子窗口、界面永远不发出
/// `config_get`，而且紧随其后的 `destroy()` 也没生效（初始化窗口卡在选择屏、
/// 按钮停在加载态）。等回调退出后再建就没有这个问题。
fn transition_from_init(app: &AppHandle, window: &WebviewWindow) {
    if window.label() != shell::INIT_WINDOW {
        return;
    }
    tracing::info!("初始化窗口已选定工作空间，切换主窗口并销毁初始化窗口");
    let app_handle = app.clone();
    let init_window = window.clone();
    std::thread::spawn(move || {
        // 给当前这次 WebView2 回调留出退出的时间（回调没返回前不碰窗口创建）
        std::thread::sleep(std::time::Duration::from_millis(50));
        let inner = app_handle.clone();
        let task = move || {
            shell::show_main_window(&inner);
            if let Err(error) = init_window.destroy() {
                tracing::warn!("销毁初始化窗口失败: {error}");
            }
        };
        if let Err(error) = app_handle.run_on_main_thread(task) {
            tracing::warn!("切换主窗口的任务投递失败: {error}");
        }
    });
}

/// 初始化窗口选定工作目录后的切换（保留 `workspace:init` 命令面）。
///
/// 本实现已由 [`workspace_open`] 自动完成切换，因此这个命令是**幂等的补充入口**：
/// 主窗口已显示时调用它不会有副作用。
#[tauri::command]
pub fn workspace_init(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, DesktopState>,
    req: WorkspaceDirRequest,
) -> ApiResult<()> {
    let raw = req.workspace_dir.trim().to_string();
    if raw.is_empty() {
        return Err(ApiError::from(AppError::bad_request(
            "工作目录路径不能为空",
        )));
    }
    state
        .config
        .update(|config| config.workspace_dir = raw.clone());

    transition_from_init(&app, &window);
    Ok(())
}

// ------------------------------------------------------------ 对话框

/// 文件夹对话框的兜底起始目录：用户主目录（正常一定存在）；
/// 万一它也不存在（一次性 HOME 的冒烟/测试环境），退回程序所在目录。
fn dialog_fallback_directory() -> PathBuf {
    let home = home_dir();
    if home.is_dir() {
        home
    } else {
        shell::app_directory()
    }
}

/// 选择目录（工作空间、日记导入/导出目录）。
///
/// 用回调 + 阻塞等待而不是对话框插件的阻塞 API：阻塞 API 在主线程会死锁，
/// 这里把等待放到异步运行时的工作线程上。
#[tauri::command]
pub async fn dialog_open(app: AppHandle, req: DialogOpenRequest) -> ApiResult<DialogOpenResponse> {
    let mut builder = app.dialog().file();
    if let Some(title) = req.title.as_deref() {
        builder = builder.set_title(title);
    }
    // **只把真实存在的目录交给对话框**：路径为空（第一次选工作空间）或已不存在
    // （目录被删/改名/换机器，配置里还留着旧路径）时，Windows 的文件夹对话框会先弹一个
    // 「位置不可用：… 不可用」的模态框，把整个选择流程挡在后面 —— 用户只能先点「确定」
    // 再自己导航。兜底用主目录（一定存在），比让对话框自己落回"桌面"更可预期。
    let start_directory = req
        .default_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(dialog_fallback_directory);
    builder = builder.set_directory(start_directory);
    let picked = pick_path(move |callback| {
        builder.pick_folder(callback);
    })
    .await?;

    Ok(match picked {
        Some(path) => DialogOpenResponse {
            canceled: false,
            file_paths: vec![path.to_string_lossy().to_string()],
            error: None,
        },
        None => DialogOpenResponse {
            canceled: true,
            file_paths: Vec::new(),
            error: None,
        },
    })
}

/// 失败信封（`canceled` 缺省，序列化时不出现在 JSON 里）。
fn save_failed(message: impl Into<String>) -> FileSaveResponse {
    FileSaveResponse {
        success: false,
        canceled: None,
        error: Some(message.into()),
    }
}

/// 把工作空间资产里的图片另存到用户选择的位置。
#[tauri::command]
pub async fn file_save_image(
    app: AppHandle,
    ipc_state: State<'_, AppState>,
    req: FileSaveRequest,
) -> ApiResult<FileSaveResponse> {
    let Ok(workspace) = ipc_state.workspace() else {
        return Ok(save_failed("未打开工作空间"));
    };

    let assets_root = workspace.assets_directory();
    let Ok(root) = assets_root.canonicalize() else {
        return Ok(save_failed("源文件不存在"));
    };
    // 防路径遍历：解析后必须仍位于 assets 目录内
    let Ok(source) = assets_root.join(&req.relative_path).canonicalize() else {
        return Ok(save_failed("源文件不存在"));
    };
    if !source.starts_with(&root) {
        return Ok(save_failed("非法文件路径"));
    }

    let default_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image")
        .to_string();

    let picked = pick_path(move |callback| {
        app.dialog()
            .file()
            .set_file_name(default_name)
            .add_filter(
                "图片文件",
                &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic"],
            )
            .save_file(callback);
    })
    .await?;

    let Some(target) = picked else {
        return Ok(FileSaveResponse {
            success: false,
            canceled: Some(true),
            error: None,
        });
    };

    match std::fs::copy(&source, &target) {
        Ok(_) => Ok(FileSaveResponse {
            success: true,
            canceled: None,
            error: None,
        }),
        Err(error) => Ok(FileSaveResponse {
            success: false,
            canceled: None,
            error: Some(error.to_string()),
        }),
    }
}

// ------------------------------------------------------------ DevTools

#[tauri::command]
pub fn devtools_get_state(window: WebviewWindow) -> ApiResult<bool> {
    Ok(window.is_devtools_open())
}

/// 返回操作后的真实状态，并广播给界面校正开关。
#[tauri::command]
pub fn devtools_toggle(
    app: AppHandle,
    window: WebviewWindow,
    req: DevToolsToggleRequest,
) -> ApiResult<bool> {
    if req.enabled {
        window.open_devtools();
    } else {
        window.close_devtools();
    }
    let opened = window.is_devtools_open();
    let _ = app.emit_to(window.label(), EVENT_DEVTOOLS_STATE_CHANGED, opened);
    Ok(opened)
}

// ------------------------------------------------------------ 配置文件路径

/// 设置页「配置文件」一栏展示用：返回当前平台的配置文件真实路径。
/// 更新相关命令（`update_check` / `update_download` / `update_cancel` / `update_install`）
/// 是自研实现，见 `src/updater.rs`；**不使用** `tauri-plugin-updater`（理由见 AGENTS.md）。
#[tauri::command]
pub fn config_file_path(state: State<'_, DesktopState>) -> ApiResult<String> {
    Ok(state.config.path().to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 锁住外壳命令的**入参字段名**：界面是按字面量手写 JSON 的，改名不会编译报错。
    /// 这两条对应真实踩过的坑（见 `AssetUrlRequest` / `DialogOpenRequest` 的注释）：
    /// `asset_url` 曾因只认 `file_path` 而必然失败；`dialog_open` 曾静默忽略 `defaultPath`。
    #[test]
    fn asset_url_request_accepts_the_camel_case_body_sent_by_the_ui() {
        let request: AssetUrlRequest =
            serde_json::from_str(r#"{"filePath":"key_events/2026-01-01/a.jpg"}"#).unwrap();
        assert_eq!(request.file_path, "key_events/2026-01-01/a.jpg");

        // 兼容旧写法，避免已有调用方回归
        let legacy: AssetUrlRequest =
            serde_json::from_str(r#"{"file_path":"key_events/2026-01-01/a.jpg"}"#).unwrap();
        assert_eq!(legacy.file_path, "key_events/2026-01-01/a.jpg");

        // 字段缺失必须是**硬错误**，不能再退化成"空路径"这种静默行为
        assert!(serde_json::from_str::<AssetUrlRequest>("{}").is_err());
    }

    #[test]
    fn dialog_open_request_reads_camel_case_and_legacy_snake_case() {
        let request: DialogOpenRequest =
            serde_json::from_str(r#"{"title":"选择工作目录","defaultPath":"E:\\ljwfile"}"#)
                .unwrap();
        assert_eq!(request.title.as_deref(), Some("选择工作目录"));
        assert_eq!(
            request.default_path.as_deref(),
            Some(r"E:\ljwfile"),
            "defaultPath 必须被读到，否则对话框不会定位到当前工作空间"
        );

        let legacy: DialogOpenRequest =
            serde_json::from_str(r#"{"default_path":"E:\\ljwfile"}"#).unwrap();
        assert_eq!(legacy.default_path.as_deref(), Some(r"E:\ljwfile"));

        // 两个字段都可省略（界面可能只传 title）
        let minimal: DialogOpenRequest = serde_json::from_str(r#"{"title":"x"}"#).unwrap();
        assert!(minimal.default_path.is_none());
    }

    #[test]
    fn window_control_actions_match_the_ui_contract() {
        // 界面只会发这三个动作；未知动作必须是错误而不是静默成功
        for action in ["minimize", "maximize", "close"] {
            let request: WindowControlRequest =
                serde_json::from_str(&format!(r#"{{"action":"{action}"}}"#)).unwrap();
            assert_eq!(request.action, action);
        }
    }

    /// `config_get` 的返回键名是数据契约（配置由别的版本写、界面按这些键读）：
    /// 改名的后果是界面静默读不到（`#[serde(default)]` 兜底成默认值），必须在这里锁住。
    #[test]
    fn config_snapshot_serializes_the_documented_keys() {
        let snapshot = ConfigSnapshot {
            workspace_dir: r"D:\ws".to_string(),
            close_behavior: "quit".to_string(),
            appearance: "system".to_string(),
            config_path: r"C:\Users\x\.transactions.json".to_string(),
            is_dev: false,
            proxy: ProxySetting::manual("http://127.0.0.1:7890"),
            features: FeatureFlags::defaults(),
            key_event_linked_open: false,
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "appearance",
                "closeBehavior",
                "configPath",
                "features",
                "isDev",
                "keyEventLinkedOpen",
                "proxy",
                "workspaceDir"
            ]
        );
        assert_eq!(value["proxy"]["mode"], "manual");
        assert_eq!(value["proxy"]["url"], "http://127.0.0.1:7890");
        // 功能开关的键名也是契约（界面共用 `tr_domain::wire::FeatureFlags`）
        assert_eq!(value["features"]["accounting"], true);
        assert_eq!(value["features"]["stock"], true);
        assert_eq!(value["features"]["keyEvent"], true);
        assert_eq!(value["features"]["diary"], true);
        // 事件页右栏偏好同样是契约（界面共用 `tr_domain::wire::ConfigSnapshot`）
        assert_eq!(value["keyEventLinkedOpen"], false);
    }

    #[test]
    fn proxy_setting_validation_reports_chinese_messages() {
        // 手动模式：归一化 + 回显
        let ok = validate_proxy_setting(&ProxySetting {
            mode: "manual".to_string(),
            url: " 127.0.0.1:7890 ".to_string(),
        })
        .unwrap();
        assert_eq!(ok.url, "http://127.0.0.1:7890");

        // 只看 http：socks / https 明确拒绝
        let err = validate_proxy_setting(&ProxySetting {
            mode: "manual".to_string(),
            url: "socks5://127.0.0.1:1080".to_string(),
        })
        .unwrap_err();
        assert_eq!(err.status, 400);
        assert!(err.msg.contains("仅支持"), "文案: {}", err.msg);

        // 未知模式
        let err = validate_proxy_setting(&ProxySetting {
            mode: "on".to_string(),
            url: String::new(),
        })
        .unwrap_err();
        assert!(err.msg.contains("无效的代理模式"), "文案: {}", err.msg);

        // off / auto 下地址原样保留（切回手动时还在）
        let auto = validate_proxy_setting(&ProxySetting {
            mode: "auto".to_string(),
            url: "127.0.0.1:7890".to_string(),
        })
        .unwrap();
        assert_eq!(auto.mode, "auto");
        assert_eq!(auto.url, "127.0.0.1:7890");
    }
}
