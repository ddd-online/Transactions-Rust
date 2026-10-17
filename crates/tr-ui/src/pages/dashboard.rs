//! 记账 · **看板**子功能：结论式回顾（打开即答，不做配置）。
//!
//! ## 它和「分析」的区别（别把它做成第二个分析页）
//!
//! * 分析：**提问式** —— 自己配曲线条件、选区间，由图表回答；
//! * 看板：**结论式** —— 收支卡 + 支出构成卡，翻到哪一期就读哪一期。
//!
//! 所以这里**没有**曲线条件、没有区间选择器、没有图表管理：只有一个「‹ 期间 ›」
//! 与「月 / 年」两档。多一个配置项，这个页就退化成分析页。
//!
//! ## 口径都在服务端
//!
//! 日常 / 固定 / 结余的算法与同比截断只实现一份，在
//! `tr_service::accounting_overview`（native 单测覆盖）。本页只负责排版与格式化，
//! 不在界面层重算任何一个数字 —— 否则卡片上的数和清单里的数会各说各话。
//!
//! ## 空态
//!
//! 一期没有记录时四张卡都显示 `—`（不是 `¥0`）：「花了 0」和「没记账」不是一回事。

use leptos::prelude::*;
use tr_domain::dto::{AccountingOverviewDto, OverviewCardsDto, OverviewSliceDto};
use tr_draw::calendar;
use tr_draw::section::{section_state, SectionState};

use crate::api;
use crate::components::ui::{Card, Empty, FeaturePage, IconButton, Segmented, SegmentedOption};
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
    // 期间字段的展开态：`change_kind` 也要能收起它，所以提到最前面来声明
    let picker_open = RwSignal::new(false);

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
        let next = calendar::shift_period_key(&reply.period.key, &period_kind, delta);
        anchor.set(next);
    };

    // 换粒度就回到当期：从「2026年10月」按年翻会得到「2019年」这种没人要的期间
    let change_kind = move |value: String| {
        picker_open.set(false);
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

    // 期间字段可以点开：翻到很早的月份时，一次点选比连按十几下箭头省事。
    // 面板直接沿用「记录」页时间范围面板的类名（`.ui-time__*`），外观与那套完全一致。
    let picker_year = RwSignal::new(1970_i32);
    let current_key = move || data.get().map(|reply| reply.period.key).unwrap_or_default();
    let open_picker = move |_| {
        let year = current_key()
            .get(0..4)
            .and_then(|text| text.parse::<i32>().ok())
            .unwrap_or(1970);
        picker_year.set(year);
        picker_open.update(|open| *open = !*open);
    };
    // 选完就收面板：这是一个"一次点选"的控件，不是需要确认的表单
    let pick = move |key: String| {
        picker_open.set(false);
        anchor.set(key);
    };

    let picker_panel = move || {
        let year = picker_year.get();
        let selected = current_key();
        if kind.get() == "year" {
            let decade = year.div_euclid(10) * 10;
            let cells = (decade..decade + 10)
                .map(|year| {
                    let key = format!("{year:04}");
                    let is_selected = selected == key;
                    let pick_key = key.clone();
                    view! {
                        <button
                            type="button"
                            class="ui-time__cell"
                            class:is-start=is_selected
                            on:click=move |_| pick(pick_key.clone())
                        >
                            {format!("{year} 年")}
                        </button>
                    }
                })
                .collect_view();
            view! {
                <div class="ui-time__panel-head">
                    {panel_nav("上一个十年", Icon::Left, -10, picker_year)}
                    <span class="ui-time__panel-title">
                        {format!("{decade} – {}", decade + 9)}
                    </span>
                    {panel_nav("下一个十年", Icon::Right, 10, picker_year)}
                </div>
                <div class="ui-time__grid ui-time__grid--year">{cells}</div>
            }
            .into_any()
        } else {
            let cells = (1..=12u32)
                .map(|month| {
                    let key = format!("{year:04}-{month:02}");
                    let is_selected = selected == key;
                    let pick_key = key.clone();
                    view! {
                        <button
                            type="button"
                            class="ui-time__cell"
                            class:is-start=is_selected
                            on:click=move |_| pick(pick_key.clone())
                        >
                            {format!("{month} 月")}
                        </button>
                    }
                })
                .collect_view();
            view! {
                <div class="ui-time__panel-head">
                    {panel_nav("上一年", Icon::Left, -1, picker_year)}
                    <span class="ui-time__panel-title">{format!("{year} 年")}</span>
                    {panel_nav("下一年", Icon::Right, 1, picker_year)}
                </div>
                <div class="ui-time__grid ui-time__grid--month">{cells}</div>
            }
            .into_any()
        }
    };

    // 期间控件：与「记录」页的 `TimeRangePicker` 同一套排法 —— 粒度分段 + 两个翻页图标
    // 按钮 + 一个期间字段。自绘那套（自定义档位按钮 + 分隔线）与页面里其它工具条对不上。
    let toolbar = view! {
        <div class="dash-period">
            <Segmented
                value=kind
                options=vec![
                    SegmentedOption::new("month", "月"),
                    SegmentedOption::new("year", "年"),
                ]
                on_change=change_kind
            />
            <IconButton
                class="ui-icon-btn--bordered dash-period__step"
                label="上一期"
                on_click=UnsyncCallback::new(move |()| step(-1))
            >
                {crate::icons::icon(Icon::Left)}
            </IconButton>
            <div
                class="dash-period__field"
                on:keydown=move |event: leptos::ev::KeyboardEvent| {
                    if event.key() == "Escape" {
                        picker_open.set(false);
                    }
                }
            >
                <button
                    type="button"
                    class="dash-period__trigger"
                    aria-haspopup="dialog"
                    aria-expanded=move || if picker_open.get() { "true" } else { "false" }
                    on:click=open_picker
                >
                    <span class="ui-date-picker__icon">
                        {crate::icons::icon(Icon::ClockCircle)}
                    </span>
                    <span class="dash-period__value" aria-live="polite">
                        {period_label}
                    </span>
                </button>
                <Show when=move || picker_open.get()>
                    {crate::components::ui::backdrop(UnsyncCallback::new(move |()| {
                        picker_open.set(false)
                    }))}
                    <div class="ui-time__panel" role="dialog" aria-label="选择期间">
                        {picker_panel}
                    </div>
                </Show>
            </div>
            <IconButton
                class="ui-icon-btn--bordered dash-period__step"
                label="下一期"
                disabled=Signal::derive(is_current)
                on_click=UnsyncCallback::new(move |()| step(1))
            >
                {crate::icons::icon(Icon::Right)}
            </IconButton>
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
                                description="记下第一笔之后，这里会长出收支和支出构成两张卡"
                                icon=Icon::Inbox
                            />
                        }
                            .into_any()
                    }
                    SectionState::Ready => data
                        .get()
                        .map(dashboard_body)
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

