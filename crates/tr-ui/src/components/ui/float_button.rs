//! 悬浮按钮 —— 对应原 `a-float-button`（右下角圆形的「记一笔」入口）。
//!
//! 固定在内容区右下角（`position: fixed`），带 `--transactions-shadow-lg` 与主色底色。

use leptos::prelude::*;

use crate::icons::{self, Icon};

#[component]
pub fn FloatButton(
    /// 点击回调
    #[prop(optional, into)]
    on_click: Option<UnsyncCallback<()>>,
    /// 悬浮提示
    #[prop(optional, into)]
    title: Option<String>,
    /// 图标，默认 `Plus`
    #[prop(default = Icon::Plus)]
    icon: Icon,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 自定义内容（给了就不再渲染图标）
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    let mut classes = String::from("ui-float-button");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <button
            type="button"
            class=classes
            title=title
            on:click=move |_| {
                if let Some(callback) = on_click {
                    callback.run(());
                }
            }
        >
            {match children {
                Some(children) => children(),
                None => icons::icon(icon),
            }}
        </button>
    }
}
