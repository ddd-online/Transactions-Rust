//! 股票统计的**指标规则**：七档指标各自的取值、Y 轴语义与上下界、是否画 0 轴参考线。
//!
//! 从 `pages/stock.rs` 的 `Metric` 搬来（ADR-0001 的第二波）。它们是这条曲线图的口径：
//!
//! * [`Metric::kind`] —— 取值语义（金额 / 百分比 / 倍数）决定刻度与 tooltip 的格式化；
//! * [`Metric::y_bounds`] —— **只有天生有边界的指标才给上下界**：胜率是 0..100（出现负数时
//!   -100..100），不给的话自动挑刻度会把"数据正好压在 100%"当成"上界还要再大一档"，
//!   于是多画一条 150% 的网格线；最大回撤同样是百分比但没有天花板，交给按数据自动挑；
//! * [`Metric::has_reference`] —— 金额类且可能为负的指标才画 y=0 虚线；
//! * [`Metric::value`] —— 从统计点里取这一档的值（平均亏损**取负**，曲线画在 0 轴下方）；
//! * [`Metric::key`] / [`Metric::label_in`] —— 界面记住的是 **key**，显示的是**随统计口径变**的文案：
//!   分组口径下「累计盈亏」叫「分组盈亏」（那个点的值是**本组**的盈亏合计，不是自起点累计），
//!   其余六档的口径天然跟着统计点走，名字不必变；
//! * [`point_span_label`] —— 统计点在横轴上的覆盖范围（分组口径下是"第 a-b 笔"）。

use tr_domain::dto::StockStatisticsPointDto;
use tr_domain::statistics::StatisticsStrategy;

use crate::chart::ChartValueKind;

/// 统计曲线的七档指标（顺序即界面上的顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    TotalPnl,
    WinRate,
    AvgWin,
    AvgLoss,
    PnlRatio,
    Expectancy,
    MaxDrawdown,
}

impl Metric {
    /// 全部指标（顺序即渲染顺序）。
    pub const ALL: [Metric; 7] = [
        Metric::TotalPnl,
        Metric::WinRate,
        Metric::AvgWin,
        Metric::AvgLoss,
        Metric::PnlRatio,
        Metric::Expectancy,
        Metric::MaxDrawdown,
    ];

    /// 界面上显示的名字（逐笔累计口径下；分组口径见 [`Metric::label_in`]）。
    pub fn label(self) -> &'static str {
        match self {
            Metric::TotalPnl => "累计盈亏",
            Metric::WinRate => "胜率",
            Metric::AvgWin => "平均盈利",
            Metric::AvgLoss => "平均亏损",
            Metric::PnlRatio => "实际盈亏比",
            Metric::Expectancy => "期望值",
            Metric::MaxDrawdown => "最大回撤",
        }
    }

    /// 稳定标识（**界面按它记住选中项**）：文案会随统计口径变，标识不能是文案。
    pub fn key(self) -> &'static str {
        match self {
            Metric::TotalPnl => "total_pnl",
            Metric::WinRate => "win_rate",
            Metric::AvgWin => "avg_win",
            Metric::AvgLoss => "avg_loss",
            Metric::PnlRatio => "pnl_ratio",
            Metric::Expectancy => "expectancy",
            Metric::MaxDrawdown => "max_drawdown",
        }
    }

    /// 某个统计口径下的显示名。
    ///
    /// 只有「累计盈亏」要改：分组口径下它是**本组**的盈亏合计（`total_pnl` 在组点上就是组内求和），
    /// 写成"累计"会让人以为曲线仍是自起点累计的那条。
    pub fn label_in(self, strategy: StatisticsStrategy) -> &'static str {
        match (self, strategy) {
            (Metric::TotalPnl, StatisticsStrategy::Grouped) => "分组盈亏",
            _ => self.label(),
        }
    }

    /// 取值语义（决定 Y 轴刻度与 tooltip 的格式化）。
    pub fn kind(self) -> ChartValueKind {
        match self {
            Metric::WinRate | Metric::MaxDrawdown => ChartValueKind::Percent,
            Metric::PnlRatio => ChartValueKind::Ratio,
            _ => ChartValueKind::Money,
        }
    }

    /// Y 轴上下界：只有**天生有边界**的指标才给（见模块文档）。
    pub fn y_bounds(self, values: &[f64]) -> Option<(f64, f64)> {
        match self {
            Metric::WinRate => Some(if values.iter().any(|value| *value < 0.0) {
                (-100.0, 100.0)
            } else {
                (0.0, 100.0)
            }),
            _ => None,
        }
    }

    /// 是否画 y=0 虚线参考线（金额类且可能为负的指标才画）。
    pub fn has_reference(self) -> bool {
        matches!(self, Metric::TotalPnl | Metric::Expectancy)
    }

    /// 曲线取值（分 / 百分数 / 倍数）。
    pub fn value(self, point: &StockStatisticsPointDto) -> f64 {
        match self {
            Metric::TotalPnl => point.total_pnl as f64,
            Metric::WinRate => point.win_rate,
            Metric::AvgWin => point.avg_win as f64,
            // 平均亏损取负（曲线在 0 轴下方）
            Metric::AvgLoss => -(point.avg_loss as f64),
            Metric::PnlRatio => point.pnl_ratio.unwrap_or(0.0),
            Metric::Expectancy => point.expectancy as f64,
            Metric::MaxDrawdown => point.max_drawdown_pct,
        }
    }
}

