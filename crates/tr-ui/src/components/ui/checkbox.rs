//! 复选框与复选框组 —— 对应原 `a-checkbox` / `a-checkbox-group`。
//!
//! * [`Checkbox`]：单个布尔勾选（日记导入的文件列表、股票标签设置的一行一标签）
//! * [`CheckboxGroup`]：多选（消费记录弹窗的标签多选、筛选条件里的标签集合）
//!
//! 勾选框用 `<button role="checkbox">` + 内联 SVG 勾号，避免原生 `<input type="checkbox">`
//! 在 WebView2 上与设计令牌不一致的默认外观。

use leptos::prelude::*;

use crate::icons::{self, Icon};

/// 一个多选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckboxOption {
    pub value: String,
    pub label: String,
}

impl CheckboxOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    /// 值即标签的便捷构造。
    pub fn same(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }
}

impl From<&str> for CheckboxOption {
    fn from(value: &str) -> Self {
        CheckboxOption::same(value)
    }
}

/// 单个复选框。
#[component]
pub fn Checkbox(
    /// 勾选状态
    checked: RwSignal<bool>,
    /// 右侧文案
    #[prop(optional, into)]
    label: Option<String>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<bool>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-checkbox");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes>
            <button
                type="button"
                role="checkbox"
                class="ui-checkbox__box"
                class:is-checked=move || checked.get()
                aria-checked=move || checked.get().to_string()
                disabled=move || disabled.get()
                on:click=move |_| {
                    if disabled.get_untracked() {
                        return;
                    }
                    let next = !checked.get_untracked();
                    checked.set(next);
                    if let Some(callback) = on_change {
                        callback.run(next);
                    }
                }
            >
                <Show when=move || checked.get()>
                    <span class="ui-checkbox__mark">{icons::icon(Icon::Check)}</span>
                </Show>
            </button>
            {label
                .map(|text| {
                    view! {
                        <span
                            class="ui-checkbox__label"
                            on:click=move |_| {
                                if disabled.get_untracked() {
                                    return;
                                }
                                let next = !checked.get_untracked();
                                checked.set(next);
                                if let Some(callback) = on_change {
                                    callback.run(next);
                                }
                            }
                        >
                            {text}
                        </span>
                    }
                })}
        </div>
    }
}

/// 复选框组（值集合可增删）。
#[component]
pub fn CheckboxGroup(
    /// 已选值集合
    values: RwSignal<Vec<String>>,
    /// 全部选项
    options: Vec<CheckboxOption>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调（参数是新的值集合）
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<Vec<String>>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-checkbox-group");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes>
            {options
                .into_iter()
                .map(|option| {
                    let value = option.value.clone();
                    let check_box = option.value.clone();
                    let check_mark = option.value.clone();
                    let label = option.label.clone();
                    view! {
                        <button
                            type="button"
                            role="checkbox"
                            class="ui-checkbox-group__item"
                            class:is-checked=move || values.with(|list| list.contains(&check_box))
                            disabled=move || disabled.get()
                            on:click=move |_| {
                                if disabled.get_untracked() {
                                    return;
                                }
                                let mut next = values.get_untracked();
                                if let Some(index) = next.iter().position(|item| item == &value) {
                                    next.remove(index);
                                } else {
                                    next.push(value.clone());
                                }
                                values.set(next.clone());
                                if let Some(callback) = on_change {
                                    callback.run(next);
                                }
                            }
                        >
                            <span class="ui-checkbox__box">
                                <Show when=move || values.with(|list| list.contains(&check_mark))>
                                    <span class="ui-checkbox__mark">
                                        {icons::icon(Icon::Check)}
                                    </span>
                                </Show>
                            </span>
                            <span>{label}</span>
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}
