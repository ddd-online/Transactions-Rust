//! 列表区的四态渲染：**加载中 / 失败 / 空 / 就绪** —— 判据在 `tr_draw::section`，
//! 这里只负责把页面自己的三段（加载 / 失败 / 空）与内容拼起来。
//!
//! 为什么要有它：这条优先级从前在每个页面各判一遍（界面里 11 处
//! `if loading { "正在加载…" } else { "暂无××" }`），于是
//!
//! * **失败被渲染成业务空态** —— 用户被告知"暂无××"，而其实是查询失败了；
//! * **首帧闪一下空态** —— `loading` 在取数 module 的 effect 里才置位，第一次渲染时是 `false`、
//!   结果还是默认值，于是"还没回来"被判成了"确实是空"。
//!
//! 四个槽位用 [`ViewFn`]（可以反复求值的视图工厂，与 [`Empty`](super::Empty) 的 `actions` 同一套写法）——
//! 状态每变一次就重挑一个槽渲染，所以槽必须是"能再求值一次"的东西，而不能是构建好的 `AnyView`。
//!
//! ```ignore
//! <AsyncSection
//!     state=Signal::derive(move || section_state(
//!         cards.value.get().is_empty(),
//!         cards.loading.get(),
//!         cards.loaded.get(),
//!         cards.failed.get().as_deref(),
//!     ))
//!     loading=ViewFn::from(move || view! { <div class="todo-loading">"正在加载…"</div> }.into_any())
//!     empty=ViewFn::from(move || view! { <Empty title="还没有卡片" …/> }.into_any())
//!     content=ViewFn::from(move || view! { <div class="todo-cards">…</div> }.into_any())
//! />
//! ```
//!
//! `loading` 与 `failed` 可以不给：不给就渲染默认的那一行（`Spin` + 「正在加载…」 /
//! `Empty` + 失败文案）。

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;
use tr_draw::section::SectionState;

use super::{Empty, Spin, SpinSize};
use crate::icons::Icon;

/// 四态容器（见模块文档）。
#[component]
pub fn AsyncSection(
    /// 四态判定（`tr_draw::section::section_state` 的结果）
    state: Signal<SectionState>,
    /// 就绪态：正常内容
    content: ViewFn,
    /// 空态：跑完了、没失败、确实没有数据
    empty: ViewFn,
    /// 加载态槽（不给就渲染默认一行「正在加载…」）
    #[prop(optional, into)]
    loading: Option<ViewFn>,
    /// 失败态槽（不给就渲染默认的失败空态：标题「加载失败」+ 失败文案）
    #[prop(optional, into)]
    failed: Option<ViewFn>,
    /// 失败文案（只在用默认失败态时读它）
    #[prop(optional, into)]
    failed_message: Option<Signal<Option<String>>>,
) -> impl IntoView {
    view! {
        {move || match state.get() {
            SectionState::Loading => match loading.as_ref() {
                Some(slot) => slot.run(),
                None => {
                    view! {
                        <div class="ui-async-loading">
                            <Spin spinning=true size=SpinSize::Small />
                            <span>"正在加载…"</span>
                        </div>
                    }
                        .into_any()
                }
            },
            SectionState::Failed => match failed.as_ref() {
                Some(slot) => slot.run(),
                None => {
                    let message = failed_message
                        .as_ref()
                        .and_then(|signal| signal.get())
                        .unwrap_or_default();
                    view! {
                        <Empty
                            title="加载失败"
                            description=message
                            icon=Icon::WarningCircle
                        />
                    }
                        .into_any()
                }
            },
            SectionState::Empty => empty.run(),
            SectionState::Ready => content.run(),
        }}
    }
}
