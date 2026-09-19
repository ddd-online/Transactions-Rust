//! 窗口与系统托盘：无边框主窗口、初始化窗口、托盘菜单、关闭行为。
//!
//! * 主窗口 1400×1000（默认），无边框，尺寸/位置写回 `~/.transactions.json`
//! * 首次启动（配置里没有工作空间目录）先显示初始化窗口（600×560，不可缩放，无边框）
//! * 系统托盘：显示主窗口 / 关闭程序；最小化到托盘后任务栏不保留图标
//! * 关闭行为：`quit` 直接退出、`tray` 隐藏到托盘、未设置时每次询问
//!   （Tauri 的消息框没有「下次不再提醒」勾选框，因此这里固定为"每次询问"，
//!   用户可在设置页把行为固定下来）

use std::path::PathBuf;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::commands::DesktopState;
use crate::config::{CLOSE_BEHAVIOR_QUIT, CLOSE_BEHAVIOR_TRAY};

/// 主窗口 label。
pub const MAIN_WINDOW: &str = "main";
/// 初始化（工作空间选择）窗口 label。
pub const INIT_WINDOW: &str = "init";
/// 托盘 id。
const TRAY_ID: &str = "transactions-tray";

/// 程序所在目录：日志与图标都相对于它。
pub fn app_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 首次启动未配置工作空间 → 初始化窗口；否则直接主窗口。
pub fn create_startup_window(app: &AppHandle) -> tauri::Result<()> {
    let workspace_dir = app.state::<DesktopState>().config.snapshot().workspace_dir;
    if workspace_dir.trim().is_empty() {
        create_init_window(app)?;
    } else {
        create_main_window(app)?;
    }
    Ok(())
}

/// 主窗口。
///
/// `disable_drag_drop_handler()` **不能删**（曾经的真实缺陷）：
/// Tauri 默认 `dragDropEnabled: true`，wry 会 `RegisterDragDrop` 到 WebView2 的宿主 HWND
/// 并 `SetAllowExternalDrop(false)`；而 Chromium 在 Windows 上**内部拖拽也是走 OLE 拖放**的，
/// 于是页面的 HTML5 拖拽（见 `components/ui/drag_sort.rs`）
/// 落点永远收不到 `drop` —— 表现就是"分类/标签/模板拖不动"。
/// 关掉它之后由 WebView2 自己处理拖放，页内拖拽恢复正常；
/// 代价是拿不到 `tauri://drag-drop` 原生文件落盘事件，而本项目本来就没有这个功能。
/// 改动这里请用 `fixtures/ui-drag.ps1` 回归。
pub fn create_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(MAIN_WINDOW) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(existing);
    }

    let config = app.state::<DesktopState>().config.snapshot();
    let mut builder =
        WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Transactions")
            .inner_size(config.width as f64, config.height as f64)
            // 最小窗口 = 1500×1000 逻辑像素（内容区尺寸，不含窗口边框）。
            // 这个下限是界面骨架撑开所需的：侧栏 200 + 内容卡片 + 顶部容器，
            // 再窄就会把工具栏挤换行、把多栏页面压成一栏。
            // 注意：**单位是逻辑像素**，与 `inner_size` 同一口径（见下方 `logical_bounds` 的注释）。
            .min_inner_size(1500.0, 1000.0)
            .disable_drag_drop_handler()
            .on_navigation(is_allowed_navigation)
            .decorations(false);

    if let (Some(x), Some(y)) = (config.x, config.y) {
        builder = builder.position(x as f64, y as f64);
    }

    let window = builder.build()?;
    apply_appearance(&window, &config.appearance);
    attach_main_window_events(app, &window);
    Ok(window)
}

/// 初始化窗口（工作空间选择）：600×560、不可缩放、无边框。
pub fn create_init_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(existing) = app.get_webview_window(INIT_WINDOW) {
        let _ = existing.set_focus();
        return Ok(existing);
    }

    let window = WebviewWindowBuilder::new(app, INIT_WINDOW, WebviewUrl::App("index.html".into()))
        .title("欢迎使用 Transactions")
        .inner_size(600.0, 560.0)
        .resizable(false)
        .disable_drag_drop_handler()
        .on_navigation(is_allowed_navigation)
        .decorations(false)
        .center()
        .build()?;

    let appearance = app.state::<DesktopState>().config.snapshot().appearance;
    apply_appearance(&window, &appearance);
    Ok(window)
}

