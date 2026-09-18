//! 窗口与系统托盘：无边框主窗口、初始化窗口、托盘菜单、关闭行为。
//!
//! 对照原 `electron/src/main.js` 的窗口与托盘部分：
//! * 主窗口 1400×1000（默认），无边框，尺寸/位置写回 `~/.transactions.json`
//! * 首次启动（配置里没有工作空间目录）先显示初始化窗口（600×560，不可缩放，无边框）
//! * 系统托盘：显示主窗口 / 关闭程序；最小化到托盘后任务栏不保留图标
//! * 关闭行为：`quit` 直接退出、`tray` 隐藏到托盘、未设置时每次询问
//!   （原实现用带「下次不再提醒」勾选框的对话框，Tauri 的消息框没有勾选框，
//!   因此这里改为"每次询问"，用户可在设置页把行为固定下来）

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

/// 程序所在目录：日志与图标都相对于它（等价 Electron 的 `appPath`）。
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
            .min_inner_size(960.0, 640.0)
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

/// 记录主窗口当前尺寸与位置（等价原 `handleWindowClose` 里的 bounds 合并）。
pub fn save_window_bounds(app: &AppHandle, window: &WebviewWindow) {
    let state = app.state::<DesktopState>();
    let Ok(size) = window.inner_size() else {
        return;
    };
    let position = window.outer_position().ok();
    state.config.update(|config| {
        config.width = size.width;
        config.height = size.height;
        if let Some(position) = position {
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
            // 与原实现一致：关闭按钮不直接销毁窗口，先走关闭行为
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
