//! 拖拽排序 —— HTML5 draggable，零 JS 库依赖。
//!
//! 拖拽结束后把**新顺序**交给调用方，由调用方逐项调用 `*_update_sort`
//! （`category_update_sort` / `tag_update_sort` / `template_update_sort`）。
//! 本组件只负责"从哪儿拖到哪儿"，不做任何持久化。
//!
//! ```
//! let drag = DragSortState::new();
//! // 列表容器里逐个渲染：
//! <DragSortItem index=i state=drag on_drop=move |(from, to)| move_item(from, to)>
//!     <div class="cell">{…}</div>
//! </DragSortItem>
//! ```
//!
//! 为什么不用 `PointerEvent` 自绘拖影：本项目只需要"上下换位"，
//! 原生 `draggable` 已经给出 dragstart / dragover / drop / dragend 四个钩子，
//! 自绘反而要接管滚动、命中测试与指针捕获，收益不成比例。

use super::with_class;
use leptos::prelude::*;

/// 一次拖拽排序的共享状态（列表容器建一份，所有子项共用）。
#[derive(Clone, Copy)]
pub struct DragSortState {
    /// 正在被拖拽的下标
    pub from: RwSignal<Option<usize>>,
    /// 当前悬停的下标（用于绘制插入位置指示线）
    pub over: RwSignal<Option<usize>>,
}

impl DragSortState {
    pub fn new() -> Self {
        Self {
            from: RwSignal::new(None),
            over: RwSignal::new(None),
        }
    }

    /// 该项是否正在被拖拽。
    pub fn is_dragging(&self, index: usize) -> bool {
        self.from.get() == Some(index)
    }

    /// 该项是否处于拖拽落点。
    pub fn is_over(&self, index: usize) -> bool {
        self.over.get() == Some(index) && self.from.get() != Some(index)
    }

    /// 清理状态（drop / dragend 都要调用，否则指示线会残留）。
    pub fn reset(&self) {
        self.from.set(None);
        self.over.set(None);
    }
}

impl Default for DragSortState {
    fn default() -> Self {
        Self::new()
    }
}

/// 可拖拽的列表项。
#[component]
pub fn DragSortItem(
    /// 本项在列表里的下标
    index: usize,
    /// 共享拖拽状态
    state: DragSortState,
    /// 拖拽结束回调：`(from, to)`；`from == to` 时不会触发
    #[prop(into)]
    on_drop: UnsyncCallback<(usize, usize)>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let classes = with_class("ui-drag-item", class.as_deref());

    view! {
        <div
            class=classes
            class:is-dragging=move || state.is_dragging(index)
            class:is-over=move || state.is_over(index)
            draggable="true"
            on:dragstart=move |ev: web_sys::DragEvent| {
                state.from.set(Some(index));
                // Firefox 要求 dragstart 里写入数据，拖拽才会真正开始；
                // WebView2 不强制，但写上更稳（也让"拖到别处"不会变成复制）。
                if let Some(transfer) = ev.data_transfer() {
                    let _ = transfer.set_data("text/plain", &index.to_string());
                    transfer.set_effect_allowed("move");
                }
            }
            on:dragover=move |ev: web_sys::DragEvent| {
                ev.prevent_default();
                if let Some(transfer) = ev.data_transfer() {
                    transfer.set_drop_effect("move");
                }
                state.over.set(Some(index));
            }
            on:dragleave=move |_| {
                if state.over.get_untracked() == Some(index) {
                    state.over.set(None);
                }
            }
            on:drop=move |ev: web_sys::DragEvent| {
                ev.prevent_default();
                let from = state.from.get_untracked();
                state.reset();
                if let Some(from) = from {
                    if from != index {
                        on_drop.run((from, index));
                    }
                }
            }
            on:dragend=move |_| state.reset()
        >
            {children()}
        </div>
    }
}
