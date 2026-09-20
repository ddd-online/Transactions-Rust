//! 多行文本域。
//!
//! 双向绑定与 [`crate::components::ui::Input`] 一致（调用方传 `RwSignal<String>`，
//! 组件内部用 `prop:value` 同步 DOM）。样式在 `ui.css` 的 `.ui-textarea*`。
//!
//! 使用场合：关键事件正文、本轮复盘、日记正文
//! （日记那里还要更细的等宽字体与撑满高度，用 `class` 覆盖）。

use super::with_class;
use leptos::prelude::*;

/// 多行文本域。
#[component]
pub fn Textarea(
    /// 双向绑定的值
    value: RwSignal<String>,
    /// 占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
    /// 可见行数（`rows` 属性）
    #[prop(optional)]
    rows: Option<u32>,
    /// 最大字符数上限
    #[prop(optional)]
    maxlength: Option<u32>,
    /// 是否允许拖动改变大小（默认不允许：编辑区需要稳定高度）
    #[prop(optional)]
    resizable: bool,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 输入回调（`on:input` 之外需要额外副作用时用，例如触发防抖保存）
    #[prop(optional, into)]
    on_input: Option<UnsyncCallback<String>>,
    /// `Ctrl+S` / `Cmd+S` 快捷键回调（日记编辑器用）
    #[prop(optional, into)]
    on_save_shortcut: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-textarea");
    if !resizable {
        classes.push_str(" ui-textarea--fixed");
    }
    let classes = with_class(&classes, class.as_deref());

    view! {
        <div class=classes class:ui-textarea--disabled=move || disabled.get()>
            <textarea
                class="ui-textarea__control"
                placeholder=placeholder
                rows=rows
                maxlength=maxlength
                disabled=move || disabled.get()
                prop:value=move || value.get()
                on:input=move |ev: leptos::ev::Event| {
                    let next = event_target_value(&ev);
                    value.set(next.clone());
                    if let Some(callback) = on_input {
                        callback.run(next);
                    }
                }
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    let is_save = (ev.ctrl_key() || ev.meta_key()) && ev.key() == "s";
                    if is_save {
                        ev.prevent_default();
                        if let Some(callback) = on_save_shortcut {
                            callback.run(());
                        }
                    }
                }
            ></textarea>
        </div>
    }
}
