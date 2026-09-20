//! 下拉菜单（点击触发、点项回调、支持分隔线与危险项）。
//!
//! 与 [`crate::components::ui::Select`] 的区别：这里没有"当前值"的概念，
//! 只是把一组操作收进一个浮层（用于表格行的「更多」、模板项的「…」）。
//!
//! ```
//! <Dropdown items=menu_items on_select=move |key| match key.as_str() { … }>
//!     <Button variant=ButtonVariant::Text size=ButtonSize::Small>"更多"</Button>
//! </Dropdown>
//! ```

use super::backdrop;
use super::with_class;
use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;

/// 一个菜单项。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DropdownItem {
    /// 回调里回传的标识；分隔线项忽略该值
    pub key: String,
    pub label: String,
    /// 分隔线（`label` / `key` 均忽略）
    pub divider: bool,
    /// 危险项（红色文字）
    pub danger: bool,
    pub disabled: bool,
}

impl DropdownItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 一条分隔线。
    pub fn separator() -> Self {
        Self {
            divider: true,
            ..Self::default()
        }
    }
}

#[component]
pub fn Dropdown(
    /// 菜单项（响应式：调用方可以在打开前重建列表）
    #[prop(into)]
    items: Signal<Vec<DropdownItem>>,
    /// 选中回调（参数是 `key`）
    #[prop(optional, into)]
    on_select: Option<UnsyncCallback<String>>,
    /// 面板对齐：`true` 时右对齐触发元素
    #[prop(optional)]
    align_end: bool,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);

    let mut classes = String::from("ui-dropdown");
    if align_end {
        classes.push_str(" ui-dropdown--end");
    }
    let classes = with_class(&classes, class.as_deref());

    view! {
        <div class=classes class:is-open=move || open.get()>
            <span class="ui-dropdown__trigger" on:click=move |_| open.update(|v| *v = !*v)>
                {children()}
            </span>

            <Show when=move || open.get()>
                {backdrop(UnsyncCallback::new(move |()| open.set(false)))}
                <div class="ui-dropdown__panel" role="menu">
                    {move || {
                        items
                            .get()
                            .into_iter()
                            .map(|item| {
                                if item.divider {
                                    return view! { <div class="ui-dropdown__divider"></div> }
                                        .into_any();
                                }
                                let key = item.key.clone();
                                let mut item_class = String::from("ui-dropdown__item");
                                if item.danger {
                                    item_class.push_str(" is-danger");
                                }
                                if item.disabled {
                                    item_class.push_str(" is-disabled");
                                }
                                let label = item.label.clone();
                                let disabled = item.disabled;
                                view! {
                                    <button
                                        type="button"
                                        role="menuitem"
                                        class=item_class
                                        disabled=disabled
                                        on:click=move |_| {
                                            if disabled {
                                                return;
                                            }
                                            open.set(false);
                                            if let Some(callback) = on_select {
                                                callback.run(key.clone());
                                            }
                                        }
                                    >
                                        {label}
                                    </button>
                                }
                                    .into_any()
                            })
                            .collect_view()
                    }}
                </div>
            </Show>
        </div>
    }
}