/// 一个统计点在横轴上的覆盖范围文案。
///
/// * 逐笔累计口径：`第 5 笔` —— 那个点覆盖 1..=5，但横轴读作"打到第 5 笔为止"
///   （`span == sequence`，写成"第 1-5 笔"反而每次都在变）；
/// * 分组口径：`第 1-5 笔` / `第 6-10 笔` —— 分组是不重叠的区间，范围本身才是信息。
pub fn point_span_label(strategy: StatisticsStrategy, sequence: i64, span: i64) -> String {
    let first = sequence - span + 1;
    if strategy == StatisticsStrategy::Grouped && span > 1 && first >= 1 {
        format!("第{first}-{sequence}笔")
    } else {
        format!("第{sequence}笔")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::dto::StockStatisticsPointDto;

    fn point() -> StockStatisticsPointDto {
        StockStatisticsPointDto {
            total_pnl: 123_456,
            win_rate: 62.5,
            avg_win: 8_800,
            avg_loss: 4_400,
            pnl_ratio: Some(2.0),
            expectancy: 1_100,
            max_drawdown_pct: 7.5,
            ..StockStatisticsPointDto::default()
        }
    }

    #[test]
    fn values_come_from_the_point_per_metric() {
        let point = point();
        assert_eq!(Metric::TotalPnl.value(&point), 123_456.0);
        assert_eq!(Metric::WinRate.value(&point), 62.5);
        assert_eq!(Metric::AvgWin.value(&point), 8_800.0);
        assert_eq!(Metric::AvgLoss.value(&point), -4_400.0, "平均亏损取负");
        assert_eq!(Metric::PnlRatio.value(&point), 2.0);
        assert_eq!(Metric::Expectancy.value(&point), 1_100.0);
        assert_eq!(Metric::MaxDrawdown.value(&point), 7.5);
    }

    /// 没有亏损样本时盈亏比缺省 0（曲线画 0，而不是断一条）。
    #[test]
    fn a_missing_ratio_reads_as_zero() {
        let point = StockStatisticsPointDto {
            pnl_ratio: None,
            ..StockStatisticsPointDto::default()
        };
        assert_eq!(Metric::PnlRatio.value(&point), 0.0);
    }

    #[test]
    fn only_win_rate_pins_its_axis() {
        assert_eq!(Metric::WinRate.y_bounds(&[10.0, 90.0]), Some((0.0, 100.0)));
        assert_eq!(
            Metric::WinRate.y_bounds(&[-10.0, 90.0]),
            Some((-100.0, 100.0)),
            "出现负数时轴要能容纳负的一半"
        );
        for metric in Metric::ALL.iter().filter(|m| **m != Metric::WinRate) {
            assert_eq!(
                metric.y_bounds(&[1.0, 2.0]),
                None,
                "{} 交给按数据自动挑",
                metric.label()
            );
        }
    }

    #[test]
    fn reference_line_only_for_money_metrics_that_can_go_negative() {
        assert!(Metric::TotalPnl.has_reference());
        assert!(Metric::Expectancy.has_reference());
        assert!(!Metric::WinRate.has_reference());
        assert!(!Metric::MaxDrawdown.has_reference());
        assert!(!Metric::AvgWin.has_reference());
    }

    #[test]
    fn kinds_drive_the_axis_and_tooltip_formatting() {
        assert_eq!(Metric::WinRate.kind(), ChartValueKind::Percent);
        assert_eq!(Metric::MaxDrawdown.kind(), ChartValueKind::Percent);
        assert_eq!(Metric::PnlRatio.kind(), ChartValueKind::Ratio);
        assert_eq!(Metric::TotalPnl.kind(), ChartValueKind::Money);
        assert_eq!(Metric::AvgLoss.kind(), ChartValueKind::Money);
    }

    /// 七档都有名字，且名字两两不同。
    #[test]
    fn every_metric_has_a_unique_label() {
        let labels: Vec<&str> = Metric::ALL.iter().map(|metric| metric.label()).collect();
        assert_eq!(labels.len(), 7);
        assert!(labels.iter().all(|label| !label.is_empty()));
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "{labels:?}");
    }

    /// 七档都有稳定的 key，两两不同，且**不随口径变** —— 界面按它记住选中项。
    #[test]
    fn every_metric_has_a_unique_key() {
        let keys: Vec<&str> = Metric::ALL.iter().map(|metric| metric.key()).collect();
        assert_eq!(keys.len(), 7);
        assert!(keys.iter().all(|key| !key.is_empty()));
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), keys.len(), "{keys:?}");
    }

    /// 显示名只在分组口径下改「累计盈亏」这一档，其余六档两个口径同名。
    #[test]
    fn only_the_cumulative_pnl_label_follows_the_strategy() {
        for metric in Metric::ALL {
            assert_eq!(
                metric.label_in(StatisticsStrategy::Cumulative),
                metric.label(),
                "{} 在逐笔累计口径下应当就是本名",
                metric.key()
            );
        }
        assert_eq!(
            Metric::TotalPnl.label_in(StatisticsStrategy::Grouped),
            "分组盈亏"
        );
        for metric in Metric::ALL.iter().filter(|m| **m != Metric::TotalPnl) {
            assert_eq!(
                metric.label_in(StatisticsStrategy::Grouped),
                metric.label(),
                "{} 的口径天然跟着统计点走，名字不必变",
                metric.key()
            );
        }
        // 两个口径下七档名字都仍然唯一（界面按文案找选项的那条老路也不会撞）
        for strategy in StatisticsStrategy::ALL {
            let labels: Vec<&str> = Metric::ALL
                .iter()
                .map(|metric| metric.label_in(strategy))
                .collect();
            let mut unique = labels.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), labels.len(), "{strategy:?} {labels:?}");
        }
    }

    /// 横轴文案：逐笔累计只报"第 N 笔"（`span` 与 `sequence` 相同也不写成区间），
    /// 分组口径报"第 a-b 笔"，单笔的组与末组只剩一笔时退回"第 N 笔"。
    #[test]
    fn span_labels_show_a_range_only_when_grouped() {
        let cumulative = StatisticsStrategy::Cumulative;
        let grouped = StatisticsStrategy::Grouped;
        assert_eq!(point_span_label(cumulative, 1, 1), "第1笔");
        assert_eq!(
            point_span_label(cumulative, 5, 5),
            "第5笔",
            "累计点不写区间"
        );
        assert_eq!(point_span_label(grouped, 5, 5), "第1-5笔");
        assert_eq!(point_span_label(grouped, 10, 5), "第6-10笔");
        assert_eq!(
            point_span_label(grouped, 23, 3),
            "第21-23笔",
            "末组不足 N 笔"
        );
        assert_eq!(point_span_label(grouped, 7, 1), "第7笔", "末组只剩一笔");
        // 序号口径不符（span 比 sequence 还大）时不写出负数开头
        assert_eq!(point_span_label(grouped, 1, 5), "第1笔");
    }
}
