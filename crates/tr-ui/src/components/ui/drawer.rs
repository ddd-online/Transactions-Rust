//! 抽屉 —— 对应原 `a-drawer`（右侧滑出，用于筛选面板等次要流程）。
//!
//! 结构与 [`crate::components::ui::Modal`] 同源（遮罩 + 头部 + 内容 + 底栏），
//! 只是从右侧进入、`footer` 可选。`children` 用 [`ChildrenFn`]：`Show` 开关会重建视图树。

use leptos::prelude::*;

use crate::icons::{self, Icon};

#[component]
pub fn Drawer(
    /// 是否打开
    #[prop(into)]
    open: Signal<bool>,
    /// 标题
    #[prop(into)]
    title: String,
    /// 宽度（px），默认 420
    #[prop(optional)]
    width: Option<u32>,
    /// 是否显示底部按钮栏（默认隐藏）
    #[prop(optional)]
    footer: bool,
    /// 确定按钮文案，默认「确定」
    #[prop(optional, into)]
    ok_text: Option<String>,
    /// 确定回调
    #[prop(optional, into)]
    on_ok: Option<UnsyncCallback<()>>,
    /// 关闭回调（遮罩 / 关闭按钮 / 取消按钮共用）
    #[prop(optional, into)]
    on_close: Option<UnsyncCallback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let ok_text = ok_text.unwrap_or_else(|| "确定".to_string());
    let content_style = width
        .map(|width| format!("width: {width}px;"))
        .unwrap_or_default();

    view! {
        <Show when=move || open.get()>
            <div class="ui-drawer__mask">
                <aside class="ui-drawer__content" style=content_style.clone()>
                    <div class="ui-drawer__header">
                        <h3 class="ui-drawer__title">{title.clone()}</h3>
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
                    <div class="ui-drawer__body">{children()}</div>
                    <div class="ui-drawer__footer" class:is-hidden=move || !footer>
                        <button
                            type="button"
                            class="ui-btn ui-btn--secondary ui-btn--sm"
                            on:click=move |_| {
                                if let Some(callback) = on_close {
                                    callback.run(());
                                }
                            }
                        >
                            "取消"
                        </button>
                        <button
                            type="button"
                            class="ui-btn ui-btn--primary ui-btn--sm"
                            on:click=move |_| {
                                if let Some(callback) = on_ok {
                                    callback.run(());
                                }
                            }
                        >
                            {ok_text.clone()}
                        </button>
                    </div>
                </aside>
            </div>
        </Show>
    }
}
