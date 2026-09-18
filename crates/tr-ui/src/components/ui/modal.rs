//! 模态框 —— 16px 圆角 + 大阴影（`--transactions-radius-xl` + `--transactions-shadow-xl`）。
//!
//! 对应原 `a-modal`：标题栏 / 内容区 / 底栏三段式；遮罩从顶部 96px 开始，
//! 与原实现 `notification.config({ top: 96 })` 的层级观感一致，也不遮挡顶部窗口控制按钮。
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
    /// 确认按钮文案，默认「确定」（同样接受闭包：编辑成交弹窗会在保存中改文案）
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
    let ok_text = ok_text.unwrap_or_else(|| Signal::derive(|| "确定".to_string()));
    let cancel_text = cancel_text.unwrap_or_else(|| Signal::derive(|| "取消".to_string()));
    // 宽度是静态 prop，样式串一次算好即可（避免依赖动态 style 的 trait 推断）
    let content_style = width
        .map(|width| format!("width: {width}px;"))
        .unwrap_or_default();

    view! {
        <Show when=move || open.get()>
            <div class="ui-modal__mask">
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
                            class="ui-btn ui-btn--secondary ui-btn--sm"
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
                            class="ui-btn ui-btn--sm"
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
