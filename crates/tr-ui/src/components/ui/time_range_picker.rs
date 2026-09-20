//! 时间范围选择器（共享组件）：**粒度分段（日 / 月 / 年）+ 区间选择 + 前后期 + 按粒度的快捷项**。
//!
//! 由消费记录页与分析子功能共用（原来只长在消费记录页里，分析子功能自己另做了一套
//! 「上一期 / 分度 / 下一期」，两处行为不一致）。
//!
//! ## 行为
//!
//! * **三个粒度都是区间**：日 → 日到日、月 → 月到月、年 → 年到年；
//!   月/年用"两次点击"协议（第一次定起点、第二次定终点，点得更早自动对调），与日期区间选择器一致。
//! * **快捷项按粒度给**（日：今天/近 7 天/本周/本月/上月；月：本月/上月/近 6 个月/今年；年：今年/去年），
//!   命中的那项高亮。
//! * 触发器文案按粒度给精度：`2026-09-19` / `2026-09` / `2026`，未选完时 `起点 ~`。
//! * **区间未选完时不发查询**由调用方负责（这里只写信号）。
//!
//! 类名用 `.ui-time*`（共享 UI 前缀）：它以前叫 `.tr-time*`，那是"长在消费记录页"的遗留。

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};

use super::backdrop;
use super::date_picker::{add_months, days_in_month};
use super::nav_button;
use super::{DateRangePicker, Segmented, SegmentedOption};
use crate::icons::{self, Icon};
use crate::time::{format_timestamp, today_ymd, ymd_to_seconds, DAY_SECONDS};

/// 时间范围粒度标签（日 / 月 / 年）。
const TIME_RANGE_MODES: [(&str, &str); 3] = [("date", "日"), ("month", "月"), ("year", "年")];

// ==================================================================== 日期算术（纯整数，无新依赖）

/// 拆分 `YYYY-MM-DD` → `(年, 月)`。
///
/// 注意：这里**故意**不复用 [`crate::time::split_ymd`]——那个是"三段都必须是数字"的
/// 完整 `(年, 月, 日)` 解析（`2026-01-xx` → `None`），而本文件的调用点只需要年月，
/// 允许日缺失/非法（`2026-01-xx` → `Some((2026, 1))`）。
pub(crate) fn split_ymd(input: &str) -> Option<(i32, u32)> {
    let trimmed = input.trim();
    let (year, rest) = trimmed.split_once('-')?;
    let (month, _) = rest.split_once('-')?;
    Some((year.parse().ok()?, month.parse().ok()?))
}

/// 某天所在周的周一。
///
/// 一周从周一算起：`get_day()` 的 0 是周日，用 `(day + 6) % 7` 折算成距周一的偏移；
fn week_monday(ymd: &str) -> Option<String> {
    let (year, month, day) = crate::time::split_ymd(ymd)?;
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32);
    let offset = ((date.get_day() + 6) % 7) as i32; // 0 = 周一
    if offset == 0 {
        return Some(ymd.to_string());
    }
    let monday =
        js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32 - offset);
    Some(format!(
        "{:04}-{:02}-{:02}",
        monday.get_full_year(),
        monday.get_month() + 1,
        monday.get_date()
    ))
}

/// 某天所在周的周日。
fn week_sunday(ymd: &str) -> Option<String> {
    let (year, month, day) = crate::time::split_ymd(ymd)?;
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32 + 6);
    Some(format!(
        "{:04}-{:02}-{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date()
    ))
}

/// 区间是否恰好 6 天（`end.diff(start, 'day') === 6`，即整周）。
fn is_six_days(start: &str, end: &str) -> bool {
    match (ymd_to_seconds(start), ymd_to_seconds(end)) {
        (Some(from), Some(to)) => (to - from) / DAY_SECONDS == 6,
        _ => false,
    }
}

/// 月份粒度对齐：起点 1 号、终点当月最后一天。
fn month_bounds(ymd: &str) -> Option<(String, String)> {
    let (year, month) = split_ymd(ymd)?;
    let last = days_in_month(year, month);
    Some((
        format!("{year:04}-{month:02}-01"),
        format!("{year:04}-{month:02}-{last:02}"),
    ))
}

/// 年份粒度对齐：1 月 1 日 ~ 12 月 31 日。
fn year_bounds(ymd: &str) -> Option<(String, String)> {
    let (year, _) = split_ymd(ymd)?;
    Some((format!("{year:04}-01-01"), format!("{year:04}-12-31")))
}

