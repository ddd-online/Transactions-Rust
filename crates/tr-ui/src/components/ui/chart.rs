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
//! 三处**在它生成的 SVG 上做定点改写**（它没暴露对应开关）：
//!
//! * 数据点改实心（`tr_draw::chart::solid_dots`）；
//! * 面积填充的基线挪到 0 轴（[`anchor_fill_at_zero`]，它只肯闭到绘图区底边）；
//! * Y 轴负刻度的千分位（`tr_draw::chart::group_negative_tick_labels`，它的 `{t}` 只给正数分组）。
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
//! ## 测试：纯算法在 tr-draw，这里只剩渲染
//!
//! 坐标、刻度、SVG 定点改写、提示框定位与几何拼装都住在 `tr-draw::chart`
//! （native + wasm32 双可编，`cargo test -p tr-draw` **真跑**那些断言）。
//!
//! 本 crate 只编译到 wasm32（`lib.rs` 顶部 `#![cfg(target_arch = "wasm32")]`），所以 native 上
//! `cargo test -p tr-ui --lib` 一条都不会执行 —— 留在下面的 `#[cfg(test)]` 只覆盖渲染侧
//! （charts-rs 出图、颜色解析、图例 CSS），它们是**给人看的规格说明**，
//! 真正的回归靠 `fixtures/ui-shots.ps1` / `ui-smoke.ps1` 那几条真启动的护栏。

use leptos::prelude::*;
use leptos::web_sys;
use wasm_bindgen::JsCast;

use charts_rs::{
    AnimationConfig, Box as ChartBox, Color as ChartColor, LineChart as ChartsLineChart,
    Series as ChartSeriesData, Symbol, THEME_DARK, THEME_LIGHT,
};

use crate::store::AppStores;

// 纯算法在 tr-draw：坐标与刻度、SVG 定点改写、提示框定位、几何拼装。
// 类型实体也搬过去了，这里 `pub use` 转发一次 —— 页面原来的
// `use crate::components::ui::{ChartSeries, ChartValueKind}` 一个字都不用改。
use tr_draw::chart::{
    anchor_fill_at_zero, display_range, display_value, group_is_series, group_negative_tick_labels,
    hit_index, nice_axis_range, paint_by_sign, place_tooltip, thin_labels, ChartGeometry,
    MeasuredPoints, FALLBACK_WIDTH, MARGIN, TICK_FONT_PX, TOOLTIP_FALLBACK_WIDTH, Y_SPLITS,
};
pub use tr_draw::chart::{ChartSeries, ChartValueKind};

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
    /// Y 轴上下界（`None` = 按数据自动挑刻度）。
    ///
    /// 天生有边界的指标必须显式给：胜率 `0..100`（出现负数时 `-100..100`）——
    /// 自动挑刻度会把"数据正好压在 100"当成"上界要严格大于数据"，
    /// 于是多撑出一档 150。数据超出给定上下界时自动退回按数据挑（防御）。
    pub y_bounds: Option<(f64, f64)>,
    /// 面积 / 折线 / 数据点按 0 轴分色：`(0 轴之上, 0 轴之下)`（`None` = 整条一个颜色）。
    ///
    /// 盈亏类曲线用（红涨绿跌：之上红、之下绿）；多序列时不分色（只有单序列才画面积）。
    pub sign_colors: Option<(String, String)>,
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
            y_bounds: None,
            sign_colors: None,
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

    pub fn y_bounds(mut self, low: f64, high: f64) -> Self {
        self.y_bounds = Some((low, high));
        self
    }

    pub fn sign_colors(mut self, above: impl Into<String>, below: impl Into<String>) -> Self {
        self.sign_colors = Some((above.into(), below.into()));
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

/// 数据点半径
const POINT_RADIUS: f32 = 2.5;
/// 入场动画时长（ms）
const ANIM_MS: u32 = 620;
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
                    display_value(
                        item.data.get(slot).copied().unwrap_or(0.0),
                        config.value_kind,
                    )
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
    // 所以同一根轴上会同时出现 `50,000` 与 `-50000`。它只给了模板字符串这一个口子、没有
    // "自定义格式化函数"，改不了上游 ⇒ 生成后在 SVG 上补一次
    // （`tr_draw::chart::group_negative_tick_labels`，见 `let svg = chart.svg()` 那里）。
    if let Some(axis) = chart.y_axis_configs.first_mut() {
        axis.axis_split_number = Y_SPLITS;
        axis.axis_font_color = token_color("--transactions-color-text-secondary");
        axis.axis_font_size = TICK_FONT_PX;
        axis.axis_stroke_color = token_color("--transactions-color-window-border");
        axis.axis_name_gap = 8.0;
        axis.axis_formatter = Some(match config.value_kind {
            ChartValueKind::Percent => "{t}%".to_string(),
            ChartValueKind::Money | ChartValueKind::Ratio | ChartValueKind::Count => {
                "{t}".to_string()
            }
        });
    }
    // **范围由我们自己定**（刻度值因此永远是大整数、跨零时 0 在正中）：把 min/max/splits
    // 一起写死，charts-rs 就不会再走它那套"按数量级把步长往上取整"的阶梯。
    // 写进**每一条** Y 轴配置：多轴时它们共用同一套网格，范围必须一致。
    let (data_min, data_max) = display_range(series, categories.len(), config.value_kind);
    let axis_range = nice_axis_range(data_min, data_max, config.y_bounds);
    if let Some((min, max, splits)) = axis_range {
        for axis in chart.y_axis_configs.iter_mut() {
            axis.axis_min = Some(min as f32);
            axis.axis_max = Some(max as f32);
            axis.axis_split_number = splits;
        }
    }

    // Y 轴负刻度的千分位：它的 `{t}` 只给正数分组，生成后补一次（见该函数的注释）
    let svg = group_negative_tick_labels(chart.svg().unwrap_or_default());
    match config.sign_colors.as_ref().filter(|_| series.len() == 1) {
        // 分色模式：填充 / 折线 / 数据点三处一起按 0 轴换色（`solid_dots` 由它代劳）
        Some((above, below)) => paint_by_sign(
            svg,
            axis_range,
            &hex_of(&resolve_color(above)),
            &hex_of(&resolve_color(below)),
        ),
        None => tr_draw::chart::solid_dots(
            anchor_fill_at_zero(svg, axis_range),
            &solid_dot_hexes(&palette),
        ),
    }
}

