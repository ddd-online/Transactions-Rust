//! Transactions 桌面端入口（Tauri 2 + Rust）。
//!
//! 结构性约定：
//! * **没有子进程内核**：业务代码以库的形式跑在本进程内，通过 Tauri IPC 暴露给界面
//! * **没有本地 HTTP 服务**：不监听端口、没有 API 令牌、没有 CORS
//! * **界面是 Rust**：Leptos 编译为 WASM，由 Tauri 窗口加载
//! * 因此不存在"内核健康检查/重启"这套机制（进程崩溃即应用崩溃），
//!   SQLite 依旧处于 WAL 保护下，不会因退出丢失已提交数据

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// 测试目标没有入口（见 `main` 上方），于是"只有 `generate_handler!` 认识"的那些命令
// 与 `main` 专属的辅助函数都会显得没人用。那是**测试目标的结构**，不是死代码：
// 这里只压住这条噪声，别把 `allow(dead_code)` 挪到生产构建上。
#![cfg_attr(test, allow(dead_code))]

mod assets;
mod commands;
mod config;
mod logging;
mod registry;
mod shell;
mod updater;

// 只服务入口里的窗口控制（`main` 被 `cfg(test)` 排除时它也用不上）
#[cfg(not(test))]
use tauri::Manager;

/// 单元测试目标（`cargo test -p transactions`）只跑**纯逻辑**。
///
/// 这里 `cfg(not(test))` 的是**入口**而不是别的什么：`tauri::generate_context!()`
/// 是"把界面产物（`crates/tr-ui/dist`，由 trunk 生成、不入库）与 config 一起变成一个
/// 上下文"的那一步，它属于启动路径。测试目标不需要窗口、托盘、资源协议，
/// 也就不该被"界面产物在不在"这种与测试无关的条件卡住。
///
/// 于是本 crate 的测试面正是那些能在 native 上真跑的纯逻辑：`commands` 的请求形状、
/// `config` 的键名、`logging`、`assets` 的路径穿越校验、`shell` 的窗口几何与导航守卫，
/// 以及 `updater` 剩下的解析与去重键。**更新流程的状态机在 `tr_domain::update`**
/// （两侧共用、native 上真跑），本文件这一票的意义就是把外壳的测试纳入验收。
#[cfg(not(test))]
fn main() {
    // debug 构建视为开发模式（配置文件名带 -dev）
    let is_dev = cfg!(debug_assertions);

    let desktop_state = commands::DesktopState::new(is_dev);
    // 代理设置推给 tr-service 的进程槽位（行情）——必须在任何 IPC 之前：
    // 更新器自己读同一处（见 updater::agent），于是"更新走代理、行情不走"这类半生效不会发生。
    tr_service::proxy::set(desktop_state.config.snapshot().proxy);
    logging::init_tracing(desktop_state.logs.clone());
    tracing::info!(
        "--------- 启动 Transactions (Rust, dev={is_dev}) --------- 配置: {} 应用日志: {}",
        desktop_state.config.path().display(),
        desktop_state.logs.app_log_path().display()
    );

    tauri::Builder::default()
        // 单实例：第二个实例只唤醒已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            shell::show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        // 自动更新走自研实现（见 src/updater.rs）：沿用 GitHub Releases + sha256 校验的既有发布管线，
        // 不使用 tauri-plugin-updater（它要求签名密钥与 latest.json，会改变发布流程）。
        // 业务命令上下文 + 桌面外壳上下文
        .manage(tr_ipc::AppState::new())
        .manage(updater::UpdaterState::default())
        .manage(desktop_state)
        // 工作空间资产：trasset:// 只读暴露 <workspace>/data/assets
        .register_uri_scheme_protocol(assets::SCHEME, |ctx, request| {
            let workspace = ctx
                .app_handle()
                .state::<tr_ipc::AppState>()
                .ws
                .opened_workspace();
            assets::handle(request, workspace)
        })
        .invoke_handler(registry::handler())
        .setup(|app| {
            let handle = app.handle();
            shell::create_tray(handle)?;
            shell::create_startup_window(handle)?;
            tracing::info!("桌面外壳初始化完成（托盘 + 启动窗口）");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| shell::handle_run_event(app, &event));
}
