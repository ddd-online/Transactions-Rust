//! 记账页（顶级功能）—— **记录 / 分析 / 标签 / 模板** 四个子功能共用一个版心与左侧图标条。
//!
//! 版心骨架见 `components/ui/feature_page.rs`：左侧 `.page-rail` 是**子功能图标条**
//! （不是内容里的栏目），点图标切换子功能；四个子功能共用标题栏，标题固定为「记账」。
//!
//! 四个子功能的实现各在自己的模块里，都只负责"工具栏 + 内容区（+ 可选底栏）"：
//! * [`crate::pages::transactions::RecordSub`]：记录（列表 + 记一笔/编辑/关联 + 筛选 + 排序）
//! * [`crate::pages::data_analysis::AnalysisSub`]：分析（图表列表 + 曲线条件 + 自绘 SVG）
//! * [`crate::pages::category_tag::TagSub`]：标签（分类 / 标签两栏 + 拖拽排序）
//! * [`crate::pages::templates::TemplateSub`]：模板（消费模板列表 + 新建 / 删除 / 拖拽排序）
//!
//! 切换子功能会**重建**该子功能的视图（信号随组件 owner 一起释放），因此来回切会重新拉数据 ——
//! 与"切一个页面"的行为一致，不会留下一份隐形的旧状态。

use leptos::prelude::*;

use crate::icons::{self, Icon};

use super::category_tag::TagSub;
use super::data_analysis::AnalysisSub;
use super::templates::TemplateSub;
use super::transactions::RecordSub;

/// 页面标题（固定文案，改动即影响界面）。侧栏条目名也用这一个来源。
pub const PAGE_TITLE: &str = "记账";

/// 记账的四个子功能。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubFunction {
    /// 记录：消费记录列表
    Record,
    /// 分析：图表（原顶级功能「数据分析」，已迁入本页并更名）
    Analysis,
    /// 标签：分类与标签
    Tag,
    /// 模板：消费模板
    Template,
}

impl SubFunction {
    /// 图标条顺序（顺序即渲染顺序）。
    pub const ALL: [SubFunction; 4] = [Self::Record, Self::Analysis, Self::Tag, Self::Template];

    /// 子功能名 —— 同时用作悬停提示与 `aria-label`（也是 fixtures 点它的可访问名）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Record => "记录",
            Self::Analysis => "分析",
            Self::Tag => "标签",
            Self::Template => "模板",
        }
    }

    /// 图标条上的图标。
    pub fn icon(self) -> Icon {
        match self {
            Self::Record => Icon::Transaction,
            Self::Analysis => Icon::LineChart,
            Self::Tag => Icon::Tag,
            Self::Template => Icon::FileText,
        }
    }
}

/// 记账页：只负责"当前是哪个子功能"，版心与图标条交给子功能自己渲染。
#[component]
pub fn AccountingPage() -> impl IntoView {
    let sub = RwSignal::new(SubFunction::Record);
    view! {
        {move || match sub.get() {
            SubFunction::Record => view! { <RecordSub sub=sub /> }.into_any(),
            SubFunction::Analysis => view! { <AnalysisSub sub=sub /> }.into_any(),
            SubFunction::Tag => view! { <TagSub sub=sub /> }.into_any(),
            SubFunction::Template => view! { <TemplateSub sub=sub /> }.into_any(),
        }}
    }
}

/// 子功能图标条 —— `FeaturePage` 的 `rail` 插槽内容（外层 `.page-rail` 由它渲染）。
#[component]
pub fn SubFunctionRail(sub: RwSignal<SubFunction>) -> impl IntoView {
    view! {
        <nav class="page-rail-nav" aria-label="记账子功能">
            {SubFunction::ALL
                .iter()
                .map(|item| {
                    let value = *item;
                    view! {
                        <button
                            type="button"
                            class="page-rail-btn"
                            class:is-active=move || sub.get() == value
                            title=item.label()
                            aria-label=item.label()
                            on:click=move |_| sub.set(value)
                        >
                            <span class="page-rail-btn-icon">{icons::icon(item.icon())}</span>
                        </button>
                    }
                })
                .collect_view()}
        </nav>
    }
}
