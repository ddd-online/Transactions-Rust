//! 分段控制器 —— 选项少（2~4 个）时的单选切换。
//!
//! 对应原 `a-segmented`：等宽分段 + 滑块式激活底色。
//! 用于「记一笔」弹窗的交易类型（支出/收入/转账）与外观设置（浅色/深色/跟随系统）。

use leptos::prelude::*;

/// 一个分段选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentedOption {
    pub value: String,
    pub label: String,
}

impl SegmentedOption {
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

impl From<(&str, &str)> for SegmentedOption {
    fn from((value, label): (&str, &str)) -> Self {
        SegmentedOption::new(value, label)
    }
}

#[component]
pub fn Segmented(
    /// 当前值
    value: RwSignal<String>,
    /// 选项（顺序即展示顺序）
    options: Vec<SegmentedOption>,
    /// 占满父容器宽度（每段等宽）
    #[prop(optional)]
    block: bool,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<String>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-segmented");
    if block {
        classes.push_str(" ui-segmented--block");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-disabled=move || disabled.get()>
            {options
                .into_iter()
                .map(|option| {
                    let option_value = option.value.clone();
                    let check_value = option.value.clone();
                    let click_value = option.value.clone();
                    let is_active = move || value.get() == check_value;
                    let thunk = StoredValue::new(option);
                    view! {
                        <button
                            type="button"
                            class="ui-segmented__item"
                            class:is-active=is_active
                            disabled=move || disabled.get()
                            on:click=move |_| {
                                if disabled.get_untracked() {
                                    return;
                                }
                                value.set(click_value.clone());
                                if let Some(callback) = on_change {
                                    callback.run(option_value.clone());
                                }
                            }
                        >
                            {move || thunk.with_value(|option| option.label.clone())}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}
