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
//! 再拿注册名去核对 `tr_domain::commands` 的**全表**（112 条）—— 那是界面侧取名字的地方。
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
//! 全部 112 条：22 条外壳命令 + 5 条更新命令 + 85 条业务命令，
//! 与 `tr_domain::commands` 的全表一一对应（分组的名字清单也在那边：
//! `SHELL_COMMANDS` / `UPDATE_COMMANDS` / `BUSINESS_COMMANDS`）。

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

    use tr_domain::commands::{
        catalog_mismatch, BUSINESS_COMMANDS, COMMAND_GROUPS, SHELL_COMMANDS, UPDATE_COMMANDS,
    };

    /// 注册表里的**路径前缀 → 清单里的那一组**。这张表是"哪条命令归谁"的唯一出处：
    /// 前缀与模块一一对应（外壳命令在 `commands.rs`、更新命令在 `updater.rs`、
    /// 业务命令在 `tr-ipc`），每组有多少条由 `tr_domain::commands` 说了算。
    const SEGMENTS: &[(&str, &[&str])] = &[
        ("crate::commands::", SHELL_COMMANDS),
        ("crate::updater::", UPDATE_COMMANDS),
        ("tr_ipc::commands::", BUSINESS_COMMANDS),
    ];

    /// 函数路径的最后一段 —— `#[tauri::command]` 就是拿它当线上命令名。
    fn command_name(path: &str) -> &str {
        path.rsplit("::").next().unwrap_or(path)
    }

    /// 注册清单里属于某个前缀的那些名字。
    fn names_with_prefix<'a>(paths: &[&'a str], prefix: &str) -> Vec<&'a str> {
        paths
            .iter()
            .filter(|path| path.starts_with(prefix))
            .map(|path| command_name(path))
            .collect()
    }

    /// **守卫本体**：拿一份注册清单（正常是 [`REGISTERED_PATHS`]，测试里也会喂改坏的
    /// 副本）逐段核对 `tr_domain::commands`，一一对应返回 `None`。
    ///
    /// 断言与"怎么比"分开，是为了让负向测试能跑**同一个函数**：只换掉输入，
    /// 不换断言路径。
    fn guard(paths: &[&str]) -> Option<String> {
        let mut matched = 0;
        for (prefix, expected) in SEGMENTS {
            let observed = names_with_prefix(paths, prefix);
            matched += observed.len();
            if let Some(problem) = catalog_mismatch(expected, &observed) {
                return Some(format!("{prefix} 那一段：{problem}"));
            }
        }
        // 各段都对上还不够：注册表里可能出现哪一段都不认的路径（模块名打错）。
        if matched != paths.len() {
            let unknown: Vec<&str> = paths
                .iter()
                .copied()
                .filter(|path| !SEGMENTS.iter().any(|(prefix, _)| path.starts_with(prefix)))
                .collect();
            return Some(format!("注册表里有清单不认的路径：{unknown:?}"));
        }
        None
    }

    /// **本票的核心断言**：注册表 == `tr_domain::commands` 的全表（112 条，不重不漏）。
    ///
    /// 两边都是"名字"：一边来自界面侧取名字的清单，一边来自注册闭包本身
    /// （`generate_handler![]` 与 [`REGISTERED_PATHS`] 同源）—— 所以这条断言真的在测
    /// "注册表漏了一条没有"。
    /// 事件名不许在应用里以**字面量**出现（除了清单本身）。
    ///
    /// 两侧都该用 `tr_domain::events::*`（编译期就保证名字对）；复制粘贴一份字面量就是漂移的开始 ——
    /// 改清单不会改到它，而且没有任何编译错误。命令名那条链守的是"注册表 == 清单"，
    /// 这一条守的是"没人绕过清单"（候选 9 / #39）。
    ///
    /// 扫描范围：`src-tauri/src` 与 `crates/tr-ui/src`（清单所在 crate 自己不算）。
    #[test]
    fn event_names_are_never_written_as_literals() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 上一级就是仓库根")
            .to_path_buf();

        let mut offenders: Vec<String> = Vec::new();
        for relative in ["src-tauri/src", "crates/tr-ui/src"] {
            let mut stack = vec![root.join(relative)];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                        continue;
                    }
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    for name in tr_domain::events::ALL {
                        if text.contains(&format!("\"{name}\"")) {
                            offenders.push(format!(
                                "{}: \"{name}\"",
                                path.strip_prefix(&root).unwrap_or(&path).display()
                            ));
                        }
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "事件名以字面量出现（应当走 tr_domain::events::*）：{offenders:#?}"
        );
    }

    #[test]
    fn catalog_matches_registration() {
        assert_eq!(
            guard(REGISTERED_PATHS),
            None,
            "命令清单与注册表不一致：要么清单加了命令没注册，要么注册了没进清单"
        );
        assert_eq!(
            REGISTERED_PATHS.len(),
            COMMAND_GROUPS
                .iter()
                .map(|group| group.len())
                .sum::<usize>()
        );
    }

    /// 负向断言：把**真实的注册清单**改坏，守卫必须红 ——
    /// 否则上面那条可能只是在测自己。
    #[test]
    fn guard_catches_a_mutated_registry() {
        assert_eq!(guard(REGISTERED_PATHS), None);

        // ① 注册表少一条（清单里有、没人注册）—— 每一段各试一次
        for (prefix, group) in SEGMENTS {
            let dropped = REGISTERED_PATHS
                .iter()
                .find(|path| path.starts_with(prefix))
                .expect("每一段都该有命令");
            let without_one: Vec<&str> = REGISTERED_PATHS
                .iter()
                .copied()
                .filter(|path| *path != *dropped)
                .collect();
            assert_eq!(without_one.len(), REGISTERED_PATHS.len() - 1);
            assert!(
                guard(&without_one).is_some(),
                "注册表少一条（{dropped}，属于 {group:?} 那一组）没被发现"
            );
        }

        // ② 注册表里某条换了名字（改了线上契约）
        let renamed: Vec<&str> = REGISTERED_PATHS
            .iter()
            .map(|path| {
                if *path == "crate::updater::update_check" {
                    "crate::updater::update_check_renamed"
                } else {
                    *path
                }
            })
            .collect();
        assert!(guard(&renamed).is_some(), "注册表改名没被发现");

        // ③ 注册表里同一个函数写两遍（多出一条死的匹配臂）
        let mut duplicated: Vec<&str> = REGISTERED_PATHS.to_vec();
        duplicated.push("crate::commands::window_control");
        assert!(guard(&duplicated).is_some(), "注册表重复没被发现");

        // ④ 每一段各自整段没注册（前缀筛选也一并被验到）
        for (prefix, group) in SEGMENTS {
            let without_this: Vec<&str> = REGISTERED_PATHS
                .iter()
                .copied()
                .filter(|path| !path.starts_with(prefix))
                .collect();
            assert!(without_this.len() < REGISTERED_PATHS.len());
            assert!(
                guard(&without_this).is_some(),
                "{prefix}（{group:?}）整段漏注册没被发现"
            );
        }

        // ⑤ 注册表里混进一个哪一段都不认的路径（模块名打错）
        let mut stray: Vec<&str> = REGISTERED_PATHS.to_vec();
        stray.push("tr_ipc::command::ledger_list");
        assert!(guard(&stray).is_some(), "不认的路径没被发现");
    }

    /// 注册表本身不能有重复项。
    #[test]
    fn registry_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for path in REGISTERED_PATHS {
            assert!(seen.insert(*path), "注册表里重复出现了：{path}");
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

    /// 清单里的三组与注册表的三段前缀一一对应 —— 组的名字改了、前缀表忘了改，这条会红。
    #[test]
    fn every_group_has_a_registry_segment() {
        assert_eq!(SEGMENTS.len(), COMMAND_GROUPS.len());
        for (index, group) in COMMAND_GROUPS.iter().enumerate() {
            assert_eq!(*group, SEGMENTS[index].1, "第 {index} 组与 SEGMENTS 对不上");
        }
    }
}
