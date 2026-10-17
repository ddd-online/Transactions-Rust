//! 记账 · **看板**子功能：结论式回顾（打开即答，不做配置）。
//!
//! ## 它和「分析」的区别（别把它做成第二个分析页）
//!
//! * 分析：**提问式** —— 自己配曲线条件、选区间，由图表回答；
//! * 看板：**结论式** —— 固定四张卡 + 折线 + 构成，翻到哪一期就读哪一期。
//!
//! 所以这里**没有**曲线条件、没有区间选择器、没有图表管理：只有一个「‹ 期间 ›」
//! 与「月 / 年」两档。多一个配置项，这个页就退化成分析页。
//!
//! ## 口径都在服务端
//!
//! 日常 / 固定 / 结余的算法与同比截断只实现一份，在
//! `tr_service::accounting_overview`（native 单测覆盖）。本页只负责排版与格式化，
//! 不在界面层重算任何一个数字 —— 否则卡片上的数和图上的线会各说各话。
//!
//! ## 空态
//!
//! 一期没有记录时四张卡都显示 `—`（不是 `¥0`）：「花了 0」和「没记账」不是一回事。

use leptos::prelude::*;
use tr_domain::dto::{AccountingOverviewDto, OverviewPeriodDto, OverviewSliceDto};
use tr_draw::calendar;
use tr_draw::section::{section_state, SectionState};

