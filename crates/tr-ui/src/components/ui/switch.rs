//! 开关（`checked` / `checked_children` / `disabled`）。
//!
//! 用 `<button role="switch" aria-checked>` 而不是 `<input type="checkbox">`：
//! 与组件套件其余部分的"按钮 + `data-*` 状态"风格一致，也免去 `:checked` 选择器的
//! 浏览器差异（Windows WebView2 的默认勾选框外观无法完全覆盖）。

use super::with_class;
use leptos::prelude::*;

#[component]
pub fn Switch(
    /// 开关状态
    checked: RwSignal<bool>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 右侧文案
    #[prop(optional, into)]
    label: Option<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调（参数是新状态）
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<bool>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let classes = with_class("ui-switch-field", class.as_deref());

    let toggle = move |_| {
        if disabled.get_untracked() {
            return;
        }
        let next = !checked.get_untracked();
        checked.set(next);
        if let Some(callback) = on_change {
            callback.run(next);
        }
    };

    view! {
        <div class=classes>
            <button
                type="button"
                role="switch"
                class="ui-switch"
                class:is-checked=move || checked.get()
                aria-checked=move || checked.get().to_string()
                disabled=move || disabled.get()
                on:click=toggle
            >
                <span class="ui-switch__handle"></span>
            </button>
            {label.map(|text| view! { <span class="ui-switch-field__label">{text}</span> })}
        </div>
    }
}
