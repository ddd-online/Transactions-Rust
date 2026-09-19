//! 下拉选择 —— 支持搜索与清空。
//!
//! 三个常用能力：
//! * `searchable = true`：按 `label` 过滤选项
//! * `allow_clear = true`：右侧清空按钮
//! * `value: RwSignal<String>`：双向绑定当前值
//!
//! 面板是真下拉（绝对定位 + 阴影），带一层透明遮罩负责"点击别处关闭"。
//! 选项用 `map + collect_view` 渲染：选项量级是"账本/分类/标签"，几十条，无需 keyed diff。

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;

use crate::icons::{self, Icon};

/// 一个选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    /// 值即标签的便捷构造（账本 id/名称不同名时用 [`SelectOption::new`]）。
    pub fn same(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }
}

#[component]
pub fn Select(
    /// 双向绑定的值
    value: RwSignal<String>,
    /// 选项列表
    options: Vec<SelectOption>,
    /// 未选中时的占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
    /// 允许搜索（在面板内过滤 label）
    #[prop(optional)]
    searchable: bool,
    /// 允许清空
    #[prop(optional)]
    allow_clear: bool,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 选中变化回调（参数是新的值）
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<String>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));
    let open = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let options = StoredValue::new(options);

    let selected_label = move || {
        let current = value.get();
        options.with_value(|list| {
            list.iter()
                .find(|option| option.value == current)
                .map(|option| option.label.clone())
        })
    };

    let filtered = move || {
        let keyword = query.get().trim().to_lowercase();
        options.with_value(|list| {
            list.iter()
                .filter(|option| {
                    keyword.is_empty() || option.label.to_lowercase().contains(&keyword)
                })
                .cloned()
                .collect::<Vec<SelectOption>>()
        })
    };

    let pick = move |option: SelectOption| {
        value.set(option.value.clone());
        if let Some(callback) = on_change {
            callback.run(option.value);
        }
        query.set(String::new());
        open.set(false);
    };

    let placeholder = placeholder.unwrap_or_else(|| "请选择".to_string());

    let mut classes = String::from("ui-select");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-open=move || open.get()>
            <button
                type="button"
                class="ui-select__trigger"
                disabled=move || disabled.get()
                on:click=move |_| {
                    if !disabled.get_untracked() {
                        open.update(|open| *open = !*open);
                    }
                }
            >
                <span class="ui-select__value">
                    {move || match selected_label() {
                        Some(label) => view! { <span>{label}</span> }.into_any(),
                        None => view! { <span class="ui-select__placeholder">{placeholder.clone()}</span> }
                            .into_any(),
                    }}
                </span>
                <Show when=move || allow_clear && !value.get().is_empty()>
                    <span
                        class="ui-input__clear"
                        role="button"
                        title="清空"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            value.set(String::new());
                            query.set(String::new());
                            if let Some(callback) = on_change {
                                callback.run(String::new());
                            }
                        }
                    >
                        {icons::icon(Icon::CloseCircle)}
                    </span>
                </Show>
                <span class="ui-select__arrow">{icons::icon(Icon::Down)}</span>
            </button>

            <Show when=move || open.get()>
                <div
                    class="ui-select__backdrop"
                    on:click=move |_| {
                        open.set(false);
                        query.set(String::new());
                    }
                ></div>
                <div class="ui-select__panel">
                    <Show when=move || searchable>
                        <div class="ui-select__search">
                            <div class="ui-input">
                                <span class="ui-input__prefix">{icons::icon(Icon::Search)}</span>
                                <input
                                    class="ui-input__control"
                                    type="text"
                                    placeholder="搜索"
                                    prop:value=move || query.get()
                                    on:input=move |ev| query.set(event_target_value(&ev))
                                />
                            </div>
                        </div>
                    </Show>
                    <div>
                        {move || {
                            let list = filtered();
                            if list.is_empty() {
                                view! { <div class="ui-select__empty">"无匹配选项"</div> }.into_any()
                            } else {
                                let current = value.get();
                                list
                                    .into_iter()
                                    .map(|option| {
                                        let is_selected = option.value == current;
                                        let clicked = option.clone();
                                        view! {
                                            <div
                                                class="ui-select__option"
                                                class:is-selected=is_selected
                                                on:click=move |_| pick(clicked.clone())
                                            >
                                                {option.label}
                                            </div>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </div>
                </div>
            </Show>
        </div>
    }
}
