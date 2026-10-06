//! 公历日历的纯算法：解析 / 格式化、月长、加减月与天、周与区间对齐、按粒度翻周期。
//!
//! 这里全是**无时区的公历事实**：`2026-06-19` 是星期五、2024 年 2 月有 29 天、
//! `2026-01-30` 到 `2026-02-05` 恰好 6 天 —— 这些答案与宿主在哪个时区、有没有夏令时无关。
//! 从前它们散在界面侧（`components/ui/date_picker.rs` 与 `time_range_picker.rs` 用
//! `js_sys::Date` 的"下月第 0 天"取月长、用本地秒差判"整周"），于是既不能在
//! `cargo test` 里断言，也会在夏令时切换那一周算出错的"整周"判定（见 [`is_six_days`]）。
//!
//! **留在界面侧的是另一件事**：`时间戳 → 本地时间`（`tr-ui::time::format_timestamp` /
//! `today_ymd` / `now_seconds` / `ymd_to_seconds`）—— 那需要一个时区数据库，只有宿主有。
//! 分界就是这一条：**日期串之间的算术在这里，日期串与 Unix 秒之间的换算在界面侧。**
//!
//! 见 `docs/adr/0001-pure-draw-crate.md`。

/// 一个公历日期（月 / 日都是 1 起）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ymd {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Ymd {
    /// `YYYY-MM-DD`（补零；字符串比较即时间先后比较）。
    pub fn padded(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// 解析完整的 `YYYY-MM-DD`；月 / 日越界或段数不对返回 `None`。
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

/// 解析 `年-月-…` 的**前两段**（时间范围选择器的月 / 年粒度只需要年月）。
///
/// 第三段（日）的内容不校验：`2026-01-xx` → `Some((2026, 1))`。
/// 但形状必须是两段以上（`2026-06` → `None`）—— 与搬过来之前的实现逐字一致，
/// 调用方拿到的值永远来自日期选择器或本模块的区间对齐，不存在"只有年月"的输入。
pub fn parse_year_month(input: &str) -> Option<(i32, u32)> {
    let trimmed = input.trim();
    let (year, rest) = trimmed.split_once('-')?;
    let (month, _) = rest.split_once('-')?;
    let year: i32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// `YYYY-MM-DD` → `2026年6月19日`（解析失败原样返回）。
pub fn format_ymd_cn(input: &str) -> String {
    match parse_ymd(input) {
        Some(date) => format!("{}年{}月{}日", date.year, date.month, date.day),
        None => input.to_string(),
    }
}

/// 星期中文名（`YYYY-MM-DD`；解析失败返回空串）。
pub fn weekday_cn(input: &str) -> String {
    match parse_ymd(input) {
        Some(date) => {
            WEEKDAY_CN[weekday_index(date.year, date.month, date.day) as usize].to_string()
        }
        None => String::new(),
    }
}

const WEEKDAY_CN: [&str; 7] = [
    "星期日",
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
];

/// 某年某月的天数（公历闰年规则：4 年一闰、100 年不闰、400 年又闰）。
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // 非法月份：调用方要么已经校验过，要么会落到"整月对齐"的兜底分支
        _ => 30,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 月份加减（`delta` 可负）。
pub fn add_months(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let total = year * 12 + (month as i32 - 1) + delta;
    (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

/// 某月 1 号是星期几（0 = 周一 … 6 = 周日）—— 月历网格的起始偏移。
pub fn monday_offset(year: i32, month: u32) -> u32 {
    // `weekday_index` 是周日开始的（与 JS 的 `get_day()` 同序），这里换算成周一起始
    (weekday_index(year, month, 1) + 6) % 7
}

/// 星期几（0 = 周日 … 6 = 周六）—— 公历事实，与宿主时区无关。
///
/// 用 1970-01-01（星期四）当锚点：日序号 0 对应星期四，所以 (日序号 + 4) mod 7 就是星期几。
fn weekday_index(year: i32, month: u32, day: u32) -> u32 {
    (day_number(Ymd { year, month, day }) + 4).rem_euclid(7) as u32
}

/// 公历日序号（1970-01-01 = 0）：两个日期相减就是相差的天数。
///
/// 用 Howard Hinnant 的 days_from_civil：把 3 月当一年之首，闰年那一天落在年末，
/// 就退化成"每 400 年 146097 天"的整数运算 —— 没有循环，也没有时区。
fn day_number(date: Ymd) -> i64 {
    let year = if date.month <= 2 {
        date.year as i64 - 1
    } else {
        date.year as i64
    };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = ((date.month + 9) % 12) as i64;
    let day_of_year = (153 * shifted_month + 2) / 5 + date.day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// 日序号 → 公历日期（[`day_number`] 的逆）。
fn date_from_day_number(days: i64) -> Ymd {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    Ymd {
        year: (year + if month <= 2 { 1 } else { 0 }) as i32,
        month,
        day,
    }
}

/// 日期串加 / 减天数（跨月跨年由日序号负责；解析失败返回 `None`）。
pub fn add_days(ymd: &str, days: i64) -> Option<String> {
    let date = parse_ymd(ymd)?;
    Some(date_from_day_number(day_number(date) + days).padded())
}

/// 两个日期相差的天数（`to - from`，可为负；解析失败返回 `None`）。
///
/// 与 [`is_six_days`] 同一口径：按日序号算，不看时刻 —— 界面侧的
/// "已过 N 天"（`todo.rs::elapsed_days`）从前也是拿本地秒差除以 86400，
/// 夏令时那一周会少算一天。
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    let from = parse_ymd(from)?;
    let to = parse_ymd(to)?;
    Some(day_number(to) - day_number(from))
}

/// 区间是否恰好 6 天（即"整周"）—— 按**日序号差**判，不看时刻。
///
/// 从前这里算的是"两端本地午夜相差几个 86400 秒"，夏令时切换那一周会少/多一小时，
/// 整数除法把 6 天 23 小时截成 5 ⇒ 整周被判成单日、翻页只挪一天。
pub fn is_six_days(start: &str, end: &str) -> bool {
    match (parse_ymd(start), parse_ymd(end)) {
        (Some(from), Some(to)) => day_number(to) - day_number(from) == 6,
        _ => false,
    }
}

/// 某月的时间范围（1 号 ~ 月末）。
pub fn month_span(year: i32, month: u32) -> (String, String) {
    let last = days_in_month(year, month);
    (
        format!("{year:04}-{month:02}-01"),
        format!("{year:04}-{month:02}-{last:02}"),
    )
}

/// 某年的时间范围（1 月 1 日 ~ 12 月 31 日）。
pub fn year_span(year: i32) -> (String, String) {
    (format!("{year:04}-01-01"), format!("{year:04}-12-31"))
}

/// 某天所在周的（周一, 周日）—— 一周从周一算起。
pub fn week_bounds(ymd: &str) -> Option<(String, String)> {
    let date = parse_ymd(ymd)?;
    let monday_offset_days = ((weekday_index(date.year, date.month, date.day) + 6) % 7) as i64;
    let monday = date_from_day_number(day_number(date) - monday_offset_days);
    let sunday = date_from_day_number(day_number(monday) + 6);
    Some((monday.padded(), sunday.padded()))
}

/// 按粒度对齐区间：月 → 整月、年 → 整年、日 → 原样。
///
/// 对齐不出合法区间（解析失败 / 起点晚于终点）时原样返回 —— 调用方据此保持"待选"状态。
pub fn normalize_range(start: &str, end: &str, mode: &str) -> (String, String) {
    let align_start = |ymd: &str| -> String {
        parse_year_month(ymd)
            .map(|(year, month)| match mode {
                "month" => month_span(year, month).0,
                _ => year_span(year).0,
            })
            .unwrap_or_default()
    };
    let align_end = |ymd: &str| -> String {
        parse_year_month(ymd)
            .map(|(year, month)| match mode {
                "month" => month_span(year, month).1,
                _ => year_span(year).1,
            })
            .unwrap_or_default()
    };
    let (from, to) = match mode {
        "month" | "year" => (align_start(start), align_end(end)),
        _ => (start.to_string(), end.to_string()),
    };
    if from.is_empty() || to.is_empty() || from > to {
        (start.to_string(), end.to_string())
    } else {
        (from, to)
    }
}

/// 按粒度前后翻一个周期：
///
/// * `date`：区间正好 6 天 → 整周（±7 天）；否则 ±1 天
/// * `month`：`start ± 1 月` 的月初 / `end ± 1 月` 的月末
/// * `year`：`start ± 1 年` 的年初 / `end ± 1 年` 的年末
///
/// 解析失败时原样返回（调用方不必先校验）。
pub fn shift_period(start: &str, end: &str, mode: &str, direction: i32) -> (String, String) {
    let unchanged = || (start.to_string(), end.to_string());
    match mode {
        "month" => {
            let (Some((start_year, start_month)), Some((end_year, end_month))) =
                (parse_year_month(start), parse_year_month(end))
            else {
                return unchanged();
            };
            let (next_start_year, next_start_month) =
                add_months(start_year, start_month, direction);
            let (next_end_year, next_end_month) = add_months(end_year, end_month, direction);
            (
                month_span(next_start_year, next_start_month).0,
                month_span(next_end_year, next_end_month).1,
            )
        }
        "year" => {
            let (Some((start_year, _)), Some((end_year, _))) =
                (parse_year_month(start), parse_year_month(end))
            else {
                return unchanged();
            };
            (
                year_span(start_year + direction).0,
                year_span(end_year + direction).1,
            )
        }
        _ => {
            let days = if is_six_days(start, end) { 7 } else { 1 } * i64::from(direction);
            match (add_days(start, days), add_days(end, days)) {
                (Some(from), Some(to)) => (from, to),
                _ => unchanged(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 月长：闰年规则四条分支都要走到（含"百年不闰、四百年又闰"）。
    #[test]
    fn month_length_follows_the_gregorian_leap_rule() {
        assert_eq!(days_in_month(2024, 2), 29, "4 年一闰");
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28, "100 年不闰");
        assert_eq!(days_in_month(2000, 2), 29, "400 年又闰");
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn month_arithmetic_wraps_across_years_and_handles_negative_deltas() {
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 3, -14), (2025, 1));
        assert_eq!(add_months(2026, 6, 0), (2026, 6));
        assert_eq!(add_months(2026, 1, 12), (2027, 1));
    }

    /// 日序号是这一切的底座：往返必须恒等（跨闰年、跨世纪都要对）。
    #[test]
    fn day_numbers_round_trip_over_two_centuries() {
        let mut date = Ymd {
            year: 1900,
            month: 1,
            day: 1,
        };
        // 从 1900-01-01 起连续走 200 年：每一步都要求"序号 → 日期"回到原处
        for days in day_number(date)..day_number(date) + 73_049 {
            assert_eq!(date_from_day_number(days), date, "{date:?}");
            let (year, month) = (date.year, date.month);
            date.day += 1;
            if date.day > days_in_month(year, month) {
                date.day = 1;
                let (next_year, next_month) = add_months(year, month, 1);
                date.year = next_year;
                date.month = next_month;
            }
        }
        assert_eq!(date.year, 2100, "走了 200 年");
    }

    #[test]
    fn parsing_is_strict_about_the_shape_but_tolerant_about_padding() {
        assert_eq!(
            parse_ymd("2026-06-19"),
            Some(Ymd {
                year: 2026,
                month: 6,
                day: 19
            })
        );
        assert_eq!(
            parse_ymd("2026-6-9"),
            Some(Ymd {
                year: 2026,
                month: 6,
                day: 9
            }),
            "不补零也认（既有行为）"
        );
        assert_eq!(
            parse_ymd(" 2026-06-19 "),
            parse_ymd("2026-06-19"),
            "两侧空白忽略"
        );
        assert_eq!(parse_ymd("2026-13-01"), None, "月份越界");
        assert_eq!(parse_ymd("2026-06-32"), None, "日子越界");
        assert_eq!(parse_ymd("2026-06"), None, "缺日");
        assert_eq!(parse_ymd("2026-06-19-1"), None, "多一段");
        assert_eq!(parse_ymd(""), None);

        // 年月解析：只取前两段，第三段内容不校验；但形状必须有两段以上
        assert_eq!(parse_year_month("2026-06-19"), Some((2026, 6)));
        assert_eq!(
            parse_year_month("2026-06-xx"),
            Some((2026, 6)),
            "日非法不影响年月"
        );
        assert_eq!(
            parse_year_month("2026-06"),
            None,
            "只有年月：与既有行为一致"
        );
        assert_eq!(parse_year_month("2026"), None);
        assert_eq!(parse_year_month("2026-13"), None, "月份越界");
    }

    /// 星期是公历事实（取几个公认的锚点）。
    #[test]
    fn weekdays_are_gregorian_facts() {
        assert_eq!(weekday_cn("1970-01-01"), "星期四");
        assert_eq!(weekday_cn("2000-01-01"), "星期六");
        assert_eq!(weekday_cn("2024-02-29"), "星期四");
        assert_eq!(weekday_cn("2026-01-01"), "星期四");
        assert_eq!(weekday_cn("2026-06-19"), "星期五");
        assert_eq!(weekday_cn("坏日期"), "");
        // 周一起始：周一到周日依次是 一…日
        assert_eq!(weekday_cn("2026-06-15"), "星期一");
        assert_eq!(weekday_cn("2026-06-21"), "星期日");
        assert_eq!(monday_offset(2026, 6), 0, "2026-06-01 是周一");
        assert_eq!(monday_offset(2026, 2), 6, "2026-02-01 是周日");
    }

    #[test]
    fn chinese_formatting_falls_back_to_the_input() {
        assert_eq!(format_ymd_cn("2026-06-19"), "2026年6月19日");
        assert_eq!(format_ymd_cn("2026-06-09"), "2026年6月9日");
        assert_eq!(format_ymd_cn("不是日期"), "不是日期");
    }

    #[test]
    fn adding_days_crosses_months_and_years() {
        assert_eq!(add_days("2025-12-31", 1).as_deref(), Some("2026-01-01"));
        assert_eq!(add_days("2026-01-01", -1).as_deref(), Some("2025-12-31"));
        assert_eq!(
            add_days("2024-02-28", 1).as_deref(),
            Some("2024-02-29"),
            "闰年"
        );
        assert_eq!(add_days("2025-02-28", 1).as_deref(), Some("2025-03-01"));
        assert_eq!(add_days("2026-06-19", 7).as_deref(), Some("2026-06-26"));
        assert_eq!(add_days("2026-06-19", -6).as_deref(), Some("2026-06-13"));
        assert_eq!(add_days("nope", 1), None);
    }

    #[test]
    fn the_six_day_rule_counts_calendar_days() {
        assert!(is_six_days("2026-06-15", "2026-06-21"), "周一 ~ 周日");
        assert!(!is_six_days("2026-06-19", "2026-06-19"), "同一天");
        assert!(!is_six_days("2026-06-14", "2026-06-21"), "7 天");
        assert!(is_six_days("2026-01-30", "2026-02-05"), "跨月");
        assert!(is_six_days("2024-02-26", "2024-03-03"), "跨闰日");
        assert!(!is_six_days("坏", "2026-06-21"));
    }

    #[test]
    fn bounds_align_to_month_year_and_week() {
        assert_eq!(
            month_span(2026, 2),
            ("2026-02-01".to_string(), "2026-02-28".to_string())
        );
        assert_eq!(
            month_span(2024, 2),
            ("2024-02-01".to_string(), "2024-02-29".to_string())
        );
        assert_eq!(
            year_span(2026),
            ("2026-01-01".to_string(), "2026-12-31".to_string())
        );
        assert_eq!(
            week_bounds("2026-06-19"),
            Some(("2026-06-15".to_string(), "2026-06-21".to_string()))
        );
        assert_eq!(
            week_bounds("2026-06-15"),
            Some(("2026-06-15".to_string(), "2026-06-21".to_string())),
            "周一自己"
        );
        assert_eq!(
            week_bounds("2026-06-21"),
            Some(("2026-06-15".to_string(), "2026-06-21".to_string())),
            "周日自己"
        );
        assert_eq!(week_bounds("坏"), None);
    }

    #[test]
    fn day_differences_are_signed_and_calendar_based() {
        assert_eq!(days_between("2026-06-15", "2026-06-21"), Some(6));
        assert_eq!(days_between("2026-06-21", "2026-06-15"), Some(-6));
        assert_eq!(days_between("2026-06-19", "2026-06-19"), Some(0));
        assert_eq!(
            days_between("2024-02-28", "2024-03-01"),
            Some(2),
            "闰日算一天"
        );
        assert_eq!(days_between("2025-12-31", "2026-01-01"), Some(1));
        assert_eq!(days_between("坏", "2026-01-01"), None);
    }

    #[test]
    fn normalize_range_aligns_or_stays_put() {
        assert_eq!(
            normalize_range("2026-02-15", "2026-03-02", "month"),
            ("2026-02-01".to_string(), "2026-03-31".to_string())
        );
        assert_eq!(
            normalize_range("2026-02-15", "2026-03-02", "year"),
            ("2026-01-01".to_string(), "2026-12-31".to_string())
        );
        assert_eq!(
            normalize_range("2026-02-15", "2026-03-02", "date"),
            ("2026-02-15".to_string(), "2026-03-02".to_string()),
            "日粒度不动"
        );
        // 反过来的区间（起点晚于终点）原样返回，交给调用方当"待选"处理
        assert_eq!(
            normalize_range("2026-03-02", "2026-02-15", "month"),
            ("2026-03-02".to_string(), "2026-02-15".to_string())
        );
        assert_eq!(
            normalize_range("坏", "2026-02-15", "month"),
            ("坏".to_string(), "2026-02-15".to_string())
        );
    }

    #[test]
    fn shifting_a_period_moves_one_unit_unless_it_is_a_whole_week() {
        // 月粒度：月初 / 月末都跟着粒度走（2 月要夹到 28 天）
        assert_eq!(
            shift_period("2026-01-01", "2026-01-31", "month", 1),
            ("2026-02-01".to_string(), "2026-02-28".to_string())
        );
        assert_eq!(
            shift_period("2026-01-01", "2026-01-31", "month", -1),
            ("2025-12-01".to_string(), "2025-12-31".to_string())
        );
        // 年粒度
        assert_eq!(
            shift_period("2026-01-01", "2026-12-31", "year", 1),
            ("2027-01-01".to_string(), "2027-12-31".to_string())
        );
        // 日粒度：整周挪 7 天，否则挪 1 天
        assert_eq!(
            shift_period("2026-06-15", "2026-06-21", "date", 1),
            ("2026-06-22".to_string(), "2026-06-28".to_string())
        );
        assert_eq!(
            shift_period("2026-06-19", "2026-06-19", "date", 1),
            ("2026-06-20".to_string(), "2026-06-20".to_string())
        );
        assert_eq!(
            shift_period("2026-06-15", "2026-06-21", "date", -1),
            ("2026-06-08".to_string(), "2026-06-14".to_string())
        );
        // 解析失败原样返回
        assert_eq!(
            shift_period("坏", "2026-06-21", "month", 1),
            ("坏".to_string(), "2026-06-21".to_string())
        );
        assert_eq!(
            shift_period("坏", "2026-06-21", "date", 1),
            ("坏".to_string(), "2026-06-21".to_string())
        );
    }
}