use crate::api;
use crate::components::ui::{
    series_color, ChartConfig, ChartSeries, Empty, FeaturePage, LineChart, Segmented,
    SegmentedOption,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::Icon;
use crate::store::AppStores;

use super::accounting::{SubFunction, SubFunctionRail};

/// 「其他」这一片的名字（服务端合并出来的，界面只负责标灰）。
/// 记账页的「看板」子功能。
#[component]
pub fn DashboardSub(sub: RwSignal<SubFunction>) -> impl IntoView {
    let stores = AppStores::global();

    // 期间：kind（month / year）+ 锚点（空串 = 当期，由服务端按今天定）
    let kind = RwSignal::new("month".to_string());
    let anchor = RwSignal::new(String::new());
    let tab = RwSignal::new("category".to_string());

    let data = RwSignal::new(Option::<AccountingOverviewDto>::None);
    let loading = RwSignal::new(false);
    let loaded = RwSignal::new(false);
    let failed = RwSignal::new(Option::<String>::None);

    // 账本 / 期间变化 → 拉一次；同一个键不重复请求
    Effect::new(move |prev: Option<String>| {
        let ledger_id = stores.current_ledger_id.get();
        let period_kind = kind.get();
        let period_key = anchor.get();
        let key = format!("{ledger_id}|{period_kind}|{period_key}");
        if prev.as_deref() == Some(key.as_str()) {
            return key;
        }
        if ledger_id.is_empty() {
            data.set(None);
            failed.set(None);
            loaded.set(true);
            return key;
        }
        loading.set(true);
        failed.set(None);
        leptos::task::spawn_local(async move {
            match api::tr::overview(&ledger_id, &period_kind, &period_key).await {
                Ok(reply) => data.set(Some(reply)),
                Err(error) => {
                    failed.set(Some("查询看板失败".to_string()));
                    notify_error("查询看板失败", &error);
                }
            }
            loading.set(false);
            loaded.set(true);
        });
        key
    });

    // 往回翻永远可以；往前翻到"当期"就停（再往后是未来，没有意义）
    let step = move |delta: i32| {
        let Some(reply) = data.get_untracked() else {
            return;
        };
        let period_kind = kind.get_untracked();
        let next = shift_key(&reply.period.key, &period_kind, delta);
        anchor.set(next);
    };

    // 换粒度就回到当期：从「2026年10月」按年翻会得到「2019年」这种没人要的期间
    let change_kind = move |value: String| {
        anchor.set(String::new());
        kind.set(value);
    };

    let is_current = move || {
        data.get()
            .map(|reply| reply.period.is_current)
            .unwrap_or(false)
    };
    let period_label = move || {
        data.get()
            .map(|reply| reply.period.label)
            .unwrap_or_default()
    };

    let toolbar = view! {
        <div class="dash-period">
            <button
                type="button"
                class="dash-period__step"
                title="上一期"
                aria-label="上一期"
                on:click=move |_| step(-1)
            >
                "‹"
            </button>
            <span class="dash-period__label">{period_label}</span>
            <button
                type="button"
                class="dash-period__step"
                title="下一期"
                aria-label="下一期"
                disabled=is_current
                on:click=move |_| step(1)
            >
                "›"
            </button>
            <Segmented
                value=kind
                options=vec![
                    SegmentedOption::new("month", "月"),
                    SegmentedOption::new("year", "年"),
                ]
                on_change=change_kind
            />
        </div>
    }
    .into_any();

    let content = view! {
        <div class="dash">
            {move || {
                let state = section_state(
                    data.get().is_none(),
                    loading.get(),
                    loaded.get(),
                    failed.get().as_deref(),
                );
                match state {
                    SectionState::Loading => {
                        view! { <div class="dash-status">"正在加载…"</div> }.into_any()
                    }
                    SectionState::Failed => {
                        view! {
                            <Empty
                                title="看板加载失败"
                                description="切一期或重开记账页再试；账目本身不受影响"
                                icon=Icon::Inbox
                            />
                        }
                            .into_any()
                    }
                    SectionState::Empty => {
                        view! {
                            <Empty
                                title="这个账本还没有记录"
                                description="记下第一笔之后，这里会长出四张卡和两条线"
                                icon=Icon::Inbox
                            />
                        }
                            .into_any()
                    }
                    SectionState::Ready => data
                        .get()
                        .map(|reply| dashboard_body(reply, tab))
                        .unwrap_or_else(|| ().into_any()),
                }
            }}
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=super::accounting::PAGE_TITLE
            class="dashboard-page"
            rail=view! { <SubFunctionRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
        />
    }
}

/// 四张卡 + 折线 + 构成。
fn dashboard_body(reply: AccountingOverviewDto, tab: RwSignal<String>) -> AnyView {
    let cards = view! {
        <div class="dash-cards">
            {card(
                "日常支出",
                reply.cards.daily_expense,
                yoy_note(reply.cards.daily_expense, reply.cards.daily_expense_last_year),
                Tone::Normal,
            )}
            {card(
                "固定支出",
                reply.cards.fixed_expense,
                fixed_note(&reply.cards.fixed_breakdown),
                Tone::Muted,
            )}
            {card(
                "收入",
                reply.cards.income,
                yoy_note(reply.cards.income, reply.cards.income_last_year),
                Tone::Normal,
            )}
            {card(
                "结余",
                reply.cards.balance_ytd,
                balance_note(&reply.period),
                Tone::Signed(reply.cards.balance_ytd),
            )}
        </div>
    }
    .into_any();

    let chart = chart_panel(&reply);

    let composition = composition_panel(
        reply.composition.by_category.clone(),
        reply.composition.by_tag.clone(),
        tab,
    );

    // 外层 `.dash` 由 `content` 提供（它才是吃掉整页高度的那个 flex 列），
    // 这里不再套一层 —— 两层 `.dash` 会让内层的 `flex: 1` 失去参照。
    view! {
        {cards}
        <div class="dash-body">
            {chart}
            {composition}
        </div>
    }
    .into_any()
}

/// 卡片色调：金额的正负要不要上色。
enum Tone {
    /// 支出/收入：不上色（支出大不是"错"，收入大也不是"对"）
    Normal,
    /// 固定支出：整体弱化（它不受当月行为影响）
    Muted,
    /// 结余：按正负上色
    Signed(Option<i64>),
}

fn card(label: &str, value: Option<i64>, note: String, tone: Tone) -> AnyView {
    let text = value
        .map(format::grouped_amount)
        .unwrap_or_else(|| "—".to_string());
    let tone_class = match tone {
        Tone::Normal => "",
        Tone::Muted => " dash-card--muted",
        Tone::Signed(Some(amount)) if amount > 0 => " dash-card--positive",
        Tone::Signed(Some(amount)) if amount < 0 => " dash-card--negative",
        Tone::Signed(_) => "",
    };
    let class = format!("dash-card{tone_class}");
    view! {
        <article class=class>
            <p class="dash-card__label">{label.to_string()}</p>
            <p class="dash-card__value">{text}</p>
            <p class="dash-card__note">{note}</p>
        </article>
    }
    .into_any()
}

/// 结余卡的范围文案。
///
/// 结余是**年度口径**（收入 − 支出，转账不计入），所以范围要跟着这一期所在的年份走：
/// 翻到 2025 年 3 月时写「2025 年 1 月至今」，不能写「今年」—— 那会把去年的数说成今年的。
fn balance_note(period: &OverviewPeriodDto) -> String {
    let scope = if period.kind == "year" {
        if period.is_current_year {
            "本年".to_string()
        } else {
            format!("{} 年", period.year)
        }
    } else if period.is_current_year {
        "今年至今".to_string()
    } else {
        format!("{} 年 1 月至今", period.year)
    };
    format!("{scope}：收入 − 支出（转账不计入）")
}

/// 同比文案：去年同月同期没有记录时给「无可比数据」，不写 `0%`。
fn yoy_note(current: Option<i64>, last_year: Option<i64>) -> String {
    match (current, last_year) {
        (Some(current), Some(previous)) if previous > 0 => {
            let percent = (current - previous) as f64 / previous as f64 * 100.0;
            format!("同比 {}", format::optional_signed_percent(Some(percent)))
        }
        _ => "去年同期无可比数据".to_string(),
    }
}

/// 固定支出卡片的副标：它自己的构成（房贷 / 还款 …）。
fn fixed_note(breakdown: &[OverviewSliceDto]) -> String {
    if breakdown.is_empty() {
        return "未标记（到「标签」页勾选）".to_string();
    }
    breakdown
        .iter()
        .map(|slice| format!("{} {}", slice.label, format::grouped_amount(slice.amount)))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn chart_panel(reply: &AccountingOverviewDto) -> AnyView {
    let labels = reply.series.labels.clone();
    let current = reply.series.current.clone();
    let last_year = reply.series.last_year.clone();
    let (title, hint) = if reply.series.mode == "monthly" {
        ("每月支出（日常）", "与去年同月逐月对比")
    } else {
        ("累计支出（日常）", "去年同期按同样的天数比")
    };
    let has_data =
        current.iter().any(|value| *value != 0.0) || last_year.iter().any(|value| *value != 0.0);

    let categories = Signal::derive(move || labels.clone());
    let series = Signal::derive(move || {
        vec![
            ChartSeries::new("本期", series_color(0), current.clone()),
            ChartSeries::new("去年同期", series_color(1), last_year.clone()),
        ]
    });

    view! {
        <section class="dash-panel dash-panel--chart">
            <header class="dash-panel__head">
                <h2 class="dash-panel__title">{title}</h2>
                <span class="dash-panel__hint">{hint}</span>
            </header>
            <Show
                when=move || has_data
                fallback=|| {
                    view! { <div class="dash-panel__empty">"本期还没有记录"</div> }
                }
            >
                <LineChart
                    categories=categories
                    series=series
                    config=ChartConfig::default().height(240)
                />
            </Show>
        </section>
    }
    .into_any()
}

fn composition_panel(
    by_category: Vec<OverviewSliceDto>,
    by_tag: Vec<OverviewSliceDto>,
    tab: RwSignal<String>,
) -> AnyView {
    // 两档数据都在手上，切换只是换一份切片 —— 所以这里用闭包读 `tab`，
    // 不再在渲染时 `get_untracked()`（那样点「标签」不会有任何反应）。
    let rows = move || {
        let slices = if tab.get() == "tag" {
            by_tag.clone()
        } else {
            by_category.clone()
        };
        let total: i64 = slices.iter().map(|slice| slice.amount).sum();
        if total == 0 {
            return view! { <div class="dash-panel__empty">"本期没有日常支出"</div> }.into_any();
        }
        let rows = slices
            .into_iter()
            .map(|slice| {
                let percent = slice.amount as f64 / total as f64 * 100.0;
                let width = format!("width: {:.1}%", percent.clamp(0.0, 100.0));
                let percent_text = if percent >= 1.0 {
                    format!("{percent:.0}%")
                } else if percent > 0.0 {
                    "<1%".to_string()
                } else {
                    "0%".to_string()
                };
                let row_class = if slice.is_other {
                    "dash-mix__row dash-mix__row--other"
                } else {
                    "dash-mix__row"
                };
                let label_title = format!("{} · {} 笔", slice.label, slice.count);
                view! {
                    <li class=row_class>
                        <span class="dash-mix__label" title=label_title>
                            {slice.label.clone()}
                        </span>
                        <span class="dash-mix__bar">
                            <span class="dash-mix__bar-fill" style=width></span>
                        </span>
                        <span class="dash-mix__amount">{format::amount(slice.amount)}</span>
                        <span class="dash-mix__percent">{percent_text}</span>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="dash-mix">{rows}</ul> }.into_any()
    };

    view! {
        <section class="dash-panel dash-panel--mix">
            <header class="dash-panel__head">
                <h2 class="dash-panel__title">"支出构成"</h2>
                <Segmented
                    value=tab
                    options=vec![
                        SegmentedOption::new("category", "分类"),
                        SegmentedOption::new("tag", "标签"),
                    ]
                />
            </header>
            {rows}
        </section>
    }
    .into_any()
}

/// 期间加减：`2026-10` 减一月是 `2026-09`，`2026` 减一年是 `2025`。
fn shift_key(key: &str, period_kind: &str, delta: i32) -> String {
    if period_kind == "year" {
        let year: i32 = key.parse().unwrap_or(1970);
        return (year + delta).to_string();
    }
    match calendar::parse_year_month(key) {
        Some((year, month)) => {
            let (year, month) = calendar::add_months(year, month, delta);
            format!("{year}-{month:02}")
        }
        None => key.to_string(),
    }
}
