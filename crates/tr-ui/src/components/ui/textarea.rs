//! 多行文本域。
//!
//! 双向绑定与 [`crate::components::ui::Input`] 一致（调用方传 `RwSignal<String>`，
//! 组件内部用 `prop:value` 同步 DOM）。样式在 `ui.css` 的 `.ui-textarea*`。
//!
//! 使用场合：事件正文、本轮复盘、日记正文
//! （日记那里还要更细的等宽字体与撑满高度，用 `class` 覆盖）。
//!
//! 两个**可选**能力，都默认关闭、默认行为一字不变：
//! * `node_ref`：把 DOM 节点交出去 —— 需要"打开就把焦点与光标放进去"的调用方用（复盘弹窗）；
//! * `auto_grow`：按内容长高（`height: auto` → `scrollHeight`，上下限交给调用方的 CSS 的
//!   `min-height` / `max-height`）。覆盖式滚动条在静止时不可见（见 `base.css` 的滚动条段落），
//!   所以"内容高于框"在观感上就是**内容丢了** —— 长正文要么给足高度，要么按内容长。

use super::with_class;
use leptos::html;
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
    /// 外部持有的节点引用（弹窗编辑器要把焦点与光标放进文本域时给）
    #[prop(optional)]
    node_ref: Option<NodeRef<html::Textarea>>,
    /// 按内容长高：量的是 `scrollHeight`，高度上限交给调用方 CSS 的 `max-height`
    #[prop(optional)]
    auto_grow: bool,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));
    // 没给就用内部引用（`auto_grow` 自己要用）
    let control = node_ref.unwrap_or_default();

    let mut classes = String::from("ui-textarea");
    if !resizable {
        classes.push_str(" ui-textarea--fixed");
    }
    let classes = with_class(&classes, class.as_deref());

    if auto_grow {
        let grow = move || {
            let Some(element) = control.get() else {
                return;
            };
            let element: web_sys::HtmlTextAreaElement = element;
            // 走 web-sys 的裸方法（`HtmlElement::style`）：leptos 的 prelude 里
            // `ElementExt::style(&self, S)` 对所有 `AsRef<Element>` 生效，会把这个名字抢走
            let style = web_sys::HtmlElement::style(&element);
            // 先回到 `auto` 才能量到"内容真正需要的高度"，否则只会量到上一次设过的高度
            let _ = style.set_property("height", "auto");
            let _ = style.set_property("height", &format!("{}px", element.scroll_height()));
        };
        // 内容变化 → 重量（读 `value` 只为建立依赖，量高本身不写任何信号）
        Effect::new(move |_| {
            let _ = value.get();
            grow();
        });
        // 窗口尺寸变化 → 换行位置变了，行数也变（窄窗下同一段文字更高）。
        // 用 leptos 的 `window_event_listener`：它给的句柄是 `Send + Sync` 的（`on_cleanup`
        // 收的就是这种），所以弹窗反复挂载时不会像 `forget()` 那样每开一次漏一个监听器
        // —— 那个监听器还会一直按旧的节点量高。
        let handle = window_event_listener(leptos::ev::resize, move |_| grow());
        on_cleanup(move || handle.remove());
    }

    view! {
        <div class=classes class:ui-textarea--disabled=move || disabled.get()>
            <textarea
                class="ui-textarea__control"
                node_ref=control
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