/// `normalizeTimeRange`：按粒度对齐后，起点取当天 00:00、终点取当天 23:59:59
/// （这里只需处理日期部分，秒数在 [`crate::time::range_to_seconds`] 里补齐）。
pub(crate) fn normalize_range(start: &str, end: &str, mode: &str) -> (String, String) {
    let (from, to) = match mode {
        "month" => {
            let from = month_bounds(start)
                .map(|bounds| bounds.0)
                .unwrap_or_default();
            let to = month_bounds(end).map(|bounds| bounds.1).unwrap_or_default();
            (from, to)
        }
        "year" => {
            let from = year_bounds(start)
                .map(|bounds| bounds.0)
                .unwrap_or_default();
            let to = year_bounds(end).map(|bounds| bounds.1).unwrap_or_default();
            (from, to)
        }
        _ => (start.to_string(), end.to_string()),
    };
    if from.is_empty() || to.is_empty() || from > to {
        (start.to_string(), end.to_string())
    } else {
        (from, to)
    }
}

/// `shiftPeriod`：按粒度前后翻一个周期。
///
/// * `date`：区间正好 6 天 → 整周（±7 天）；否则 ±1 天
/// * `month`：`start ± 1 月` 的月初 / `end ± 1 月` 的月末
/// * `year`：`start ± 1 年` 的年初 / `end ± 1 年` 的年末
pub(crate) fn shift_period(start: &str, end: &str, mode: &str, direction: i32) -> (String, String) {
    match mode {
        "month" => {
            let Some((start_year, start_month)) = split_ymd(start) else {
                return (start.to_string(), end.to_string());
            };
            let Some((end_year, end_month)) = split_ymd(end) else {
                return (start.to_string(), end.to_string());
            };
            let (next_start_year, next_start_month) =
                add_months(start_year, start_month, direction);
            let (next_end_year, next_end_month) = add_months(end_year, end_month, direction);
            let last = days_in_month(next_end_year, next_end_month);
            (
                format!("{next_start_year:04}-{next_start_month:02}-01"),
                format!("{next_end_year:04}-{next_end_month:02}-{last:02}"),
            )
        }
        "year" => {
            let Some((start_year, _)) = split_ymd(start) else {
                return (start.to_string(), end.to_string());
            };
            let Some((end_year, _)) = split_ymd(end) else {
                return (start.to_string(), end.to_string());
            };
            (
                format!("{:04}-01-01", start_year + direction),
                format!("{:04}-12-31", end_year + direction),
            )
        }
        _ => {
            let days = if is_six_days(start, end) { 7 } else { 1 };
            let delta = DAY_SECONDS * i64::from(days) * i64::from(direction);
            let next_start = ymd_to_seconds(start).map(|seconds| seconds + delta);
            let next_end = ymd_to_seconds(end).map(|seconds| seconds + delta);
            match (
                next_start.map(|seconds| format_timestamp(seconds, "YYYY-MM-DD")),
                next_end.map(|seconds| format_timestamp(seconds, "YYYY-MM-DD")),
            ) {
                (Some(from), Some(to)) => (from, to),
                _ => (start.to_string(), end.to_string()),
            }
        }
    }
}

/// 时间范围的展示文案（范围输入框里的内容）：
/// **按粒度给出对应精度**——日 → `2026-09-19`、月 → `2026-09`、年 → `2026`；
/// 还没选终点时显示 `起点 ~`（与日期区间选择器同一写法，提示"还差一次点击"）。
fn range_text(start: &str, end: &str, mode: &str) -> String {
    if start.is_empty() {
        return "请选择时间范围".to_string();
    }
    let label = |ymd: &str| -> String {
        match (mode, split_ymd(ymd)) {
            ("month", Some((year, month))) => format!("{year:04}-{month:02}"),
            ("year", Some((year, _))) => format!("{year:04}"),
            _ => ymd.to_string(),
        }
    };
    let from = label(start);
    if end.is_empty() {
        return format!("{from} ~");
    }
    let to = label(end);
    if from == to {
        from
    } else {
        format!("{from} ~ {to}")
    }
}

