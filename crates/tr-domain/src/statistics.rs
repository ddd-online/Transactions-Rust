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
//!
//! 同一个类型后来还装进了**统计口径**（[`StatisticsStrategy`] 与 `group_size`）：它不是
//! "筛掉哪些记录"，而是"把入选的记录切成哪些统计点"，所以 [`StatisticsFilter::is_unfiltered`]
//! 不看它 —— 全量 + 分组依然是"没筛"。两个口径分别算出哪条序列，见 `tr-service::stock_statistics`。

/// `recent <= 0` 时的错误文案（与 `tr-ipc` 的 `AppError::bad_request` 用同一句）。
pub const RECENT_NOT_POSITIVE: &str = "recent 必须为正整数";

/// 月区间与"最近 N 笔"同时给时的错误文案（服务层原来硬编码这一句）。
pub const RANGE_AND_RECENT: &str = "时间范围与笔数筛选不能同时使用";

/// `strategy` 不是两个口径之一时的错误文案。
pub const STRATEGY_INVALID: &str = "strategy 取值需为 cumulative（逐笔累计）或 grouped（分组统计）";

/// 分组口径下 `group_size` 缺失或越界时的错误文案。
pub const GROUP_SIZE_INVALID: &str = "group_size 需为 2 到 1000 的整数";

/// 每组至少 2 笔：`1` 就是"每组一笔"，与逐笔累计的差别只剩"要不要组间累计"，
/// 摆两个选项却看不出区别，不如不收。
pub const GROUP_SIZE_MIN: i64 = 2;

/// 每组笔数上限（防手改请求塞一个 1e18 进来；界面上只提供 5 / 10 / 20 / 50）。
pub const GROUP_SIZE_MAX: i64 = 1000;

/// 统计口径：把筛选出来的结算切成哪些"统计点"。
///
/// * [`StatisticsStrategy::Cumulative`]（逐笔累计）：第 i 个点 = 集合里第 1..=i 笔的**累计**值，
///   点的个数 = 集合里的结算笔数；
/// * [`StatisticsStrategy::Grouped`]（分组统计）：第 k 个点 = 集合里第 (k−1)·N+1 ..= k·N 笔
///   **本组自己**的值（不做组间累计，回撤在组内从 0 起步），点的个数 = ⌈笔数 ÷ N⌉。
///
/// 这个类型只管**口径**，不管"哪些结算入选"（那是月区间 / 最近 N 笔 / 标签三件事），
/// 所以它不进 [`StatisticsFilter::is_unfiltered`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatisticsStrategy {
    /// 逐笔累计（默认：字段缺省、空串都是它）
    #[default]
    Cumulative,
    /// 分组统计（每 `group_size` 笔一组）
    Grouped,
}

impl StatisticsStrategy {
    /// 两个口径（顺序即界面下拉里的顺序）。
    pub const ALL: [StatisticsStrategy; 2] =
        [StatisticsStrategy::Cumulative, StatisticsStrategy::Grouped];

    /// wire 上的取值（`StockStatisticsRequest::strategy`）。
    pub fn as_str(self) -> &'static str {
        match self {
            StatisticsStrategy::Cumulative => "cumulative",
            StatisticsStrategy::Grouped => "grouped",
        }
    }

    /// 界面上的名字（下拉选项文案的唯一来源，界面按 [`StatisticsStrategy::ALL`] 生成选项）。
    pub fn label(self) -> &'static str {
        match self {
            StatisticsStrategy::Cumulative => "逐笔累计",
            StatisticsStrategy::Grouped => "分组统计",
        }
    }

    /// wire 文本 → 口径：**空串按"没传"处理**（默认逐笔累计），非空必须精确命中两个取值之一。
    ///
    /// 这里比 `recent` 严：那个字段的"非数字串按没传处理"是历史行为（见模块文档），
    /// 口径写错却悄悄按逐笔累计算，用户看到的是"下拉选了分组、图上还是累计"。
    pub fn parse_wire(raw: &str) -> Result<Self, &'static str> {
        match raw {
            "" | "cumulative" => Ok(StatisticsStrategy::Cumulative),
            "grouped" => Ok(StatisticsStrategy::Grouped),
            _ => Err(STRATEGY_INVALID),
        }
    }
}

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

/// 一端筛选条件的三段（月份区间 / 最近 N 笔 / 标签）+ 统计口径。
///
/// 筛选那三段今天以裸形参在界面 → `tr-ipc` → `tr-service` 之间传递（`get_statistics_range` 收 6 个形参），
/// 靠位置对齐；这个类型是它们**同一份**的样子，也是"哪一端不限"的唯一判据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatisticsFilter {
    pub start_month: String,
    pub end_month: String,
    /// `> 0` 才算数；`None` 表示不限（这一步由 [`normalize_recent`] 保证）
    pub recent: Option<i64>,
    pub tag: String,
    /// 统计口径（缺省 = 逐笔累计，见 [`StatisticsFilter::with_strategy`]）
    pub strategy: StatisticsStrategy,
    /// 分组口径下每组的结算笔数；逐笔累计口径下**忽略**（[`StatisticsFilter::validate`] 只查分组）
    pub group_size: Option<i64>,
}

