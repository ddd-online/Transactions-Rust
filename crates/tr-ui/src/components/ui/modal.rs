//! 模态框 —— 16px 圆角 + 大阴影（`--transactions-radius-xl` + `--transactions-shadow-xl`）。
//!
//! 标题栏 / 内容区 / 底栏三段式；遮罩从顶部 96px 开始，
//! 让浮层与顶部窗口控制按钮保持距离，不遮挡它们。
//!
//! `children` 用 [`ChildrenFn`]（可重复调用的 children）：模态内容包在 `Show` 里，
//! 开关时会重建视图树，因此 children 必须能多次求值。
//!
//! 用法：
//! ```ignore
//! <Modal open=show title="创建账本" on_ok=move || { /* ... */ }>
//!     <Input value=name placeholder="请输入账本名称" />
//! </Modal>
//! ```

use leptos::prelude::*;

use crate::icons::{self, Icon};

/// 模态框。
#[component]
pub fn Modal(
    /// 是否打开（`bool`、信号或闭包均可）
    #[prop(into)]
    open: Signal<bool>,
    /// 标题（`&str` / `String` / 信号 / 闭包均可：下单弹窗的标题随交易类型变化）
    #[prop(into)]
    title: Signal<String>,
    /// 内容宽度（px），默认由 CSS 决定（520）
    #[prop(optional)]
    width: Option<u32>,
    /// 是否显示底部按钮栏，默认 `true`
    #[prop(optional)]
    footer: Option<bool>,
    /// 确认按钮文案，默认「确认」；标题已给出动作时传具体动词（如「新增」「保存」）
    #[prop(optional, into)]
    ok_text: Option<Signal<String>>,
    /// 取消按钮文案，默认「取消」
    #[prop(optional, into)]
    cancel_text: Option<Signal<String>>,
    /// 确认按钮加载态
    #[prop(optional, into)]
    ok_loading: Option<Signal<bool>>,
    /// 确认按钮用危险样式（删除类确认框）
    #[prop(optional)]
    ok_danger: bool,
    /// 关闭回调（遮罩、关闭按钮、取消按钮共用）
    #[prop(optional, into)]
    on_close: Option<UnsyncCallback<()>>,
    /// 确认回调
    #[prop(optional, into)]
    on_ok: Option<UnsyncCallback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let ok_loading = ok_loading.unwrap_or_else(|| Signal::derive(|| false));
    let show_footer = footer.unwrap_or(true);
    let ok_text = ok_text.unwrap_or_else(|| Signal::derive(|| "确认".to_string()));
    let cancel_text = cancel_text.unwrap_or_else(|| Signal::derive(|| "取消".to_string()));
    // 宽度是静态 prop，样式串一次算好即可（避免依赖动态 style 的 trait 推断）
    let content_style = width
        .map(|width| format!("width: {width}px;"))
        .unwrap_or_default();

    view! {
        <Show when=move || open.get()>
            <div
                class="ui-modal__mask"
                on:click=move |ev| {
                    // 点遮罩空白处关闭；内容区的点击会冒泡到这里，但那时 `target`
                    // 是内容里的元素，所以不会误关（见 [`is_mask_self_click`]）
                    if is_mask_self_click(&ev) {
                        if let Some(callback) = on_close {
                            callback.run(());
                        }
                    }
                }
            >
                <div class="ui-modal__content" style=content_style.clone()>
                    <div class="ui-modal__header">
                        <h3 class="ui-modal__title">{move || title.get()}</h3>
                        <button
                            type="button"
                            class="ui-modal__close"
                            title="关闭"
                            aria-label="关闭"
                            on:click=move |_| {
                                if let Some(callback) = on_close {
                                    callback.run(());
                                }
                            }
                        >
                            {icons::icon(Icon::Close)}
                        </button>
                    </div>
                    <div class="ui-modal__body">{children()}</div>
                    // 底栏用 class 切换而不是内层 `Show`：内层 `Show` 的 children 是 `Fn`，
                    // 会把 `cancel_text` / `ok_text` 以 move 捕获，导致外层闭包退化成 FnOnce。
                    <div
                        class="ui-modal__footer"
                        class:is-hidden=move || !show_footer
                    >
                        <button
                            type="button"
                            class="ui-btn ui-btn--secondary"
                            on:click=move |_| {
                                if let Some(callback) = on_close {
                                    callback.run(());
                                }
                            }
                        >
                            {move || cancel_text.get()}
                        </button>
                        <button
                            type="button"
                            class="ui-btn ui-btn--primary"
                            class:ui-btn--primary=!ok_danger
                            class:ui-btn--primary-danger=ok_danger
                            disabled=move || ok_loading.get()
                            on:click=move |_| {
                                if let Some(callback) = on_ok {
                                    callback.run(());
                                }
                            }
                        >
                            <Show when=move || ok_loading.get()>
                                <span class="ui-btn__icon ui-spin__indicator">
                                    {icons::icon(Icon::Loading)}
                                </span>
                            </Show>
                            {move || ok_text.get()}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// 点击目标是否就是遮罩**本身**（而不是从弹窗内容冒泡上来的点击）。
///
/// 用 `Object.is(target, currentTarget)` 判定，而不是给内容挂 `stop_propagation`：
/// 本仓库有过一次"`stop_propagation` 吃掉 `Popconfirm` 捕获阶段触发"的真实缺陷
/// （见 AGENTS.md），能不打断事件传播就不打断。抽屉的遮罩复用同一个判定。
pub(super) fn is_mask_self_click(ev: &leptos::ev::MouseEvent) -> bool {
    match (ev.target(), ev.current_target()) {
        (Some(target), Some(current)) => js_sys::Object::is(target.as_ref(), current.as_ref()),
        _ => false,
    }
}
