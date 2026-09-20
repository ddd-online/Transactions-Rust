//! 折线图：**几何与曲线由 [`charts_rs`] 生成 SVG**（界面层零 JS 图表库，也没有自绘的坐标变换）。
//!
//! 能力：
//!
//! * 多序列 —— [`ChartSeries`] 列表
//! * 类目轴 —— `categories`（月份 / 年份 / 序号），按可用宽度**抽稀**（首尾必显）
//! * 数值轴 —— 刻度文案随 [`ChartValueKind`] 走（金额千分位元、百分比带 `%`）
//! * 图例 —— [`ChartConfig::hide_legend`] 控制，由 charts-rs 画在 SVG 内
//! * tooltip —— `pointermove` 命中最近类目 → 竖参考线 + 浮层（多序列整列显示）
//! * 虚线参考线 —— [`ChartConfig::reference`]
//! * 入场动画 —— 折线自左向右描出（`prefers-reduced-motion` 下由 CSS 关闭）
//!
//! 折线**不做平滑**：只用直线段连接采样点。平滑曲线会在两点之间造出数据里没有的起伏，
//! 记账图表要的是"忠实反映这些月度/逐笔数值"。
//!
//! ## 为什么颜色在 Rust 里解析（而不是写 `var(--transactions-*)`）
//!
//! charts-rs 输出的是**定值颜色**（`stroke="#3964FE"`），它不认识 CSS 变量。
//! 所以渲染前用 `getComputedStyle` 把 `var(--x)` 解析成真值、再逐项覆盖它自带主题的
//! 配色 —— 令牌仍是颜色的唯一来源；主题变了就重新渲染（显式浅/深色看
//! `AppStores::appearance`，「跟随系统」再看 `prefers-color-scheme`）。
//!
//! ## 分工：charts-rs 画什么、叠层画什么
//!
//! * charts-rs：网格、Y 轴刻度与轴线、X 轴类目标签、图例、折线与面积填充、数据点；
//! * HTML/SVG 叠层：悬停竖线、命中点、tooltip、参考线。
//!
//! 叠层需要"每个类目的像素坐标"，而 charts-rs 不暴露布局 —— 但它会为每个数据点画
//! `<circle>`（`Symbol::Circle`），所以渲染后**从 DOM 读回**这些圆心即可拿到精确几何。
//! X 轴类目抽稀也在这里做：charts-rs 没有抽稀开关，把不要的类目传成空串即可。
//!
//! ## 纪律
//!
//! 数据为空、全为 0、只有 1 个类目、极值相等等边界都不 panic（charts-rs 对退化数据
//! 也不 panic，已用探针实测）：读 DOM 失败时叠层直接不画，不吞异常也不 unwrap。

use leptos::prelude::*;
use leptos::web_sys;
use wasm_bindgen::JsCast;

use charts_rs::{
    Align, AnimationConfig, Box as ChartBox, Color as ChartColor, LegendCategory,
    LineChart as ChartsLineChart, Series as ChartSeriesData, Symbol, THEME_DARK, THEME_LIGHT,
};

use crate::store::AppStores;

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
    /// CSS 变量名或任意合法颜色值（推荐 `var(--transactions-color-*)`）
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
    /// 金额：分 → 元；刻度千分位、tooltip 两位小数带 `¥`
    Money,
    /// 百分比：已是百分数（`12.5` → `12.50%`）
    Percent,
    /// 计数：整数
    Count,
}

/// 图表配置。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartConfig {
    /// 画布高度（px）兜底值；实际按容器实测高度渲染
    pub height: u32,
    /// Y 轴标题（charts-rs 没有独立的 Y 轴标题位，字段保留给调用方描述语义）
    pub y_title: String,
    /// Y 轴取值语义
    pub value_kind: ChartValueKind,
    /// 虚线参考线（金额时单位是分），例如 0
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

// ==================================================================== 常量

/// 拿不到容器尺寸时的兜底宽度
const FALLBACK_WIDTH: f64 = 720.0;
/// X 轴标签最小间距（px，按渲染像素算）
const X_LABEL_MIN_GAP: f64 = 56.0;
/// Y 轴分段数（段数 + 1 = 刻度条数）
const Y_SPLITS: usize = 4;
/// 数据点半径
const POINT_RADIUS: f32 = 2.5;
/// 入场动画时长（ms）
const ANIM_MS: u32 = 620;
/// 刻度与图例字号（px）：取字阶的 caption/12px
const TICK_FONT_PX: f32 = 12.0;
/// 图表内边距
const MARGIN: f32 = 8.0;
/// Y 轴文案区预留宽度（抽稀时估算可用绘图宽度用）
const Y_LABEL_AREA: f64 = 56.0;

// ==================================================================== 组件