/// 按外观设置应用窗口主题（`system` 表示跟随系统）。
fn apply_appearance(window: &WebviewWindow, appearance: &str) {
    let theme = match appearance {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    let _ = window.set_theme(theme);
}

/// 托盘：显示主窗口 / 关闭程序；左键单击显示主窗口。
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "关闭程序", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Transactions")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// 显示并聚焦主窗口（必要时创建）。
pub fn show_main_window(app: &AppHandle) {
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        None => {
            let _ = create_main_window(app);
        }
    }
}

/// 是否允许 WebView 导航到该 URL。
///
/// 只放行自己的界面与开发服务器域名：
/// 关掉 Tauri 的拖放处理器之后（见 `create_main_window` 的注释），把文件从资源管理器**拖进窗口**
/// 会走 WebView2 的默认行为 —— 直接**导航到 `file:///…`**，界面就被整个换掉了。
/// 这里把非本应用的导航一律拦掉。
fn is_allowed_navigation(url: &tauri::Url) -> bool {
    match url.scheme() {
        // 生产构建：`http://tauri.localhost/index.html`（Windows 的 custom-protocol）
        // 开发构建：`http://localhost:1520/`（trunk serve）
        // 个别平台/版本会用 `tauri://localhost`。
        "http" | "https" | "tauri" => matches!(
            url.host_str(),
            Some("tauri.localhost") | Some("localhost") | Some("127.0.0.1") | Some("[::1]")
        ),
        _ => false,
    }
}

/// 物理像素 → 逻辑像素（配置里存的单位）。
///
/// 单独抽出来是为了能单测：高 DPI 下漏掉这一步，窗口每次启动都会放大 `scale` 倍并偏移
/// （见 `save_window_bounds` 的注释）。`scale` 非法时按 1.0 处理，避免除零。
fn logical_bounds(
    size: tauri::PhysicalSize<u32>,
    position: Option<tauri::PhysicalPosition<i32>>,
    scale: f64,
) -> (tauri::LogicalSize<u32>, Option<tauri::LogicalPosition<i32>>) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (
        size.to_logical(scale),
        position.map(|value| value.to_logical(scale)),
    )
}

/// 记录主窗口当前尺寸与位置（与配置里已存的其它字段合并写回）。
///
/// **单位必须是逻辑像素（DIP）**，这是踩过的坑：
/// - Windows 上 Tauri 的 `inner_size()` / `outer_position()` 返回的是**物理像素**，
///   而 `WebviewWindowBuilder::inner_size()` / `position()` 收的是**逻辑像素**。
/// - 直接把物理值存下来、下次当逻辑值用，在 150% 缩放的本机上窗口每次启动都会放大 1.5 倍、
///   并且位置越跑越偏（用户实际反馈："每次打开软件都没有保留上次的窗口大小和位置"）。
/// - 因此这里必须做物理 → 逻辑换算（纯函数 `logical_bounds` + 4 个单测），
///   回归：`fixtures/window-bounds.ps1`（启动 → 关闭 → 再启动，尺寸/位置必须一致）。
pub fn save_window_bounds(app: &AppHandle, window: &WebviewWindow) {
    let state = app.state::<DesktopState>();
    let Ok(size) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let (logical_size, logical_position) =
        logical_bounds(size, window.outer_position().ok(), scale);
    state.config.update(|config| {
        config.width = logical_size.width;
        config.height = logical_size.height;
        if let Some(position) = logical_position {
            config.x = Some(position.x);
            config.y = Some(position.y);
        }
    });
}

fn hide_to_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        save_window_bounds(app, &window);
        let _ = window.hide();
    }
}

/// 退出应用：保存窗口尺寸后退出（连接池随进程结束释放，SQLite 处于 WAL 安全状态）。
pub fn quit_app(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        save_window_bounds(app, &window);
    }
    app.exit(0);
}

/// 关闭按钮的统一入口：按关闭行为隐藏或退出；未设置则询问一次。
pub fn request_close(app: &AppHandle, window: &WebviewWindow, state: &DesktopState) {
    let behavior = state.config.snapshot().close_behavior;
    match behavior.as_str() {
        CLOSE_BEHAVIOR_TRAY => hide_to_tray(app),
        CLOSE_BEHAVIOR_QUIT => quit_app(app),
        _ => {
            save_window_bounds(app, window);
            ask_close_behavior(app.clone());
        }
    }
}

/// 首次关闭时询问（无勾选框：每次询问，直到用户在设置页固定行为）。
fn ask_close_behavior(app: AppHandle) {
    let dialog_app = app.clone();
    app.dialog()
        .message(
            "请选择关闭行为：\n「是」= 直接关闭应用；「否」= 缩小到系统托盘。\n\
             可在「设置 → 通用设置 → 关闭行为」中固定该选择。",
        )
        .title("关闭选项")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::YesNo)
        .show(move |confirmed| {
            // message 对话框的 `show` 回调给出的是「是否点了第一个按钮」的布尔值
            if confirmed {
                quit_app(&dialog_app);
            } else {
                hide_to_tray(&dialog_app);
            }
        });
}

