//! Transactions 桌面端入口（Tauri 2 + Rust）。
//!
//! 结构性约定：
//! * **没有子进程内核**：业务代码以库的形式跑在本进程内，通过 Tauri IPC 暴露给界面
//! * **没有本地 HTTP 服务**：不监听端口、没有 API 令牌、没有 CORS
//! * **界面是 Rust**：Leptos 编译为 WASM，由 Tauri 窗口加载
//! * 因此不存在"内核健康检查/重启"这套机制（进程崩溃即应用崩溃），
//!   SQLite 依旧处于 WAL 保护下，不会因退出丢失已提交数据

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod commands;
mod config;
mod logging;
mod shell;
mod updater;

use tauri::Manager;

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
        .invoke_handler(tauri::generate_handler![
            // 外壳命令
            commands::window_control,
            commands::app_info,
            commands::asset_url,
            commands::config_get,
            commands::config_set_close_behavior,
            commands::config_set_appearance,
            commands::config_set_proxy,
            commands::config_set_feature,
            commands::proxy_detect,
            commands::config_file_path,
            commands::workspace_get,
            commands::workspace_set,
            commands::workspace_open,
            commands::workspace_init,
            commands::dialog_open,
            commands::file_save_image,
            commands::devtools_get_state,
            commands::devtools_toggle,
            // 自动更新（自研实现）
            updater::update_check,
            updater::update_download,
            updater::update_cancel,
            updater::update_install,
            // 业务命令（tr-ipc）
            tr_ipc::commands::ledger_list,
            tr_ipc::commands::ledger_create,
            tr_ipc::commands::ledger_get,
            tr_ipc::commands::ledger_update,
            tr_ipc::commands::ledger_delete,
            tr_ipc::commands::tr_query,
            tr_ipc::commands::tr_chart_data,
            tr_ipc::commands::tr_create,
            tr_ipc::commands::tr_batch_create,
            tr_ipc::commands::tr_delete,
            tr_ipc::commands::tr_link,
            tr_ipc::commands::tr_unlink,
            tr_ipc::commands::tr_linked_by_date,
            tr_ipc::commands::diary_list_dates,
            tr_ipc::commands::diary_get,
            tr_ipc::commands::diary_upsert,
            tr_ipc::commands::diary_delete,
            tr_ipc::commands::diary_import_scan,
            tr_ipc::commands::diary_import_file,
            tr_ipc::commands::diary_export,
            tr_ipc::commands::category_list,
            tr_ipc::commands::category_create,
            tr_ipc::commands::category_delete,
            tr_ipc::commands::category_update_sort,
            tr_ipc::commands::category_initialize,
            tr_ipc::commands::tag_list,
            tr_ipc::commands::tag_create,
            tr_ipc::commands::tag_delete,
            tr_ipc::commands::tag_update_sort,
            tr_ipc::commands::template_create,
            tr_ipc::commands::template_list,
            tr_ipc::commands::template_delete,
            tr_ipc::commands::template_update_sort,
            tr_ipc::commands::chart_create,
            tr_ipc::commands::chart_delete,
            tr_ipc::commands::chart_list,
            tr_ipc::commands::chart_update,
            tr_ipc::commands::key_event_list_by_year,
            tr_ipc::commands::key_event_dates_by_year,
            tr_ipc::commands::key_event_get,
            tr_ipc::commands::key_event_upsert,
            tr_ipc::commands::key_event_delete,
            tr_ipc::commands::key_event_images_list,
            tr_ipc::commands::key_event_image_add,
            tr_ipc::commands::key_event_image_delete,
            // 股票（P4）：账户 / 费用与标签设置 / 资金记录 / 持仓 / 委托与成交 / 历史与轮次 / 统计
            tr_ipc::commands::stock_overview,
            tr_ipc::commands::stock_principal_set,
            tr_ipc::commands::stock_principal_add,
            tr_ipc::commands::stock_withdraw,
            tr_ipc::commands::stock_fee_settings_get,
            tr_ipc::commands::stock_fee_settings_put,
            tr_ipc::commands::stock_tag_settings_get,
            tr_ipc::commands::stock_tag_settings_put,
            tr_ipc::commands::stock_fund_records,
            tr_ipc::commands::stock_positions,
            tr_ipc::commands::stock_position_review,
            tr_ipc::commands::stock_trades,
            tr_ipc::commands::stock_trade_create,
            tr_ipc::commands::stock_trade_update,
            tr_ipc::commands::stock_trade_order_delete,
            tr_ipc::commands::stock_trade_impact,
            tr_ipc::commands::stock_history,
            tr_ipc::commands::stock_history_detail,
            tr_ipc::commands::stock_history_summary,
            tr_ipc::commands::stock_round_review,
            tr_ipc::commands::stock_round_tag,
            tr_ipc::commands::stock_statistics,
            tr_ipc::commands::stock_name,
            tr_ipc::commands::stock_reset,
        ])
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
