//! IPC 命令的**注册清单** —— 唯一事实来源。
//!
//! ## 为什么要有这个文件
//!
//! 命令的注册名由 `#[tauri::command]` 从**函数名**生成（宏内部就是
//! `stringify!(fn)`），而 `tauri::generate_handler![]` 展开出来的只是一个闭包，
//! 没有任何办法在运行时枚举它。于是"注册表里到底有哪些命令"这件事，在过去只能靠
//! **人读 `main.rs` 那张清单**；漏掉一条的表现是使用者点下去报"命令不存在"。
//!
//! 现在清单只写一次（[`app_commands!`] 的调用处）：注册闭包与注册名清单
//! （[`REGISTERED_PATHS`]）从**同一份 token** 展开，所以"注册的东西"和
//! "守卫读的东西"不可能各说各话。[`tests::catalog_matches_registration`]
//! 再拿注册名去核对 `tr_domain::commands` 的业务命令清单 —— 那是界面侧取名字的地方。
//!
//! ## 名字怎么来的
//!
//! `stringify!($path)` 给出函数全路径（`tr_ipc::commands::ledger_list`），
//! 取最后一段就是线上命令名。这条等价关系依赖"命令没有 `rename`"这个前提，
//! 由 `tr-ipc` 的 `registration_name_is_the_function_name` 兜底（它扫命令源码里
//! 的 `#[tauri::command]` 属性，发现带参数就红）。
//!
//! ## 覆盖范围
//!
//! 全部 112 条：22 条外壳命令 + 5 条更新命令（**尚未进 `tr_domain::commands` 清单**，
//! 见 #28）+ 85 条业务命令。因此本文件只对"业务命令"那一段做清单核对；
//! 等 #28 落地，核对范围自然扩到全表。

/// 声明全部 IPC 命令：写一次函数路径清单，同时得到注册闭包与注册名清单。
///
/// 唯一入口是 `.invoke_handler()`：不要在别处再写 `generate_handler![]`，
/// 否则那张清单就脱离了本条守卫。
macro_rules! app_commands {
    ($($path:path),* $(,)?) => {
        /// 注册清单里的每一条（函数路径，顺序 = 注册顺序）。
        ///
        /// 只在测试里存在：它唯一的作用是让守卫能读到"实际注册了哪些命令"
        /// （生产构建不需要这份字符串表，留着一个没人用的常量只会招来死代码警告）。
        #[cfg(test)]
        pub(crate) const REGISTERED_PATHS: &[&str] = &[$(stringify!($path)),*];

        /// 交给 `.invoke_handler()` 的注册闭包（仍然是 Tauri 官方的注册宏）。
        ///
        /// 运行时刻意写死 `tauri::Wry`：外壳命令里有几条直接收 `AppHandle`
        /// （= `AppHandle<Wry>`，见 `#[default_runtime]`），泛型 `R` 反而凑不出
        /// `CommandArg`；应用本来也只有这一个运行时刻（`tauri::Builder::default()`）。
        pub(crate) fn handler(
        ) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($path),*]
        }
    };
}

app_commands! {
    // ---- 外壳命令（`src-tauri/src/commands.rs`）----
    crate::commands::window_control,
    crate::commands::app_info,
    crate::commands::asset_url,
    crate::commands::config_get,
    crate::commands::config_set_close_behavior,
    crate::commands::config_set_appearance,
    crate::commands::config_set_proxy,
    crate::commands::config_set_feature,
    crate::commands::config_set_key_event_linked_open,
    crate::commands::config_set_sidebar_collapsed,
    crate::commands::proxy_detect,
    crate::commands::config_file_path,
    crate::commands::workspace_get,
    crate::commands::workspace_set,
    crate::commands::workspace_icon_get,
    crate::commands::workspace_icon_set,
    crate::commands::workspace_open,
    crate::commands::workspace_init,
    crate::commands::dialog_open,
    crate::commands::file_save_image,
    crate::commands::devtools_get_state,
    crate::commands::devtools_toggle,

    // ---- 自动更新（`src-tauri/src/updater.rs`，自研实现）----
    crate::updater::update_check,
    crate::updater::update_download,
    crate::updater::update_download_status,
    crate::updater::update_cancel,
    crate::updater::update_install,

    // ---- 业务命令（`tr-ipc`）：顺序与 `tr_domain::commands` 的清单一致 ----
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
    tr_ipc::commands::stock_overview,
    tr_ipc::commands::stock_principal_add,
    tr_ipc::commands::stock_interest_add,
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
    tr_ipc::commands::stock_archive,
    tr_ipc::commands::stock_operation_list,
    tr_ipc::commands::stock_operation_preview,
    tr_ipc::commands::stock_operation_rollback,
    tr_ipc::commands::todo_cards,
    tr_ipc::commands::todo_history,
    tr_ipc::commands::todo_card_create,
    tr_ipc::commands::todo_card_delete,
    tr_ipc::commands::todo_card_sort,
    tr_ipc::commands::todo_item_create,
    tr_ipc::commands::todo_item_update,
    tr_ipc::commands::todo_item_status,
    tr_ipc::commands::todo_item_delete,
    tr_ipc::commands::todo_progress_add,
    tr_ipc::commands::todo_progress_delete,
    tr_ipc::commands::todo_progress_done,
}