/// 折线图。
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
    let hover = RwSignal::new(Option::<usize>::None);
    let class = class.unwrap_or_default();
    let geometry_config = config.clone();
    let reference = config.reference;
    let value_kind = config.value_kind;

    // 主题（显式浅/深色）与系统配色（跟随系统）都会换掉图表的定值颜色 → 重渲染
    let stores = AppStores::global();
    let theme = stores.appearance;
    let system_revision = system_theme_revision();

    // 画布实测尺寸：charts-rs **按容器真实像素渲染**，图表因此自适应高宽、铺满区域
    let canvas = NodeRef::<leptos::html::Div>::new();
    let size = RwSignal::new((FALLBACK_WIDTH, f64::from(config.height.max(120))));
    watch_canvas_size(canvas, size);

    // 渲染结果（SVG 文本）：数据、配置、主题、容器尺寸任一变化即重算
    let svg = Signal::derive(move || {
        let appearance = theme.get();
        let _ = system_revision.get();
        let (width, height) = size.get();
        let categories = categories.get();
        let series = series.get();
        build_chart(
            &categories,
            &series,
            &geometry_config,
            width,
            height,
            appearance.as_str() == "dark",
        )
    });

    // 悬停所需的几何：渲染后从 DOM 读回数据点圆心，再与本组件的原始数据拼起来
    let geometry = RwSignal::new(Option::<ChartGeometry>::None);
    Effect::new(move |_| {
        let _ = svg.get();
        let measured = read_points(canvas.get());
        geometry.set(measured.map(|points| {
            let categories = categories.get();
            let series = series.get();
            ChartGeometry::assemble(points, &categories, &series, value_kind)
        }));
    });

    let on_move = move |event: leptos::ev::PointerEvent| {
        let Some(geometry) = geometry.get_untracked() else {
            return;
        };
        if geometry.xs.is_empty() {
            return;
        }
        hover.set(hit_index(&geometry, f64::from(event.offset_x())));
    };

    view! {
        <div class=format!("chart {class}")>
            <div class="chart__canvas" node_ref=canvas>
                // ---- charts-rs 的静态图层：网格 / 刻度 / 轴线 / 图例 / 折线与数据点 ----
                {move || {
                    let svg = svg.get();
                    if svg.is_empty() {
                        return ().into_any();
                    }
                    view! { <div class="chart__plot" inner_html=svg></div> }.into_any()
                }}

                // ---- 叠层：参考线 + 悬停竖线 + 命中点 ----
                <svg
                    class="chart__overlay"
                    viewBox=move || {
                        let (width, height) = size.get();
                        format!("0 0 {width} {height}")
                    }
                    preserveAspectRatio="none"
                    on:pointermove=on_move
                    on:pointerleave=move |_| hover.set(None)
                >
                    {move || {
                        let Some(geometry) = geometry.get() else {
                            return ().into_any();
                        };
                        let Some(reference) = reference else {
                            return ().into_any();
                        };
                        let Some(y) = geometry.y_of(reference) else {
                            return ().into_any();
                        };
                        let (first, last) = geometry.x_extent();
                        if last <= first {
                            return ().into_any();
                        }
                        view! {
                            <line class="chart__reference" x1=first x2=last y1=y y2=y></line>
                        }
                        .into_any()
                    }}

                    {move || {
                        let Some(index) = hover.get() else {
                            return ().into_any();
                        };
                        let Some(geometry) = geometry.get() else {
                            return ().into_any();
                        };
                        let Some(x) = geometry.xs.get(index).copied() else {
                            return ().into_any();
                        };
                        let (top, bottom) = geometry.y_range();
                        view! {
                            <line class="chart__cursor" x1=x x2=x y1=top y2=bottom></line>
                        }
                        .into_any()
                    }}

                    {move || {
                        let Some(index) = hover.get() else {
                            return ().into_any();
                        };
                        let Some(geometry) = geometry.get() else {
                            return ().into_any();
                        };
                        geometry
                            .points_at(index)
                            .into_iter()
                            .map(|(color, x, y)| {
                                view! {
                                    <circle
                                        class="chart__dot"
                                        cx=x
                                        cy=y
                                        r="3.5"
                                        fill=color
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
                    let Some(geometry) = geometry.get() else {
                        return ().into_any();
                    };
                    let Some(category) = geometry.categories.get(index).cloned() else {
                        return ().into_any();
                    };
                    let rows = geometry.rows_at(index);
                    if rows.is_empty() {
                        return ().into_any();
                    }
                    let (left, right) = geometry.x_extent();
                    let percent = if right > left {
                        ((geometry.xs.get(index).copied().unwrap_or(left) - left) / (right - left))
                            * 100.0
                    } else {
                        0.0
                    };
                    view! {
                        <div class="chart__tooltip" style=format!("left: {percent:.2}%")>
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

                <Show when=move || svg.get().is_empty()>
                    <div class="chart__empty">"暂无数据"</div>
                </Show>
            </div>
        </div>
    }
}

// ==================================================================== 渲染

/// 渲染出 charts-rs 的 SVG（无数据时返回空串）。
fn build_chart(
    categories: &[String],
    series: &[ChartSeries],
    config: &ChartConfig,
    width: f64,
    height: f64,
    dark: bool,
) -> String {
    if categories.is_empty() || series.is_empty() {
        return String::new();
    }
    let width = width.max(240.0);
    let height = height.max(140.0);

    let palette: Vec<ChartColor> = series
        .iter()
        .map(|item| resolve_color(&item.color))
        .collect();
    let data: Vec<ChartSeriesData> = series
        .iter()
        .map(|item| {
            let values: Vec<f32> = (0..categories.len())
                .map(|slot| {
                    display_value(item.data.get(slot).copied().unwrap_or(0), config.value_kind)
                })
                .collect();
            ChartSeriesData::new(item.label.clone(), values)
        })
        .collect();
    let labels = thin_labels(categories, width);

    let mut chart =
        ChartsLineChart::new_with_theme(data, labels, if dark { THEME_DARK } else { THEME_LIGHT });

    // ---- 画布 ----
    chart.width = width as f32;
    chart.height = height as f32;
    chart.margin = ChartBox {
        left: MARGIN,
        top: MARGIN,
        // 右侧多留半个标签的宽度：最右类目的文案是**居中**在刻度上的，只留 MARGIN
        // 会被 SVG 边界裁掉（实测「2026-09」只剩「2026」）
        right: MARGIN + 34.0,
        bottom: MARGIN,
    };
    chart.is_light = !dark;
    chart.font_family = token_text("--transactions-font-mono");
    let background = token_color("--transactions-color-major-background");
    chart.background_color = background;
    chart.title_text = String::new();
    chart.sub_title_text = String::new();

    // ---- 网格与轴：全部取自设计令牌 ----
    chart.grid_stroke_color = if config.hide_grid {
        ChartColor {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    } else {
        token_color("--transactions-color-divider")
    };
    chart.grid_stroke_width = 1.0;
    chart.x_axis_stroke_color = token_color("--transactions-color-window-border");
    chart.x_axis_font_color = token_color("--transactions-color-text-secondary");
    chart.x_axis_font_size = TICK_FONT_PX;
    chart.x_axis_height = 26.0;
    chart.x_axis_name_gap = 8.0;
    // **X 轴两端各留半格**（charts-rs 的 `x_boundary_gap = true`，也是它的默认值）。
    //
    // 这个开关就是"X 轴起点从哪儿开始"：false = 第一个数据点**正好压在 Y 轴**上，
    // true = 起点内缩半格、首个点落在第一个格子的中线上（末点同理）。取 true 的三个理由：
    //
    // 1. **单点时它是唯一正确解**：false 时 charts-rs 用 `unit_width = 绘图宽 / (数据点数 - 1)`
    //    定位（`charts/base.rs` 的 `split_unit_count = series_data_count - 1`），单点会**除以 0**
    //    → `unit_width = ∞`、`x = ∞ × 0 = NaN`，圆点被画到未定义位置（浏览器把 `cx="NaN"`
    //    的圆画在 SVG 最左边、半个圆被裁掉 —— 实测「统计曲线」只有一笔时点就漂到那里压住 Y 轴刻度）；
    //    true 时改用 `宽 / 点数` 再加半格偏移，单点正好落在绘图区中间。
    // 2. 多点时首点不再与 Y 轴的刻度文字/轴线贴在一起（原先贴边，读起来像"点在轴外"）。
    // 3. 与柱状图的分格一致 —— 每条折线的采样点都占一个格子，而不是格子的边界。
    //
    // 代价：折线不再从绘图区最左/最右边缘起止，两端各空半格（约 `绘图宽 / 点数 / 2`）。
    // 若哪天要回到"多点首尾贴边"，把这里改成"仅单点为 true"即可（单点必须为 true，否则见第 1 条）。
    chart.x_boundary_gap = Some(true);

    // ---- 折线 ----
    chart.series_colors = palette.clone();
    chart.series_stroke_width = 2.0;
    // 直连折线：本项目的图表是"读数据"用的，平滑曲线会在两点之间造出数据里没有的
    // 起伏（尤其月度金额），读起来像真发生过。折线忠实反映"只有这些采样点"。
    chart.series_smooth = false;
    // 面积填充**只给单序列**：charts-rs 的填充接近实色，多条叠在一起会糊成一片浑色
    // （浅色下变橄榄、深色下变暗绿灰），既读不出数据也违背"用发丝线与层次、不做色块"。
    chart.series_fill = series.len() == 1;
    chart.series_symbol = Some(Symbol::Circle(POINT_RADIUS, None));
    // 自绘 tooltip：charts-rs 自带的是"按图形 hover 的单行提示"，这里要"整列多行"
    chart.tooltip_show = false;
    chart.animation = Some(AnimationConfig {
        duration: ANIM_MS,
        // 设计系统的动效曲线：指数型 ease-out
        easing: "cubic-bezier(0.22, 1, 0.36, 1)".to_string(),
        delay: 60,
    });

    // ---- 图例 ----
    chart.legend_show = Some(!config.hide_legend);
    chart.legend_align = Align::Left;
    chart.legend_category = LegendCategory::Circle;
    chart.legend_font_color = token_color("--transactions-color-text-secondary");
    chart.legend_font_size = TICK_FONT_PX;
    chart.legend_margin = Some(ChartBox {
        left: MARGIN,
        top: MARGIN,
        right: MARGIN,
        bottom: 4.0,
    });

    // ---- Y 轴 ----
    // charts-rs 的模板只有两档：`{c}` 会缩写（38.5k）、`{t}` 是千分位全值（38,500）。
    // 记账场景必须看全值，所以用 `{t}`；百分比再补一个 `%`。
    if let Some(axis) = chart.y_axis_configs.first_mut() {
        axis.axis_split_number = Y_SPLITS;
        axis.axis_font_color = token_color("--transactions-color-text-secondary");
        axis.axis_font_size = TICK_FONT_PX;
        axis.axis_stroke_color = token_color("--transactions-color-window-border");
        axis.axis_name_gap = 8.0;
        axis.axis_formatter = Some(match config.value_kind {
            ChartValueKind::Percent => "{t}%".to_string(),
            ChartValueKind::Money | ChartValueKind::Count => "{t}".to_string(),
        });
    }

    let svg = chart.svg().unwrap_or_default();
    solid_dots(svg, &palette)
}

/// 数据点改成**实心**。
///
/// charts-rs 的 `Symbol::Circle(r, None)` 表示"不填充"（渲染出来是空心环），而它只提供
/// "整个图表一个 symbol"的开关、没法逐序列指定填充色。所以在 SVG 串里按 `<circle>` 标签
/// 做定向替换：`stroke="#该序列色" fill="none"` → `fill="#该序列色"`。
///
/// **必须逐标签处理**：折线路径同样是 `stroke="#色" fill="none"`，整串替换会把折线
/// 填成一整块面积（tests 里锁了这条）。
fn solid_dots(svg: String, palette: &[ChartColor]) -> String {
    let hexes: Vec<String> = palette.iter().map(hex_of).collect();
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg.as_str();
    while let Some(start) = rest.find("<circle") {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let Some(end) = tail.find("/>") else {
            out.push_str(tail);
            return out;
        };
        let (tag, after) = tail.split_at(end + 2);
        let mut tag = tag.to_string();
        for hex in &hexes {
            let ring = format!("stroke=\"{hex}\" fill=\"none\"");
            if tag.contains(&ring) {
                tag = tag.replace(&ring, &format!("stroke=\"{hex}\" fill=\"{hex}\""));
            }
        }
        out.push_str(&tag);
        rest = after;
    }
    out.push_str(rest);
    out
}
/// charts-rs 输出颜色的写法（`#RRGGBB`，带透明度时 `#RRGGBBAA`）。
fn hex_of(color: &ChartColor) -> String {
    if color.a == 255 {
        format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            color.r, color.g, color.b, color.a
        )
    }
}

/// 按可用宽度抽稀 X 轴类目标签（首尾必显）：不要的类目传空串，charts-rs 不画空标签。
fn thin_labels(categories: &[String], width: f64) -> Vec<String> {
    let count = categories.len();
    let usable = (width - 2.0 * f64::from(MARGIN) - Y_LABEL_AREA).max(48.0);
    let widest = categories
        .iter()
        .map(|label| estimated_label_width(label, f64::from(TICK_FONT_PX)))
        .fold(0.0_f64, f64::max);
    let min_gap = X_LABEL_MIN_GAP.max(widest + 12.0);
    let max_labels = ((usable / min_gap).floor() as usize).max(1);
    let stride = if count > max_labels {
        (count as f64 / max_labels as f64).ceil() as usize
    } else {
        1
    }
    .max(1);

    let mut keep: Vec<usize> = (0..count).filter(|index| index % stride == 0).collect();
    if keep.last().copied() != Some(count - 1) {
        keep.push(count - 1);
    }
    // 补上的最后一个若与前一个太近，就去掉前一个（否则两个标签会叠着画）
    if keep.len() >= 2 {
        let step = usable / (count.max(2) - 1) as f64;
        let last = keep[keep.len() - 1];
        let previous = keep[keep.len() - 2];
        if (last - previous) as f64 * step < min_gap {
            keep.remove(keep.len() - 2);
        }
    }

    (0..count)
        .map(|index| {
            if keep.contains(&index) {
                categories.get(index).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        })
        .collect()
}

/// 数值 → charts-rs 的画图值（金额换算成"元"，其余原样）。
fn display_value(value: i64, kind: ChartValueKind) -> f32 {
    match kind {
        ChartValueKind::Money => tr_domain::money::cents_to_yuan(value)
            .parse::<f32>()
            .unwrap_or(0.0),
        ChartValueKind::Percent | ChartValueKind::Count => value as f32,
    }
}

// ==================================================================== 叠层几何

/// 从 DOM 量回来的数据点（每个序列一组 `(颜色, x, y)`，像素相对画布左上角）。
type MeasuredPoints = Vec<Vec<(String, f64, f64)>>;

/// 叠层几何：DOM 量回来的像素坐标 + 组件自己知道的数据。
#[derive(Debug, Clone, Default)]
pub struct ChartGeometry {
    /// 每个类目的 x 像素
    pub xs: Vec<f64>,
    /// 每个类目上各序列的 `(颜色, x, y)`
    pub dots: Vec<Vec<(String, f64, f64)>>,
    /// X 轴类目文案
    pub categories: Vec<String>,
    /// tooltip 行：每个序列一组 `(label, 颜色, 文案)`
    pub rows: Vec<Vec<(String, String, String)>>,
    /// 值 → y 的仿射映射 `(v0, y0, v1, y1)`（画图值，金额是元）；退化时为 `None`
    pub value_axis: Option<(f64, f64, f64, f64)>,
}

impl ChartGeometry {
    /// 把量回来的点与原始数据拼成完整几何。
    fn assemble(
        points: MeasuredPoints,
        categories: &[String],
        series: &[ChartSeries],
        kind: ChartValueKind,
    ) -> Self {
        let count = points.iter().map(|item| item.len()).max().unwrap_or(0);
        // 每个类目的 x：多个序列取平均（同一类目上它们共享 x）
        let xs: Vec<f64> = (0..count)
            .map(|index| {
                let values: Vec<f64> = points
                    .iter()
                    .filter_map(|item| item.get(index).map(|(_, x, _)| *x))
                    .collect();
                if values.is_empty() {
                    0.0
                } else {
                    values.iter().sum::<f64>() / values.len() as f64
                }
            })
            .collect();
        // 值 → y 的仿射映射：找任意一个"两个点取值不同"的序列
        let mut value_axis = None;
        for (series_index, item) in points.iter().enumerate() {
            let Some(data) = series.get(series_index) else {
                continue;
            };
            let Some(first) = item.first() else {
                continue;
            };
            let first_value = display_value(data.data.first().copied().unwrap_or(0), kind) as f64;
            for (offset, (_, _, y)) in item.iter().enumerate().skip(1) {
                let value = display_value(data.data.get(offset).copied().unwrap_or(0), kind) as f64;
                if (value - first_value).abs() > f64::EPSILON {
                    value_axis = Some((first_value, first.2, value, *y));
                    break;
                }
            }
            if value_axis.is_some() {
                break;
            }
        }
        let rows = series
            .iter()
            .map(|item| {
                (0..count)
                    .map(|slot| {
                        let value = item.data.get(slot).copied().unwrap_or(0);
                        (
                            item.label.clone(),
                            item.color.clone(),
                            format_value(value, kind, true),
                        )
                    })
                    .collect()
            })
            .collect();
        Self {
            xs,
            dots: points,
            categories: categories.to_vec(),
            rows,
            value_axis,
        }
    }

    /// 参考值 → y 像素。
    pub fn y_of(&self, value: i64) -> Option<f64> {
        let (v0, y0, v1, y1) = self.value_axis?;
        if (v1 - v0).abs() < f64::EPSILON {
            return None;
        }
        Some(y0 + (value as f64 - v0) / (v1 - v0) * (y1 - y0))
    }

    /// 绘图区的纵向范围（数据点上下边界）。
    pub fn y_range(&self) -> (f64, f64) {
        let mut top = f64::INFINITY;
        let mut bottom = f64::NEG_INFINITY;
        for series in &self.dots {
            for (_, _, y) in series {
                top = top.min(*y);
                bottom = bottom.max(*y);
            }
        }
        if !top.is_finite() || !bottom.is_finite() {
            return (0.0, 0.0);
        }
        (top, bottom)
    }

    /// 绘图区的横向范围（首尾类目）。
    pub fn x_extent(&self) -> (f64, f64) {
        (
            self.xs.first().copied().unwrap_or(0.0),
            self.xs.last().copied().unwrap_or(0.0),
        )
    }

    /// 某个类目上各序列的命中点。
    pub fn points_at(&self, index: usize) -> Vec<(String, f64, f64)> {
        self.dots
            .iter()
            .filter_map(|series| series.get(index).cloned())
            .collect()
    }

    /// 某个类目的 tooltip 行。
    pub fn rows_at(&self, index: usize) -> Vec<(String, String, String)> {
        self.rows
            .iter()
            .filter_map(|series| series.get(index).cloned())
            .collect()
    }
}

/// 渲染后从 DOM 读回数据点圆心。
///
/// charts-rs 为每个序列输出一个 `<g>`：内含一条折线 `<path>` 与逐点 `<circle>`。
/// 图例组只有圆没有线、网格组只有线没有圆，据此筛出序列组。
fn read_points(canvas: Option<web_sys::HtmlDivElement>) -> Option<MeasuredPoints> {
    let host = canvas?;
    let groups = host.query_selector_all(".chart__plot svg g").ok()?;
    let origin = host.get_bounding_client_rect();
    let mut measured: MeasuredPoints = Vec::new();
    for group_index in 0..groups.length() {
        let Some(node) = groups.item(group_index) else {
            continue;
        };
        let Ok(group) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let Ok(paths) = group.query_selector_all("path") else {
            continue;
        };
        let Ok(circles) = group.query_selector_all("circle") else {
            continue;
        };
        if paths.length() == 0 || circles.length() < 2 {
            continue;
        }
        let color = paths
            .item(0)
            .and_then(|path| path.dyn_into::<web_sys::Element>().ok())
            .and_then(|path| path.get_attribute("stroke"))
            .filter(|value| value != "none")
            .unwrap_or_else(|| "var(--transactions-color-transfer)".to_string());
        let mut points = Vec::new();
        for circle_index in 0..circles.length() {
            let Some(node) = circles.item(circle_index) else {
                continue;
            };
            let Ok(circle) = node.dyn_into::<web_sys::Element>() else {
                continue;
            };
            let rect = circle.get_bounding_client_rect();
            points.push((
                color.clone(),
                rect.left() + rect.width() / 2.0 - origin.left(),
                rect.top() + rect.height() / 2.0 - origin.top(),
            ));
        }
        if !points.is_empty() {
            measured.push(points);
        }
    }
    if measured.is_empty() {
        None
    } else {
        Some(measured)
    }
}

/// 命中最近的类目。
pub(crate) fn hit_index(geometry: &ChartGeometry, offset_x: f64) -> Option<usize> {
    if geometry.xs.is_empty() {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for (index, candidate) in geometry.xs.iter().enumerate() {
        let distance = (candidate - offset_x).abs();
        if best.map(|(_, current)| distance < current).unwrap_or(true) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

// ==================================================================== 颜色

/// 读 `<html>` 上的设计令牌计算值。
///
/// 非 wasm 目标（只有测试会在那儿跑）直接返回空串 —— `web_sys` 的导入在非 wasm 上
/// 是会 panic 的桩，不能无条件调用。
#[cfg(target_arch = "wasm32")]
pub(crate) fn computed_token(name: &str) -> String {
    let Some(window) = web_sys::window() else {
        return String::new();
    };
    let Some(root) = window.document().and_then(|doc| doc.document_element()) else {
        return String::new();
    };
    window
        .get_computed_style(&root)
        .ok()
        .flatten()
        .and_then(|style| style.get_property_value(name).ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn computed_token(_name: &str) -> String {
    String::new()
}

/// 令牌名 → charts-rs 颜色（解析不出来时退到次级文字色）。
fn token_color(name: &str) -> ChartColor {
    parse_css_color(&computed_token(name)).unwrap_or(ChartColor {
        r: 128,
        g: 132,
        b: 138,
        a: 255,
    })
}

/// 令牌名 → 原始文本（字体栈这类直接透传）。
fn token_text(name: &str) -> String {
    let value = computed_token(name);
    if value.trim().is_empty() {
        "ui-monospace, Consolas, monospace".to_string()
    } else {
        value
    }
}

/// `var(--x)` / `#rrggbb` / `rgb(...)` → charts-rs 颜色。
fn resolve_color(value: &str) -> ChartColor {
    let raw = value.trim();
    let resolved = if let Some(inner) = raw
        .strip_prefix("var(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        computed_token(inner.trim())
    } else {
        raw.to_string()
    };
    parse_css_color(&resolved).unwrap_or_else(|| token_color("--transactions-color-text-secondary"))
}

/// 解析 `#rgb` / `#rrggbb` / `#rrggbbaa` / `rgb(r,g,b)` / `rgba(r,g,b,a)`。
pub(crate) fn parse_css_color(raw: &str) -> Option<ChartColor> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 | 8 => hex.to_string(),
            _ => return None,
        };
        let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&expanded[range], 16).ok();
        return Some(ChartColor {
            r: channel(0..2)?,
            g: channel(2..4)?,
            b: channel(4..6)?,
            a: if expanded.len() == 8 {
                channel(6..8)?
            } else {
                255
            },
        });
    }
    if let Some(inner) = raw
        .strip_prefix("rgb(")
        .or_else(|| raw.strip_prefix("rgba("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split([',', ' ']).filter(|p| !p.is_empty()).collect();
        let channel = |index: usize| -> Option<u8> {
            let text = parts.get(index)?;
            let value: f64 = text.trim().parse().ok()?;
            Some(value.clamp(0.0, 255.0).round() as u8)
        };
        let alpha = parts
            .get(3)
            .and_then(|text| text.trim().parse::<f64>().ok())
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
            .unwrap_or(255);
        return Some(ChartColor {
            r: channel(0)?,
            g: channel(1)?,
            b: channel(2)?,
            a: alpha,
        });
    }
    None
}

/// 「跟随系统」时必须跟着系统配色重渲染：订阅一次 `prefers-color-scheme`，
/// 变化时把修订号 +1（信号被渲染闭包读取）。
fn system_theme_revision() -> RwSignal<u32> {
    let revision = RwSignal::new(0_u32);
    let Some(window) = web_sys::window() else {
        return revision;
    };
    let Ok(Some(query)) = window.match_media("(prefers-color-scheme: dark)") else {
        return revision;
    };
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        revision.update(|value| *value = value.wrapping_add(1));
    });
    query
        .add_event_listener_with_callback("change", listener.as_ref().unchecked_ref())
        .ok();
    // 监听器活到应用结束（界面进程即应用进程）
    listener.forget();
    revision
}

// ==================================================================== 尺寸

/// 观察画布尺寸：挂载后量一次，之后窗口尺寸变化时重量。
fn watch_canvas_size(canvas: NodeRef<leptos::html::Div>, size: RwSignal<(f64, f64)>) {
    let measure = move || {
        let Some(element) = canvas.get() else {
            return;
        };
        let width = f64::from(element.client_width());
        let height = f64::from(element.client_height());
        if width >= 1.0 && height >= 1.0 {
            size.set((width, height));
        }
    };
    Effect::new(move |_| {
        measure();
        if let Some(window) = web_sys::window() {
            let callback = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || measure());
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
            callback.forget();
        }
    });
    if let Some(window) = web_sys::window() {
        let listener = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || measure());
        let _ =
            window.add_event_listener_with_callback("resize", listener.as_ref().unchecked_ref());
        // 监听器活到应用结束（界面进程即应用进程）
        listener.forget();
    }
}

/// 估算标签像素宽度（只用来决定抽稀，粗算即可）：CJK/全角按 1.0 em，其余按 0.6 em。
fn estimated_label_width(label: &str, font_px: f64) -> f64 {
    label
        .chars()
        .map(|c| {
            if is_wide_char(c) {
                font_px
            } else {
                font_px * 0.6
            }
        })
        .sum()
}

/// 是否宽字符（常用 CJK 与全角标点的粗略区间）。
fn is_wide_char(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    )
}

// ==================================================================== 数值格式

/// 数值 → 展示文案。
///
/// * [`ChartValueKind::Money`]：分 → 元两位小数（`with_symbol` 为真时带 `¥`）；
///   换算走 `tr_domain::money`。
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
    fn empty_data_renders_nothing() {
        assert!(build_chart(&[], &[], &config(), 720.0, 360.0, false).is_empty());
    }

    #[test]
    fn series_without_categories_renders_nothing() {
        let svg = build_chart(
            &[],
            &[ChartSeries::new("x", "var(--x)", vec![1, 2])],
            &config(),
            720.0,
            360.0,
            false,
        );
        assert!(svg.is_empty());
    }

    #[test]
    fn degenerate_data_does_not_panic() {
        let single = build_chart(
            &["2026-01".to_string()],
            &[ChartSeries::new("支出", "var(--x)", vec![0])],
            &config(),
            320.0,
            160.0,
            true,
        );
        assert!(!single.is_empty());
        let all_zero = build_chart(
            &["a".to_string(), "b".to_string()],
            &[ChartSeries::new("x", "var(--x)", vec![0, 0])],
            &config(),
            320.0,
            160.0,
            false,
        );
        assert!(!all_zero.is_empty());
    }

    #[test]
    fn thin_labels_keeps_first_and_last() {
        let categories: Vec<String> = (0..12).map(|i| format!("2026-{i:02}")).collect();
        let narrow = thin_labels(&categories, 320.0);
        assert_eq!(narrow.len(), categories.len());
        assert_eq!(narrow[0], categories[0]);
        assert_eq!(narrow[11], categories[11]);
        assert!(narrow.iter().filter(|label| !label.is_empty()).count() < categories.len());
        let wide = thin_labels(&categories, 2400.0);
        assert_eq!(
            wide.iter().filter(|label| !label.is_empty()).count(),
            categories.len()
        );
    }

    #[test]
    fn money_is_drawn_in_yuan() {
        assert!((display_value(123_456, ChartValueKind::Money) - 1234.56).abs() < 0.01);
        assert!((display_value(50, ChartValueKind::Percent) - 50.0).abs() < f32::EPSILON);
        assert!((display_value(7, ChartValueKind::Count) - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tooltip_rows_are_formatted_per_kind() {
        assert_eq!(
            format_value(123_456, ChartValueKind::Money, true),
            "¥1234.56"
        );
        assert_eq!(
            format_value(12_500, ChartValueKind::Percent, true),
            "125.00%"
        );
        assert_eq!(format_value(3, ChartValueKind::Count, true), "3");
    }

    #[test]
    fn solid_dots_only_touches_circles() {
        let svg = concat!(
            r##"<path d="M0 0 L1 1" stroke="#DC2626" fill="none"/>"##,
            r##"<circle cx="1" cy="1" r="2.5" stroke-width="2" stroke="#DC2626" fill="none"/>"##,
            r##"<circle cx="3" cy="3" r="5.5" stroke-width="2" stroke="#16A34A" fill="#16A34A"/>"##,
        )
        .to_string();
        let palette = [
            ChartColor {
                r: 0xDC,
                g: 0x26,
                b: 0x26,
                a: 255,
            },
            ChartColor {
                r: 0x16,
                g: 0xA3,
                b: 0x4A,
                a: 255,
            },
        ];
        let out = solid_dots(svg, &palette);
        // 折线路径**不许**被填成面积
        assert!(out.contains(r##"<path d="M0 0 L1 1" stroke="#DC2626" fill="none"/>"##));
        // 数据点变成实心
        assert!(out.contains(
            r##"<circle cx="1" cy="1" r="2.5" stroke-width="2" stroke="#DC2626" fill="#DC2626"/>"##
        ));
        // 图例标记本来就是实心，保持原样
        assert!(out.contains(
            r##"<circle cx="3" cy="3" r="5.5" stroke-width="2" stroke="#16A34A" fill="#16A34A"/>"##
        ));
    }
    #[test]
    fn css_color_parsing() {
        assert_eq!(
            parse_css_color("#3964fe"),
            Some(ChartColor {
                r: 0x39,
                g: 0x64,
                b: 0xfe,
                a: 255
            })
        );
        assert_eq!(
            parse_css_color("#39f"),
            Some(ChartColor {
                r: 0x33,
                g: 0x99,
                b: 0xff,
                a: 255
            })
        );
        assert_eq!(
            parse_css_color("rgb(1, 2, 3)"),
            Some(ChartColor {
                r: 1,
                g: 2,
                b: 3,
                a: 255
            })
        );
        assert_eq!(
            parse_css_color("rgba(10, 20, 30, 0.5)"),
            Some(ChartColor {
                r: 10,
                g: 20,
                b: 30,
                a: 128
            })
        );
        assert_eq!(parse_css_color("nonsense"), None);
        assert_eq!(parse_css_color(""), None);
    }

    #[test]
    fn hit_index_picks_nearest_category() {
        let geometry = ChartGeometry {
            xs: vec![10.0, 50.0, 90.0],
            ..Default::default()
        };
        assert_eq!(hit_index(&geometry, 12.0), Some(0));
        assert_eq!(hit_index(&geometry, 48.0), Some(1));
        assert_eq!(hit_index(&geometry, 200.0), Some(2));
        assert_eq!(hit_index(&ChartGeometry::default(), 12.0), None);
    }

    #[test]
    fn reference_line_uses_affine_mapping() {
        let geometry = ChartGeometry {
            value_axis: Some((0.0, 100.0, 100.0, 0.0)),
            ..Default::default()
        };
        assert_eq!(geometry.y_of(50), Some(50.0));
        assert_eq!(geometry.y_of(0), Some(100.0));
        assert_eq!(ChartGeometry::default().y_of(0), None);
    }

    #[test]
    fn geometry_assembles_rows_and_axis() {
        let categories = vec!["a".to_string(), "b".to_string()];
        let series = vec![ChartSeries::new("支出", "var(--x)", vec![100, 300])];
        let measured: MeasuredPoints = vec![vec![
            ("#dc2626".to_string(), 10.0, 90.0),
            ("#dc2626".to_string(), 20.0, 70.0),
        ]];
        let geometry =
            ChartGeometry::assemble(measured, &categories, &series, ChartValueKind::Money);
        assert_eq!(geometry.xs, vec![10.0, 20.0]);
        assert_eq!(geometry.x_extent(), (10.0, 20.0));
        assert_eq!(geometry.y_range(), (70.0, 90.0));
        assert_eq!(geometry.rows_at(0).len(), 1);
        assert_eq!(geometry.points_at(1).len(), 1);
        // 1.00 元 → y=90，3.00 元 → y=70 ⇒ 2.00 元（200 分）→ y=80
        assert_eq!(geometry.y_of(200), Some(80.0));
    }
}
