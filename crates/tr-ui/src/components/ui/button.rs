//! 按钮 —— 统一按钮系统。
//!
//! 变体与尺寸：
//! `primary`（实心主色）/ `secondary`（描边次要）/ `text`（纯文字）/
//! `text-danger`（文字危险）/ `primary-danger`（实心危险）/ `dashed`（虚线）/ `link`（链接）；
//! 尺寸 `sm` 28px / `md` 36px / `lg` 44px。
//!
//! `icon_only` 让按钮退化为正方形图标按钮（`min-width: auto` + 与高度等宽）。

use leptos::prelude::*;

use crate::icons::{self, Icon};

/// 按钮变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Text,
    TextDanger,
    PrimaryDanger,
    Dashed,
    Link,
}

impl ButtonVariant {
    pub fn class(self) -> &'static str {
        match self {
            ButtonVariant::Primary => "ui-btn--primary",
            ButtonVariant::Secondary => "ui-btn--secondary",
            ButtonVariant::Text => "ui-btn--text",
            ButtonVariant::TextDanger => "ui-btn--text-danger",
            ButtonVariant::PrimaryDanger => "ui-btn--primary-danger",
            ButtonVariant::Dashed => "ui-btn--dashed",
            ButtonVariant::Link => "ui-btn--link",
        }
    }
}

/// 按钮尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Small,
    #[default]
    Middle,
    Large,
}

impl ButtonSize {
    pub fn class(self) -> &'static str {
        match self {
            ButtonSize::Small => "ui-btn--sm",
            ButtonSize::Middle => "",
            ButtonSize::Large => "ui-btn--lg",
        }
    }
}

/// 通用按钮。
///
/// * `loading` / `disabled` 接受 `bool`、信号或闭包（`#[prop(into)]`）；
/// * `on_click` 直接接受 `move || { ... }` 闭包；
/// * `children` 是按钮文案或图标。
#[component]
pub fn Button(
    /// 外观变体
    #[prop(optional)]
    variant: ButtonVariant,
    /// 尺寸
    #[prop(optional)]
    size: ButtonSize,
    /// 占满父容器宽度
    #[prop(optional)]
    block: bool,
    /// 图标按钮（正方形，无最小宽度）
    #[prop(optional)]
    icon_only: bool,
    /// 原生 `title`（悬浮提示）
    #[prop(optional, into)]
    title: Option<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 加载中：显示旋转指示并禁用点击
    #[prop(optional, into)]
    loading: Option<Signal<bool>>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 点击回调
    #[prop(optional, into)]
    on_click: Option<UnsyncCallback<()>>,
    children: Children,
) -> impl IntoView {
    let loading = loading.unwrap_or_else(|| Signal::derive(|| false));
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-btn ");
    classes.push_str(variant.class());
    classes.push(' ');
    classes.push_str(size.class());
    if icon_only {
        classes.push_str(" ui-btn--icon-only");
    }
    if block {
        classes.push_str(" ui-btn--block");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <button
            type="button"
            class=classes
            title=title
            disabled=move || disabled.get() || loading.get()
            on:click=move |_| {
                if let Some(callback) = on_click {
                    callback.run(());
                }
            }
        >
            <Show when=move || loading.get()>
                <span class="ui-btn__icon ui-spin__indicator">
                    {icons::icon(Icon::Loading)}
                </span>
            </Show>
            {children()}
        </button>
    }
}
