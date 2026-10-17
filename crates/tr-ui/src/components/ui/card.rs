//! 卡片 —— 看板那种「瀑布流里的一块内容」。
//!
//! 三段式，上中下各司其职：
//!
//! ```text
//! .ui-card
//!   ├─ .ui-card__head     上：标题（h2，唯一必填）
//!   ├─ .ui-card__tools    中：可选的按钮行（分段 / 切换 / 更多），不给就整段不渲染
//!   └─ .ui-card__body     下：内容
//! ```
//!
//! ## 为什么不用固定宽度
//!
//! 卡片的**宽度由内容决定**（`width: fit-content`，上限 `max-width: 100%`），高度随内容
//! 自动增长。这样同一个组件能装下一行三个数字，也能装一张分类清单 —— 调用方不必为每张卡
//! 挑一个宽度档。摆在瀑布流容器里（`display: flex; flex-wrap: wrap; align-items: flex-start`）
//! 就是"宽窄不一、高低不齐"的卡片流。
//!
//! ## 中段为什么是独立一行
//!
//! 标题行右侧塞按钮看着更省地方，但那样每张卡的标题宽度都被最长的那个按钮挤走，山字形
//! 对不齐；而中段本来就该是"这张卡自己的操作区"，它随卡片内容出现或消失，不该改变标题
//! 的位置。所以标题永远贴卡片左上角。

use super::with_class;
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;

#[component]
pub fn Card(
    /// 卡片标题（必给：看板的每一块都要能一眼叫出名字）
    title: String,
    /// 中段：可选的按钮区（分段切换等）；**不给就整段不渲染**
    #[prop(optional, into)]
    tools: Option<AnyView>,
    /// 附加类名（页面级微调，例如 `dash-card--mix`）
    #[prop(optional, into)]
    class: Option<String>,
    /// 内容
    children: Children,
) -> impl IntoView {
    let classes = with_class("ui-card", class.as_deref());

    view! {
        <section class=classes>
            <header class="ui-card__head">
                <h2 class="ui-card__title">{title}</h2>
            </header>

            {tools.map(|tools| view! { <div class="ui-card__tools">{tools}</div> })}

            <div class="ui-card__body">{children()}</div>
        </section>
    }
}