/// 月 / 年两个粒度共用的**两次点击协议**（与日期区间选择器一致）：
/// 第一次点击定起点、终点留空等待；第二次定终点；点得比起点早则自动对调。
///
/// `clicked_start` / `clicked_end` 是这一格的**区间边界**（月 → 该月首末日，年 → 该年首末日），
/// 所以落库的区间永远与粒度对齐。
fn pick_period(
    clicked_start: &str,
    clicked_end: &str,
    start: RwSignal<String>,
    end: RwSignal<String>,
) {
    let current_start = start.get_untracked();
    let current_end = end.get_untracked();
    if current_start.is_empty() || !current_end.is_empty() {
        // 新一轮：先定起点，终点清空等第二次点击
        start.set(clicked_start.to_string());
        end.set(String::new());
    } else if clicked_start < current_start.as_str() {
        // 点得比起点早 → 对调，区间永远是"早 → 晚"
        start.set(clicked_start.to_string());
        end.set(current_start);
    } else {
        end.set(clicked_end.to_string());
    }
}

// ==================================================================== 时间范围选择器

/// 快捷区间：**日 / 月 / 年 各 5 个，互不相同**——每个粒度给这一粒度下真正常用的区间。
///
/// 每个预选都按当前粒度对齐（月 → 整月、年 → 整年），点完后区间形态与手选完全一致。
/// 旧实现是一份与粒度无关的固定 6 项（今天 / 本周 / 本月 / 上月 / 上周 / 今年），
/// 月粒度下点「今天」会得到"粒度是月、区间只有一天"的自相矛盾状态。
fn preset_ranges(mode: &str) -> Vec<(&'static str, String, String)> {
    let today = today_ymd();
    let Some((year, month)) = split_ymd(&today) else {
        return Vec::new();
    };
    let month_span = |year: i32, month: u32| -> (String, String) {
        let last = days_in_month(year, month);
        (
            format!("{year:04}-{month:02}-01"),
            format!("{year:04}-{month:02}-{last:02}"),
        )
    };
    let year_span = |year: i32| -> (String, String) {
        (format!("{year:04}-01-01"), format!("{year:04}-12-31"))
    };
    // 近 N 个月（含本月）的区间
    let months_ago = |back: i32| -> (String, String) {
        let (start_year, start_month) = add_months(year, month, -back);
        let (_, end) = month_span(year, month);
        (format!("{start_year:04}-{start_month:02}-01"), end)
    };

    match mode {
        "month" => {
            let mut presets: Vec<(&'static str, String, String)> = Vec::new();
            let (from, to) = month_span(year, month);
            presets.push(("本月", from, to));
            let (last_year, last_month) = add_months(year, month, -1);
            let (from, to) = month_span(last_year, last_month);
            presets.push(("上月", from, to));
            let (from, to) = months_ago(5);
            presets.push(("近 6 个月", from, to));
            let (from, to) = year_span(year);
            presets.push(("今年", from, to));
            presets
        }
        "year" => {
            let mut presets: Vec<(&'static str, String, String)> = Vec::new();
            let (from, to) = year_span(year);
            presets.push(("今年", from, to));
            let (from, to) = year_span(year - 1);
            presets.push(("去年", from, to));
            presets
        }
        _ => {
            let mut presets: Vec<(&'static str, String, String)> = Vec::new();
            presets.push(("今天", today.clone(), today.clone()));
            if let Some(seconds) = ymd_to_seconds(&today) {
                presets.push((
                    "近 7 天",
                    format_timestamp(seconds - DAY_SECONDS * 6, "YYYY-MM-DD"),
                    today.clone(),
                ));
            }
            if let (Some(monday), Some(sunday)) = (week_monday(&today), week_sunday(&today)) {
                presets.push(("本周", monday, sunday));
            }
            let (from, to) = month_span(year, month);
            presets.push(("本月", from, to));
            let (last_year, last_month) = add_months(year, month, -1);
            let (from, to) = month_span(last_year, last_month);
            presets.push(("上月", from, to));
            presets
        }
    }
}

/// 时间范围选择器：
/// 粒度分段（日/月/年）+ 区间选择器 + 前后翻页 + 一排预设。
#[component]
pub fn TimeRangePicker(
    /// 粒度：`date` / `month` / `year`
    mode: RwSignal<String>,
    /// 起点（`YYYY-MM-DD`）
    start: RwSignal<String>,
    /// 终点（`YYYY-MM-DD`）
    end: RwSignal<String>,
) -> impl IntoView {
    let picker_open = RwSignal::new(false);
    // 面板当前展示的 (年, 月)：整数二元组是 `Copy`，可以随便进闭包
    let visible = RwSignal::new(split_ymd(&start.get_untracked()).unwrap_or((1970, 1)));

    let shift = move |delta: i32| {
        let current_start = start.get_untracked();
        let current_end = end.get_untracked();
        let current_mode = mode.get_untracked();
        let (from, to) = shift_period(&current_start, &current_end, &current_mode, delta);
        start.set(from);
        end.set(to);
    };

    let open_picker = move |_| {
        let next = split_ymd(&start.get_untracked()).unwrap_or((1970, 1));
        visible.set(next);
        picker_open.update(|open| *open = !*open);
    };

    let change_mode = move |next: String| {
        let from = start.get_untracked();
        let to = end.get_untracked();
        if to.is_empty() {
            // 只选了起点：起点按新粒度对齐，终点继续留空等第二次点击
            let (normalized_from, _) = normalize_range(&from, &from, &next);
            start.set(normalized_from);
        } else {
            let (normalized_from, normalized_to) = normalize_range(&from, &to, &next);
            start.set(normalized_from);
            end.set(normalized_to);
        }
        mode.set(next);
    };

    // 预设点击：按当前粒度对齐后落库（面板不收起——用户常要连着比较几个区间，
    // 而且当前命中的预设会高亮出来）
    let apply_preset = move |from: String, to: String| {
        let (normalized_from, normalized_to) = normalize_range(&from, &to, &mode.get_untracked());
        start.set(normalized_from);
        end.set(normalized_to);
    };

    let mode_options = TIME_RANGE_MODES
        .iter()
        .map(|(value, label)| SegmentedOption::new(*value, *label))
        .collect::<Vec<_>>();

    view! {
        <div class="ui-time">
            <Segmented
                value=mode
                options=mode_options
                on_change=move |next: String| change_mode(next)
            />
            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "上一周期", Icon::Left, UnsyncCallback::new(move |()| shift(-1)))}

            <div class="ui-time__field" class:is-open=move || picker_open.get()>
                <button
                    type="button"
                    class="ui-time__trigger"
                    on:click=open_picker
                >
                    <span class="ui-date-picker__icon">{icons::icon(Icon::ClockCircle)}</span>
                    <span class="ui-time__value">
                        {move || range_text(&start.get(), &end.get(), &mode.get())}
                    </span>
                </button>

                <Show when=move || picker_open.get()>
                    {backdrop(UnsyncCallback::new(move |()| picker_open.set(false)))}
                    <div class="ui-time__panel">
                        {move || {
                            let current_mode = mode.get();
                            if current_mode == "date" {
                                // 内联：直接把日历铺在面板里（旧实现在面板里又套了一个触发器，
                                // 用户得连点两次才看得到日历）
                                view! {
                                    <DateRangePicker
                                        inline=true
                                        start=start
                                        end=end
                                        on_change=move |(from, to): (String, String)| {
                                            let current_mode = mode.get_untracked();
                                            if from.is_empty() || to.is_empty() {
                                                // 只选了起点：起点按粒度对齐即可，终点保持"待选"
                                                if !from.is_empty() {
                                                    let (normalized_from, _) =
                                                        normalize_range(&from, &from, &current_mode);
                                                    start.set(normalized_from);
                                                    end.set(String::new());
                                                }
                                                return;
                                            }
                                            let (normalized_from, normalized_to) =
                                                normalize_range(&from, &to, &current_mode);
                                            start.set(normalized_from);
                                            end.set(normalized_to);
                                        }
                                    />
                                }
                                    .into_any()
                            } else if current_mode == "month" {
                                month_panel(visible, start, end)
                            } else {
                                year_panel(visible, start, end)
                            }
                        }}
                        <div class="ui-time__presets">
                            {move || {
                                let current_start = start.get();
                                let current_end = end.get();
                                preset_ranges(&mode.get())
                                    .into_iter()
                                    .map(|(label, from, to)| {
                                        // 命中的预设高亮：一眼看出"现在这个区间是哪个快捷项"
                                        let is_active = current_start == from && current_end == to;
                                        let click_from = from.clone();
                                        let click_to = to.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="ui-time__preset"
                                                class:is-active=is_active
                                                on:click=move |_| {
                                                    apply_preset(
                                                        click_from.clone(),
                                                        click_to.clone(),
                                                    )
                                                }
                                            >
                                                {label}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                </Show>
            </div>

            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "下一周期", Icon::Right, UnsyncCallback::new(move |()| shift(1)))}
        </div>
    }
}

/// 月份粒度面板：年份切换 + 12 个月，**两次点击选「月 → 月」区间**
/// （第一次点起点、第二次点终点，点得更早则自动对调；协议与日期区间选择器一致）。
fn month_panel(
    visible: RwSignal<(i32, u32)>,
    start: RwSignal<String>,
    end: RwSignal<String>,
) -> AnyView {
    let shift_year = move |delta: i32| {
        visible.update(|(year, month)| {
            let (next_year, next_month) = add_months(*year, *month, delta * 12);
            *year = next_year;
            *month = next_month;
        });
    };

    view! {
        <div class="ui-time__panel-head">
            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "上一年", Icon::Left, UnsyncCallback::new(move |()| shift_year(-1)))}
            <span class="ui-time__panel-title">
                {move || format!("{} 年", visible.get().0)}
            </span>
            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "下一年", Icon::Right, UnsyncCallback::new(move |()| shift_year(1)))}
        </div>
        <div class="ui-time__grid ui-time__grid--month">
            {move || {
                let (year, _) = visible.get();
                let start_value = start.get();
                let end_value = end.get();
                (1..=12u32)
                    .map(|month| {
                        let last = days_in_month(year, month);
                        let from = format!("{year:04}-{month:02}-01");
                        let to = format!("{year:04}-{month:02}-{last:02}");
                        let is_start = !start_value.is_empty() && start_value == from;
                        let is_end = !end_value.is_empty() && end_value == to;
                        // 已选完两端时，中间那些月才算"在区间内"
                        let in_range = !start_value.is_empty() && !end_value.is_empty()
                            && from > start_value && to < end_value;
                        let click_from = from.clone();
                        let click_to = to.clone();
                        view! {
                            <button
                                type="button"
                                class="ui-time__cell"
                                class:is-start=is_start
                                class:is-end=is_end
                                class:is-in-range=in_range
                                on:click=move |_| {
                                    pick_period(&click_from, &click_to, start, end)
                                }
                            >
                                {format!("{month} 月")}
                            </button>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
    .into_any()
}

/// 年份粒度面板：一个十年网格 + 十年切换，**两次点击选「年 → 年」区间**。
fn year_panel(
    visible: RwSignal<(i32, u32)>,
    start: RwSignal<String>,
    end: RwSignal<String>,
) -> AnyView {
    let shift_decade = move |delta: i32| {
        visible.update(|(year, _)| *year += delta * 10);
    };

    view! {
        <div class="ui-time__panel-head">
            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "上一个十年", Icon::Left, UnsyncCallback::new(move |()| shift_decade(-1)))}
            <span class="ui-time__panel-title">
                {move || {
                    let decade = visible.get().0.div_euclid(10) * 10;
                    format!("{decade} – {}", decade + 9)
                }}
            </span>
            {nav_button("ui-icon-btn ui-icon-btn--bordered ui-time__nav", "下一个十年", Icon::Right, UnsyncCallback::new(move |()| shift_decade(1)))}
        </div>
        <div class="ui-time__grid ui-time__grid--year">
            {move || {
                let current_year = visible.get().0;
                let decade = current_year.div_euclid(10) * 10;
                let start_value = start.get();
                let end_value = end.get();
                (decade..decade + 10)
                    .map(|year| {
                        let from = format!("{year:04}-01-01");
                        let to = format!("{year:04}-12-31");
                        let is_start = !start_value.is_empty() && start_value == from;
                        let is_end = !end_value.is_empty() && end_value == to;
                        let in_range = !start_value.is_empty() && !end_value.is_empty()
                            && from > start_value && to < end_value;
                        let click_from = from.clone();
                        let click_to = to.clone();
                        view! {
                            <button
                                type="button"
                                class="ui-time__cell"
                                class:is-start=is_start
                                class:is-end=is_end
                                class:is-in-range=in_range
                                on:click=move |_| {
                                    pick_period(&click_from, &click_to, start, end)
                                }
                            >
                                {format!("{year} 年")}
                            </button>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
    .into_any()
}
