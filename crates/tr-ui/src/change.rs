//! 界面侧的写路径小工具：把 `tr_draw::change` 的判据接到本 crate 的全局状态上。
//!
//! 页面在异步回包里判断"这次写还是不是替当前账本做的"，从前到处手写
//! `AppStores::global().current_ledger_id.get_untracked() != ledger_id`（全仓 9 处）。
//! 规则本身在 [`tr_draw::change::WriteTarget`]（纯逻辑、native 上真跑），这里只是把它接到
//! 全局账本信号上 —— 于是"什么算过期"只有一份实现。
//!
//! 需要**出发前**就把账本 id 抓下来的地方（例如要跨 `await` 的自动保存）直接用
//! `WriteTarget::new(...)` + `is_stale(...)`，那样子在手上、时机也更显眼。

use leptos::prelude::GetUntracked;
use tr_draw::change::WriteTarget;

use crate::store::AppStores;

/// 这次写出发时记下的账本 `ledger_id`，现在已经不是当前账本了？
///
/// `true` → 这次写的结果属于旧账本：本地不落地、也不提示。
pub fn stale(ledger_id: &str) -> bool {
    WriteTarget::new(ledger_id).is_stale(&AppStores::global().current_ledger_id.get_untracked())
}

/// 反过来那一问（少数调用点写的是 `== ledger_id` 才继续）。
pub fn current(ledger_id: &str) -> bool {
    !stale(ledger_id)
}
