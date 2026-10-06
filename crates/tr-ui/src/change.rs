//! 界面侧的写路径小工具：把 `tr_draw::change` 的判据接到本 crate 的全局状态上。
//!
//! 页面在异步回包里判断"这次写还是不是替当前账本做的"，从前到处手写
//! `AppStores::global().current_ledger_id.get_untracked() != ledger_id`（全仓 9 处）。
//! 规则本身在 [`tr_draw::change::WriteTarget`]（纯逻辑、native 上真跑），这里只是把它接到
//! 全局账本信号上 —— 于是"什么算过期"只有一份实现。
//!
//! 需要**出发前**就把账本 id 抓下来的地方（例如要跨 `await` 的自动保存）直接用
//! `WriteTarget::new(...)` + `is_stale(...)`，那样子在手上、时机也更显眼。

use leptos::prelude::{GetUntracked, RwSignal, Set};

use crate::store::AppStores;

/// 这次写出发时记下的账本 `ledger_id`，现在已经不是当前账本了？
///
/// `true` → 这次写的结果属于旧账本：本地不落地、也不提示。
pub fn stale(ledger_id: &str) -> bool {
    tr_draw::change::is_stale(
        ledger_id,
        &AppStores::global().current_ledger_id.get_untracked(),
    )
}

/// 反过来那一问（少数调用点写的是 `== ledger_id` 才继续）。
pub fn current(ledger_id: &str) -> bool {
    !stale(ledger_id)
}

/// 提交一次写请求：**管"在飞"与"失败怎么报"**，成功之后做什么留给调用点。
///
/// 为什么只有这一小块（`#34` 的结论）：写调用点的真实形状是"请求 → 成功后若干**界面动作**
/// （关弹窗 / 清输入 / 成功提示 / 重拉）→ 失败提示"。那些动作逐处不同、也不该被一个声明式
/// 计划吞掉 —— 所以"受影响的读 / 成功副作用"留在调用点。但有两件事**逐处相同**：
///
/// 1. **在飞标记**：置位 → 请求 → 复位，**无论成败都要复位**。今天的写法靠"把复位写在 `match`
///    之后"这个位置约定保证，改成早返回（`?`）就会漏掉它 —— 按钮永远转下去；
/// 2. **失败面**：一律 `notify_error(prefix, &error)`（前缀逐处不同，但"必须提示、必须带前缀"是同一条规则）。
///
/// 返回 `Some(值)` 表示成功（失败已经提示过了，调用点什么都不用做）。
pub async fn submit<T>(
    running: Option<RwSignal<bool>>,
    prefix: &'static str,
    request: impl std::future::Future<Output = Result<T, crate::ipc::IpcError>>,
) -> Option<T> {
    if let Some(flag) = running {
        flag.set(true);
    }
    let result = request.await;
    // **先复位再处理结果**：这样即使下面的分支将来被改成早返回，也不会把按钮留在在飞态
    if let Some(flag) = running {
        flag.set(false);
    }
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            crate::error_handler::notify_error(prefix, &error);
            None
        }
    }
}
