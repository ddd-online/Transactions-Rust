//! 气泡确认框 —— 对应原 `a-popconfirm`。
//!
//! 行为：点击触发元素弹出小气泡（标题 + 可选描述 + 取消/确定），点面板外的透明遮罩关闭。
//! 浮层用绝对定位 + `--transactions-shadow-lg`，与 `ui-select` 的下拉面板同一套手法。
//!
//! ```
//! <Popconfirm title="确定删除这条记录吗？" on_confirm=move || do_delete()>
//!     <Button variant=ButtonVariant::TextDanger>"删除"</Button>
//! </Popconfirm>
//! ```

use leptos::prelude::*;

use crate::icons::{self, Icon};

#[component]
pub fn Popconfirm(
    /// 标题（必填）
    #[prop(into)]
    title: String,
    /// 补充说明
    #[prop(optional, into)]
    description: Option<String>,
    /// 确认按钮文案，默认「确定」
    #[prop(optional, into)]
    ok_text: Option<String>,
    /// 取消按钮文案，默认「取消」
    #[prop(optional, into)]
    cancel_text: Option<String>,
    /// 是否显示取消按钮（原 `a-popconfirm` 的 `:show-cancel="false"` 用法）
    #[prop(default = true)]
    show_cancel: bool,
    /// 确认回调（气泡会自动关闭）
    #[prop(optional, into)]
    on_confirm: Option<UnsyncCallback<()>>,
    /// 触发元素里的定位修正类名（例如右对齐的 `ui-popconfirm--end`）
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let ok_text = ok_text.unwrap_or_else(|| "确定".to_string());
    let cancel_text = cancel_text.unwrap_or_else(|| "取消".to_string());

    let mut classes = String::from("ui-popconfirm");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-open=move || open.get()>
            // 触发挂**捕获阶段**：调用方常在子元素上写 `stop_propagation()`（例如列表项里的删除按钮，
            // 为了不触发整行的"选中"）。如果这里挂在冒泡阶段，那个 stop_propagation 会把点击吃掉，
            // 气泡永远弹不出来 —— 删除图表/删除事件/删除关联交易三处都因此失效过（点删除毫无反应）。
            <span class="ui-popconfirm__trigger" on:click:capture=move |_| open.update(|v| *v = !*v)>
                {children()}
            </span>

            <Show when=move || open.get()>
                <div class="ui-select__backdrop" on:click=move |_| open.set(false)></div>
                <div class="ui-popconfirm__panel" role="dialog">
                    <div class="ui-popconfirm__header">
                        {icons::icon(Icon::WarningCircle)}
                        <span class="ui-popconfirm__title">{title.clone()}</span>
                    </div>
                    {description
                        .clone()
                        .map(|text| view! { <p class="ui-popconfirm__description">{text}</p> })}
                    <div class="ui-popconfirm__actions">
                        // 用 `is-hidden` 而不是嵌套 `Show`：`Show` 的 children 必须是 `Fn`，
                        // 而 `cancel_text` 一旦被闭包 move 进去就会让整段退化成 `FnOnce`。
                        <button
                            type="button"
                            class="ui-btn ui-btn--secondary ui-btn--sm"
                            class:is-hidden=move || !show_cancel
                            on:click=move |_| open.set(false)
                        >
                            {cancel_text.clone()}
                        </button>
                        <button
                            type="button"
                            class="ui-btn ui-btn--primary ui-btn--sm"
                            on:click=move |_| {
                                open.set(false);
                                if let Some(callback) = on_confirm {
                                    callback.run(());
                                }
                            }
                        >
                            {ok_text.clone()}
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