#[cfg(test)]
mod tests {
    use super::*;

    use tr_domain::commands::{catalog_mismatch, BUSINESS_COMMANDS};

    /// 注册名的前缀：`tr-ipc` 提供的那些（其余是外壳 / 更新命令，尚未进清单）。
    const BUSINESS_PREFIX: &str = "tr_ipc::commands::";

    /// 函数路径的最后一段 —— `#[tauri::command]` 就是拿它当线上命令名。
    fn command_name(path: &str) -> &str {
        path.rsplit("::").next().unwrap_or(path)
    }

    /// **守卫本体**：拿一份注册清单（正常是 [`REGISTERED_PATHS`]，测试里也会喂改坏的
    /// 副本）去核对 `tr_domain::commands` 的业务命令清单，一一对应返回 `None`。
    ///
    /// 断言与"怎么比"分开，是为了让负向测试能跑**同一个函数**：只换掉输入，
    /// 不换断言路径。
    fn guard(paths: &[&str]) -> Option<String> {
        let registered: Vec<&str> = paths
            .iter()
            .filter(|path| path.starts_with(BUSINESS_PREFIX))
            .map(|path| command_name(path))
            .collect();
        if registered.len() != BUSINESS_COMMANDS.len() {
            return Some(format!(
                "条数不同：清单 {} 条，注册表 {} 条",
                BUSINESS_COMMANDS.len(),
                registered.len()
            ));
        }
        catalog_mismatch(BUSINESS_COMMANDS, &registered)
    }

    /// **本票的核心断言**：注册表里的业务命令 == `tr_domain::commands` 的业务命令清单。
    ///
    /// 两边都是"名字"：一边来自界面侧取名字的清单，一边来自注册闭包本身
    /// （`generate_handler![]` 与 [`REGISTERED_PATHS`] 同源）—— 所以这条断言真的在测
    /// "注册表漏了一条没有"。
    #[test]
    fn catalog_matches_registration() {
        assert_eq!(
            guard(REGISTERED_PATHS),
            None,
            "业务命令清单与注册表不一致：要么清单加了命令没注册，要么注册了没进清单"
        );
    }

    /// 负向断言：把**真实的注册清单**改坏，守卫必须红 ——
    /// 否则上面那条可能只是在测自己。
    #[test]
    fn guard_catches_a_mutated_registry() {
        assert_eq!(guard(REGISTERED_PATHS), None);

        // ① 注册表少一条（清单里有、没人注册）
        let without_one: Vec<&str> = REGISTERED_PATHS
            .iter()
            .copied()
            .filter(|path| *path != "tr_ipc::commands::ledger_list")
            .collect();
        assert_eq!(without_one.len(), REGISTERED_PATHS.len() - 1);
        assert!(guard(&without_one).is_some(), "注册表少一条没被发现");

        // ② 注册表里某条换了名字（改了线上契约）
        let renamed: Vec<&str> = REGISTERED_PATHS
            .iter()
            .map(|path| {
                if *path == "tr_ipc::commands::ledger_list" {
                    "tr_ipc::commands::ledger_list_renamed"
                } else {
                    *path
                }
            })
            .collect();
        assert!(guard(&renamed).is_some(), "注册表改名没被发现");

        // ③ 注册表里同一个函数写两遍（多出一条死的匹配臂）
        let mut duplicated: Vec<&str> = REGISTERED_PATHS.to_vec();
        duplicated.push("tr_ipc::commands::ledger_list");
        assert!(guard(&duplicated).is_some(), "注册表重复没被发现");

        // ④ 业务命令整段没注册（前缀筛选也一并被验到）
        let desktop_only: Vec<&str> = REGISTERED_PATHS
            .iter()
            .copied()
            .filter(|path| !path.starts_with(BUSINESS_PREFIX))
            .collect();
        assert!(!desktop_only.is_empty(), "外壳命令应当在注册表里");
        assert!(guard(&desktop_only).is_some(), "整段漏注册没被发现");
    }

    /// 注册表本身不能有重复项。
    #[test]
    fn registry_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for path in REGISTERED_PATHS {
            assert!(seen.insert(*path), "注册表里重复出现了：{path}");
        }
    }

    /// 外壳与更新那 27 条命令**仍在**注册表里（它们还没进清单，#28 才收）。
    /// 这条防的是"搬清单时把外壳那一段漏掉了"。
    #[test]
    fn desktop_commands_are_still_registered() {
        for path in [
            "crate::commands::window_control",
            "crate::commands::config_get",
            "crate::commands::workspace_open",
            "crate::commands::devtools_toggle",
            "crate::updater::update_check",
            "crate::updater::update_install",
            "crate::updater::update_cancel",
        ] {
            assert!(
                REGISTERED_PATHS.contains(&path),
                "外壳/更新命令不在注册表里：{path}"
            );
        }
    }

    /// 注册名 = 函数路径的最后一段（`stringify!` 出来的形状）。
    #[test]
    fn registered_names_are_function_names() {
        assert_eq!(command_name("tr_ipc::commands::ledger_list"), "ledger_list");
        assert_eq!(
            command_name("crate::commands::window_control"),
            "window_control"
        );
        assert_eq!(command_name("update_check"), "update_check");
        assert!(REGISTERED_PATHS
            .iter()
            .all(|path| !command_name(path).is_empty()));
    }
}
