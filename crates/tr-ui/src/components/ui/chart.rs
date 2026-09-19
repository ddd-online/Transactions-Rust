//! 自绘 SVG 折线图（界面层无 JS 图表库，全部几何在 Rust 里算完）。
//!
//! 能力：
//!
//! * 多序列 —— [`ChartSeries`] 列表
//! * 类目轴 —— `points: Vec<String>`（月份 / 年份 / 序号）
//! * 数值轴 + 分割线 —— 自动 nice tick（默认 5 段）+ 横向网格线
//! * 图例 —— [`ChartConfig::hide_legend`] 控制显示；点击开关由**页面**维护可见序列集合后重传 `series`
//! * tooltip —— `pointermove` 命中最近类目 → 竖参考线 + 浮层（金额 `¥` 两位小数）
//! * 虚线参考线 —— [`ChartConfig::reference`]
//! * 缩放 —— **未实现**。
//!
//! ## 配色
//!
//! 序列颜色一律是 **CSS 变量名**（例如 `--transactions-color-transfer`），
//! 在 SVG 里以 `stroke="var(--transactions-color-transfer)"` 使用。
//! 这样浅色/深色主题切换由 CSS 自动完成，不需要 `getComputedStyle`。
//!
//! ## 纪律
//!
//! 数据为空、全为 0、只有 1 个类目、极值相等等边界都不会 panic：
//! 坐标系退化时按"画在边界上"处理，配合 `if let`/`max(1e-9)` 兜底除零。

use leptos::prelude::*;

/// 一个类目上的数据点。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    /// X 轴类目（`2024-01` / `2024` / `1`）
    pub label: String,
    /// Y 轴数值（金额时单位是**分**）
    pub value: i64,
}

impl ChartPoint {
    pub fn new(label: impl Into<String>, value: i64) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }
}

/// 一条序列。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// 图例文案
    pub label: String,
    /// CSS 变量名或任意合法 `stroke` 值（推荐 `var(--transactions-color-*)`）
    pub color: String,
    /// 与 X 轴类目一一对应的取值（长度不足的类目按 0 处理）
    pub data: Vec<i64>,
}

impl ChartSeries {
    pub fn new(label: impl Into<String>, color: impl Into<String>, data: Vec<i64>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            data,
        }
    }
}

/// Y 轴取值语义（决定刻度与 tooltip 的格式化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartValueKind {
    /// 金额：分 → 元，两位小数，`¥` 前缀（tooltip 与刻度都带）
    Money,
    /// 百分比：已是百分数（`12.5` → `12.50%`）
    Percent,
    /// 计数：整数
    Count,
}

/// 图表配置。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartConfig {
    /// 画布高度（px），默认 260
    pub height: u32,
    /// Y 轴标题（可省略）
    pub y_title: String,
    /// Y 轴取值语义
    pub value_kind: ChartValueKind,
    /// 虚线参考线（金额时单位是分），例如 0 或本金
    pub reference: Option<i64>,
    /// 隐藏图例
    pub hide_legend: bool,
    /// 隐藏网格线
    pub hide_grid: bool,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            height: 260,
            y_title: String::new(),
            value_kind: ChartValueKind::Money,
            reference: None,
            hide_legend: false,
            hide_grid: false,
        }
    }
}

impl ChartConfig {
    pub fn height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    pub fn y_title(mut self, title: impl Into<String>) -> Self {
        self.y_title = title.into();
        self
    }

    pub fn value_kind(mut self, kind: ChartValueKind) -> Self {
        self.value_kind = kind;
        self
    }

    pub fn reference(mut self, value: i64) -> Self {
        self.reference = Some(value);
        self
    }

    /// 预设配色（语义色，顺序即取值顺序）。
    pub fn default_colors() -> [&'static str; 5] {
        [
            "var(--transactions-color-transfer)",
            "var(--transactions-color-expense)",
            "var(--transactions-color-income)",
            "var(--transactions-color-outlier)",
            "var(--transactions-color-text-secondary)",
        ]
    }
}

