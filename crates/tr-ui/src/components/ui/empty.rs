//! 空态 —— 图标 + 标题 + 描述 + 可选操作区。
//!
//! 消费记录页另有一个 `empty-guide` 引导块（在页面里单独实现，
//! 因为它的操作按钮是页面职责）。

use leptos::prelude::*;

use crate::icons::{self, Icon};

#[component]
pub fn Empty(
    /// 主标题
    #[prop(into)]
    title: String,
    /// 补充说明
    #[prop(optional, into)]
    description: Option<String>,
    /// 图标，默认 `Inbox`
    #[prop(default = Icon::Inbox)]
    icon: Icon,
    /// 操作区（可省略）
    #[prop(optional, into)]
    actions: ViewFn,
) -> impl IntoView {
    view! {
        <div class="ui-empty">
            <span class="ui-empty__icon">{icons::icon(icon)}</span>
            <p class="ui-empty__title">{title}</p>
            {description
                .map(|text| view! { <p class="ui-empty__description">{text}</p> })}
            {actions.run()}
        </div>
    }
}
