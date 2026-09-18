//! 加载指示 —— 对应原 `a-spin`。
//!
//! 两种用法：
//! * 独立指示：`<Spin spinning=loading />` —— 居中显示旋转图标；
//! * 包裹内容：`<Spin spinning=loading> ...内容... </Spin>` —— 内容上方叠一层遮罩 + 图标。
//!
//! `children` 在组件体内一次性求值（不放进条件渲染闭包），因此用 [`Children`] 即可。

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;

use crate::icons::{self, Icon};

/// 尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinSize {
    Small,
    #[default]
    Middle,
    Large,
}

impl SpinSize {
    pub fn class(self) -> &'static str {
        match self {
            SpinSize::Small => "ui-spin--sm",
            SpinSize::Middle => "",
            SpinSize::Large => "ui-spin--lg",
        }
    }
}

#[component]
pub fn Spin(
    /// 是否旋转（`bool`、信号或闭包均可）
    #[prop(optional, into)]
    spinning: Option<Signal<bool>>,
    /// 尺寸
    #[prop(optional)]
    size: SpinSize,
    /// 被包裹的内容（可省略）
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    let spinning = spinning.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-spin");
    let size_class = size.class();
    if !size_class.is_empty() {
        classes.push(' ');
        classes.push_str(size_class);
    }

    if let Some(children) = children {
        classes.push_str(" ui-spin--wrapping");
        view! {
            <div class=classes>
                {children()}
                <Show when=move || spinning.get()>
                    <div class="ui-spin__overlay">
                        <span class="ui-spin__indicator">{icons::icon(Icon::Loading)}</span>
                    </div>
                </Show>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div class=classes>
                <Show when=move || spinning.get()>
                    <div class="ui-spin__mask">
                        <span class="ui-spin__indicator">{icons::icon(Icon::Loading)}</span>
                    </div>
                </Show>
            </div>
        }
        .into_any()
    }
}
