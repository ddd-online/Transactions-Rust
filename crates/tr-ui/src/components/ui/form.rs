//! 表单容器与表单项 —— 标签 / 必填星号 / 校验错误文案。
//!
//! 对应原 `a-form` + `a-form-item` 的 `label` / `required` / `validate-status` / `help`：
//! * 标签在控件上方（`layout = Vertical`，原设置页与弹窗的默认）
//! * 标签在控件左侧（`layout = Horizontal`，原「通用设置」的一行一项）
//! * `required` 在标签前加红色 `*`
//! * `error` 非空时在控件下方显示红色校验文案，并把控件边框染红

use leptos::prelude::*;

/// 表单布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormLayout {
    /// 标签在控件上方
    #[default]
    Vertical,
    /// 标签在控件左侧
    Horizontal,
}

impl FormLayout {
    fn class(self) -> &'static str {
        match self {
            FormLayout::Vertical => "ui-form--vertical",
            FormLayout::Horizontal => "ui-form--horizontal",
        }
    }
}

/// 表单容器。
#[component]
pub fn Form(
    /// 布局
    #[prop(optional)]
    layout: FormLayout,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let mut classes = String::from("ui-form ");
    classes.push_str(layout.class());
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! { <div class=classes>{children()}</div> }
}

/// 单个表单项。
#[component]
pub fn FormItem(
    /// 标签文案（空串表示不渲染标签）
    #[prop(into)]
    label: String,
    /// 必填（标签前加红色 `*`）
    #[prop(optional)]
    required: bool,
    /// 校验错误文案；非空时显示并染色
    #[prop(optional, into)]
    error: Option<Signal<String>>,
    /// 标签下方的补充说明（灰色小字）
    #[prop(optional, into)]
    hint: Option<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let error = error.unwrap_or_else(|| Signal::derive(String::new));
    let has_error = move || !error.get().is_empty();

    let mut classes = String::from("ui-form-item");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-error=has_error class:is-required=required>
            {(!label.is_empty())
                .then(|| {
                    view! {
                        <label class="ui-form-item__label">
                            <Show when=move || required>
                                <span class="ui-form-item__required">"*"</span>
                            </Show>
                            {label.clone()}
                        </label>
                    }
                })}
            <div class="ui-form-item__control">{children()}</div>
            {hint.map(|text| view! { <p class="ui-form-item__hint">{text}</p> })}
            <Show when=has_error>
                <p class="ui-form-item__error">{move || error.get()}</p>
            </Show>
        </div>
    }
}
