//! 标签页 —— 顶部横向标签栏 + 内容面板。
//!
//! 对应原 `a-tabs`：受控的 `v-model:activeKey` 换成了 `RwSignal<String>`，
//! 内容面板用 [`TabPane`] 包一层（`Show` 语义，非激活时不渲染）。
//!
//! 之所以不做"`TabPane` 自动注册到 `Tabs`"的 context 方案：设置页的分栏内容差异极大
//! （通用/模板/日记/股票/关于），显式列出 `items` 再按 `active` 分支渲染更直观，
//! 也避免为了一处用法引入一层 context。

use leptos::prelude::*;

/// 一个标签项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabItem {
    pub key: String,
    pub label: String,
}

impl TabItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

impl From<(&str, &str)> for TabItem {
    fn from((key, label): (&str, &str)) -> Self {
        TabItem::new(key, label)
    }
}

/// 标签栏。
#[component]
pub fn Tabs(
    /// 当前激活的 key
    active: RwSignal<String>,
    /// 标签列表
    items: Vec<TabItem>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let mut classes = String::from("ui-tabs");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes role="tablist">
            {items
                .into_iter()
                .map(|item| {
                    // 每个闭包各持一份 key：`view!` 里多个 `move` 闭包共用同一个 String
                    // 会被第一个闭包 move 走，后面的就取不到了。
                    let key_active = item.key.clone();
                    let key_aria = item.key.clone();
                    let key_click = item.key.clone();
                    view! {
                        <button
                            type="button"
                            role="tab"
                            class="ui-tabs__tab"
                            class:is-active=move || active.get() == key_active
                            aria-selected=move || active.get() == key_aria
                            on:click=move |_| active.set(key_click.clone())
                        >
                            {item.label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// 内容面板：`active == key` 时渲染 children。
///
/// `children` 必须是 [`ChildrenFn`]：`Show` 在切换时会重建视图树。
#[component]
pub fn TabPane(
    /// 当前的激活 key（通常直接传 `Tabs` 用的那个信号）
    #[prop(into)]
    active: Signal<String>,
    /// 本面板的 key
    #[prop(into)]
    key: String,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: ChildrenFn,
) -> impl IntoView {
    let extra = class.unwrap_or_default();

    view! {
        <Show when=move || active.get() == key>
            // class 用 `format!` 每次重建：直接把 String move 进 `Show` 的 children 闭包
            // 会让它退化成 `FnOnce`（`Show` 要求 `Fn`）。
            <div class=format!("ui-tab-pane {extra}") role="tabpanel">
                {children()}
            </div>
        </Show>
    }
}
