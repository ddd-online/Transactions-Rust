//! 日期选择 —— 单个日期 + 日期区间，**周一起始**，弹层沿用 `ui-select` 的浮层手法。
//!
//! 对照原 `a-date-picker` / `a-range-picker` 在本项目里的用法：
//! 值一律是 `YYYY-MM-DD` 字符串（与后端 `transactionAt` 的日期表示一致），
//! 不做时区换算——只有 `time::ymd_to_seconds` 才把日期串变成 Unix 秒。
//!
//! ## 日期算术为什么用 `js_sys::Date`
//!
//! 与 [`crate::time`] 同样的理由：宿主本地时区 + 夏令时由 JS 引擎负责，
//! Rust 侧手算会在跨时区/闰年边界上与原实现（dayjs）分叉。
//!
//! ## 与 Ant Design 的差异（有意为之）
//!
//! * 区间选择用**单面板 + 两次点击**（先起点、后终点），不是双面板；
//!   悬浮预览范围也没做——本项目只有"筛选时间范围"一处用区间，两次点击已经够直观。
//! * 邻月的日期正常渲染但标成 `is-outside`，点击会**选中并跳月**（与 Ant Design 一致）。

use leptos::prelude::*;

use crate::icons::{self, Icon};

// ---------------------------------------------------------------- 日期算术

/// 一个公历日期（月 / 日都是 1 起）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ymd {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Ymd {
    /// `YYYY-MM-DD`（补零；字符串比较即时间先后比较）。
    pub fn to_string_padded(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// 解析 `YYYY-MM-DD`；格式非法返回 `None`。
pub fn parse_ymd(input: &str) -> Option<Ymd> {
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(Ymd { year, month, day })
}

/// 今天（本地时区）。
pub fn today() -> Ymd {
    let now = js_sys::Date::new_0();
    Ymd {
        year: now.get_full_year() as i32,
        month: now.get_month() + 1,
        day: now.get_date(),
    }
}

/// 月份加减（`delta` 可负）。
pub fn add_months(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let total = year * 12 + (month as i32 - 1) + delta;
    let next_year = total.div_euclid(12);
    let next_month = total.rem_euclid(12) as u32 + 1;
    (next_year, next_month)
}

/// 某年某月的天数（用 JS 的"下月第 0 天"技巧，自动处理闰年）。
fn days_in_month(year: i32, month: u32) -> u32 {
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32, 0);
    date.get_date()
}

/// 某月 1 号是星期几（0 = 周一 … 6 = 周日）。
fn monday_offset(year: i32, month: u32) -> u32 {
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, 1);
    // JS 的 get_day() 是 0 = 周日，换算成周一起始
    (date.get_day() + 6) % 7
}

const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

// ---------------------------------------------------------------- 日历面板

/// 一个日历单元格的取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    date: Ymd,
    outside: bool,
}

/// 生成固定 42 格（6 行 × 7 列）的月历网格。
fn month_grid(year: i32, month: u32) -> Vec<Cell> {
    let offset = monday_offset(year, month);
    let total = days_in_month(year, month);
    let (prev_year, prev_month) = add_months(year, month, -1);
    let prev_total = days_in_month(prev_year, prev_month);

    (0..42)
        .map(|index| {
            let number = index as i64 - offset as i64 + 1;
            if number < 1 {
                Cell {
                    date: Ymd {
                        year: prev_year,
                        month: prev_month,
                        day: (prev_total as i64 + number) as u32,
                    },
                    outside: true,
                }
            } else if number > total as i64 {
                let (next_year, next_month) = add_months(year, month, 1);
                Cell {
                    date: Ymd {
                        year: next_year,
                        month: next_month,
                        day: (number - total as i64) as u32,
                    },
                    outside: true,
                }
            } else {
                Cell {
                    date: Ymd {
                        year,
                        month,
                        day: number as u32,
                    },
                    outside: false,
                }
            }
        })
        .collect()
}

