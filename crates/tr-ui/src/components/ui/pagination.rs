//! 分页 —— 从 `pages/transactions.rs` 抽出复用（含每页条数）。
//!
//! 三个入参：
//! * `page`：当前页
//! * `total_pages`：由调用方自己算出
//! * `page_size`（`None` 时隐藏每页条数控件）
//!
//! 页码收敛规则：首页、末页、当前页 ±1 必显，其余折叠为 `…`；
//! 靠近两端时多显示几个，避免出现 `1 … 2` 这类空洞。

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;

use crate::icons::{self, Icon};

/// 每页条数可选项。
pub const PAGE_SIZE_OPTIONS: [i32; 6] = [15, 20, 30, 50, 100, 200];

/// 页码槽位：`None` 表示省略号。
pub fn page_slots(current: i32, pages: i32) -> Vec<Option<i32>> {
    let pages = pages.max(1);
    let current = current.clamp(1, pages);
    let mut slots: Vec<Option<i32>> = Vec::new();
    for candidate in 1..=pages {
        let visible = candidate == 1
            || candidate == pages
            || (candidate - current).abs() <= 1
            || (current <= 3 && candidate <= 5)
            || (current >= pages - 2 && candidate >= pages - 4);
        if visible {
            slots.push(Some(candidate));
        } else if !slots.last().map(|slot| slot.is_none()).unwrap_or(false) {
            slots.push(None);
        }
    }
    slots
}

/// 分页控件（上一页 / 页码 / 下一页 / 每页条数）。
#[component]
pub fn Pagination(
    /// 当前页（1 起）
    page: RwSignal<i32>,
    /// 总页数（<= 0 表示没有数据，此时两个箭头都禁用）
    #[prop(into)]
    total_pages: Signal<i32>,
    /// 每页条数；给了才渲染下拉选择器
    #[prop(optional)]
    page_size: Option<RwSignal<i32>>,
    /// 每页条数可选项
    #[prop(optional)]
    page_size_options: Option<Vec<i32>>,
    /// 禁用（例如加载中）
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("tr-footer-controls");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    let size_options = page_size_options.unwrap_or_else(|| PAGE_SIZE_OPTIONS.to_vec());

    view! {
        <div class=classes>
            <div class="tr-pagination">
                <button
                    type="button"
                    class="tr-pagination__item"
                    title="上一页"
                    aria-label="上一页"
                    disabled=move || page.get() <= 1 || disabled.get()
                    on:click=move |_| page.update(|value| *value = (*value - 1).max(1))
                >
                    {icons::icon(Icon::Left)}
                </button>
                {move || {
                    let current = page.get();
                    let pages = total_pages.get().max(1);
                    page_slots(current, pages)
                        .into_iter()
                        .map(move |slot| match slot {
                            Some(target) => {
                                view! {
                                    <button
                                        type="button"
                                        class="tr-pagination__item"
                                        class:is-active=target == current
                                        on:click=move |_| page.set(target)
                                    >
                                        {target}
                                    </button>
                                }
                                    .into_any()
                            }
                            None => {
                                view! { <span class="tr-pagination__ellipsis">"…"</span> }.into_any()
                            }
                        })
                        .collect_view()
                }}
                <button
                    type="button"
                    class="tr-pagination__item"
                    title="下一页"
                    aria-label="下一页"
                    disabled=move || {
                        let pages = total_pages.get();
                        pages <= 0 || page.get() >= pages || disabled.get()
                    }
                    on:click=move |_| {
                        let pages = total_pages.get_untracked();
                        if pages > 0 {
                            page.update(|value| *value = (*value + 1).min(pages));
                        }
                    }
                >
                    {icons::icon(Icon::Right)}
                </button>
            </div>
            {page_size
                .map(|page_size| {
                    view! {
                        <select
                            class="tr-pagination__size"
                            title="每页条数"
                            prop:value=move || page_size.get().to_string()
                            on:change=move |ev| {
                                if let Ok(size) = event_target_value(&ev).parse::<i32>() {
                                    if size > 0 {
                                        page_size.set(size);
                                        page.set(1);
                                    }
                                }
                            }
                        >
                            {size_options
                                .iter()
                                .map(|size| {
                                    view! {
                                        <option value=size.to_string()>
                                            {format!("{size} 条/页")}
                                        </option>
                                    }
                                })
                                .collect_view()}
                        </select>
                    }
                })}
        </div>
    }
}