/// 一屏两张卡：收支 + 支出构成。
///
/// 左窄右宽的两栏（`.dash-flow` 是 grid）：左边是"对账单"，右边是排行榜。两栏都贴住版心
/// 左右边缘 —— 卡片按内容定宽时整块会缩在左上角，页面像没画完；两栏等高不必，底边参差
/// 本来就是瀑布该有的样子。
fn dashboard_body(reply: AccountingOverviewDto) -> AnyView {
    let figures = figures_card(reply.cards);
    let composition = composition_card(reply.categories);
    view! {
        <div class="dash-flow">
            {figures}
            {composition}
        </div>
    }
    .into_any()
}

/// 收支卡：三行"标签 + 金额"。
///
/// 卡上只有数字，没有脚注：同比需要先解释"和哪一段比"才不算误读，而在一个期间可以
/// 按月/按年切换的页面上，任何一句标注都会和顶部的期间控件打架（结余尤其 —— 它永远是
/// 年度口径，选「月」的时候也一样）。
fn figures_card(cards: OverviewCardsDto) -> AnyView {
    view! {
        <Card title="收支".to_string() class="dash-card">
            <dl class="dash-statement">
                {figure("支出", cards.expense, "")}
                {figure("收入", cards.income, "")}
                {figure("结余", cards.balance_ytd, signed_class(cards.balance_ytd))}
            </dl>
        </Card>
    }
    .into_any()
}

/// 收支卡里的一行。`class` 只给结余用（正负着色），支出与收入不上色。
fn figure(label: &str, value: Option<i64>, class: &str) -> AnyView {
    let value_class = format!("dash-statement__value {class}");
    view! {
        <div class="dash-statement__row">
            <dt class="dash-statement__label">{label.to_string()}</dt>
            <dd class=value_class>{amount_text(value)}</dd>
        </div>
    }
    .into_any()
}

/// 金额文字：没有数据时给 `—`（「花了 ¥0」和「没记账」不是一回事）。
fn amount_text(value: Option<i64>) -> String {
    value
        .map(format::grouped_amount)
        .unwrap_or_else(|| "—".to_string())
}

/// 结余的正负着色（支出 / 收入不上色：花得多不是"错"，赚得多也不是"对"）。
fn signed_class(value: Option<i64>) -> &'static str {
    match value {
        Some(amount) if amount > 0 => "is-positive",
        Some(amount) if amount < 0 => "is-negative",
        _ => "",
    }
}

fn composition_card(by_category: Vec<OverviewSliceDto>) -> AnyView {
    let total: i64 = by_category.iter().map(|slice| slice.amount).sum();
    let rows = if total == 0 {
        view! { <div class="dash-empty">"本期没有支出"</div> }.into_any()
    } else {
        let rows = by_category
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
                        <span class="dash-mix__amount">{format::grouped_amount(slice.amount)}</span>
                        <span class="dash-mix__percent">{percent_text}</span>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="dash-mix">{rows}</ul> }.into_any()
    };

    view! {
        <Card title="支出构成".to_string() class="dash-card">
            {rows}
        </Card>
    }
    .into_any()
}

/// 期间面板头部的翻页按钮：与「记录」页时间范围面板同一颗（`ui-icon-btn--bordered`），
/// 按 `delta` 加减面板里正在看的年份（月面板一年、年面板十年）。
fn panel_nav(label: &'static str, icon: Icon, delta: i32, year: RwSignal<i32>) -> AnyView {
    view! {
        <IconButton
            class="ui-icon-btn--bordered"
            label=label
            on_click=UnsyncCallback::new(move |()| year.update(|value| *value += delta))
        >
            {crate::icons::icon(icon)}
        </IconButton>
    }
    .into_any()
}
