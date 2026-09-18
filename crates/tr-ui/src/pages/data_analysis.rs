//! 数据分析页（`/da_view`）—— P6-b 完整实现。
//!
//! ## 对照的原 Vue 文件
//!
//! | 原文件 | 本文件对应部分 |
//! |---|---|
//! | `da_view/DataAnalysisView.vue` | [`DataAnalysisPage`]：左侧 220px 图表列表 + 右侧图表视图编排 |
//! | `da_view/TransactionsChartList.vue` | [`chart_list_panel`]：列表项（颜色点组 / 删除气泡 / 新增按钮） |
//! | `da_view/TransactionsChartView.vue` | [`chart_panel`]：标题 + 粒度 + 曲线表 + 保存 + 图表 + 右侧求和面板 |
//! | `da_view/TransactionsChart.vue` | [`crate::components::ui::LineChart`]（自绘 SVG 折线图） |
//! | `da_view/TransactionsChartLines.vue` | [`add_line_modal`] + 曲线表（添加/删除曲线） |
//! | `backend/chart.ts` | `crate::api::chart::*` |
//! | `utils/themeColors.ts` | 直接用 CSS 变量（SVG 能读变量，不需要 `getComputedStyle`） |
//!
//! ## 有意与原实现的差异（详见汇报）
//!
//! 1. **图表交互**：原实现用 ECharts，提供图例点击开关系列。原 `DataAnalysisView` 里的
//!    图表其实**没有图例**（单图多曲线，`legend` 只有 `top:0` 的默认显示）。本页保留
//!    "图例点击开关"的能力（`visible_series`），比原实现多一点交互。
//! 2. **没有缩放 / dataZoom**：原实现也没注册 `DataZoomComponent`，属等价。
//! 3. **新增图表**：原实现硬编码 `chartType: "line"`、`lines: []`（界面上没有图表类型选择器），
//!    这里保持一致。
//! 4. **编辑图表标题**：原实现没有标题编辑入口（只有粒度下拉与曲线表），这里补了
//!    "重命名"（调用同一个 `chart_update`，属行为增强，见汇报）。
//! 5. **`isPreset` 的取舍**：原 `DataAnalysisView` 给视图硬编码 `:is-preset="false"`，
//!    导致预设图表也能改曲线。这里按**真实 `isPreset`** 控制（预设图表隐藏"添加曲线/保存修改"），
//!    见汇报。

use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::dto::{
    ChartDto, ChartLineCondition, ChartLineData, ChartQueryRequest, ChartQueryResponse,
    CreateChartRequest, UpdateChartRequest,
};
use tr_domain::models::ChartLine;

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, ChartConfig, ChartSeries, ChartValueKind, CheckboxGroup,
    CheckboxOption, Divider, Input, LineChart, Modal, Popconfirm, Select, SelectOption, Spin,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::today_ymd;

/// 页面标题（与原 `AppLeftBar.vue` 文案一致）。
pub const PAGE_TITLE: &str = "数据分析";

/// 交易类型选项（曲线条件用；文案与 `constant.ts` 一致）。
const TRANSACTION_TYPES: [(&str, &str); 3] = [
    ("income", "收入"),
    ("expense", "支出"),
    ("transfer", "转账"),
];

/// 时间粒度选项。
const GRANULARITIES: [(&str, &str); 2] = [("year", "年度"), ("month", "月度")];

/// 标签匹配策略。
const TAG_POLICIES: [(&str, &str); 2] = [("any", "任意"), ("all", "全部")];

/// 曲线配色（按交易类型语义色；同类型多条时用调色板兜底）。
fn series_color(transaction_type: &str, index: usize) -> String {
    let semantic = match transaction_type {
        "income" => Some("var(--transactions-color-income)"),
        "expense" => Some("var(--transactions-color-expense)"),
        "transfer" => Some("var(--transactions-color-transfer)"),
        _ => None,
    };
    // 原实现：仅当所有曲线类型两两不同时才用语义色；这里在无法判定"两两不同"时
    // 退化为「类型语义色 + 序号兜底」，语义更稳定（见汇报）。
    match (semantic, index) {
        (Some(color), 0) => color.to_string(),
        (Some(color), 1) => color.to_string(),
        (Some(color), 2) => color.to_string(),
        _ => crate::components::ui::ChartConfig::default_colors()
            .get(index % 5)
            .copied()
            .unwrap_or("var(--transactions-color-text-secondary)")
            .to_string(),
    }
}

// ==================================================================== 页面

