//! 折线图：**几何与曲线由 [`charts_rs`] 生成 SVG**（界面层零 JS 图表库，也没有自绘的坐标变换）。
//!
//! 能力：
//!
//! * 多序列 —— [`ChartSeries`] 列表
//! * 类目轴 —— `categories`（月份 / 年份 / 序号），按可用宽度**抽稀**（首尾必显）
//! * 数值轴 —— 刻度文案随 [`ChartValueKind`] 走（金额千分位元、百分比带 `%`）
//! * 图例 —— **自绘 HTML**（可换行），由 [`ChartConfig::hide_legend`] 控制显隐。
//!   不用 charts-rs 自带的那份：它按内置 Roboto 量文字宽度排版，而 SVG 里的文字按页面字体
//!   （JetBrains Mono + 中文回退）渲染，中文系列名会被排得太近而**互相重叠**；
//!   它只能注册 TTF/OTF（仓库里的字体是 woff2，fontdue 解析不了），量不准这件事没法从它那侧修。
//! * tooltip —— `pointermove` 命中最近类目 → 竖参考线 + 浮层（多序列整列显示）；
//!   浮层**跟随鼠标**（贴着指针显示、自动避让画布四边），不再钉在图表顶部
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
//! * charts-rs：网格、Y 轴刻度与轴线、X 轴类目标签、折线与面积填充、数据点；
//! * HTML/SVG 叠层：悬停竖线、命中点、tooltip、参考线。
//!
//! 两处**在它生成的 SVG 上做定点改写**（它没暴露对应开关）：
//!
//! * 数据点改实心（[`solid_dots`]）；
//! * 面积填充的基线挪到 0 轴（[`anchor_fill_at_zero`]，它只肯闭到绘图区底边）。
//!
//! 叠层需要"每个类目的像素坐标"，而 charts-rs 不暴露布局 —— 但它会为每个数据点画
//! `<circle>`（`Symbol::Circle`），所以渲染后**从 DOM 读回**这些圆心即可拿到精确几何。
//! X 轴类目抽稀也在这里做：charts-rs 没有抽稀开关，把不要的类目传成空串即可。
//!
//! ## 纪律
//!
//! 数据为空、全为 0、只有 1 个类目、极值相等等边界都不 panic（charts-rs 对退化数据
//! 也不 panic，已用探针实测）：读 DOM 失败时叠层直接不画，不吞异常也不 unwrap。
//!
//! ## 这些 `#[cfg(test)]` 怎么才算真的跑了
//!
//! 本 crate 只编译到 wasm32（`lib.rs` 顶部 `#![cfg(target_arch = "wasm32")]`），所以 native 上
//! `cargo test -p tr-ui --lib` **一条都不会执行** —— 它们是给人看的规格说明。要真跑就把函数的
//! **原文**抽出来配桩在 native 上执行：现成做法见 `fixtures/chart-tests.ps1`
//! （把 Y 轴范围与填充基线的函数抽进一个 rustc 能编的小程序里跑，其中填充基线用 **charts-rs
//! 真实输出的 SVG** 做定点断言）。别只信自己脑补的期望值 —— 它抓到过一条写错的断言。

use leptos::prelude::*;
use leptos::web_sys;
use wasm_bindgen::JsCast;

