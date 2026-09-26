//! 时间格式化。
//!
//! 全部按**宿主（WebView）的本地时区**格式化，与后端的 Unix 秒语义对应。
//!
//! 为什么用 `js_sys::Date` 而不是自己按秒算：本地时区/夏令时只有宿主知道，
//! Rust 侧没有时区数据库，手算必然在跨时区、夏令时上分叉。
//! `js_sys::Date` 的 `get_month()` / `get_date()` 等访问器同样是本地时区语义。
//!
//! 支持的占位符：`YYYY` 年、`MM` 月、`DD` 日、`HH` 时、`mm` 分、`ss` 秒
//! ——够当前界面使用。

use wasm_bindgen::JsValue;

/// 一天的秒数（时间范围翻页等纯整数计算用；不做时区换算）。
pub const DAY_SECONDS: i64 = 86_400;

/// 秒级时间戳 → 本地时间格式化字符串。
///
/// ```ignore
/// format_timestamp(1_700_000_000, "MM-DD")  // 形如 "11-15"
/// format_timestamp(1_700_000_000, "")       // 默认 "YYYY-MM-DD"
/// ```
pub fn format_timestamp(timestamp: i64, format: &str) -> String {
    let format = if format.is_empty() {
        "YYYY-MM-DD"
    } else {
        format
    };
    let date = js_sys::Date::new(&JsValue::from_f64(timestamp as f64 * 1000.0));

    // 注意：js_sys 的 getter 是「带 this 的方法」，即实例方法，返回 u32。
    let year = date.get_full_year();
    let month = date.get_month() + 1;
    let day = date.get_date();
    let hour = date.get_hours();
    let minute = date.get_minutes();
    let second = date.get_seconds();

    // 按占位符替换；顺序扫描，避免 "YYYY" 被 "YY" 之类的规则二次命中。
    let mut out = String::with_capacity(format.len() + 8);
    let chars: Vec<char> = format.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let rest = &chars[index..];
        if rest.starts_with(&['Y', 'Y', 'Y', 'Y']) {
            out.push_str(&year.to_string());
            index += 4;
        } else if rest.starts_with(&['M', 'M']) {
            out.push_str(&pad2(month));
            index += 2;
        } else if rest.starts_with(&['D', 'D']) {
            out.push_str(&pad2(day));
            index += 2;
        } else if rest.starts_with(&['H', 'H']) {
            out.push_str(&pad2(hour));
            index += 2;
        } else if rest.starts_with(&['m', 'm']) {
            out.push_str(&pad2(minute));
            index += 2;
        } else if rest.starts_with(&['s', 's']) {
            out.push_str(&pad2(second));
            index += 2;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn pad2(value: u32) -> String {
    format!("{value:02}")
}

/// 当前本地日期的 `YYYY-MM-DD`（等价 `formatTimestamp(now, 'YYYY-MM-DD')`）。
pub fn today_ymd() -> String {
    format_timestamp(now_seconds(), "YYYY-MM-DD")
}

/// 当前 Unix 秒。
pub fn now_seconds() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

/// `YYYY-MM-DD` 串 → 当天 00:00:00（本地时区）的 Unix 秒。
///
/// 解析失败返回 `None`（调用方决定提示文案）。
pub fn ymd_to_seconds(input: &str) -> Option<i64> {
    let (year, month, day) = split_ymd(input)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // 0 基月份 + 本地时区
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32);
    Some((date.get_time() / 1000.0) as i64)
}

/// `YYYY-MM-DD` 区间 → **闭区间** Unix 秒 `(起点, 终点)`：
/// 起点取当天 00:00:00、终点取当天 23:59:59（本地时区）。
///
/// 查询区间只有这一份实现（消费记录页的 `tr_query` 与分析子功能的图表查询共用），
/// 任一端解析失败返回 `None`——调用方各自决定是"不发查询"还是"发空区间"。
pub fn range_to_seconds(from: &str, to: &str) -> Option<(i64, i64)> {
    let start = ymd_to_seconds(from)?;
    let end = ymd_to_seconds(to)?;
    Some((start, end + DAY_SECONDS - 1))
}

/// 星期中文名（`js_sys::Date::get_day()` 的 0 = 周日）。
const WEEKDAY_CN: [&str; 7] = [
    "星期日",
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
];

/// `YYYY-MM-DD` → `2026年6月19日`。
///
/// 解析失败时原样返回输入（不 panic）。
pub fn format_ymd_cn(input: &str) -> String {
    // 去掉前导零：月 / 日
    match split_ymd(input) {
        Some((year, month, day)) => format!("{year}年{month}月{day}日"),
        None => input.to_string(),
    }
}

/// `YYYY-MM-DD` → 星期中文名（中文 locale）。
///
/// 解析失败返回空串。
pub fn weekday_cn(input: &str) -> String {
    let Some((year, month, day)) = split_ymd(input) else {
        return String::new();
    };
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32);
    WEEKDAY_CN
        .get(date.get_day() as usize)
        .copied()
        .unwrap_or_default()
        .to_string()
}

/// 拆分 `YYYY-MM-DD` → `(年, 月, 日)`（解析失败返回 `None`）。
pub fn split_ymd(input: &str) -> Option<(i32, u32, u32)> {
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((year, month, day))
}
