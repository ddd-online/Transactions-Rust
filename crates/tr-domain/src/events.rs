//! IPC 事件的**名字清单** —— 外壳（发送方）与界面（订阅方）共用的唯一来源。
//!
//! 事件名从前是两侧各写一份 `const`，或者干脆两边各写一个字符串字面量，靠"逐字相同"
//! 这句注释维持一致：改一边忘另一边，表现是"事件永远不来"，而且不报任何错。
//! 现在应用里**每一个** IPC 事件名都在这里（共 6 条），两侧都引用它 —— 想核对
//! "还有没有漏网的名字"，`grep` 一遍 `emit(` 与 `listen` 就知道。
//!
//! 事件**载荷**不在这里 —— 那是各事件自己的结构（例如 `update:*` 的载荷是
//! [`crate::update::UpdateSnapshot`]，见该模块）。

/// 更新：下载进度（载荷是完整快照）。
pub const UPDATE_DOWNLOAD_PROGRESS: &str = "update:download-progress";
/// 更新：下载完成（载荷是完整快照）。
pub const UPDATE_DOWNLOAD_COMPLETE: &str = "update:download-complete";
/// 更新：下载失败（载荷是完整快照）。
pub const UPDATE_DOWNLOAD_ERROR: &str = "update:download-error";
/// 开发者工具：开关状态变化（载荷是 `bool`）。
pub const DEVTOOLS_STATE_CHANGED: &str = "devtools:state-changed";
/// 主窗口：最大化 / 还原状态变化（载荷是 `bool`）。
///
/// 双击标题栏 / Win+↑ / 拖到屏幕顶部贴靠都不经过 `window_control`，
/// 界面靠这条事件把"乐观取反"纠正回真实状态。
pub const WINDOW_STATE_CHANGED: &str = "window-state-changed";
/// 工作空间切换成功（载荷是新工作空间目录的字符串）。
pub const WORKSPACE_CHANGED: &str = "workspace-changed";

/// 全部事件名（含各自的名字空间），供单测遍历。
pub const ALL: &[&str] = &[
    UPDATE_DOWNLOAD_PROGRESS,
    UPDATE_DOWNLOAD_COMPLETE,
    UPDATE_DOWNLOAD_ERROR,
    DEVTOOLS_STATE_CHANGED,
    WINDOW_STATE_CHANGED,
    WORKSPACE_CHANGED,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for name in ALL {
            assert!(seen.insert(*name), "事件名重复：{name}");
        }
        assert_eq!(seen.len(), ALL.len());
    }

    /// 事件名的形状：**新**名字一律写成 `<名字空间>:<动作>`。
    ///
    /// `window-state-changed` 与 `workspace-changed` 是既有的名字，**没有**名字空间
    /// （两侧早就在用）。名字是对外契约，一个字都不能改 —— 所以这里把那两个登记成
    /// 例外，而不是把形状检查改成"随便什么都行"：将来给它们改名是另一码事。
    #[test]
    fn names_are_namespaced_unless_they_are_an_existing_contract() {
        const LEGACY_WITHOUT_NAMESPACE: &[&str] = &[WINDOW_STATE_CHANGED, WORKSPACE_CHANGED];

        for name in ALL {
            if LEGACY_WITHOUT_NAMESPACE.contains(name) {
                continue;
            }
            let mut parts = name.split(':');
            let namespace = parts.next().unwrap_or_default();
            let rest = parts.next().unwrap_or_default();
            assert!(!namespace.is_empty(), "缺少名字空间：{name}");
            assert!(!rest.is_empty(), "缺少动作：{name}");
            assert!(parts.next().is_none(), "冒号过多：{name}");
        }

        // 例外表本身不能过期：那两条必须真的没有冒号，否则它们该挪出例外表
        for name in LEGACY_WITHOUT_NAMESPACE {
            assert!(
                !name.contains(':'),
                "有了名字空间就不该留在例外表里：{name}"
            );
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "既有的名字是 kebab-case：{name}"
            );
        }
    }

    #[test]
    fn update_events_share_one_namespace() {
        let update: Vec<&str> = ALL
            .iter()
            .copied()
            .filter(|name| name.starts_with("update:"))
            .collect();
        assert_eq!(update.len(), 3, "update 事件应当是进度 / 完成 / 失败三条");
        assert!(update.contains(&UPDATE_DOWNLOAD_PROGRESS));
        assert!(update.contains(&UPDATE_DOWNLOAD_COMPLETE));
        assert!(update.contains(&UPDATE_DOWNLOAD_ERROR));
    }

    /// 应用里**每一个** IPC 事件名都要在这张表里（今天 6 条）。
    ///
    /// 完整性只能靠人核对（`grep` 一遍 `emit(` / `emit_to(` 与 `listen`）：
    /// 两侧的调用点分别在外壳与界面两个 crate 里，而本 crate 是纯领域层 ——
    /// 让它去 `include_str!` 别的 crate 的源码，会把分层与构建耦合一起搞坏。
    /// 表里少一条的代价是"新事件忘了登记"，而不是"名字漂移"：名字只在这里定义，
    /// 两侧都引用它，改了不可能只改到一边。
    #[test]
    fn every_event_name_is_declared_here() {
        for name in ALL {
            assert!(!name.is_empty());
        }
        assert_eq!(ALL.len(), 6, "事件数量变了就顺手 grep 一遍调用点");
    }
}
