//! 功能页骨架（7 个功能页共用）。
//!
//! **标题栏之下的整个区域都由它铺满**——它不是"卡片"：没有外边距、边框、圆角，
//! 它就是这一屏的版心。取代了此前 7 个页面各自抄一份的
//! `<section class="page">` + `<PageHeader>` + `<div class="page-body">` + `<div class="page-toolbar">`。
//!
//! ```text
//! .page
//!   ├─ .page-header               标题栏（48px，右上角给窗口三键留位，拖动区）
//!   └─ .page-region               标题栏之下的整个区域（横向：图标条 + 主列）
//!        ├─ .page-rail           可选：**子功能图标条**（同一顶级功能下的子功能切换）
//!        └─ .page-main
//!             ├─ .page-toolbar    本页操作（底边发丝线；页签下划线压在这条线上）
//!             ├─ .page-content    内容区（吃掉剩余高度）
//!             └─ .page-footer-bar 可选底栏（**只在主列里**，图标条在它左边通到底）
//! ```
//!
//! 两条对齐约定：
//! * **标题栏横跨整宽**（图标条从它下面开始）—— 这样标题与侧栏的账本容器同一起点，
//!   是既有的"左右顶对齐"约定；
//! * **底栏只在主列里**：图标条在它左边一路通到底，底栏不铺到图标条下面。
//!
//! 底栏按需给：不需要的页面不传 `footer`，内容区直接触底。
//!
//! ## 插槽为什么是 `AnyView` 而不是 `ChildrenFn`
//!
//! 四个插槽都是**只渲染一次**的版心结构，不需要重复求值；用构建好的 `AnyView`
//! 同时避开了"闭包必须可多次调用（`Fn`）"的约束 —— 工具栏/内容视图里常把局部
//! 变量 move 进子闭包，外面再包一层 `move || …` 很容易退化成 `FnOnce` 而编译不过。
//! 调用方写成 `toolbar=view! { … }.into_any()`（或先建局部变量再传）即可。
//!
//! ## `rail` 是子功能切换，不是内容里的栏目
//!
//! 它对应"一个顶级功能下有多个子功能"（例如消费记录 / 分类标签 / 消费模板合成一个功能），
//! 从上到下排图标、点击切换。页面内容里的左栏（日记的日期树、分类标签的分类栏、
//! 关键事件的三栏……）**不属于这里**，仍然留在 `content` 里自己排。

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};

use super::page_header::PageHeader;
use super::with_class;

/// 功能页骨架（标题栏 + 铺满的版心）。
#[component]
pub fn FeaturePage(
    /// 页面标题（各页的 `PAGE_TITLE` 常量）
    title: &'static str,
    /// 内容区（必给）
    content: AnyView,
    /// 页面级类名（如 `stock-page`）：**页面自己的样式钩子**，会追加到 `page` 之后。
    /// 迁移到本组件时曾漏掉它，导致 `.stock-page …` 那批规则整体失效（股票页高度链断裂）。
    #[prop(optional, into)]
    class: Option<String>,
    /// 工具栏（本页操作）
    #[prop(optional, into)]
    toolbar: Option<AnyView>,
    /// 子功能图标条（**只有该功能有子功能时才给**）
    #[prop(optional, into)]
    rail: Option<AnyView>,
    /// 底栏（可选；只在**主列**里，图标条在它左边一路通到底）
    #[prop(optional, into)]
    footer: Option<AnyView>,
    /// 底栏内容贴右（只有一段内容、且语义上是"汇总/动作"时用；默认两端对齐）
    #[prop(optional)]
    footer_end: bool,
) -> impl IntoView {
    let classes = with_class("page", class.as_deref());
    view! {
        <section class=classes>
            <PageHeader title=title />
            <div class="page-region">
                {rail.map(|rail| view! { <aside class="page-rail">{rail}</aside> })}
                <div class="page-main">
                    {toolbar
                        .map(|toolbar| view! { <div class="page-toolbar">{toolbar}</div> })}
                    <div class="page-content">{content}</div>
                    {footer
                        .map(|footer| {
                            view! {
                                <div
                                    class="page-footer-bar"
                                    class:page-footer-bar--end=footer_end
                                >{footer}</div>
                            }
                        })}
                </div>
            </div>
        </section>
    }
    .into_any()
}
