//! 悬浮提示 —— 纯 CSS 气泡（`data-tooltip` + `::after`），零 JS 依赖。
//!
//! 对应原 `a-tooltip` 的"仅文字提示"用法（表格行内按钮、面板标题问号等）。
//! 复杂内容（带标题/富文本的 popover）本阶段不做，留给后续阶段的 `Popover` 组件。

use leptos::prelude::*;

#[component]
pub fn Tooltip(
    /// 提示文案
    #[prop(into)]
    title: String,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let mut classes = String::from("ui-tooltip");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <span class=classes data-tooltip=title>
            {children()}
        </span>
    }
}
