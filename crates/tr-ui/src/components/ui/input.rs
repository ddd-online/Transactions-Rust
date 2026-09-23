//! 输入框 —— 36px 高 / 8px 圆角 / 1px 发丝边框。
//!
//! 双向绑定：调用方传入 `RwSignal<String>`，组件内部用 `prop:value` 保持 DOM 与信号同步
//! （输入时写回信号，信号外部变更时刷新 DOM）。
//!
//! 附加能力：`placeholder`、`maxlength`、`allow_clear`（右侧清空按钮）、`on_enter`。

use super::with_class;
use leptos::prelude::*;

use crate::icons::{self, Icon};

#[component]
pub fn Input(
    /// 双向绑定的值
    value: RwSignal<String>,
    /// 占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 最大字符数上限
    #[prop(optional)]
    maxlength: Option<u32>,
    /// 显示清空按钮
    #[prop(optional)]
    allow_clear: bool,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 回车回调
    #[prop(optional, into)]
    on_enter: Option<UnsyncCallback<()>>,
    /// 失焦回调（例：股票代码填完就自动查名称，省一个「查询」按钮）
    #[prop(optional, into)]
    on_blur: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let classes = with_class("ui-input", class.as_deref());

    view! {
        <div class=classes class:ui-input--disabled=move || disabled.get()>
            <input
                class="ui-input__control"
                type="text"
                placeholder=placeholder
                maxlength=maxlength
                disabled=move || disabled.get()
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
                on:keydown=move |ev| {
                    if ev.key() == "Enter" {
                        if let Some(callback) = on_enter {
                            callback.run(());
                        }
                    }
                }
                on:blur=move |_| {
                    if let Some(callback) = on_blur {
                        callback.run(());
                    }
                }
            />
            <Show when=move || allow_clear && !value.get().is_empty()>
                <button
                    type="button"
                    class="ui-input__clear"
                    title="清空"
                    aria-label="清空"
                    on:click=move |_| value.set(String::new())
                >
                    {icons::icon(Icon::CloseCircle)}
                </button>
            </Show>
        </div>
    }
}
