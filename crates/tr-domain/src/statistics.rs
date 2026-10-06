//! 统计取数的**筛选条件**：`recent` 的取值规则、月份区间的归一化。
//!
//! 从 `tr-ipc/src/commands/stock.rs:463-473` 搬来（候选 6 / #35）。搬的理由是那句注释与代码
//! **互相矛盾**，而只有代码是对的：
//!
//! ```text
//! // 空串按"没传"处理（保持宽松）；非数字串或 <= 0 一律报错
//! ```
//!
//! 实际行为（`query_number_as_i64` 把解析失败的文本都折成 `None`）：
//!
//! * 字段缺失 / `null` / 空串 / **非数字串** → 都按"没传"处理（不限）；
//! * 数字 `> 0` → 取该值；
//! * 数字 `<= 0` → 报错 `recent 必须为正整数`。
//!
//! 也就是说非数字串**不会**报错。这条差别在界面上看不出来（界面只送数字），但它是契约的一部分，
//! 所以这里逐条钉住，别再让注释与代码打架。

/// `recent <= 0` 时的错误文案（与 `tr-ipc` 的 `AppError::bad_request` 用同一句）。
pub const RECENT_NOT_POSITIVE: &str = "recent 必须为正整数";

/// 月区间与"最近 N 笔"同时给时的错误文案（服务层原来硬编码这一句）。
pub const RANGE_AND_RECENT: &str = "时间范围与笔数筛选不能同时使用";

/// 文本 → 数字那一半：**解析失败一律 `None`**（宽松，与既有行为逐字一致；不做 trim）。
pub fn parse_recent_text(raw: &str) -> Option<i64> {
    raw.parse::<i64>().ok()
}

/// 数字之后那一半：`None` = 不限，正数 = 取该值，其余报错。
pub fn normalize_recent(parsed: Option<i64>) -> Result<Option<i64>, &'static str> {
    match parsed {
        None => Ok(None),
        Some(value) if value > 0 => Ok(Some(value)),
        Some(_) => Err(RECENT_NOT_POSITIVE),
    }
}

/// 月份区间的归一化：空串 = 不限；只填一端时另一端不限（半开）。
///
/// 返回 `None` 表示这一端不限。
pub fn normalize_month(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// 一端筛选条件的三段（月份区间 / 最近 N 笔 / 标签）。
///
/// 今天这三段以裸形参在界面 → `tr-ipc` → `tr-service` 之间传递（`get_statistics_range` 收 6 个形参），
/// 靠位置对齐；这个类型是它们**同一份**的样子，也是"哪一端不限"的唯一判据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatisticsFilter {
    pub start_month: String,
    pub end_month: String,
    /// `> 0` 才算数；`None` 表示不限（这一步由 [`normalize_recent`] 保证）
    pub recent: Option<i64>,
    pub tag: String,
}

impl StatisticsFilter {
    /// 从请求里的四段拼出一个筛选条件（`recent` 已经过 [`normalize_recent`]）。
    pub fn new(
        start_month: impl Into<String>,
        end_month: impl Into<String>,
        recent: Option<i64>,
        tag: impl Into<String>,
    ) -> Self {
        Self {
            start_month: start_month.into(),
            end_month: end_month.into(),
            recent,
            tag: tag.into(),
        }
    }

    /// 月份区间那一端（`None` = 不限）。
    pub fn start(&self) -> Option<&str> {
        normalize_month(&self.start_month)
    }

    /// 月份区间那一端（`None` = 不限）。
    pub fn end(&self) -> Option<&str> {
        normalize_month(&self.end_month)
    }

    /// 标签筛选（空 = 不限）。
    pub fn tag_filter(&self) -> Option<&str> {
        normalize_month(&self.tag)
    }

    /// 是不是"什么都不筛"（全量）。
    pub fn is_unfiltered(&self) -> bool {
        self.start().is_none()
            && self.end().is_none()
            && self.recent.is_none()
            && self.tag_filter().is_none()
    }