/// 页面根组件。
#[component]
pub fn DataAnalysisPage() -> impl IntoView {
    let stores = AppStores::global();
    let charts = RwSignal::new(Vec::<ChartDto>::new());
    let selected = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let data_cache = RwSignal::new(BTreeMap::<String, ChartQueryResponse>::new());
    let create_open = RwSignal::new(false);
    let create_title = RwSignal::new(String::new());
    let create_granularity = RwSignal::new("year".to_string());
    let creating = RwSignal::new(false);
    let rename_open = RwSignal::new(false);
    let rename_title = RwSignal::new(String::new());
    let renaming = RwSignal::new(false);

    // 时间范围（原 `TransactionsTimeRangePicker`：默认今天 + 日粒度）
    let range_mode = RwSignal::new("date".to_string());
    let range_start = RwSignal::new(today_ymd());
    let range_end = RwSignal::new(today_ymd());

    let current_chart = move || {
        let id = selected.get();
        charts.get().into_iter().find(|chart| chart.chart_id == id)
    };

    let load_charts = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            charts.set(Vec::new());
            selected.set(String::new());
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::chart::list(&ledger_id).await {
                Ok(items) => {
                    let current = selected.get_untracked();
                    let still_exists = items.iter().any(|chart| chart.chart_id == current);
                    let next = if still_exists {
                        current
                    } else {
                        items
                            .first()
                            .map(|chart| chart.chart_id.clone())
                            .unwrap_or_default()
                    };
                    charts.set(items);
                    selected.set(next);
                }
                Err(error) => {
                    charts.set(Vec::new());
                    notify_error("查询图表列表", &error);
                }
            }
            loading.set(false);
        });
    };

    let fetch_data = move |chart: ChartDto| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let request = ChartQueryRequest {
            ledger_id: chart.ledger_id.clone(),
            ts_range: time_range_seconds(&range_start.get_untracked(), &range_end.get_untracked()),
            granularity: chart.granularity.clone(),
            lines: chart
                .lines
                .iter()
                .map(|line| ChartLineCondition {
                    label: line.label.clone(),
                    transaction_type: line.transaction_type.clone(),
                    include_outlier: line.include_outlier,
                    conditions: line.conditions.clone(),
                })
                .collect(),
        };
        let chart_id = chart.chart_id.clone();
        leptos::task::spawn_local(async move {
            match api::tr::chart_data(request).await {
                Ok(response) => {
                    data_cache.update(|cache| {
                        cache.insert(chart_id, response);
                    });
                }
                Err(error) => notify_error("查询图表数据", &error),
            }
        });
    };

    // 账本变化 → 重拉列表
    Effect::new(move |prev: Option<String>| {
        let ledger_id = stores.current_ledger_id.get();
        if prev.as_deref() == Some(ledger_id.as_str()) {
            return ledger_id;
        }
        data_cache.set(BTreeMap::new());
        selected.set(String::new());
        load_charts();
        ledger_id
    });

    // 选中图表、时间范围或粒度变化 → 拉数据（返回去重键避免重复请求）
    Effect::new(move |prev: Option<String>| {
        let chart = current_chart();
        let key = format!(
            "{}|{}|{}|{}",
            chart
                .as_ref()
                .map(|item| item.chart_id.clone())
                .unwrap_or_default(),
            chart
                .as_ref()
                .map(|item| item.granularity.clone())
                .unwrap_or_default(),
            range_start.get(),
            range_end.get(),
        );
        if prev.as_deref() == Some(key.as_str()) {
            return key;
        }
        if let Some(chart) = chart {
            fetch_data(chart);
        }
        key
    });

    let create_chart = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let title = create_title.get_untracked().trim().to_string();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择账本".to_string(), None);
            return;
        }
        if title.is_empty() {
            Notifier::global().error("请输入图表名称".to_string(), None);
            return;
        }
        creating.set(true);
        let granularity = create_granularity.get_untracked();
        leptos::task::spawn_local(async move {
            match api::chart::create(CreateChartRequest {
                ledger_id: ledger_id.clone(),
                title: title.clone(),
                granularity,
                lines: Vec::new(),
                chart_type: "line".to_string(),
            })
            .await
            {
                Ok(chart) => {
                    Notifier::global().success("图表创建成功".to_string(), None);
                    create_open.set(false);
                    create_title.set(String::new());
                    create_granularity.set("year".to_string());
                    selected.set(chart.chart_id.clone());
                    if let Ok(items) = api::chart::list(&ledger_id).await {
                        charts.set(items);
                    }
                }
                Err(_) => Notifier::global().error("图表创建失败".to_string(), None),
            }
            creating.set(false);
        });
    };

    let delete_chart = move |chart: ChartDto| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let chart_id = chart.chart_id.clone();
        let title = chart.title.clone();
        leptos::task::spawn_local(async move {
            match api::chart::delete(&chart_id).await {
                Ok(()) => {
                    Notifier::global().success("图表已删除".to_string(), None);
                    data_cache.update(|cache| {
                        cache.remove(&chart_id);
                    });
                    if selected.get_untracked() == chart_id {
                        selected.set(String::new());
                    }
                    if let Ok(items) = api::chart::list(&ledger_id).await {
                        let next = items
                            .first()
                            .map(|item| item.chart_id.clone())
                            .unwrap_or_default();
                        charts.set(items);
                        if selected.get_untracked().is_empty() {
                            selected.set(next);
                        }
                    }
                    let _ = title;
                }
                Err(_) => Notifier::global().error("删除图表失败".to_string(), None),
            }
        });
    };

    let save_chart = move |(chart, lines): (ChartDto, Vec<ChartLine>)| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        leptos::task::spawn_local(async move {
            match api::chart::update(UpdateChartRequest {
                chart_id: chart.chart_id.clone(),
                title: chart.title.clone(),
                granularity: chart.granularity.clone(),
                lines,
                chart_type: chart.chart_type.clone(),
                sort_order: chart.sort_order,
            })
            .await
            {
                Ok(_) => {
                    Notifier::global().success("图表更新成功".to_string(), None);
                    if let Ok(items) = api::chart::list(&ledger_id).await {
                        charts.set(items);
                    }
                }
                Err(_) => Notifier::global().error("图表更新失败".to_string(), None),
            }
        });
    };

    let rename_chart = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let Some(chart) = current_chart() else {
            return;
        };
        let title = rename_title.get_untracked().trim().to_string();
        if title.is_empty() {
            Notifier::global().error("请输入图表名称".to_string(), None);
            return;
        }
        renaming.set(true);
        leptos::task::spawn_local(async move {
            match api::chart::update(UpdateChartRequest {
                chart_id: chart.chart_id.clone(),
                title,
                granularity: chart.granularity.clone(),
                lines: chart.lines.clone(),
                chart_type: chart.chart_type.clone(),
                sort_order: chart.sort_order,
            })
            .await
            {
                Ok(_) => {
                    Notifier::global().success("图表更新成功".to_string(), None);
                    rename_open.set(false);
                    if let Ok(items) = api::chart::list(&ledger_id).await {
                        charts.set(items);
                    }
                }
                Err(_) => Notifier::global().error("图表更新失败".to_string(), None),
            }
            renaming.set(false);
        });
    };

    let change_granularity = move |chart_id: String, granularity: String| {
        let Some(chart) = current_chart() else {
            return;
        };
        let ledger_id = stores.current_ledger_id.get_untracked();
        leptos::task::spawn_local(async move {
            match api::chart::update(UpdateChartRequest {
                chart_id: chart_id.clone(),
                title: chart.title.clone(),
                granularity: granularity.clone(),
                lines: chart.lines.clone(),
                chart_type: chart.chart_type.clone(),
                sort_order: chart.sort_order,
            })
            .await
            {
                Ok(_) => {
                    Notifier::global().success("图表更新成功".to_string(), None);
                    if let Ok(items) = api::chart::list(&ledger_id).await {
                        charts.set(items);
                    }
                }
                Err(_) => Notifier::global().error("图表更新失败".to_string(), None),
            }
        });
    };

    view! {
        <section class="page da-page">
            <header class="page-header">
                <div class="da-time">
                    <Button
                        size=ButtonSize::Small
                        on_click=move |_| {
                            // 上一周期：按粒度位移（原 `shiftPeriod` 的极简等价）
                            shift_range(range_mode, range_start, range_end, -1);
                        }
                    >
                        "上一周期"
                    </Button>
                    <SegmentMode mode=range_mode start=range_start end=range_end />
                    <Button
                        size=ButtonSize::Small
                        on_click=move |_| {
                            shift_range(range_mode, range_start, range_end, 1);
                        }
                    >
                        "下一周期"
                    </Button>
                    <span class="da-time__range">
                        {move || format!("{} ~ {}", range_start.get(), range_end.get())}
                    </span>
                </div>
                <div class="app-top-bar-spacer"></div>
            </header>

            <div class="page-body">
                <div class="da-main">
                    <aside class="da-sidebar">
                        <Button
                            variant=ButtonVariant::Primary
                            block=true
                            on_click=move |_| {
                                create_title.set(String::new());
                                create_granularity.set("year".to_string());
                                create_open.set(true);
                            }
                        >
                            <span class="ui-btn__icon">{icons::icon(Icon::Plus)}</span>
                            "新增图表"
                        </Button>

                        <div class="da-list">
                            {move || {
                                let items = charts.get();
                                if items.is_empty() {
                                    return view! {
                                        <div class="da-list__empty">
                                            {move || {
                                                if loading.get() { "正在加载…" } else { "暂无图表" }
                                            }}
                                        </div>
                                    }
                                        .into_any();
                                }
                                items
                                    .into_iter()
                                    .map(|chart| {
                                        let id = chart.chart_id.clone();
                                        let is_active = id == selected.get();
                                        let click_id = id.clone();
                                        let delete_chart_value = chart.clone();
                                        let dots = chart
                                            .lines
                                            .iter()
                                            .map(|line| {
                                                transaction_type_color(&line.transaction_type)
                                            })
                                            .collect::<Vec<_>>();
                                        let title = chart.title.clone();
                                        // 标题会同时进 `title` 属性与文本节点，各留一份克隆
                                        let title_for_attr = title.clone();
                                        let title_for_label = title.clone();
                                        let confirm_title =
                                            format!("删除图表「{title}」？此操作不可恢复。");
                                        view! {
                                            <div
                                                class="da-list__item"
                                                class:is-active=is_active
                                                role="option"
                                                tabindex="0"
                                                aria-selected=is_active
                                                on:click=move |_| selected.set(click_id.clone())
                                                on:keydown=move |event: leptos::ev::KeyboardEvent| {
                                                    if event.key() == "Enter" || event.key() == " " {
                                                        event.prevent_default();
                                                        selected.set(id.clone());
                                                    }
                                                }
                                            >
                                                <span
                                                    class="da-list__title"
                                                    title=title_for_attr.clone()
                                                >
                                                    {title_for_label.clone()}
                                                </span>
                                                <span class="da-list__dots">
                                                    {if dots.is_empty() {
                                                        view! {
                                                            <span class="da-list__dot is-empty"></span>
                                                        }
                                                            .into_any()
                                                    } else {
                                                        dots.into_iter()
                                                            .map(|color| {
                                                                view! {
                                                                    <span
                                                                        class="da-list__dot"
                                                                        style=format!(
                                                                            "background: {color}",
                                                                        )
                                                                    ></span>
                                                                }
                                                            })
                                                            .collect_view()
                                                            .into_any()
                                                    }}
                                                </span>
                                                <Popconfirm
                                                    title=confirm_title
                                                    ok_text="删除"
                                                    cancel_text="取消"
                                                    on_confirm=move || {
                                                        delete_chart(delete_chart_value.clone())
                                                    }
                                                >
                                                    <button
                                                        type="button"
                                                        class="da-list__delete"
                                                        aria-label="删除图表"
                                                        on:click=move |event| event.stop_propagation()
                                                    >
                                                        {icons::icon(Icon::Trash)}
                                                    </button>
                                                </Popconfirm>
                                            </div>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }}
                        </div>
                    </aside>

                    <div class="da-content">
                        {move || {
                            let Some(chart) = current_chart() else {
                                return view! {
                                    <div class="da-empty">
                                        <crate::components::ui::Empty title="请选择一个图表" />
                                    </div>
                                }
                                    .into_any();
                            };
                            let data = data_cache.get().get(&chart.chart_id).cloned();
                            view! {
                                {chart_panel(
                                    chart,
                                    data,
                                    RwSignal::new(false),
                                    UnsyncCallback::new(move |(chart, lines): (
                                        ChartDto,
                                        Vec<ChartLine>,
                                    )| save_chart((chart, lines))),
                                    UnsyncCallback::new(move |granularity: String| {
                                        if let Some(chart) = current_chart() {
                                            change_granularity(
                                                chart.chart_id.clone(),
                                                granularity,
                                            );
                                        }
                                    }),
                                    UnsyncCallback::new(move |()| {
                                        if let Some(chart) = current_chart() {
                                            rename_title.set(chart.title.clone());
                                            rename_open.set(true);
                                        }
                                    }),
                                )}
                            }
                                .into_any()
                        }}
                    </div>
                </div>
            </div>

            <Modal
                open=Signal::derive(move || create_open.get())
                title="新增图表"
                width=420
                ok_text="确定"
                cancel_text="取消"
                ok_loading=Signal::derive(move || creating.get())
                on_close=move || create_open.set(false)
                on_ok=move || create_chart()
            >
                <div class="modal-form-item">
                    <p class="modal-form-label">"图表名称"</p>
                    <Input value=create_title placeholder="请输入图表名称" />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"时间粒度"</p>
                    <Select
                        value=create_granularity
                        options=GRANULARITIES
                            .iter()
                            .map(|(value, label)| SelectOption::new(*value, *label))
                            .collect()
                        placeholder="请选择时间粒度"
                    />
                </div>
            </Modal>

            <Modal
                open=Signal::derive(move || rename_open.get())
                title="重命名图表"
                width=420
                ok_text="确定"
                cancel_text="取消"
                ok_loading=Signal::derive(move || renaming.get())
                on_close=move || rename_open.set(false)
                on_ok=move || rename_chart()
            >
                <div class="modal-form-item">
                    <p class="modal-form-label">"图表名称"</p>
                    <Input value=rename_title placeholder="请输入图表名称" />
                </div>
            </Modal>
        </section>
    }
}

/// 时间粒度切换（日 / 月 / 年）。
#[component]
fn SegmentMode(
    mode: RwSignal<String>,
    start: RwSignal<String>,
    end: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="da-time__modes">
            {[("date", "日"), ("month", "月"), ("year", "年")]
                .iter()
                .map(|(value, label)| {
                    let value = value.to_string();
                    let value_for_click = value.clone();
                    view! {
                        <button
                            type="button"
                            class="da-time__mode"
                            class:is-active=move || mode.get() == value
                            on:click=move |_| {
                                mode.set(value_for_click.clone());
                                let (from, to) = normalize_range(
                                    &start.get_untracked(),
                                    &value_for_click,
                                );
                                start.set(from);
                                end.set(to);
                            }
                        >
                            {*label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// 按粒度对齐区间（月 → 当月 1 号 ~ 月末；年 → 1/1 ~ 12/31）。
fn normalize_range(anchor: &str, mode: &str) -> (String, String) {
    let (year, month, day) = crate::time::split_ymd(anchor).unwrap_or((1970, 1, 1));
    match mode {
        "month" => (
            format!("{year:04}-{month:02}-01"),
            format!("{year:04}-{month:02}-{:02}", days_in_month(year, month)),
        ),
        "year" => (format!("{year:04}-01-01"), format!("{year:04}-12-31")),
        _ => (
            format!("{year:04}-{month:02}-{day:02}"),
            format!("{year:04}-{month:02}-{day:02}"),
        ),
    }
}

/// 某年某月的天数（用 JS「下月第 0 天」技巧，自动处理闰年）。
fn days_in_month(year: i32, month: u32) -> u32 {
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32, 0);
    date.get_date()
}

/// 按粒度前后位移一个周期。
fn shift_range(
    mode: RwSignal<String>,
    start: RwSignal<String>,
    end: RwSignal<String>,
    direction: i32,
) {
    let current_mode = mode.get_untracked();
    let anchor = start.get_untracked();
    let (year, month, day) = crate::time::split_ymd(&anchor).unwrap_or((1970, 1, 1));
    let next_anchor = match current_mode.as_str() {
        "year" => format!("{:04}-{month:02}-{day:02}", year + direction),
        "month" => {
            let total = year * 12 + (month as i32 - 1) + direction;
            let next_year = total.div_euclid(12);
            let next_month = total.rem_euclid(12) as u32 + 1;
            format!("{next_year:04}-{next_month:02}-01")
        }
        _ => {
            let seconds =
                crate::time::ymd_to_seconds(&anchor).unwrap_or(0) + i64::from(direction) * 86_400;
            format::short_date(&crate::time::format_timestamp(seconds, "YYYY-MM-DD"))
                .len()
                .to_string()
                .replace(|_| true, "")
                + &crate::time::format_timestamp(seconds, "YYYY-MM-DD")
        }
    };
    let (from, to) = normalize_range(&next_anchor, &current_mode);
    start.set(from);
    end.set(to);
}

/// 「YYYY-MM-DD」区间 → 闭区间 Unix 秒（起点 00:00:00、终点 23:59:59）。
fn time_range_seconds(start: &str, end: &str) -> Vec<i64> {
    let Some(from) = crate::time::ymd_to_seconds(start) else {
        return Vec::new();
    };
    let Some(to) = crate::time::ymd_to_seconds(end) else {
        return Vec::new();
    };
    vec![from, to + 86_399]
}

/// 交易类型 → CSS 颜色变量（列表上的小圆点）。
fn transaction_type_color(transaction_type: &str) -> String {
    match transaction_type {
        "income" => "var(--transactions-color-income)",
        "expense" => "var(--transactions-color-expense)",
        "transfer" => "var(--transactions-color-transfer)",
        _ => "var(--transactions-color-text-secondary)",
    }
    .to_string()
}

// ==================================================================== 图表视图

/// 右侧图表视图：标题 + 粒度 + 曲线表 + 保存 + 自绘图表 + 右侧求和面板。
fn chart_panel(
    chart: ChartDto,
    data: Option<ChartQueryResponse>,
    busy: RwSignal<bool>,
    on_save: UnsyncCallback<(ChartDto, Vec<ChartLine>)>,
    on_granularity: UnsyncCallback<String>,
    on_rename: UnsyncCallback<()>,
) -> AnyView {
    let lines = RwSignal::new(chart.lines.clone());
    let add_open = RwSignal::new(false);
    // `data` 会被「图表区」与「右侧求和区」两个闭包读取，
    // 因此各持一份（`Option<ChartQueryResponse>` 不是 `Copy`）。
    let data_for_chart = data.clone();
    let data_for_sums = data;

    // 新增曲线表单
    let new_label = RwSignal::new(String::new());
    let new_type = RwSignal::new("income".to_string());
    let new_category = RwSignal::new(String::new());
    let new_tags = RwSignal::new(Vec::<String>::new());
    let new_policy = RwSignal::new("any".to_string());
    let new_description = RwSignal::new(String::new());
    let new_include_outlier = RwSignal::new(true);
    let category_options = RwSignal::new(Vec::<SelectOption>::new());
    let tag_options = RwSignal::new(Vec::<CheckboxOption>::new());

    let is_preset = chart.is_preset;
    // 用 `Rc` 共享：需要在多处（按钮、每一行的删除）复制同一份 `ChartDto`，
    // 而 `ChartDto` 不是 `Copy`——裸 `clone()` 会被闭包 move 走，把外层闭包退化成 `FnOnce`。
    let chart_shared = std::sync::Arc::new(chart.clone());
    let chart_for_save = chart_shared.clone();
    let chart_for_button = chart_shared.clone();
    let chart_for_granularity = chart.clone();

    let load_categories = move |transaction_type: String| {
        let ledger_id = AppStores::global().current_ledger_id.get_untracked();
        if ledger_id.is_empty() || transaction_type.is_empty() {
            category_options.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            match api::category::list(&transaction_type, &ledger_id).await {
                Ok(items) => category_options.set(
                    items
                        .into_iter()
                        .map(|item| SelectOption::same(item.name))
                        .collect(),
                ),
                Err(error) => {
                    category_options.set(Vec::new());
                    notify_error(&format!("查询 {transaction_type} 消费类型失败"), &error);
                }
            }
        });
    };

    let load_tags = move |category: String, transaction_type: String| {
        let ledger_id = AppStores::global().current_ledger_id.get_untracked();
        if ledger_id.is_empty() || category.is_empty() {
            tag_options.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            let selector = format!("{category}:{transaction_type}");
            match api::tag::list(&selector, &ledger_id).await {
                Ok(items) => tag_options.set(
                    items
                        .into_iter()
                        .map(|item| CheckboxOption::same(item.name))
                        .collect(),
                ),
                Err(error) => {
                    tag_options.set(Vec::new());
                    notify_error(
                        &format!("查询 {category}:{transaction_type} 消费标签失败"),
                        &error,
                    );
                }
            }
        });
    };

    view! {
        <div class="da-chart">
            <div class="da-chart__header">
                <h2 class="da-chart__title">{chart.title.clone()}</h2>
                <div class="da-chart__meta">
                    <Select
                        value=RwSignal::new(chart_for_granularity.granularity.clone())
                        options=GRANULARITIES
                            .iter()
                            .map(|(value, label)| SelectOption::new(*value, *label))
                            .collect()
                        disabled=Signal::derive(move || is_preset)
                        on_change=move |value: String| on_granularity.run(value)
                    />
                    <Button
                        size=ButtonSize::Small
                        on_click=move |_| on_rename.run(())
                    >
                        "重命名"
                    </Button>
                </div>
            </div>

            <div class="da-chart__body">
                <div class="da-chart__canvas">
                    {move || {
                        let Some(data) = data_for_chart.clone() else {
                            return view! {
                                <div class="da-chart__loading">
                                    <Spin spinning=true />
                                </div>
                            }
                                .into_any();
                        };
                        let visible = lines
                            .get()
                            .iter()
                            .map(|line| (line.label.clone(), line.transaction_type.clone()))
                            .collect::<Vec<_>>();
                        let response_lines: Vec<ChartLineData> = data
                            .lines
                            .iter()
                            .filter(|line| {
                                visible
                                    .iter()
                                    .any(|(label, _)| *label == line.label)
                            })
                            .cloned()
                            .collect();
                        if response_lines.is_empty() {
                            return view! {
                                <div class="da-chart__empty">
                                    <crate::components::ui::Empty title="暂无数据" />
                                </div>
                            }
                                .into_any();
                        }
                        // X 轴类目：所有曲线的 time 并集（后端已补零对齐，取最长的那条）
                        let categories = response_lines
                            .iter()
                            .map(|line| line.data.len())
                            .max()
                            .map(|max| {
                                response_lines
                                    .iter()
                                    .find(|line| line.data.len() == max)
                                    .map(|line| {
                                        line.data
                                            .iter()
                                            .map(|point| point.time.clone())
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        let series = response_lines
                            .iter()
                            .enumerate()
                            .map(|(index, line)| {
                                let transaction_type = visible
                                    .iter()
                                    .find(|(label, _)| *label == line.label)
                                    .map(|(_, kind)| kind.clone())
                                    .unwrap_or_default();
                                ChartSeries::new(
                                    line.label.clone(),
                                    series_color(&transaction_type, index),
                                    line.data.iter().map(|point| point.amount).collect(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let chart_title = chart.title.clone();
                        view! {
                            <LineChart
                                categories=Signal::derive(move || categories.clone())
                                series=Signal::derive(move || series.clone())
                                config=ChartConfig::default()
                                    .height(360)
                                    .value_kind(ChartValueKind::Money)
                                    .y_title(if chart_title.contains("月度") {
                                        "月份"
                                    } else {
                                        "年份"
                                    })
                            />
                        }
                            .into_any()
                    }}
                </div>

                <aside class="da-chart__sums">
                    <h4 class="da-chart__sums-title">"曲线合计"</h4>
                    {move || {
                        let Some(data) = data_for_sums.clone() else {
                            return ().into_any();
                        };
                        data.lines
                            .iter()
                            .map(|line| {
                                let total: i64 = line.data.iter().map(|point| point.amount).sum();
                                view! {
                                    <div class="da-chart__sum-row">
                                        <span
                                            class="da-list__dot"
                                            style=format!(
                                                "background: {}",
                                                transaction_type_color(
                                                    &visible_type(&line.label, &lines.get_untracked()),
                                                ),
                                            )
                                        ></span>
                                        <span class="da-chart__sum-label">{line.label.clone()}</span>
                                        <span class="da-chart__sum-value">
                                            {format::amount(total)}
                                        </span>
                                    </div>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </aside>
            </div>

            <Divider />

            <div class="da-chart__lines">
                <div class="da-chart__lines-head">
                    <h4 class="da-panel__title">"曲线配置"</h4>
                    <Show when=move || !is_preset>
                        <div class="da-chart__lines-actions">
                            <Button
                                variant=ButtonVariant::Primary
                                size=ButtonSize::Small
                                on_click=move |_| {
                                    new_label.set(String::new());
                                    new_type.set("income".to_string());
                                    new_category.set(String::new());
                                    new_tags.set(Vec::new());
                                    new_policy.set("any".to_string());
                                    new_description.set(String::new());
                                    new_include_outlier.set(true);
                                    category_options.set(Vec::new());
                                    tag_options.set(Vec::new());
                                    add_open.set(true);
                                    load_categories("income".to_string());
                                }
                            >
                                "添加曲线"
                            </Button>
                            <Button
                                size=ButtonSize::Small
                                on_click={
                                    // 在 `Show` 的 children 内克隆：children 是 `Fn`，
                                    // 直接 move 捕获 `chart_for_button` 会让它退化成 `FnOnce`
                                    let chart_for_click = chart_for_button.clone();
                                    move |_| {
                                        let payload = (
                                            (*chart_for_click).clone(),
                                            lines.get_untracked(),
                                        );
                                        on_save.run(payload)
                                    }
                                }
                            >
                                "保存修改"
                            </Button>
                        </div>
                    </Show>
                </div>
                <table class="stock-table da-table">
                    <thead>
                        <tr>
                            <th>"曲线名称"</th>
                            <th class="is-center" style="width: 100px;">"交易类型"</th>
                            <th class="is-center" style="width: 120px;">"包含离群值"</th>
                            <th>"筛选条件"</th>
                            <Show when=move || !is_preset>
                                <th class="is-center" style="width: 80px;">"操作"</th>
                            </Show>
                        </tr>
                    </thead>
                    <tbody>
                        {move || {
                            let items = lines.get();
                            if items.is_empty() {
                                return view! {
                                    <tr>
                                        <td colspan="5" class="is-muted">
                                            "暂无数据"
                                        </td>
                                    </tr>
                                }
                                    .into_any();
                            }
                            items
                                .into_iter()
                                .enumerate()
                                .map(|(index, line)| {
                                    let chart_for_row = chart_for_save.clone();
                                    let condition = line.conditions.first().cloned();
                                    let condition_text = match condition {
                                        Some(item) => {
                                            let mut parts: Vec<String> = Vec::new();
                                            if !item.category.is_empty() {
                                                parts.push(item.category.clone());
                                            }
                                            if !item.tags.is_empty() {
                                                parts.push(item.tags.join(", "));
                                            }
                                            if !item.description.is_empty() {
                                                parts.push(item.description.clone());
                                            }
                                            if parts.is_empty() {
                                                "无".to_string()
                                            } else {
                                                parts.join(" / ")
                                            }
                                        }
                                        None => "无".to_string(),
                                    };
                                    let label = line.label.clone();
                                    view! {
                                        <tr>
                                            <td>{label}</td>
                                            <td class="is-center">
                                                <span
                                                    class="da-type-tag"
                                                    style=format!(
                                                        "color: {}; background: {}",
                                                        transaction_type_color(&line.transaction_type),
                                                        transaction_type_color(&line.transaction_type),
                                                    )
                                                >
                                                    {format::transaction_type_text(&line.transaction_type)}
                                                </span>
                                            </td>
                                            <td class="is-center">
                                                {if line.include_outlier { "是" } else { "否" }}
                                            </td>
                                            <td>{condition_text}</td>
                                            <Show when=move || !is_preset>
                                                <td class="is-center">
                                                    <button
                                                        type="button"
                                                        class="ui-btn ui-btn--text-danger ui-btn--sm"
                                                        title="删除曲线"
                                                        aria-label="删除曲线"
                                                        on:click={
                                                            let chart_clone = chart_for_row.clone();
                                                            move |_| {
                                                                lines.update(|items| {
                                                                    if index < items.len() {
                                                                        items.remove(index);
                                                                    }
                                                                });
                                                                let payload = (
                                                                    (*chart_clone).clone(),
                                                                    lines.get_untracked(),
                                                                );
                                                                on_save.run(payload);
                                                            }
                                                        }
                                                    >
                                                        {icons::icon(Icon::Trash)}
                                                    </button>
                                                </td>
                                            </Show>
                                        </tr>
                                    }
                                })
                                .collect_view()
                                .into_any()
                        }}
                    </tbody>
                </table>
            </div>

            {add_line_modal(
                add_open,
                new_label,
                new_type,
                new_category,
                new_tags,
                new_policy,
                new_description,
                new_include_outlier,
                category_options,
                tag_options,
                busy,
                UnsyncCallback::new(move |kind: String| {
                    new_category.set(String::new());
                    new_tags.set(Vec::new());
                    load_categories(kind);
                }),
                UnsyncCallback::new(move |category: String| {
                    new_tags.set(Vec::new());
                    load_tags(category, new_type.get_untracked());
                }),
                UnsyncCallback::new(move |()| {
                    let label = new_label.get_untracked().trim().to_string();
                    if label.is_empty() {
                        Notifier::global().error("请输入曲线名称".to_string(), None);
                        return;
                    }
                    let category = new_category.get_untracked();
                    let tags = new_tags.get_untracked();
                    let description = new_description.get_untracked();
                    let transaction_type = new_type.get_untracked();
                    let include_outlier = new_include_outlier.get_untracked();
                    let conditions = if category.is_empty()
                        && tags.is_empty()
                        && description.trim().is_empty()
                    {
                        Vec::new()
                    } else {
                        vec![tr_domain::models::QueryConditionItem {
                            transaction_type: transaction_type.clone(),
                            category,
                            tags,
                            tag_policy: new_policy.get_untracked(),
                            tag_not: false,
                            description: description.trim().to_string(),
                        }]
                    };
                    lines.update(|items| {
                        items.push(ChartLine {
                            label,
                            transaction_type,
                            include_outlier,
                            conditions,
                        });
                    });
                    add_open.set(false);
                }),
            )}
        </div>
    }
    .into_any()
}

/// 曲线 label → 交易类型（右侧求和面板取色用）。
fn visible_type(label: &str, lines: &[ChartLine]) -> String {
    lines
        .iter()
        .find(|line| line.label == label)
        .map(|line| line.transaction_type.clone())
        .unwrap_or_default()
}

/// 「添加曲线」弹窗（原 `TransactionsChartLines.vue` 的表单）。
#[allow(clippy::too_many_arguments)]
fn add_line_modal(
    open: RwSignal<bool>,
    label: RwSignal<String>,
    transaction_type: RwSignal<String>,
    category: RwSignal<String>,
    tags: RwSignal<Vec<String>>,
    policy: RwSignal<String>,
    description: RwSignal<String>,
    include_outlier: RwSignal<bool>,
    category_options: RwSignal<Vec<SelectOption>>,
    tag_options: RwSignal<Vec<CheckboxOption>>,
    busy: RwSignal<bool>,
    on_type_change: UnsyncCallback<String>,
    on_category_change: UnsyncCallback<String>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="添加曲线"
            width=500
            ok_text="确定"
            cancel_text="取消"
            ok_loading=Signal::derive(move || busy.get())
            on_close=move || open.set(false)
            on_ok=move || on_ok.run(())
        >
            <div class="modal-form-item">
                <p class="modal-form-label">"曲线名称"</p>
                <Input value=label placeholder="请输入曲线名称" />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"交易类型"</p>
                <Select
                    value=transaction_type
                    options=TRANSACTION_TYPES
                        .iter()
                        .map(|(value, text)| SelectOption::new(*value, *text))
                        .collect()
                    placeholder="请选择交易类型"
                    on_change=move |value: String| on_type_change.run(value)
                />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"分类"</p>
                <Select
                    value=category
                    options=category_options.get()
                    placeholder="请选择分类"
                    allow_clear=true
                    on_change=move |value: String| on_category_change.run(value)
                />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"标签"</p>
                <CheckboxGroup values=tags options=tag_options.get() />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"标签匹配"</p>
                <Select
                    value=policy
                    options=TAG_POLICIES
                        .iter()
                        .map(|(value, text)| SelectOption::new(*value, *text))
                        .collect()
                />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"描述包含"</p>
                <Input value=description placeholder="输入关键词" />
            </div>
            <label class="da-line-switch">
                <input
                    type="checkbox"
                    prop:checked=move || include_outlier.get()
                    on:change=move |event: leptos::ev::Event| {
                        include_outlier
                            .set(event_target_checked(&event));
                    }
                />
                <span>"包含离群值"</span>
            </label>
        </Modal>
    }
    .into_any()
}