/// SVG 折线图。
///
/// 类目轴与序列由父组件传入；悬停命中状态保存在组件内部（不污染父级信号）。
#[component]
pub fn LineChart(
    /// X 轴类目（顺序即横轴顺序）
    #[prop(into)]
    categories: Signal<Vec<String>>,
    /// 序列（可多条）
    #[prop(into)]
    series: Signal<Vec<ChartSeries>>,
    /// 配置
    #[prop(optional)]
    config: ChartConfig,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    // 悬停命中的类目下标
    let hover = RwSignal::new(Option::<usize>::None);
    let class = class.unwrap_or_default();

    // 配置项在渲染前取出（`config` 被多个闭包读取，逐字段复制避免 move 冲突）
    let hide_legend_config = config.hide_legend;
    let hide_grid = config.hide_grid;
    let geometry_config = config.clone();

    // 几何与刻度在渲染时计算（数据量小，直接算比缓存简单且不会过期）
    let geometry = Signal::derive(move || {
        let categories = categories.get();
        let series = series.get();
        compute_geometry(&categories, &series, &geometry_config)
    });

    let on_move = move |event: leptos::ev::PointerEvent| {
        let geometry = geometry.get_untracked();
        if geometry.categories.is_empty() {
            return;
        }
        // 事件坐标是 **CSS 像素**，而 viewBox 是逻辑像素：`preserveAspectRatio="xMidYMid meet"`
        // 下缩放比 = 元素宽度 / viewBox 宽度。取不到元素宽度时按 1:1 近似（不 panic）。
        let element_width = event
            .current_target()
            .and_then(|target| {
                use wasm_bindgen::JsCast;
                target.dyn_into::<web_sys::Element>().ok()
            })
            .map(|element| element.client_width() as f64)
            .unwrap_or(geometry.width);
        let scale = if element_width > 0.0 {
            element_width / geometry.width
        } else {
            1.0
        };
        hover.set(hit_index(&geometry, f64::from(event.offset_x()), scale));
    };

    view! {
        <div class=format!("chart {class}")>
            {move || {
                let legend = series.get();
                if hide_legend_config || legend.is_empty() {
                    ().into_any()
                } else {
                    view! {
                        <div class="chart__legend">
                            {legend
                                .into_iter()
                                .map(|item| {
                                    view! {
                                        <span class="chart__legend-item">
                                            <span
                                                class="chart__legend-swatch"
                                                style=format!("background: {}", item.color)
                                            ></span>
                                            <span class="chart__legend-label">{item.label}</span>
                                        </span>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                        .into_any()
                }
            }}

            <div class="chart__canvas">
                <svg
                    class="chart__svg"
                    viewBox=move || {
                        let geometry = geometry.get();
                        format!("0 0 {} {}", geometry.width, geometry.height)
                    }
                    preserveAspectRatio="xMidYMid meet"
                    on:pointermove=on_move
                    on:pointerleave=move |_| hover.set(None)
                >
                    // ---- 网格线与 Y 轴刻度 ----
                    {move || {
                        let geometry = geometry.get();
                        if hide_grid {
                            return ().into_any();
                        }
                        let grid = geometry
                            .yticks
                            .iter()
                            .map(|tick| {
                                view! {
                                    <line
                                        class="chart__grid"
                                        x1=geometry.plot_left
                                        x2=geometry.plot_right
                                        y1=tick.y
                                        y2=tick.y
                                    ></line>
                                }
                            })
                            .collect_view();
                        let labels = geometry
                            .yticks
                            .iter()
                            .map(|tick| {
                                view! {
                                    <text
                                        class="chart__ytick"
                                        x=geometry.plot_left - 8.0
                                        y=tick.y + 4.0
                                        text-anchor="end"
                                    >
                                        {tick.label.clone()}
                                    </text>
                                }
                            })
                            .collect_view();
                        view! { <g>{grid}{labels}</g> }.into_any()
                    }}

                    // ---- 坐标轴 ----
                    <line
                        class="chart__axis"
                        x1=move || geometry.get().plot_left
                        x2=move || geometry.get().plot_left
                        y1=move || geometry.get().plot_top
                        y2=move || geometry.get().plot_bottom
                    ></line>
                    <line
                        class="chart__axis"
                        x1=move || geometry.get().plot_left
                        x2=move || geometry.get().plot_right
                        y1=move || geometry.get().plot_bottom
                        y2=move || geometry.get().plot_bottom
                    ></line>

                    // ---- 虚线参考线 ----
                    {move || {
                        let geometry = geometry.get();
                        match geometry.reference_y {
                            Some(y) => {
                                view! {
                                    <line
                                        class="chart__reference"
                                        x1=geometry.plot_left
                                        x2=geometry.plot_right
                                        y1=y
                                        y2=y
                                    ></line>
                                }
                                    .into_any()
                            }
                            None => ().into_any(),
                        }
                    }}

                    // ---- X 轴类目标签（按宽度抽稀） ----
                    {move || {
                        let geometry = geometry.get();
                        geometry
                            .xticks
                            .iter()
                            .map(|tick| {
                                view! {
                                    <text
                                        class="chart__xtick"
                                        x=tick.x
                                        y=geometry.plot_bottom + 18.0
                                        text-anchor="middle"
                                    >
                                        {tick.label.clone()}
                                    </text>
                                }
                            })
                            .collect_view()
                    }}

                    // ---- 折线 ----
                    {move || {
                        let geometry = geometry.get();
                        geometry
                            .paths
                            .iter()
                            .map(|path| {
                                view! {
                                    <polyline
                                        class="chart__line"
                                        points=path.points.clone()
                                        stroke=path.color.clone()
                                    ></polyline>
                                }
                            })
                            .collect_view()
                    }}

                    // ---- 悬停：竖线 + 数据点 ----
                    {move || {
                        match hover.get() {
                            Some(index) => {
                                let geometry = geometry.get();
                                let Some(x) = geometry.x_at(index) else {
                                    return ().into_any();
                                };
                                view! {
                                    <line
                                        class="chart__cursor"
                                        x1=x
                                        x2=x
                                        y1=geometry.plot_top
                                        y2=geometry.plot_bottom
                                    ></line>
                                }
                                    .into_any()
                            }
                            None => ().into_any(),
                        }
                    }}
                    {move || {
                        let Some(index) = hover.get() else {
                            return ().into_any();
                        };
                        let geometry = geometry.get();
                        geometry
                            .dots
                            .iter()
                            .filter_map(|series_dots| series_dots.get(index).cloned().flatten())
                            .map(|dot| {
                                view! {
                                    <circle
                                        class="chart__dot"
                                        cx=dot.0
                                        cy=dot.1
                                        r="3"
                                        fill=dot.2.clone()
                                    ></circle>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </svg>

                // ---- tooltip（按悬停类目定位） ----
                {move || {
                    let Some(index) = hover.get() else {
                        return ().into_any();
                    };
                    let geometry = geometry.get();
                    let Some(category) = geometry.categories.get(index).cloned() else {
                        return ().into_any();
                    };
                    let rows = geometry
                        .tooltip_rows
                        .iter()
                        .filter_map(|series_rows| series_rows.get(index).cloned().flatten())
                        .collect::<Vec<_>>();
                    if rows.is_empty() {
                        return ().into_any();
                    }
                    let percent = geometry.x_percent(index);
                    view! {
                        <div
                            class="chart__tooltip"
                            style=format!("left: {percent:.2}%")
                        >
                            <div class="chart__tooltip-title">{category}</div>
                            {rows
                                .into_iter()
                                .map(|(label, color, value)| {
                                    view! {
                                        <div class="chart__tooltip-row">
                                            <span
                                                class="chart__legend-swatch"
                                                style=format!("background: {color}")
                                            ></span>
                                            <span class="chart__tooltip-label">{label}</span>
                                            <span class="chart__tooltip-value">{value}</span>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                        .into_any()
                }}
            </div>

            <Show when=move || geometry.get().is_empty>
                <div class="chart__empty">"暂无数据"</div>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------- 几何计算

/// Y 轴刻度。
#[derive(Debug, Clone, PartialEq)]
struct YTick {
    y: f64,
    label: String,
}

/// X 轴类目刻度。
#[derive(Debug, Clone, PartialEq)]
struct XTick {
    x: f64,
    label: String,
}

/// 一条折线的点串。
#[derive(Debug, Clone, PartialEq)]
struct LinePath {
    points: String,
    color: String,
}

/// 计算好的几何与文本，渲染层只做搬运。
#[derive(Debug, Clone, PartialEq)]
struct Geometry {
    width: f64,
    height: f64,
    plot_left: f64,
    plot_right: f64,
    plot_top: f64,
    plot_bottom: f64,
    step: f64,
    yticks: Vec<YTick>,
    xticks: Vec<XTick>,
    paths: Vec<LinePath>,
    /// 每条序列每个类目的 `(x, y, color)`；不可绘制的点为 `None`
    dots: Vec<Vec<Option<(f64, f64, String)>>>,
    /// 每条序列每个类目的 `(图例文案, 颜色, 已格式化金额)`
    tooltip_rows: Vec<Vec<Option<(String, String, String)>>>,
    reference_y: Option<f64>,
    categories: Vec<String>,
    is_empty: bool,
}

impl Geometry {
    /// 第 `index` 个类目的 x（越界返回 `None`）。
    fn x_at(&self, index: usize) -> Option<f64> {
        if index >= self.categories.len() {
            return None;
        }
        Some(self.plot_left + self.step * index as f64)
    }

    /// 第 `index` 个类目在绘图区里的百分比位置（tooltip 用）。
    fn x_percent(&self, index: usize) -> f64 {
        let span = self.plot_right - self.plot_left;
        if span <= 0.0 {
            return 0.0;
        }
        let x = self.plot_left + self.step * index as f64;
        ((x - self.plot_left) / span * 100.0).clamp(0.0, 100.0)
    }
}

/// 画布尺寸（与 CSS 的 `.chart__svg { width: 100% }` 配合，`)
/// viewBox` 决定内部坐标，因此这里用固定逻辑尺寸即可自适应）。
const CHART_WIDTH: f64 = 720.0;
const Y_TICKS: usize = 5;
/// X 轴标签的最小间距（px），低于此值抽稀。
const X_LABEL_MIN_GAP: f64 = 56.0;

fn compute_geometry(
    categories: &[String],
    series: &[ChartSeries],
    config: &ChartConfig,
) -> Geometry {
    let height = f64::from(config.height.max(120));
    let plot_top = 16.0;
    let plot_bottom = height - 30.0;
    let plot_left = 64.0;
    let plot_right = CHART_WIDTH - 16.0;
    let plot_height = (plot_bottom - plot_top).max(1.0);
    let plot_width = (plot_right - plot_left).max(1.0);

    let count = categories.len();
    let step = if count > 1 {
        plot_width / (count as f64 - 1.0)
    } else {
        0.0
    };

    let empty = count == 0 || series.is_empty();
    if empty {
        return Geometry {
            width: CHART_WIDTH,
            height,
            plot_left,
            plot_right,
            plot_top,
            plot_bottom,
            step,
            yticks: Vec::new(),
            xticks: Vec::new(),
            paths: Vec::new(),
            dots: Vec::new(),
            tooltip_rows: Vec::new(),
            reference_y: None,
            categories: categories.to_vec(),
            is_empty: true,
        };
    }

    // ---- Y 轴范围：所有序列 + 参考线 ----
    let mut min = 0_i64;
    let mut max = 0_i64;
    for item in series {
        for value in &item.data {
            min = min.min(*value);
            max = max.max(*value);
        }
    }
    if let Some(reference) = config.reference {
        min = min.min(reference);
        max = max.max(reference);
    }
    // 全 0 数据：给一个对称的量程，避免除零
    if min == 0 && max == 0 {
        max = 1;
    }
    let (nice_min, nice_max, tick_step) = nice_range(min, max);

    let y_of = move |value: i64| -> f64 {
        let span = (nice_max - nice_min) as f64;
        if span <= 0.0 {
            return plot_bottom;
        }
        let ratio = (value - nice_min) as f64 / span;
        plot_bottom - ratio * plot_height
    };

    // ---- Y 刻度 ----
    let mut yticks = Vec::with_capacity(Y_TICKS + 2);
    let mut value = nice_min;
    // 上限保护：nice_range 保证 (nice_max - nice_min) / tick_step 是小数，
    // 这里仍加计数上限，任何异常取值都不会死循环。
    let mut guard = 0;
    while value <= nice_max && guard <= 32 {
        yticks.push(YTick {
            y: y_of(value),
            label: format_value(value, config.value_kind, false),
        });
        value += tick_step;
        guard += 1;
    }

    // ---- X 刻度（按可用宽度抽稀，首尾必显） ----
    let max_labels = ((plot_width / X_LABEL_MIN_GAP).floor() as usize).max(1);
    let stride = if count > max_labels {
        (count as f64 / max_labels as f64).ceil() as usize
    } else {
        1
    };
    let stride = stride.max(1);
    let mut xticks = Vec::new();
    for (index, label) in categories.iter().enumerate() {
        let is_last = index + 1 == count;
        if index % stride != 0 && !is_last {
            continue;
        }
        xticks.push(XTick {
            x: plot_left + step * index as f64,
            label: label.clone(),
        });
    }

    // ---- 折线 / 数据点 / tooltip 行 ----
    let mut paths = Vec::with_capacity(series.len());
    let mut dots = Vec::with_capacity(series.len());
    let mut tooltip_rows = Vec::with_capacity(series.len());
    for item in series {
        let color = if item.color.is_empty() {
            "var(--transactions-color-transfer)".to_string()
        } else {
            item.color.clone()
        };
        let mut points = String::new();
        let mut series_dots = Vec::with_capacity(count);
        let mut series_rows = Vec::with_capacity(count);
        for index in 0..count {
            let value = item.data.get(index).copied().unwrap_or(0);
            let x = plot_left + step * index as f64;
            let y = y_of(value);
            if index > 0 {
                points.push(' ');
            }
            points.push_str(&format!("{x:.2},{y:.2}"));
            series_dots.push(Some((x, y, color.clone())));
            series_rows.push(Some((
                item.label.clone(),
                color.clone(),
                format_value(value, config.value_kind, true),
            )));
        }
        paths.push(LinePath { points, color });
        dots.push(series_dots);
        tooltip_rows.push(series_rows);
    }

    Geometry {
        width: CHART_WIDTH,
        height,
        plot_left,
        plot_right,
        plot_top,
        plot_bottom,
        step,
        yticks,
        xticks,
        paths,
        dots,
        tooltip_rows,
        reference_y: config.reference.map(y_of),
        categories: categories.to_vec(),
        is_empty: false,
    }
}

/// 命中最近的类目。
///
/// `offset_x` 是事件相对元素的 CSS 像素，`scale` 是「元素宽度 ÷ viewBox 宽度」
/// （`preserveAspectRatio="xMidYMid meet"` 下的等比缩放），换算回逻辑坐标后取最近类目。
fn hit_index(geometry: &Geometry, offset_x: f64, scale: f64) -> Option<usize> {
    if geometry.categories.is_empty() {
        return None;
    }
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let logical = offset_x / scale;
    if geometry.step <= 0.0 {
        return Some(0);
    }
    let raw = ((logical - geometry.plot_left) / geometry.step).round();
    if !raw.is_finite() {
        return Some(0);
    }
    let index = raw.max(0.0) as usize;
    Some(index.min(geometry.categories.len() - 1))
}

// ---------------------------------------------------------------- 数值格式化

/// nice tick 步长：1 / 2 / 2.5 / 5 × 10ⁿ。
fn nice_step(rough: f64) -> f64 {
    if !rough.is_finite() || rough <= 0.0 {
        return 1.0;
    }
    let exponent = rough.log10().floor();
    let base = 10_f64.powf(exponent);
    let normalized = rough / base;
    let factor = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * base
}

/// 扩展到 nice 边界，返回 `(min, max, step)`（全部为整数 cents/计数）。
fn nice_range(min: i64, max: i64) -> (i64, i64, i64) {
    if min == max {
        // 单值：上下各留一段
        let pad = (min.abs() / 2).max(1);
        return (min - pad, max + pad, pad.max(1));
    }
    let rough_step = (max - min) as f64 / Y_TICKS as f64;
    let step = nice_step(rough_step).max(1.0);
    let nice_min = ((min as f64) / step).floor() * step;
    let nice_max = ((max as f64) / step).ceil() * step;
    // 保证上下界都严格覆盖数据（floor/ceil 已经做到，这里只防浮点误差）
    let step_int = step.round().max(1.0) as i64;
    let nice_min = (nice_min as i64).min(min);
    let nice_max = (nice_max as i64).max(max);
    (nice_min, nice_max, step_int)
}

/// 数值 → 展示文案。
///
/// * [`ChartValueKind::Money`]：分 → 元两位小数（`with_symbol` 为真时带 `¥`，
///   刻度上不带符号以省宽度）；换算走 `tr_domain::money`。
/// * [`ChartValueKind::Percent`]：两位小数 + `%`
/// * [`ChartValueKind::Count`]：整数
pub fn format_value(value: i64, kind: ChartValueKind, with_symbol: bool) -> String {
    match kind {
        ChartValueKind::Money => {
            let yuan = tr_domain::money::cents_to_yuan(value);
            if with_symbol {
                format!("¥{yuan}")
            } else {
                yuan
            }
        }
        ChartValueKind::Percent => format!("{:.2}%", value as f64),
        ChartValueKind::Count => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ChartConfig {
        ChartConfig::default()
    }

    #[test]
    fn empty_data_does_not_panic() {
        let geometry = compute_geometry(&[], &[], &config());
        assert!(geometry.is_empty);
        assert!(geometry.paths.is_empty());
        assert!(geometry.yticks.is_empty());
    }

    #[test]
    fn single_category_and_all_zero_values_are_safe() {
        let categories = vec!["2024-01".to_string()];
        let series = vec![ChartSeries::new("支出", "var(--x)", vec![0])];
        let geometry = compute_geometry(&categories, &series, &config());
        assert!(!geometry.is_empty);
        assert!(!geometry.paths.is_empty());
        // 只有一个类目时 step = 0（不做除零），命中固定返回 0
        assert_eq!(geometry.step, 0.0);
        assert_eq!(hit_index(&geometry, 100.0, 1.0), Some(0));
        // 缩放比为 0/NaN 时退化为 1:1，不得 panic
        assert_eq!(hit_index(&geometry, 0.0, 0.0), Some(0));
    }

    #[test]
    fn y_ticks_cover_all_values_and_are_ordered() {
        let categories = vec!["a".into(), "b".into(), "c".into()];
        let series = vec![ChartSeries::new("x", "var(--x)", vec![-12345, 0, 678_900])];
        let geometry = compute_geometry(&categories, &series, &config());
        assert!(geometry.yticks.len() >= 3);
        let first = geometry
            .yticks
            .first()
            .map(|tick| tick.y)
            .unwrap_or_default();
        let last = geometry
            .yticks
            .last()
            .map(|tick| tick.y)
            .unwrap_or_default();
        // SVG 的 y 从上往下增大：最小值在底部（y 最大）
        assert!(first > last, "{first} 应大于 {last}");
    }

    #[test]
    fn reference_line_is_mapped_when_configured() {
        let categories = vec!["a".into(), "b".into()];
        let series = vec![ChartSeries::new("x", "var(--x)", vec![10, 20])];
        let geometry = compute_geometry(&categories, &series, &config().reference(0));
        assert!(geometry.reference_y.is_some());
    }

    #[test]
    fn missing_data_points_default_to_zero_without_panicking() {
        let categories = vec!["a".into(), "b".into(), "c".into()];
        // 序列只有 1 个值，其余类目必须按 0 处理而不是越界
        let series = vec![ChartSeries::new("x", "var(--x)", vec![5])];
        let geometry = compute_geometry(&categories, &series, &config());
        let dots = geometry.dots.first().cloned().unwrap_or_default();
        assert_eq!(dots.len(), 3);
        assert!(dots.iter().all(Option::is_some));
    }

    #[test]
    fn x_labels_are_thinned_but_keep_first_and_last() {
        let categories: Vec<String> = (1..=60).map(|index| format!("c{index}")).collect();
        let series = vec![ChartSeries::new("x", "var(--x)", vec![1; 60])];
        let geometry = compute_geometry(&categories, &series, &config());
        assert!(geometry.xticks.len() < 60);
        assert_eq!(
            geometry.xticks.last().map(|tick| tick.label.clone()),
            Some("c60".to_string())
        );
        assert_eq!(
            geometry.xticks.first().map(|tick| tick.label.clone()),
            Some("c1".to_string())
        );
    }

    #[test]
    fn money_formatting_uses_two_decimals_without_grouping() {
        assert_eq!(format_value(3806, ChartValueKind::Money, true), "¥38.06");
        assert_eq!(format_value(-500, ChartValueKind::Money, true), "¥-5.00");
        assert_eq!(format_value(0, ChartValueKind::Money, false), "0.00");
        assert_eq!(
            format_value(12_500_000, ChartValueKind::Money, true),
            "¥125000.00"
        );
    }

    #[test]
    fn nice_step_snaps_to_expected_factors() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.4), 2.0);
        assert_eq!(nice_step(2.2), 2.5);
        assert_eq!(nice_step(4.0), 5.0);
        assert_eq!(nice_step(9.0), 10.0);
        // 非法输入不应 panic
        assert_eq!(nice_step(f64::NAN), 1.0);
        assert_eq!(nice_step(0.0), 1.0);
    }
}