    /// 筛选条件本身是否自洽（与数据库无关的那两条规则）。
    ///
    /// 从 `tr-service::stock_statistics::get_statistics_range` 开头那两句搬来 —— 那条
    /// "月区间与最近 N 笔互斥"的口径从前只写在服务层的 `if` 里。
    ///
    /// 注意 [`StatisticsFilter::recent`] 已是"归一化之后"的值（`None` = 不限），所以这里不处理
    /// `recent == 0`；负数只在调用方绕过归一化时才会出现，保留判断以防万一。
    pub fn validate(&self) -> Result<(), &'static str> {
        if matches!(self.recent, Some(value) if value < 0) {
            return Err(RECENT_NOT_POSITIVE);
        }
        if self.recent.is_some() && (self.start().is_some() || self.end().is_some()) {
            return Err(RANGE_AND_RECENT);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 字段缺失 / 空串 / 非数字串都按"没传"处理 —— 注释里那句"非数字串一律报错"是错的。
    #[test]
    fn missing_blank_and_unparsable_all_mean_no_limit() {
        assert_eq!(normalize_recent(None), Ok(None), "字段缺失");
        assert_eq!(normalize_recent(parse_recent_text("")), Ok(None), "空串");
        assert_eq!(
            normalize_recent(parse_recent_text("abc")),
            Ok(None),
            "非数字串（宽松，不报错）"
        );
        assert_eq!(
            normalize_recent(parse_recent_text("3.5")),
            Ok(None),
            "小数文本解析失败 → 不限"
        );
    }

    #[test]
    fn positive_numbers_are_kept() {
        assert_eq!(normalize_recent(parse_recent_text("1")), Ok(Some(1)));
        assert_eq!(normalize_recent(parse_recent_text("20")), Ok(Some(20)));
        assert_eq!(normalize_recent(Some(7)), Ok(Some(7)));
    }

    #[test]
    fn zero_and_negative_are_rejected_with_the_shared_message() {
        assert_eq!(normalize_recent(Some(0)), Err(RECENT_NOT_POSITIVE));
        assert_eq!(normalize_recent(Some(-5)), Err(RECENT_NOT_POSITIVE));
        assert_eq!(
            normalize_recent(parse_recent_text("-1")),
            Err(RECENT_NOT_POSITIVE)
        );
        assert_eq!(RECENT_NOT_POSITIVE, "recent 必须为正整数");
    }

    /// 不做 trim：带空白的文本按"解析失败"处理（与既有行为一致）。
    #[test]
    fn text_is_not_trimmed() {
        assert_eq!(parse_recent_text(" 5"), None);
    }

    #[test]
    fn months_are_half_open() {
        assert_eq!(normalize_month(""), None);
        assert_eq!(normalize_month("   "), None);
        assert_eq!(normalize_month("2026-06"), Some("2026-06"));
        assert_eq!(normalize_month(" 2026-06 "), Some("2026-06"));
    }

    #[test]
    fn an_empty_filter_is_unfiltered() {
        let filter = StatisticsFilter::default();
        assert!(filter.is_unfiltered());
        assert_eq!(filter.start(), None);
        assert_eq!(filter.end(), None);
        assert_eq!(filter.tag_filter(), None);
    }

    #[test]
    fn each_part_alone_makes_it_filtered() {
        assert!(!StatisticsFilter::new("2026-01", "", None, "").is_unfiltered());
        assert!(!StatisticsFilter::new("", "2026-12", None, "").is_unfiltered());
        assert!(!StatisticsFilter::new("", "", Some(5), "").is_unfiltered());
        assert!(!StatisticsFilter::new("", "", None, "打新").is_unfiltered());
    }

    /// 两条与数据库无关的自洽规则（从服务层的两句 `if` 搬来）。
    #[test]
    fn range_and_recent_are_mutually_exclusive() {
        assert_eq!(
            StatisticsFilter::new("2026-01", "2026-06", Some(5), "").validate(),
            Err(RANGE_AND_RECENT)
        );
        assert_eq!(
            StatisticsFilter::new("2026-01", "", Some(5), "").validate(),
            Err(RANGE_AND_RECENT),
            "只填一端也算用了区间"
        );
        assert_eq!(
            StatisticsFilter::new("", "2026-06", Some(5), "").validate(),
            Err(RANGE_AND_RECENT)
        );
        // 各自单独用都合法
        assert_eq!(
            StatisticsFilter::new("2026-01", "2026-06", None, "").validate(),
            Ok(())
        );
        assert_eq!(
            StatisticsFilter::new("", "", Some(5), "").validate(),
            Ok(())
        );
        assert_eq!(StatisticsFilter::default().validate(), Ok(()));
        // 带空白的一端正则化为"不限"，于是不再与 recent 冲突
        assert_eq!(
            StatisticsFilter::new("   ", "", Some(5), "").validate(),
            Ok(())
        );
    }

    #[test]
    fn a_negative_recent_is_rejected_wherever_it_comes_from() {
        assert_eq!(
            StatisticsFilter::new("", "", Some(-1), "").validate(),
            Err(RECENT_NOT_POSITIVE)
        );
    }
}