/// 序列色 → 实心点改写用的色号（`charts_rs::Color` 只有渲染侧认识，改写本身在 tr-draw）。
fn solid_dot_hexes(palette: &[ChartColor]) -> Vec<String> {
    palette.iter().map(hex_of).collect()
}

// ---------------------------------------------------------------- 颜色

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
        let fallback = paths
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
            // 数据点自己的颜色优先（按 0 轴分色时逐点不同，见 `paint_by_sign`）；
            // 取不到才退回整条序列的折线色
            let color = circle
                .get_attribute("fill")
                .filter(|value| value != "none")
                .unwrap_or_else(|| fallback.clone());
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
///
/// 需要"按真实像素出图"的自绘 SVG（折线图、待办的四象限图）共用它。
pub(crate) fn watch_canvas_size(canvas: NodeRef<leptos::html::Div>, size: RwSignal<(f64, f64)>) {
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
            &[ChartSeries::new("x", "var(--x)", vec![1.0, 2.0])],
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
            &[ChartSeries::new("支出", "var(--x)", vec![0.0])],
            &config(),
            320.0,
            160.0,
            true,
        );
        assert!(!single.is_empty());
        let all_zero = build_chart(
            &["a".to_string(), "b".to_string()],
            &[ChartSeries::new("x", "var(--x)", vec![0.0, 0.0])],
            &config(),
            320.0,
            160.0,
            false,
        );
        assert!(!all_zero.is_empty());
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

    /// 定位串与 CSS 的契约：内联样式负责摆位，`ui.css` 里**不许**再写 top/left/transform
    /// （会盖掉它）。前半段（`style()` 的内容）是 tr-draw 的纯算法，
    /// 后半段读的是本 crate 的样式表 —— 所以这条留在界面侧。
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
}
