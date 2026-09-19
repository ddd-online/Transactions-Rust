//! 气泡卡片（点击触发，标题 + 任意内容）。
//!
//! 与 [`crate::components::ui::Popconfirm`] 的区别：这里的内容完全由调用方决定
//! （例如筛选条件的小结、图表曲线的图例说明），没有固定的确认/取消按钮。

use leptos::prelude::*;

#[component]
pub fn Popover(
    /// 面板标题（可省略）
    #[prop(optional, into)]
    title: Option<String>,
    /// 面板内容
    #[prop(optional, into)]
    content: Option<ViewFn>,
    /// 面板对齐：`true` 时右对齐触发元素
    #[prop(optional)]
    align_end: bool,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);

    let mut classes = String::from("ui-popover");
    if align_end {
        classes.push_str(" ui-popover--end");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-open=move || open.get()>
            <span class="ui-popover__trigger" on:click=move |_| open.update(|v| *v = !*v)>
                {children()}
            </span>

            <Show when=move || open.get()>
                <div class="ui-select__backdrop" on:click=move |_| open.set(false)></div>
            </Show>
            // 面板常驻 DOM，用 `is-hidden` 控制显隐：`Show` 的 children 必须是 `Fn`，
            // 而 `content`（`ViewFn`）一旦被闭包 move 进去就退化成 `FnOnce`。
            <div class="ui-popover__panel" class:is-hidden=move || !open.get() role="dialog">
                {title
                    .clone()
                    .map(|text| view! { <p class="ui-popover__title">{text}</p> })}
                <div class="ui-popover__content">{content.map(|content| content.run())}</div>
            </div>
        </div>
    }
}
