//! 分割线（水平 / 垂直 / 带文字）。
//!
//! 取值全部来自 `--transactions-color-divider`；带文字时文字为次要色小字。

use leptos::prelude::*;

#[component]
pub fn Divider(
    /// 垂直分割线（默认水平）
    #[prop(optional)]
    vertical: bool,
    /// 水平分割线中间的文字（垂直时忽略）
    #[prop(optional, into)]
    text: Option<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let mut classes = String::from("ui-divider");
    classes.push_str(if vertical {
        " ui-divider--vertical"
    } else {
        " ui-divider--horizontal"
    });
    let has_text = !vertical && text.as_deref().map(|t| !t.is_empty()).unwrap_or(false);
    if has_text {
        classes.push_str(" ui-divider--with-text");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes role="separator">
            <span class="ui-divider__line"></span>
            {has_text.then(|| view! { <span class="ui-divider__text">{text.clone()}</span> })}
            <span class="ui-divider__line"></span>
        </div>
    }
}