impl StatisticsFilter {
    /// 从请求里的四段拼出一个筛选条件（`recent` 已经过 [`normalize_recent`]），口径默认逐笔累计。
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
            strategy: StatisticsStrategy::Cumulative,
            group_size: None,
        }
    }

    /// 设定统计口径（`size` 是分组口径下每组的笔数，逐笔累计口径下传 `None`）。
    ///
    /// 构造器不校验 `size`：越界（含"分组却没给笔数"）一律留给
    /// [`StatisticsFilter::validate`] 报 [`GROUP_SIZE_INVALID`] —— 规则只有一处。
    pub fn with_strategy(mut self, strategy: StatisticsStrategy, size: Option<i64>) -> Self {
        self.strategy = strategy;
        self.group_size = size;
        self
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

    /// 筛选条件本身是否自洽（与数据库无关的那三条规则）。
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
        if self.strategy == StatisticsStrategy::Grouped
            && !matches!(self.group_size, Some(size) if (GROUP_SIZE_MIN..=GROUP_SIZE_MAX).contains(&size))
        {
            return Err(GROUP_SIZE_INVALID);
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

    /// 默认口径是逐笔累计（`new` 与 `Default` 都是），界面不传 `strategy` 时行为与从前一致。
    #[test]
    fn the_default_strategy_is_cumulative() {
        assert_eq!(
            StatisticsFilter::new("", "", None, "").strategy,
            StatisticsStrategy::Cumulative
        );
        assert_eq!(
            StatisticsFilter::default().strategy,
            StatisticsStrategy::Cumulative
        );
        assert_eq!(StatisticsStrategy::default().as_str(), "cumulative");
    }

    /// wire 文本：空串 = 没传 = 逐笔累计；两个取值精确命中；其余报错（**不宽松**）。
    #[test]
    fn strategy_wire_text_is_exact() {
        assert_eq!(
            StatisticsStrategy::parse_wire(""),
            Ok(StatisticsStrategy::Cumulative)
        );
        for strategy in StatisticsStrategy::ALL {
            assert_eq!(
                StatisticsStrategy::parse_wire(strategy.as_str()),
                Ok(strategy)
            );
        }
        assert_eq!(
            StatisticsStrategy::parse_wire("Grouped"),
            Err(STRATEGY_INVALID),
            "大小写不匹配"
        );
        assert_eq!(
            StatisticsStrategy::parse_wire("batch"),
            Err(STRATEGY_INVALID)
        );
        assert_eq!(StatisticsStrategy::parse_wire(" "), Err(STRATEGY_INVALID));
    }

    /// 两个口径的名字两两不同且非空 —— 界面下拉的选项文案就是它们。
    #[test]
    fn every_strategy_has_a_name() {
        let labels: Vec<&str> = StatisticsStrategy::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, vec!["逐笔累计", "分组统计"]);
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());

        // wire 取值同样两两不同（否则界面选的与后端认的不是一回事）
        let wires: Vec<&str> = StatisticsStrategy::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(wires, vec!["cumulative", "grouped"]);
    }

    /// 分组口径的笔数只有一处判据：`2..=1000`，缺失与越界都是同一句错误。
    #[test]
    fn grouped_requires_a_size_in_range() {
        let grouped = |size: i64| {
            StatisticsFilter::new("", "", None, "")
                .with_strategy(StatisticsStrategy::Grouped, Some(size))
        };
        assert_eq!(grouped(GROUP_SIZE_MIN).validate(), Ok(()));
        assert_eq!(grouped(50).validate(), Ok(()));
        assert_eq!(grouped(GROUP_SIZE_MAX).validate(), Ok(()));
        assert_eq!(
            grouped(1).validate(),
            Err(GROUP_SIZE_INVALID),
            "每组一笔没有意义"
        );
        assert_eq!(grouped(0).validate(), Err(GROUP_SIZE_INVALID));
        assert_eq!(grouped(-5).validate(), Err(GROUP_SIZE_INVALID));
        assert_eq!(
            grouped(GROUP_SIZE_MAX + 1).validate(),
            Err(GROUP_SIZE_INVALID)
        );
        assert_eq!(
            StatisticsFilter::new("", "", None, "")
                .with_strategy(StatisticsStrategy::Grouped, None)
                .validate(),
            Err(GROUP_SIZE_INVALID),
            "分组却没给笔数"
        );
    }

    /// 逐笔累计口径下 `group_size` 不参与校验（界面上那个下拉还没出现，
    /// 收尾时留在信号里的旧值不该把请求判死）。
    #[test]
    fn a_size_is_ignored_under_cumulative() {
        let filter = StatisticsFilter::new("", "", None, "")
            .with_strategy(StatisticsStrategy::Cumulative, Some(1));
        assert_eq!(filter.validate(), Ok(()));
        assert_eq!(
            StatisticsFilter::new("", "", Some(5), "")
                .with_strategy(StatisticsStrategy::Cumulative, Some(999_999))
                .validate(),
            Ok(())
        );
    }

    /// 口径不是筛选：全量 + 分组依然是"没筛"，两条规则各自独立生效。
    #[test]
    fn grouping_does_not_make_a_filter() {
        let filter = StatisticsFilter::new("", "", None, "")
            .with_strategy(StatisticsStrategy::Grouped, Some(5));
        assert!(filter.is_unfiltered());
        assert_eq!(filter.validate(), Ok(()));
        // 三条规则可以同时踩：区间 + 笔数先报
        assert_eq!(
            StatisticsFilter::new("2026-01", "2026-06", Some(5), "")
                .with_strategy(StatisticsStrategy::Grouped, Some(1))
                .validate(),
            Err(RANGE_AND_RECENT)
        );
    }
}
