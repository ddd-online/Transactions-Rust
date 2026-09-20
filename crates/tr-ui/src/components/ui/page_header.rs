//! 页面页头（5 个顶级功能共用）。
//!
//! 三段式骨架的第一段：左侧页面标题（固定在左上角）、右侧给外壳的窗口控制按钮留位。
//! 5 个顶级功能（记账 / 股票 / 关键事件 / 日记 / 应用设置）原来各自抄了一份逐字相同的
//! `<header class="page-header">…`（记账的 4 个子功能共用同一个页头）。

use leptos::prelude::*;

/// 页头（标题由调用方传入——各页的 `PAGE_TITLE` 常量）。
#[component]
pub fn PageHeader(
    /// 页面标题
    title: &'static str,
) -> impl IntoView {
    view! {
        <header class="page-header">
            <div class="page-header-text">
                <h1 class="page-title">{title}</h1>
            </div>
            <div class="app-top-bar-spacer"></div>
        </header>
    }
}