/// 日历面板（内部复用：单日期与区间共用）。
#[component]
fn CalendarPanel(
    /// 当前展示的 (年, 月)
    visible: RwSignal<(i32, u32)>,
    /// 起点（单日期模式下就是选中值）
    #[prop(into)]
    start: Signal<String>,
    /// 终点（单日期模式恒为空串）
    #[prop(into)]
    end: Signal<String>,
    /// 选中回调（传回 `YYYY-MM-DD`）
    #[prop(into)]
    on_pick: UnsyncCallback<String>,
) -> impl IntoView {
    let today_value = today().to_string_padded();
    let today_for_class = today_value.clone();

    let title = move || {
        let (year, month) = visible.get();
        format!("{year}年{month}月")
    };

    let shift = move |delta: i32| {
        visible.update(|(year, month)| {
            let (next_year, next_month) = add_months(*year, *month, delta);
            *year = next_year;
            *month = next_month;
        });
    };

    view! {
        <div class="ui-date-picker__panel">
            <div class="ui-date-picker__header">
                <button
                    type="button"
                    class="ui-date-picker__nav"
                    title="上一月"
                    aria-label="上一月"
                    on:click=move |_| shift(-1)
                >
                    {icons::icon(Icon::Left)}
                </button>
                <button
                    type="button"
                    class="ui-date-picker__title"
                    title="回到今天"
                    on:click=move |_| {
                        let now = today();
                        visible.set((now.year, now.month));
                    }
                >
                    {title}
                </button>
                <button
                    type="button"
                    class="ui-date-picker__nav"
                    title="下一月"
                    aria-label="下一月"
                    on:click=move |_| shift(1)
                >
                    {icons::icon(Icon::Right)}
                </button>
            </div>

            <div class="ui-date-picker__weekdays">
                {WEEKDAYS.iter().map(|name| view! { <span>{*name}</span> }).collect_view()}
            </div>

            <div class="ui-date-picker__grid">
                {move || {
                    let (year, month) = visible.get();
                    let start_value = start.get();
                    let end_value = end.get();
                    let today_value = today_for_class.clone();
                    month_grid(year, month)
                        .into_iter()
                        .map(|cell| {
                            let value = cell.date.to_string_padded();
                            let is_start = !start_value.is_empty() && value == start_value;
                            let is_end = !end_value.is_empty() && value == end_value;
                            let in_range = !start_value.is_empty() && !end_value.is_empty()
                                && value > start_value && value < end_value;
                            let is_today = value == today_value;
                            let click_value = value.clone();
                            view! {
                                <button
                                    type="button"
                                    class="ui-date-picker__cell"
                                    class:is-outside=cell.outside
                                    class:is-today=is_today
                                    class:is-selected=is_start || is_end
                                    class:is-in-range=in_range
                                    on:click=move |_| on_pick.run(click_value.clone())
                                >
                                    {cell.date.day}
                                </button>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

/// 当前值对应的展示月份（空串 → 今天所在月）。
fn month_of(value: &str) -> (i32, u32) {
    match parse_ymd(value) {
        Some(date) => (date.year, date.month),
        None => {
            let now = today();
            (now.year, now.month)
        }
    }
}

// ---------------------------------------------------------------- 单日期

#[component]
pub fn DatePicker(
    /// 选中值（`YYYY-MM-DD`；空串表示未选）
    value: RwSignal<String>,
    /// 未选中时的占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
    /// 允许清空
    #[prop(optional)]
    allow_clear: bool,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<String>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));
    let open = RwSignal::new(false);
    let visible = RwSignal::new(month_of(&value.get_untracked()));
    let placeholder = placeholder.unwrap_or_else(|| "请选择日期".to_string());
    // 单日期模式：`end` 恒为空（面板只用得到"起点"）
    let empty_end = Signal::derive(String::new);

    let display = move || {
        let current = value.get();
        if current.is_empty() {
            placeholder.clone()
        } else {
            current
        }
    };

    let mut classes = String::from("ui-date-picker");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-open=move || open.get()>
            <button
                type="button"
                class="ui-date-picker__trigger"
                disabled=move || disabled.get()
                on:click=move |_| {
                    if disabled.get_untracked() {
                        return;
                    }
                    if !open.get_untracked() {
                        visible.set(month_of(&value.get_untracked()));
                    }
                    open.update(|v| *v = !*v);
                }
            >
                <span class="ui-date-picker__icon">{icons::icon(Icon::ClockCircle)}</span>
                <span class="ui-date-picker__value" class:is-placeholder=move || value.get().is_empty()>
                    {display}
                </span>
                <Show when=move || allow_clear && !value.get().is_empty()>
                    <span
                        class="ui-input__clear"
                        role="button"
                        title="清空"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            value.set(String::new());
                            if let Some(callback) = on_change {
                                callback.run(String::new());
                            }
                        }
                    >
                        {icons::icon(Icon::CloseCircle)}
                    </span>
                </Show>
            </button>

            <Show when=move || open.get()>
                <div class="ui-select__backdrop" on:click=move |_| open.set(false)></div>
                <div class="ui-date-picker__dropdown">
                    <CalendarPanel
                        visible=visible
                        start=Signal::derive(move || value.get())
                        end=empty_end
                        on_pick=UnsyncCallback::new(move |picked: String| {
                            value.set(picked.clone());
                            open.set(false);
                            if let Some(callback) = on_change {
                                callback.run(picked);
                            }
                        })
                    />
                    <div class="ui-date-picker__footer">
                        <button
                            type="button"
                            class="ui-btn ui-btn--link ui-btn--sm"
                            on:click=move |_| {
                                let now = today();
                                let picked = now.to_string_padded();
                                visible.set((now.year, now.month));
                                value.set(picked.clone());
                                open.set(false);
                                if let Some(callback) = on_change {
                                    callback.run(picked);
                                }
                            }
                        >
                            "今天"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------- 日期区间

#[component]
pub fn DateRangePicker(
    /// 起点（`YYYY-MM-DD`）
    start: RwSignal<String>,
    /// 终点（`YYYY-MM-DD`）
    end: RwSignal<String>,
    /// 未选择时的占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
    /// 允许清空
    #[prop(optional)]
    allow_clear: bool,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 变化回调：`(start, end)`
    #[prop(optional, into)]
    on_change: Option<UnsyncCallback<(String, String)>>,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));
    let open = RwSignal::new(false);
    let visible = RwSignal::new(month_of(&start.get_untracked()));
    let placeholder = placeholder.unwrap_or_else(|| "请选择时间范围".to_string());

    let display = move || {
        let from = start.get();
        let to = end.get();
        if from.is_empty() && to.is_empty() {
            placeholder.clone()
        } else if to.is_empty() {
            format!("{from} ~")
        } else {
            format!("{from} ~ {to}")
        }
    };

    let mut classes = String::from("ui-date-picker ui-date-range-picker");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes class:is-open=move || open.get()>
            <button
                type="button"
                class="ui-date-picker__trigger"
                disabled=move || disabled.get()
                on:click=move |_| {
                    if disabled.get_untracked() {
                        return;
                    }
                    if !open.get_untracked() {
                        visible.set(month_of(&start.get_untracked()));
                    }
                    open.update(|v| *v = !*v);
                }
            >
                <span class="ui-date-picker__icon">{icons::icon(Icon::ClockCircle)}</span>
                <span
                    class="ui-date-picker__value"
                    class:is-placeholder=move || start.get().is_empty() && end.get().is_empty()
                >
                    {display}
                </span>
                <Show when=move || allow_clear && (!start.get().is_empty() || !end.get().is_empty())>
                    <span
                        class="ui-input__clear"
                        role="button"
                        title="清空"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            start.set(String::new());
                            end.set(String::new());
                            if let Some(callback) = on_change {
                                callback.run((String::new(), String::new()));
                            }
                        }
                    >
                        {icons::icon(Icon::CloseCircle)}
                    </span>
                </Show>
            </button>

            <Show when=move || open.get()>
                <div class="ui-select__backdrop" on:click=move |_| open.set(false)></div>
                <div class="ui-date-picker__dropdown">
                    <CalendarPanel
                        visible=visible
                        start=Signal::derive(move || start.get())
                        end=Signal::derive(move || end.get())
                        on_pick=UnsyncCallback::new(move |picked: String| {
                            // 第一次点击定起点，第二次定终点；点得比起点早则重开一轮
                            let current_start = start.get_untracked();
                            let current_end = end.get_untracked();
                            if current_start.is_empty() || !current_end.is_empty() {
                                start.set(picked.clone());
                                end.set(String::new());
                                if let Some(callback) = on_change {
                                    callback.run((picked, String::new()));
                                }
                            } else if picked.as_str() < current_start.as_str() {
                                start.set(picked.clone());
                                end.set(current_start.clone());
                                open.set(false);
                                if let Some(callback) = on_change {
                                    callback.run((picked, current_start));
                                }
                            } else {
                                end.set(picked.clone());
                                open.set(false);
                                if let Some(callback) = on_change {
                                    callback.run((current_start, picked));
                                }
                            }
                        })
                    />
                    <div class="ui-date-picker__footer">
                        <span class="ui-date-picker__hint">"先点开始，再点结束"</span>
                        <button
                            type="button"
                            class="ui-btn ui-btn--link ui-btn--sm"
                            on:click=move |_| {
                                start.set(String::new());
                                end.set(String::new());
                                if let Some(callback) = on_change {
                                    callback.run((String::new(), String::new()));
                                }
                            }
                        >
                            "清除"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
