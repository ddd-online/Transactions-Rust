//! 时间格式化。
//!
//! 对照原 `app/src/backend/functions.ts` 的 `formatTimestamp(timestamp, format)`：
//! `dayjs(timestamp * 1000).format(format)` —— 即**本地时区**格式化。
//!
//! 为什么用 `js_sys::Date` 而不是自己按秒算：dayjs 走的是宿主本地时区，
//! Rust 侧没有时区数据库，手算必然在跨时区/夏令时上与原实现分叉。
//! `js_sys::Date` 的 `get_month()` / `get_date()` 等访问器同样是本地时区语义，
//! 与 dayjs 完全一致。
//!
//! 支持的占位符（dayjs 常用子集，够当前界面使用）：
//! `YYYY` 年、`MM` 月、`DD` 日、`HH` 时、`mm` 分、`ss` 秒。

use wasm_bindgen::JsValue;

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
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // 0 基月份 + 本地时区（与原实现用 dayjs 解析同一串的行为一致）
    let date = js_sys::Date::new_with_year_month_day(year, month as i32 - 1, day as i32);
    Some((date.get_time() / 1000.0) as i64)
}

/// `YYYY-MM` 串 → 该月 1 日 00:00:00（本地时区）的 Unix 秒。
pub fn ym_to_seconds(input: &str) -> Option<i64> {
    let trimmed = input.trim();
    let (year, month) = trimmed.split_once('-')?;
    let year: u32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let date = js_sys::Date::new_with_year_month_day(year, month as i32 - 1, 1);
    Some((date.get_time() / 1000.0) as i64)
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

/// `YYYY-MM-DD` → `2026年6月19日`（原 `dayjs(date).format('YYYY年M月D日')`）。
///
/// 解析失败时原样返回输入（不 panic）。
pub fn format_ymd_cn(input: &str) -> String {
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return input.to_string();
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return input.to_string();
    };
    // 去掉前导零：M / D（dayjs 的 M/D 占位符语义）
    format!("{year}年{month}月{day}日")
}

/// `YYYY-MM-DD` → 星期中文名（原 `dayjs(date).format('dddd')` + `zh-cn` locale）。
///
/// 解析失败返回空串。
pub fn weekday_cn(input: &str) -> String {
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return String::new();
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return String::new();
    };
    let date = js_sys::Date::new_with_year_month_day(year, month as i32 - 1, day as i32);
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
