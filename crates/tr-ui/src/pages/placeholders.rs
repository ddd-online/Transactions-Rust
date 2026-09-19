//! 尚未启用页面的占位实现。
//!
//! **P6-b 之后 7 个页面全部实现**（消费记录 / 数据分析 / 股票交易 / 关键事件 / 日记管理 /
//! 分类标签 / 应用设置），因此本模块只是外壳的**兜底视图**：`shell.rs` 的 `match` 已穷尽
//! 所有 [`Page`]，这里保留 [`PlaceholderPage`] 是为了让"将来新增页面"时有一个现成的骨架。
//!
//! 占位页刻意保留与真实页面相同的骨架（`.page` / `.page-header` / `.page-toolbar` /
//! `.page-body`），这样新增页面时只改内容，外壳与导航代码不用动。
//!
//! 新增页面请照抄这个三段式（详见 `static/css/app.css` 里 `.page-toolbar` 上方的注释）：
//! 标题固定在左上角（右上角留给外壳的窗口控制按钮），工具栏在标题下、内容在工具栏下。

use leptos::prelude::*;

use crate::shell::Page;

/// 占位页：标题 + 说明 + 该页将使用的命令清单。
#[component]
pub fn PlaceholderPage(page: Page) -> impl IntoView {
    let label = page.label();
    let route = page.route();
    let commands = page.planned_commands();

    view! {
        <section class="page">
            <header class="page-header">
                <div class="page-header-text">
                    <h1 class="page-title">{label}</h1>
                    <p class="page-subtitle">{format!("路由 {route}")}</p>
                </div>
                <div class="app-top-bar-spacer"></div>
            </header>
            <div class="page-body">
                <div class="page-placeholder">
                    <span class="page-placeholder-badge">"占位页"</span>
                    <p class="page-hint">
                        {format!("「{label}」尚未接入界面。该页将调用：{commands}。")}
                    </p>
                </div>
            </div>
        </section>
    }
}