/// 给主窗口挂上事件处理：拦截关闭请求（走关闭行为），同步最大化状态给界面。
pub fn attach_main_window_events(app: &AppHandle, window: &WebviewWindow) {
    let app_handle = app.clone();
    let main_window = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // 关闭按钮不直接销毁窗口，先走关闭行为
            api.prevent_close();
            let state = app_handle.state::<DesktopState>();
            request_close(&app_handle, &main_window, &state);
        }
        tauri::WindowEvent::Resized(_) => {
            let maximized = main_window.is_maximized().unwrap_or(false);
            let _ = main_window.emit("window-state-changed", maximized);
        }
        _ => {}
    });
}

/// 运行期事件：退出前保存窗口尺寸（界面状态无需额外处理，均由文件与数据库承载）。
pub fn handle_run_event(app: &AppHandle, event: &RunEvent) {
    if let RunEvent::ExitRequested { .. } = event {
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            save_window_bounds(app, &window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn physical(width: u32, height: u32) -> tauri::PhysicalSize<u32> {
        tauri::PhysicalSize::new(width, height)
    }

    fn physical_position(x: i32, y: i32) -> tauri::PhysicalPosition<i32> {
        tauri::PhysicalPosition::new(x, y)
    }

    /// 150% 缩放：1920×1290 物理像素 = 1280×860 逻辑像素（配置里存的就是这个）。
    #[test]
    fn logical_bounds_divides_by_scale_factor() {
        let (size, position) =
            logical_bounds(physical(1920, 1290), Some(physical_position(300, 225)), 1.5);
        assert_eq!((size.width, size.height), (1280, 860));
        let position = position.unwrap();
        assert_eq!((position.x, position.y), (200, 150));
    }

    /// 100% 缩放：原样返回（不能"顺手取整"出偏差）。
    #[test]
    fn logical_bounds_is_identity_at_scale_one() {
        let (size, position) =
            logical_bounds(physical(1400, 1000), Some(physical_position(-120, 40)), 1.0);
        assert_eq!((size.width, size.height), (1400, 1000));
        let position = position.unwrap();
        assert_eq!((position.x, position.y), (-120, 40));
    }

    /// 没有位置（窗口尚未定位成功）时只写尺寸，不能把 `None` 变成 `(0,0)`——
    /// 那会让下次启动把窗口放到左上角。
    #[test]
    fn logical_bounds_keeps_missing_position_missing() {
        let (size, position) = logical_bounds(physical(1600, 1200), None, 2.0);
        assert_eq!((size.width, size.height), (800, 600));
        assert!(position.is_none());
    }

    /// 非法缩放因子按 1.0 处理（除零会得到 inf/NaN 并写出垃圾配置）。
    #[test]
    fn logical_bounds_falls_back_on_invalid_scale() {
        for scale in [0.0, -1.5, f64::NAN, f64::INFINITY] {
            let (size, _) = logical_bounds(physical(1000, 800), None, scale);
            assert_eq!((size.width, size.height), (1000, 800), "scale={scale}");
        }
    }

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).unwrap()
    }

    /// 自己的界面（生产 / 开发 / tauri 协议）都必须放行，否则窗口会白屏。
    #[test]
    fn navigation_allows_own_pages() {
        for allowed in [
            "http://tauri.localhost/index.html",
            "http://tauri.localhost/",
            "http://localhost:1520/index.html",
            "http://127.0.0.1:1520/",
            "tauri://localhost/index.html",
        ] {
            assert!(is_allowed_navigation(&url(allowed)), "应放行: {allowed}");
        }
    }

    /// 拖进来的本地文件（以及任何外部站点）必须拦住：WebView2 的默认行为是**导航**过去，
    /// 那会把整个界面换成一个 file:// 页面。
    #[test]
    fn navigation_blocks_external_and_file_urls() {
        for blocked in [
            "file:///C:/Users/ljw/Desktop/photo.jpg",
            "file:///D:/github/Transactions-Rust/target/x.png",
            "https://example.com/",
            "http://evil.example.com/index.html",
            "javascript:alert(1)",
            "data:text/html,<h1>hi</h1>",
            "trasset://localhost/key_events/2026-01-01/a.png",
        ] {
            assert!(!is_allowed_navigation(&url(blocked)), "应拦截: {blocked}");
        }
    }
}
