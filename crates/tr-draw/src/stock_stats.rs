//! 股票统计的**指标规则**：七档指标各自的取值、Y 轴语义与上下界、是否画 0 轴参考线。
//!
//! 从 `pages/stock.rs` 的 `Metric` 搬来（ADR-0001 的第二波）。它们是这条曲线图的口径：
//!
//! * [`Metric::kind`] —— 取值语义（金额 / 百分比 / 倍数）决定刻度与 tooltip 的格式化；
//! * [`Metric::y_bounds`] —— **只有天生有边界的指标才给上下界**：胜率是 0..100（出现负数时
//!   -100..100），不给的话自动挑刻度会把"数据正好压在 100%"当成"上界还要再大一档"，
//!   于是多画一条 150% 的网格线；最大回撤同样是百分比但没有天花板，交给按数据自动挑；
//! * [`Metric::has_reference`] —— 金额类且可能为负的指标才画 y=0 虚线；
//! * [`Metric::value`] —— 从统计点里取这一档的值（平均亏损**取负**，曲线画在 0 轴下方）。

use tr_domain::dto::StockStatisticsPointDto;

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

    /// 界面上显示的名字（也是选中项的标识 —— 界面按字符串记住当前选的是哪一档）。
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

    /// 七档都有名字，且名字两两不同（界面按名字记住选中项）。
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
}