use charts_rs::{
    AnimationConfig, Box as ChartBox, Color as ChartColor, LineChart as ChartsLineChart,
    Series as ChartSeriesData, Symbol, THEME_DARK, THEME_LIGHT,
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
/// Y 轴分段数（段数 + 1 = 刻度条数）。**只在算不出"大整数"范围时兜底**，
/// 正常路径由 [`nice_axis_range`] 给出段数（见那里的注释）。
const Y_SPLITS: usize = 4;
/// Y 轴最多分几段（= 最多 6 条刻度）。再密就压过折线本身了。
const Y_AXIS_MAX_SPLITS: usize = 5;
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
/// tooltip 与指针之间留的空隙（px，浮层不盖住光标）
const TOOLTIP_GAP: f64 = 14.0;
/// 量不到 tooltip 实际宽度时的兜底估算（首帧用；CSS 的 `min-width` 是 160，加上两侧内边距）
const TOOLTIP_FALLBACK_WIDTH: f64 = 200.0;

/// tooltip 相对画布的落点（纯计算，便于单测）。
///
/// 语义：**跟着鼠标** —— 以指针为基准，但收在画布内：
/// * 左右贴边时不再居中，`transform` 由 `-50%` 换成 `0` / `-100%`（浮层完整可见）；
/// * 上方放不下（画布顶边）就翻到指针下方。
#[derive(Debug, Clone, Copy, PartialEq)]
struct TooltipPlacement {
    /// 浮层左边缘（px，相对画布）
    left: f64,
    /// 浮层上边缘（px，相对画布）
    top: f64,
    /// `translateX` 的百分比（0 / 50 / 100，配合负号）
    translate_x: f64,
    /// `translateY` 的百分比（`-100` = 浮层整体位于 `top` 之上，`0` = 位于其下）
    translate_y: f64,
}

impl TooltipPlacement {
    /// 内联样式串（`view!` 里直接用）。
    fn style(self) -> String {
        format!(
            "left: {:.1}px; top: {:.1}px; transform: translateX(-{:.0}%) translateY({:.0}%);",
            self.left, self.top, self.translate_x, self.translate_y
        )
    }
}

/// 按"指针位置 + 画布尺寸 + 浮层尺寸"算落点。
fn place_tooltip(
    cursor: (f64, f64),
    canvas_width: f64,
    tooltip_width: f64,
    tooltip_height: f64,
) -> TooltipPlacement {
    let (cursor_x, cursor_y) = cursor;
    let width = if tooltip_width > 0.0 {
        tooltip_width
    } else {
        TOOLTIP_FALLBACK_WIDTH
    };
    let half = width / 2.0;

    // 水平：能居中就居中，贴边则夹住（`translate_x` 随之从 50 变 0 / 100）
    let (left, translate_x) = if canvas_width <= width {
        // 画布比浮层还窄：居中即可，没有可挪的余地
        (canvas_width / 2.0, 50.0)
    } else if cursor_x - half < 0.0 {
        (0.0, 0.0)
    } else if cursor_x + half > canvas_width {
        (canvas_width, 100.0)
    } else {
        (cursor_x, 50.0)
    };

    // 垂直：默认在指针上方，画布顶部放不下就翻到下方
    let (top, translate_y) = if cursor_y - tooltip_height < TOOLTIP_GAP {
        (cursor_y + TOOLTIP_GAP, 0.0)
    } else {
        (cursor_y - TOOLTIP_GAP, -100.0)
    };

    TooltipPlacement {
        left,
        top,
        translate_x,
        translate_y,
    }
}

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
    // 图例由本组件自绘（原因见 `view!` 里的注释），所以这里读配置、不再交给 charts-rs
    let hide_legend = RwSignal::new(config.hide_legend);
    // 指针在**画布坐标系**里的位置（叠层与 plotters 层同一个 viewBox，所以取自 `offsetX/offsetY`）。
    // tooltip 按它定位 —— 用户的要求是"跟着鼠标"，所以位置与命中的类目分开存：
    // 命中类目决定**内容**与竖参考线，指针位置决定**浮层放在哪**。
    let pointer = RwSignal::new(None::<(f64, f64)>);
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
    // tooltip 元素：用来量它的实际宽高（宽 → 左右避让，高 → 上下翻转）
    let tooltip = NodeRef::<leptos::html::Div>::new();
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
        // 先记指针位置（即使还没量到几何，浮层位置也该跟上）
        pointer.set(Some((
            f64::from(event.offset_x()),
            f64::from(event.offset_y()),
        )));
        let Some(geometry) = geometry.get_untracked() else {
            return;
        };
        if geometry.xs.is_empty() {
            return;
        }
        hover.set(hit_index(&geometry, f64::from(event.offset_x())));
    };
    let on_leave = move |_| {
        hover.set(None);
        pointer.set(None);
    };

    view! {
        <div class=format!("chart {class}")>
            // ---- 图例：**自绘 HTML**，不交给 charts-rs ----
            //
            // 为什么不用它自带的：它按**自己内置的 Roboto** 量文字宽度来排图例，
            // 而 SVG 里的文字是按我们声明的字体（JetBrains Mono + 中文回退）渲染的。
            // 中文/CJK 回退字比 Roboto 宽不少，于是"量出来 40px、画出来 60px"，
            // 相邻两项就叠在一起（实测「餐饮总支出」和「餐饮支出 - 商场」首尾重叠）。
            // charts-rs 只能注册 TTF/OTF，而仓库里的字体是 woff2（fontdue 解析不了），
            // 所以量不准这件事没法从它那侧修 —— 改由浏览器排版：换行、间距都由真实字体度量决定。
            <Show when=move || !hide_legend.get() && !series.get().is_empty()>
                <div class="chart__legend">
                    {move || {
                        series
                            .get()
                            .into_iter()
                            .map(|item| {
                                view! {
                                    <span class="chart__legend-item">
                                        <span
                                            class="chart__legend-swatch"
                                            style=format!(
                                                "background: {}",
                                                hex_of(&resolve_color(&item.color)),
                                            )
                                        ></span>
                                        <span class="chart__legend-label">{item.label}</span>
                                    </span>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </Show>

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
                    on:pointerleave=on_leave
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

                // ---- tooltip（跟着鼠标走，位置由指针决定、内容由命中的类目决定） ----
                {move || {
                    let Some(index) = hover.get() else {
                        return ().into_any();
                    };
                    let Some((cursor_x, cursor_y)) = pointer.get() else {
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
                    // 水平/垂直避让都收在 `place_tooltip` 里（纯函数，单测覆盖）
                    let (canvas_width, _) = size.get_untracked();
                    let measured = tooltip.get_untracked();
                    // 浮层的实际宽高：量到的值优先，量不到（首帧/还未挂载）用兜底值
                    let tip_width = measured
                        .as_ref()
                        .map(|element| f64::from(element.offset_width()))
                        .filter(|width| *width > 0.0)
                        .unwrap_or(TOOLTIP_FALLBACK_WIDTH);
                    let tip_height = measured
                        .as_ref()
                        .map(|element| f64::from(element.offset_height()))
                        .filter(|height| *height > 0.0)
                        .unwrap_or(0.0);
                    let placement =
                        place_tooltip((cursor_x, cursor_y), canvas_width, tip_width, tip_height);
                    view! {
                        <div
                            class="chart__tooltip"
                            node_ref=tooltip
                            style=placement.style()
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
    // **关掉 charts-rs 自带的图例**：它按内置 Roboto 量宽，与 SVG 实际渲染的字体
    // （JetBrains Mono + 中文回退）不一致，中文系列名会叠在一起。图例改由组件自绘成 HTML
    // （见 `view!` 顶部那段注释），排版交给浏览器，字体度量天然一致。
    chart.legend_show = Some(false);

    // ---- Y 轴 ----
    // charts-rs 的模板只有两档：`{c}` 会缩写（38.5k）、`{t}` 是千分位全值（38,500）。
    // 记账场景必须看全值，所以用 `{t}`；百分比再补一个 `%`。
    //
    // ⚠ 它的 `{t}` 对**负数不分组**（`charts/util.rs::thousands_format_float` 开头
    // `if value < 1000.0 { return format_float(value) }` 把负数全挡进了这条分支），
    // 所以同一根轴上会同时出现 `50,000` 与 `-50000`。已知、未修：改它得动上游
    // （它只给了模板字符串这一个口子，没有"自定义格式化函数"）。
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
    // **范围由我们自己定**（刻度值因此永远是大整数、跨零时 0 在正中）：把 min/max/splits
    // 一起写死，charts-rs 就不会再走它那套"按数量级把步长往上取整"的阶梯。
    // 写进**每一条** Y 轴配置：多轴时它们共用同一套网格，范围必须一致。
    let (data_min, data_max) = display_range(series, categories.len(), config.value_kind);
    let axis_range = nice_axis_range(data_min, data_max);
    if let Some((min, max, splits)) = axis_range {
        for axis in chart.y_axis_configs.iter_mut() {
            axis.axis_min = Some(min as f32);
            axis.axis_max = Some(max as f32);
            axis.axis_split_number = splits;
        }
    }

    let svg = anchor_fill_at_zero(chart.svg().unwrap_or_default(), axis_range);
    solid_dots(svg, &palette)
}

/// 面积填充 path 的判据：**只有它带 `fill-opacity`**（折线是 `fill="none" stroke="…"`，
/// 数据点是 `<circle>`，背景是 `<rect>`）。
const FILL_OPACITY_ATTR: &str = "fill-opacity";
/// SVG path 元素的起始标记
const SVG_PATH_OPEN: &str = "<path ";

/// 把面积填充的基线从"绘图区底边"挪到 **0 轴**。
///
/// charts-rs 的 `StraightLineFill` 只会把折线闭到绘图区底边（`charts/base.rs` 里写死
/// `bottom: axis_height`），于是数据**既有正又有负**时，负的那一段会被填成"从折线一路铺到图底"
/// 的大色块 —— 看着像一路亏到底，读不出"亏了多少 / 又涨回来多少"。
/// 记账图里"面积"的语义是**相对 0 的盈亏**，基线必须落在 0 轴上：正的往上长、负的往下长。
///
/// charts-rs 没暴露这个基线，所以在生成好的 SVG 上做一次**定点改写**。填充 path 的形状是固定的
/// "折线各点 + 三个收尾点"（`L (last.x, bottom) L (first.x, bottom) L first`，见
/// `StraightLineFill::svg_with_grad_seen`），把其中**前两个**收尾点的 y 换成 0 轴的 y 即可。
/// 跨零的那一段不用特殊处理：多边形自己按 nonzero 规则填充，结果就是标准的"以 0 为基线"的面积图。
///
/// 0 轴的 y 由几何反推：`charts-rs` 的映射是 `y = H × (1 − (v − min) / (max − min))`
/// （`util.rs::get_offset_height`，H 为绘图区高度），而填充的基线 y 正是 `margin.top + H`，
/// 所以 `y(0) = bottom + (bottom − margin.top) × min / (max − min)`。
/// `margin.top` 就是 [`MARGIN`]，且绘图区顶边确实落在它上面 —— 本组件始终把 charts-rs 的
/// 标题/副标题置空、自带图例关闭（图例是自绘 HTML），它不会再往顶部塞东西。
///
/// 只在**我们自己的范围真的生效**（`axis_range` 有值）且算出的 0 轴落在绘图区内时才改写；
/// 否则原样返回（宁可保持现状，也不画一条位置错的基线）。
fn anchor_fill_at_zero(svg: String, axis_range: Option<(f64, f64, usize)>) -> String {
    let Some((min, max, _splits)) = axis_range else {
        return svg;
    };
    if !(max > min) {
        return svg;
    }
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg.as_str();
    while let Some(start) = rest.find(SVG_PATH_OPEN) {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let Some(end) = tail.find("/>") else {
            out.push_str(tail);
            return out;
        };
        let (tag, after) = tail.split_at(end + 2);
        out.push_str(&rewrite_fill_baseline(tag, min, max).unwrap_or_else(|| tag.to_string()));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// 把一条填充 path 的收尾基线挪到 0 轴；不是填充 path（或结构不是预期的那三个收尾点）时返回
/// `None`，调用方保持原样。
fn rewrite_fill_baseline(tag: &str, min: f64, max: f64) -> Option<String> {
    if !tag.contains(FILL_OPACITY_ATTR) {
        return None;
    }
    let points = path_points(attr_value(tag, "d")?)?;
    // 折线各点之后固定跟三个收尾点：(last.x, bottom) (first.x, bottom) first
    if points.len() < 4 {
        return None;
    }
    let bottom = points[points.len() - 3].1;
    let height = bottom - f64::from(MARGIN);
    if !(height > 0.0) {
        return None;
    }
    let zero = bottom + height * min / (max - min);
    let top = f64::from(MARGIN);
    // 0 在 [min, max] 内 ⇒ 0 轴必定落在绘图区内；落在外面说明几何假设不成立（别画错基线）
    if !zero.is_finite() || zero < top - 0.5 || zero > bottom + 0.5 {
        return None;
    }

    let mut parts = Vec::with_capacity(points.len());
    for (index, (x, y)) in points.iter().enumerate() {
        let action = if index == 0 { "M" } else { "L" };
        let y = if index + 3 >= points.len() {
            // 最后三个点：前两个是基线端点，第三个回到首点（保持原样）
            if index == points.len() - 1 {
                *y
            } else {
                zero
            }
        } else {
            *y
        };
        parts.push(format!("{action} {} {}", fmt_coord(*x), fmt_coord(y)));
    }
    replace_attr(tag, "d", &parts.join(" "))
}

/// 解析 `StraightLineFill` 生成的 path（只有 `M`/`L`，每个命令后面固定一个 x,y 对）。
///
/// 只接受**单段**路径：charts-rs 只在数据里有空洞时才会拆成多段（多个 `M`），
/// 那时"最后三个点"就不再是收尾基线了，宁可放弃改写也不要改错。
fn path_points(d: &str) -> Option<Vec<(f64, f64)>> {
    if d.matches("M ").count() != 1 {
        return None;
    }
    let mut numbers = Vec::new();
    for token in d.split_whitespace() {
        if token == "M" || token == "L" {
            continue;
        }
        numbers.push(token.parse::<f64>().ok()?);
    }
    if numbers.len() % 2 != 0 {
        return None;
    }
    let points: Vec<(f64, f64)> = numbers
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    // 只有"每个命令恰好一个坐标对"的写法才能这样配对，`H`/`V`/`C` 之类必须放弃改写
    let commands = d.matches("M ").count() + d.matches("L ").count();
    if commands != points.len() {
        return None;
    }
    Some(points)
}

/// charts-rs 写坐标的格式（`format_float`：一位小数，`.0` 去掉）
fn fmt_coord(value: f64) -> String {
    let mut text = format!("{value:.1}");
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    text
}

/// 取 SVG 元素里某个属性的值（`name="…"`）
fn attr_value<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = element.find(&key)? + key.len();
    let end = element[start..].find('"')? + start;
    Some(&element[start..end])
}

/// 替换 SVG 元素里某个属性的值，其余字节原样保留
fn replace_attr(element: &str, name: &str, value: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = element.find(&key)?;
    let value_start = start + key.len();
    let value_end = element[value_start..].find('"')? + value_start;
    let mut out = String::with_capacity(element.len() + value.len());
    out.push_str(&element[..value_start]);
    out.push_str(value);
    out.push_str(&element[value_end..]);
    Some(out)
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

/// 全部序列的显示值范围（与 charts-rs 自己算范围的口径一致：只看数据点）。
fn display_range(series: &[ChartSeries], slots: usize, kind: ChartValueKind) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for item in series {
        for slot in 0..slots {
            let value = f64::from(display_value(
                item.data.get(slot).copied().unwrap_or(0),
                kind,
            ));
            min = min.min(value);
            max = max.max(value);
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 0.0);
    }
    (min, max)
}

/// "大整数"步长阶梯：`1 / 2 / 5 × 10^k`（… 500 / 1000 / 2000 / 5000 / 10000 …），升序。
///
/// 从比数据量级低两档起步（数据是几万元时从百位起步），往上给 12 档 —— 足够覆盖到
/// `f32` 能表示的极端值；[`nice_axis_range`] 取其中第一个"段数够用"的。
fn nice_steps(scale: f64) -> impl Iterator<Item = f64> {
    let start_power = if scale.is_finite() && scale > 0.0 {
        scale.log10().floor() as i32 - 2
    } else {
        0
    };
    (start_power..=start_power + 12).flat_map(|power| {
        let base = 10_f64.powi(power);
        [1.0, 2.0, 5.0].into_iter().map(move |factor| factor * base)
    })
}

/// Y 轴范围：刻度必须是**大整数**，并且**跨零时 0 落在正中**。
///
/// 返回 `(min, max, splits)`；刻度就是 `min + i × (max - min) / splits`。
///
/// 为什么自己算而不用 charts-rs 的默认：它那套"把步长往上取整"的阶梯按数量级分档
/// （`< 10000` 的步长按 100 取整），于是 `36000 ÷ 4 = 9000` 会被抬成 9100/9500/9600 之类，
/// 刻度就变成 9,500 / 19,000 / 28,500 —— 读图时得先做除法。这里只从
/// [`nice_steps`]（1/2/5 × 10^k）里挑步长，刻度因此永远是可心算的数。
///
/// 挑法是**能满足段数上限的最小步长**（网格尽量细），所以代价是：数据最大值恰好落在刻度上时，
/// 那一档因为"上界必须严格大于数据"要多出一段，可能撞上段数上限而让位给粗一档的步长
/// （如数据 `0..5000` 给的是 0/2000/4000/6000，而不是 0/1000/…/6000）。
///
/// 0 居中的规则：数据**跨零**时范围取成上下对称的 `±half` 且段数取**偶数**，
/// 于是 `i = splits / 2` 那条刻度正好是 0 —— 涨与跌从同一条基准线读，
/// 而不是各自从图的上下边缘起算。
///
/// 算不出（数据非有限、或极端量级下 6 段都放不下）时返回 `None`，
/// 交给 charts-rs 自己决定（见调用点的兜底）。
fn nice_axis_range(data_min: f64, data_max: f64) -> Option<(f64, f64, usize)> {
    let scale = data_min.abs().max(data_max.abs()).max(data_max - data_min);
    for step in nice_steps(scale) {
        let (min, max) = if data_min < 0.0 && data_max > 0.0 {
            let mut half_units = (data_min.abs().max(data_max) / step).ceil().max(1.0);
            // 上界必须**严格**大于数据最大值：charts-rs 只在 `axis_max > 数据最大值` 时才认这个
            // 自定义上界，否则它退回自己那套阶梯（等于白算）。用 f32 比较（它就是 f32），
            // 所以可能要多抬一格。
            while (half_units * step) as f32 <= data_max as f32 {
                half_units += 1.0;
            }
            let half = half_units * step;
            (-half, half)
        } else {
            // 不跨零：整段对齐到步长的整数倍；全正的数据从 0 起算（与 charts-rs 的默认一致）
            let low = if data_min > 0.0 { 0.0 } else { data_min };
            let low_units = (low / step).floor();
            let mut high_units = (low.max(data_max) / step).ceil();
            while (high_units * step) as f32 <= data_max as f32 {
                high_units += 1.0;
            }
            (low_units * step, high_units * step)
        };
        let splits = ((max - min) / step).round();
        if splits.is_finite() && splits >= 1.0 && splits <= Y_AXIS_MAX_SPLITS as f64 {
            return Some((min, max, splits as usize));
        }
    }
    None
}

// ==================================================================== 叠层几何

/// 从 DOM 量回来的数据点（每个序列一组 `(颜色, x, y)`，像素相对画布左上角）。
type MeasuredPoints = Vec<Vec<(String, f64, f64)>>;

/// 这个 `<g>` 分组算不算"一条可用的序列"（判定抽成纯函数，便于单测）。
///
/// ⚠ **不能要求至少两个圆**：只有一个类目时每个序列只有一个数据点、只有一个 `<circle>`，
/// 按"< 2 就跳过"会把整条序列丢掉 → 几何为空 → tooltip 永远不显示
/// （实测缺陷：「只有一组数据时图表上的 tooltip 不显示」）。非空即可。
fn group_is_series(paths: usize, circles: usize) -> bool {
    paths > 0 && circles > 0
}

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
        // ⚠ 判定见 `group_is_series`：只有一个类目时也只有一个圆，不能当成"没量到"。
        if !group_is_series(paths.length() as usize, circles.length() as usize) {
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

    /// tooltip 必须**跟着鼠标**（用户报的缺陷：它一直钉在图表顶部）。
    #[test]
    fn tooltip_follows_the_pointer_vertically() {
        // 同一根竖线上，鼠标在上半部 vs 下半部 → 浮层的 top 必须跟着走
        let high = place_tooltip((400.0, 60.0), 800.0, 200.0, 80.0);
        let low = place_tooltip((400.0, 300.0), 800.0, 200.0, 80.0);
        assert!(
            low.top > high.top,
            "浮层要跟着鼠标往下走（high={}, low={}）",
            high.top,
            low.top
        );
        // 鼠标在上方时浮层在其**上方**（`translateY(-100%)`），在下方时翻到其**下方**
        assert_eq!(high.translate_y, -100.0);
        assert_eq!(high.top, 60.0 - TOOLTIP_GAP);
        assert_eq!(low.translate_y, -100.0);
        // 贴着画布顶边放不下 → 翻到指针下方
        let at_top = place_tooltip((400.0, 20.0), 800.0, 200.0, 80.0);
        assert_eq!(at_top.translate_y, 0.0);
        assert_eq!(at_top.top, 20.0 + TOOLTIP_GAP);
    }

    #[test]
    fn tooltip_stays_inside_the_canvas_horizontally() {
        // 中间：以指针为中心
        let center = place_tooltip((400.0, 300.0), 800.0, 200.0, 80.0);
        assert_eq!((center.left, center.translate_x), (400.0, 50.0));
        // 贴左边：贴住左边缘（完整可见）
        let left = place_tooltip((30.0, 300.0), 800.0, 200.0, 80.0);
        assert_eq!((left.left, left.translate_x), (0.0, 0.0));
        // 贴右边：贴住右边缘
        let right = place_tooltip((790.0, 300.0), 800.0, 200.0, 80.0);
        assert_eq!((right.left, right.translate_x), (800.0, 100.0));
        // 画布比浮层窄：居中，没有可挪的余地
        let narrow = place_tooltip((40.0, 300.0), 120.0, 200.0, 80.0);
        assert_eq!((narrow.left, narrow.translate_x), (60.0, 50.0));
        // 首帧量不到宽度（0）时按兜底宽度算，不会算出 NaN / 负值
        let unmeasured = place_tooltip((400.0, 300.0), 800.0, 0.0, 0.0);
        assert_eq!(unmeasured.translate_x, 50.0);
        assert!(unmeasured.left.is_finite() && unmeasured.left >= 0.0);
    }

    #[test]
    fn tooltip_style_string_matches_the_placement() {
        let placement = place_tooltip((400.0, 300.0), 800.0, 200.0, 80.0);
        let style = placement.style();
        assert!(style.contains("left: 400.0px"), "{style}");
        assert!(style.contains("top: 286.0px"), "{style}");
        assert!(style.contains("translateX(-50%)"), "{style}");
        assert!(style.contains("translateY(-100%)"), "{style}");
        // 定位必须**全部**走内联样式：CSS 里再写 top/left/transform 会盖掉它
        let css = include_str!("../../../static/css/ui.css");
        let rule_start = css
            .find(".chart__tooltip {")
            .expect("ui.css 应有 .chart__tooltip");
        let rule = &css[rule_start..rule_start + 600];
        let body = &rule[..rule.find('}').expect("规则应闭合")];
        for forbidden in ["top:", "left:", "transform:"] {
            assert!(
                !body.contains(forbidden),
                "`.chart__tooltip` 里不该再写 `{forbidden}`（会盖掉内联定位）"
            );
        }
    }

    /// 图例必须是**自绘 HTML + 可换行**：交给 charts-rs 时它按内置 Roboto 量宽，
    /// 而 SVG 按页面字体渲染（JetBrains Mono + 中文回退），中文系列名会互相重叠。
    #[test]
    fn legend_is_self_drawn_html_that_wraps() {
        let css = include_str!("../../../static/css/ui.css");
        let rule = |selector: &str| -> String {
            let start = css
                .find(selector)
                .unwrap_or_else(|| panic!("ui.css 里应有 {selector}"));
            let tail = &css[start..];
            let end = tail.find('}').expect("规则应闭合");
            tail[..end].to_string()
        };

        // 自绘图例：容器允许换行（否则长系列名会把整行撑破而不是换行）
        let legend = rule(".chart__legend {");
        assert!(legend.contains("display: flex"), "{legend}");
        assert!(
            legend.contains("flex-wrap: wrap"),
            "图例必须可换行: {legend}"
        );

        // 单项允许收缩、标签过长走省略号，不会把别的项挤出去
        let item = rule(".chart__legend-item {");
        assert!(item.contains("min-width: 0"), "{item}");
        let label = rule(".chart__legend-label {");
        assert!(label.contains("text-overflow: ellipsis"), "{label}");
        assert!(label.contains("white-space: nowrap"), "{label}");

        // 反向断言：SVG 内置图例必须关掉，否则会和自绘图例同时出现
        let source = include_str!("chart.rs");
        assert!(
            source.contains("chart.legend_show = Some(false)"),
            "charts-rs 自带的图例必须关掉（它的量宽与页面字体不一致）"
        );
    }

    /// 只有一组数据时，tooltip 也必须能出来（实测缺陷：单点图 hover 没反应）。
    ///
    /// 根因是"每个序列至少两个圆才算序列"那条判定把单点序列整条丢掉了。
    #[test]
    fn single_point_series_still_yields_geometry() {
        // 判定本身：一个圆也算序列
        assert!(group_is_series(1, 1), "单点序列必须被收下");
        assert!(!group_is_series(1, 0), "没有数据点的分组才算无效");
        assert!(!group_is_series(0, 1), "没有路径的分组算无效");

        // 单点几何：3 个序列各 1 个点、同一个类目
        let points: MeasuredPoints = vec![
            vec![("#DC2626".to_string(), 100.0, 40.0)],
            vec![("#16A34A".to_string(), 100.0, 55.0)],
            vec![("#3964FE".to_string(), 100.0, 90.0)],
        ];
        let categories = vec!["2022-04".to_string()];
        let series = vec![
            ChartSeries::new("支出", "#DC2626", vec![10_400]),
            ChartSeries::new("收入", "#16A34A", vec![8_000]),
            ChartSeries::new("转账", "#3964FE", vec![0]),
        ];
        let geometry = ChartGeometry::assemble(points, &categories, &series, ChartValueKind::Money);

        assert_eq!(geometry.xs, vec![100.0], "单类目也要有一个 x");
        assert_eq!(geometry.categories.len(), 1);
        // tooltip 的内容来自 `rows_at`：单点上三个序列都要在
        let rows = geometry.rows_at(0);
        assert_eq!(rows.len(), 3, "三个序列都应出现在 tooltip 里");
        assert!(rows.iter().any(|(label, _, _)| label == "支出"));
        // 命中：鼠标落在那个点上（唯一类目）
        assert_eq!(hit_index(&geometry, 100.0), Some(0));
        assert_eq!(hit_index(&geometry, 40.0), Some(0), "唯一类目怎么指都是它");
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

    // ---------------------------------------------------------------- Y 轴范围

    /// 把范围展开成刻度（与 charts-rs 的取法一致：`min + i × (max-min)/splits`）。
    fn ticks(min: f64, max: f64, splits: usize) -> Vec<f64> {
        let step = (max - min) / splits as f64;
        (0..=splits).map(|i| min + step * i as f64).collect()
    }

    #[test]
    fn axis_ticks_are_round_numbers_for_positive_money() {
        // 用户报的那张图：月支出最高约 3.6 万 —— 以前是 9,500 / 19,000 / 28,500 / 38,000
        let (min, max, splits) = nice_axis_range(0.0, 36_000.0).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![0.0, 10_000.0, 20_000.0, 30_000.0, 40_000.0]
        );
    }

    #[test]
    fn axis_zero_sits_in_the_middle_when_data_crosses_zero() {
        // 跨零：上下对称 + 偶数段 → 正中那条刻度就是 0
        let (min, max, splits) = nice_axis_range(-3_000.0, 3_000.0).unwrap();
        assert_eq!((-min, max), (4_000.0, 4_000.0), "必须上下对称");
        assert_eq!(splits % 2, 0, "段数必须是偶数，0 才会落在刻度上");
        assert_eq!(
            ticks(min, max, splits),
            vec![-4_000.0, -2_000.0, 0.0, 2_000.0, 4_000.0]
        );

        // 不对称时按"离 0 更远的那一侧"取对称范围，0 仍在正中
        let (min, max, splits) = nice_axis_range(-1_000.0, 5_000.0).unwrap();
        assert_eq!((-min, max), (10_000.0, 10_000.0));
        assert_eq!(
            ticks(min, max, splits),
            vec![-10_000.0, -5_000.0, 0.0, 5_000.0, 10_000.0]
        );

        // 只到 0（不跨零）时**不**做对称：从 0 起算即可。
        // 这里顺带钉住"上界那格"的代价：数据最大值 5000 恰好是 1000 的整数倍，而自定义上界
        // 必须**严格**大于它，于是 1000 的步长要 6 段（0…6000）—— 超过 5 段的上限，
        // 这一档只能让位给 2000 的步长（宁可网格粗一档，也不超段数）。
        let (min, max, splits) = nice_axis_range(0.0, 5_000.0).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![0.0, 2_000.0, 4_000.0, 6_000.0]
        );
    }

    #[test]
    fn axis_range_covers_the_data_and_keeps_a_strict_upper_bound() {
        // charts-rs 只在 `axis_max > 数据最大值` 时才认这个自定义上界，否则退回它自己的阶梯；
        // 数据最大值恰好落在刻度上时（36000 是 1000 的整数倍）也必须抬一格。
        for (data_min, data_max) in [
            (0.0, 36_000.0),
            (0.0, 40_000.0),
            (0.0, 0.0),
            (-3_000.0, 3_000.0),
            (-1_000.0, 5_000.0),
            (-900.0, -100.0),
            (0.0, 100.0),
            (0.0, 2.5),
            (4_000.0, 4_000.0),
            (12_345.67, 98_765.43),
        ] {
            let (min, max, splits) = nice_axis_range(data_min, data_max)
                .unwrap_or_else(|| panic!("算不出范围: {data_min}..{data_max}"));
            assert!(min <= data_min, "{min} 没盖住下界 {data_min}");
            assert!(
                (max as f32) > (data_max as f32),
                "{max} 没有严格大于上界 {data_max}"
            );
            assert!(splits >= 1 && splits <= Y_AXIS_MAX_SPLITS);
            // 每条刻度都是步长的整数倍（也就是"大整数"）
            let step = (max - min) / splits as f64;
            for value in ticks(min, max, splits) {
                let units = value / step;
                assert!(
                    (units - units.round()).abs() < 1e-6,
                    "刻度 {value} 不是步长 {step} 的整数倍"
                );
            }
        }
    }

    // ---------------------------------------------------------------- 面积填充基线

    /// **charts-rs 1.0.0 的真实输出**（裁剪版）：把 [`build_chart`] 的配置抄进一个基于 charts-rs 的
    /// 小程序、打印 `svg()` 得到（900×320、Y 轴 -100000..100000 共 4 段、数据 -20000 / -5000 / 60000）。
    /// 升级 charts-rs 后要照这个配置重新抓一份 —— 下面的基线改写依赖它的 path 结构。
    /// 关键几何：绘图区顶边 y=8（= [`MARGIN`]）、
    /// 底边 y=286，0 刻度在 y=147（正中那条网格线）。
    /// 填充 path 的结构就是这里要锁住的东西：折线各点之后固定跟三个收尾点。
    const PROBE_SVG: &str = r##"<svg width="900" height="320" viewBox="0 0 900 320" xmlns="http://www.w3.org/2000/svg"><rect x="0" y="0" width="900" height="320" fill="#FFFFFF"/><g stroke="#E0E6F2"><line stroke-width="1" x1="64" y1="8" x2="858" y2="8"/><line stroke-width="1" x1="64" y1="147" x2="858" y2="147"/><line stroke-width="1" x1="64" y1="286" x2="858" y2="286"/></g><path d="M 196.3 174.8 L 461 153.9 L 725.7 63.6 L 725.7 286 L 196.3 286 L 196.3 174.8" fill="#5470C6" fill-opacity="0.4"/><g><path d="M 196.3 174.8 L 461 153.9 L 725.7 63.6" stroke-width="2" fill="none" stroke="#5470C6"/><circle cx="196.3" cy="174.8" r="2.5" stroke-width="2" stroke="#5470C6" fill="none"/></g></svg>"##;

    /// 从改写后的 SVG 里取填充 path 的 `d`
    fn fill_path_d(svg: &str) -> String {
        let tag = svg
            .split(SVG_PATH_OPEN)
            .find(|part| part.contains(FILL_OPACITY_ATTR))
            .expect("SVG 里应该有填充 path");
        attr_value(tag, "d")
            .expect("填充 path 必须有 d")
            .to_string()
    }

    #[test]
    fn fill_baseline_moves_to_the_zero_axis() {
        // 跨零：填充的两个收尾点必须落在 0 刻度（y=147）上，折线各点与"回到首点"保持不变
        let svg = anchor_fill_at_zero(PROBE_SVG.to_string(), Some((-100_000.0, 100_000.0, 4)));
        assert_eq!(
            fill_path_d(&svg),
            "M 196.3 174.8 L 461 153.9 L 725.7 63.6 L 725.7 147 L 196.3 147 L 196.3 174.8"
        );
        // 折线 path 与数据点圆**一个字节都不许动**（它们同样以 `<path `/`<circle` 开头）
        assert!(svg.contains(
            r#"<path d="M 196.3 174.8 L 461 153.9 L 725.7 63.6" stroke-width="2" fill="none""#
        ));
        assert!(svg.contains(r#"<circle cx="196.3" cy="174.8" r="2.5""#));
        // 网格线与其余部分原样
        assert!(svg.contains(r#"<line stroke-width="1" x1="64" y1="147" x2="858" y2="147"/>"#));
        assert_eq!(
            svg.len(),
            PROBE_SVG.len(),
            "只该改数字，不该改变长度以外的结构"
        );
    }

    #[test]
    fn fill_baseline_is_untouched_when_zero_is_an_edge_of_the_axis() {
        // 全正：0 就在绘图区底边 —— 基线本来就在 0 上，改写结果必须与原来一致
        let positive = anchor_fill_at_zero(PROBE_SVG.to_string(), Some((0.0, 40_000.0, 4)));
        assert_eq!(fill_path_d(&positive), fill_path_d(PROBE_SVG));

        // 全负：0 在绘图区**顶边**（y=8），面积应该向上长
        let negative = anchor_fill_at_zero(PROBE_SVG.to_string(), Some((-1_000.0, 0.0, 5)));
        assert_eq!(
            fill_path_d(&negative),
            "M 196.3 174.8 L 461 153.9 L 725.7 63.6 L 725.7 8 L 196.3 8 L 196.3 174.8"
        );
    }

    #[test]
    fn fill_baseline_is_left_alone_without_a_trustworthy_axis() {
        // 没有我们自己的范围（`nice_axis_range` 兜底失败）⇒ 不知道 0 在哪儿，不许动
        assert_eq!(
            anchor_fill_at_zero(PROBE_SVG.to_string(), None),
            PROBE_SVG.to_string()
        );
        // 退化范围同样不动
        assert_eq!(
            anchor_fill_at_zero(PROBE_SVG.to_string(), Some((1.0, 1.0, 4))),
            PROBE_SVG.to_string()
        );
        // 多段路径（数据有空洞时 charts-rs 会拆段）「最后三个点是基线」不再成立 ⇒ 宁可不动
        let multi = PROBE_SVG.replace(
            "L 725.7 63.6 L 725.7 286",
            "L 725.7 63.6 M 461 100 L 725.7 286",
        );
        assert_eq!(
            anchor_fill_at_zero(multi.clone(), Some((-100_000.0, 100_000.0, 4))),
            multi
        );
    }

    #[test]
    fn zero_axis_geometry_agrees_with_the_data_points() {
        // 基线的 y 是**反推**出来的（`bottom + H×min/(max−min)`），这里拿探针 SVG 里的
        // 数据点做交叉验证：-20000 → y=174.8、60000 → y=63.6 定出的映射，0 必须落在同一条 0 轴上。
        let (y_a, v_a) = (174.8_f64, -20_000.0_f64);
        let (y_b, v_b) = (63.6_f64, 60_000.0_f64);
        let from_points = y_a + (0.0 - v_a) * (y_b - y_a) / (v_b - v_a);
        let svg = anchor_fill_at_zero(PROBE_SVG.to_string(), Some((-100_000.0, 100_000.0, 4)));
        let baseline: f64 = fill_path_d(&svg)
            .split_whitespace()
            .filter_map(|token| token.parse::<f64>().ok())
            .nth(7) // 第 4 个点的 y（收尾基线）
            .expect("d 里应该有 8 个数");
        assert!(
            (baseline - from_points).abs() < 0.5,
            "反推的 0 轴 {baseline} 与数据点定出的 {from_points} 不一致"
        );
    }
}
