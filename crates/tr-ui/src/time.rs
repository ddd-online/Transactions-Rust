//! 时间格式化：**日期串 ↔ Unix 秒**这一段（需要宿主时区的那半）。
//!
//! 全部按**宿主（WebView）的本地时区**换算，与后端的 Unix 秒语义对应。
//!
//! 为什么用 `js_sys::Date` 而不是自己按秒算：本地时区/夏令时只有宿主知道，
//! Rust 侧没有时区数据库，手算必然在跨时区、夏令时上分叉。
//! `js_sys::Date` 的 `get_month()` / `get_date()` 等访问器同样是本地时区语义。
//!
//! **纯公历的那一半不在这儿**：解析 / 月长 / 加减月与天 / 周几 / 区间对齐都在
//! `tr_draw::calendar`（native 上 `cargo test -p tr-draw` 真跑）。判据是"要不要时区数据库"：
//! `2026-06-19` 是星期五与宿主无关，而"这个时间戳在本地是几号"只有宿主知道。
//!
//! 支持的占位符：`YYYY` 年、`MM` 月、`DD` 日、`HH` 时、`mm` 分、`ss` 秒
//! ——够当前界面使用。

use tr_draw::calendar::parse_ymd;
use wasm_bindgen::JsValue;

/// 一天的秒数（`range_to_seconds` 把"闭区间终点"补到 23:59:59 用它）。
const DAY_SECONDS: i64 = 86_400;

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
    let date = parse_ymd(input)?;
    // 0 基月份 + 本地时区
    let date = js_sys::Date::new_with_year_month_day(
        date.year as u32,
        date.month as i32 - 1,
        date.day as i32,
    );
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
