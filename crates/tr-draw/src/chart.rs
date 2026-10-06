//! 折线图的纯算法：坐标、刻度、路径、填充与提示框定位。
//!
//! 本模块不认识 DOM，也不认识 charts-rs：输入是 `f64` / `String` / 本模块自己的类型，
//! 输出是数值、路径字符串或改写过的 SVG 文本。渲染侧（`crates/tr-ui` 的
//! `components/ui/chart.rs`）负责把 DOM 测量值喂进来、把结果画出去。
//!
//! 分界就是这条：**能在 native 上断言的东西放这里**（`cargo test -p tr-draw`），
//! 需要 `getComputedStyle` / `charts_rs::Color` / 真实布局的留在界面侧。
//! 另外，渲染配置本身（`ChartConfig`、`POINT_RADIUS`、`ANIM_MS`）虽然也是纯数据，
//! 但只有渲染用得到，所以留在界面侧 —— 判据的完整表述见 `docs/adr/0001-pure-draw-crate.md`。

/// 一条序列。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// 图例文案
    pub label: String,
    /// CSS 变量名或任意合法颜色值（推荐 `var(--transactions-color-*)`）
    pub color: String,
    /// 与 X 轴类目一一对应的取值（长度不足的类目按 0 处理）。
    ///
    /// **金额仍是"分"**（由 `ChartValueKind::Money` 换算成元）；用 `f64` 是因为比值、
    /// 百分比这类指标本来就是小数，取整会把 0.81 变成 1（见 `ChartValueKind::Ratio`）。
    pub data: Vec<f64>,
}

impl ChartSeries {
    pub fn new(label: impl Into<String>, color: impl Into<String>, data: Vec<f64>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            data,
        }
    }
}

/// Y 轴取值语义（决定刻度与 tooltip 的格式化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartValueKind {
    /// 金额：分 → 元；刻度千分位、tooltip 两位小数带 `¥`
    #[default]
    Money,
    /// 百分比：已是百分数（`12.5` → `12.50%`）
    Percent,
    /// 比值/倍数：两位小数（`0.81` → `0.81`）
    Ratio,
    /// 计数：整数
    Count,
}

/// 拿不到容器尺寸时的兜底宽度
pub const FALLBACK_WIDTH: f64 = 720.0;

/// X 轴标签最小间距（px，按渲染像素算）
const X_LABEL_MIN_GAP: f64 = 56.0;

/// Y 轴分段数（段数 + 1 = 刻度条数）。**只在算不出"大整数"范围时兜底**，
/// 正常路径由 [`nice_axis_range`] 给出段数（见那里的注释）。
pub const Y_SPLITS: usize = 4;

/// Y 轴最多分几段（= 最多 6 条刻度）。再密就压过折线本身了。
const Y_AXIS_MAX_SPLITS: usize = 5;

/// 上界贴着数据时抬起的微量（只为过 charts-rs 的"`axis_max` 必须严格大于数据"那一关；
/// 刻度按一位小数格式化，0.01 不会出现在刻度文字里）。
const AXIS_LIMIT_EPSILON: f64 = 0.01;

/// 刻度与类目标签的字号（px）。标签抽稀按它估文字宽度。
pub const TICK_FONT_PX: f32 = 12.0;

/// 图表内边距
pub const MARGIN: f32 = 8.0;

/// Y 轴文案区预留宽度（抽稀时估算可用绘图宽度用）
const Y_LABEL_AREA: f64 = 56.0;

/// tooltip 与指针之间留的空隙（px，浮层不盖住光标）
const TOOLTIP_GAP: f64 = 14.0;

/// 量不到 tooltip 实际宽度时的兜底估算（首帧用；CSS 的 `min-width` 是 160，加上两侧内边距）
pub const TOOLTIP_FALLBACK_WIDTH: f64 = 200.0;

/// tooltip 相对画布的落点（纯计算，便于单测）。
///
/// 语义：**跟着鼠标** —— 以指针为基准，但收在画布内：
/// * 左右贴边时不再居中，`transform` 由 `-50%` 换成 `0` / `-100%`（浮层完整可见）；
/// * 上方放不下（画布顶边）就翻到指针下方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooltipPlacement {
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
    pub fn style(self) -> String {
        format!(
            "left: {:.1}px; top: {:.1}px; transform: translateX(-{:.0}%) translateY({:.0}%);",
            self.left, self.top, self.translate_x, self.translate_y
        )
    }
}

/// 按"指针位置 + 画布尺寸 + 浮层尺寸"算落点。
pub fn place_tooltip(
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

/// 面积填充 path 的判据：**只有它带 `fill-opacity`**（折线是 `fill="none" stroke="…"`，
/// 数据点是 `<circle>`，背景是 `<rect>`）。
const FILL_OPACITY_ATTR: &str = "fill-opacity";

/// SVG path 元素的起始标记
const SVG_PATH_OPEN: &str = "<path ";

/// SVG 里"没有填充"的写法（折线 path 用它；填充 path 用的是具体颜色）
const FILL_NONE_ATTR: &str = r#"fill="none""#;

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
pub fn anchor_fill_at_zero(svg: String, axis_range: Option<(f64, f64, usize)>) -> String {
    let Some((min, max, _splits)) = axis_range else {
        return svg;
    };
    // 同 [`zero_axis_y`]：严格 `max > min` 才算数（NaN / 相等都不改写）。
    if !matches!(max.partial_cmp(&min), Some(std::cmp::Ordering::Greater)) {
        return svg;
    }
    rewrite_tags(&svg, SVG_PATH_OPEN, |tag| {
        rewrite_fill_baseline(tag, min, max)
    })
}

/// 逐个替换 SVG 里以 `open` 开头的自闭合标签（`…/>`）；`f` 返回 `None` 的标签原样保留。
///
/// 三处定点改写（填充基线 / 实心点 / 按 0 轴分色）共用这一套扫描：只认 `<path `/`<circle`
/// 这样的起始标记，标签之间的字节（网格线、文字、缩进）原样搬过去。
pub fn rewrite_tags(svg: &str, open: &str, f: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find(open) {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let Some(end) = tail.find("/>") else {
            out.push_str(tail);
            return out;
        };
        let (tag, after) = tail.split_at(end + 2);
        out.push_str(&f(tag).unwrap_or_else(|| tag.to_string()));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// 数据点改成**实心**：给出每个序列的色号（`#RRGGBB` 或带 alpha 的 `#RRGGBBAA`）。
///
/// charts-rs 的 `Symbol::Circle(r, None)` 表示"不填充"（渲染出来是空心环），而它只提供
/// "整个图表一个 symbol"的开关、没法逐序列指定填充色。所以在 SVG 串里按 `<circle>` 标签
/// 做定向替换：`stroke="#该序列色" fill="none"` → `fill="#该序列色"`。
///
/// **必须逐标签处理**：折线路径同样是 `stroke="#色" fill="none"`，整串替换会把折线
/// 填成一整块面积（下面 `solid_dots_only_touches_circles` 锁的就是这条）。
///
/// 色号由界面侧给（`charts_rs::Color` 只有渲染侧认识，本 crate 不许依赖它）。
pub fn solid_dots(svg: String, hexes: &[String]) -> String {
    rewrite_tags(&svg, "<circle", |tag| {
        let mut tag = tag.to_string();
        for hex in hexes {
            let ring = format!("stroke=\"{hex}\" fill=\"none\"");
            if tag.contains(&ring) {
                tag = tag.replace(&ring, &format!("stroke=\"{hex}\" fill=\"{hex}\""));
            }
        }
        Some(tag)
    })
}

/// 0 轴的 y 像素：由填充 path 的**收尾基线**（绘图区底边）与 Y 轴范围反推。
///
/// 填充 path 的形状是"折线各点 + 三个收尾点"，倒数第三个点的 y 就是绘图区底边。
/// 算不出高度、或 0 落在绘图区外时返回 `None`（几何假设不成立就别画）。
fn zero_axis_y(points: &[(f64, f64)], min: f64, max: f64) -> Option<f64> {
    // 只认严格 `max > min`：NaN 与相等都不成立（写成 `partial_cmp` 是为了让
    // "NaN 也不想画"这条意图显式 —— `!(max > min)` 读起来像笔误）。
    if !matches!(max.partial_cmp(&min), Some(std::cmp::Ordering::Greater)) || points.len() < 4 {
        return None;
    }
    let bottom = points[points.len() - 3].1;
    let height = bottom - f64::from(MARGIN);
    if !matches!(height.partial_cmp(&0.0), Some(std::cmp::Ordering::Greater)) {
        return None;
    }
    let zero = bottom + height * min / (max - min);
    if !zero.is_finite() || zero < f64::from(MARGIN) - 0.5 || zero > bottom + 0.5 {
        return None;
    }
    Some(zero)
}

/// 把一条填充 path 的收尾基线挪到 0 轴；不是填充 path（或结构不是预期的那三个收尾点）时返回
/// `None`，调用方保持原样。
fn rewrite_fill_baseline(tag: &str, min: f64, max: f64) -> Option<String> {
    if !tag.contains(FILL_OPACITY_ATTR) {
        return None;
    }
    let points = path_points(attr_value(tag, "d")?)?;
    let zero = zero_axis_y(&points, min, max)?;

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

/// 折线按 0 轴切成若干**连续段**：`(点集, 是否在 0 轴之上)`。
///
/// 相邻两点的 y 跨过 0 轴时插入**交点**（两段各收一个），段与段首尾相接 —— 于是每段都能
/// 单独闭合成"以 0 轴为底"的多边形，颜色可以按符号分开（红涨绿跌）。
fn split_at_zero(points: &[(f64, f64)], zero: f64) -> Vec<(Vec<(f64, f64)>, bool)> {
    // SVG 里 y 越小越靠上：`y <= 0 轴` 就是"在 0 轴之上"
    let above = |point: &(f64, f64)| point.1 <= zero;
    let mut runs: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
    for (index, point) in points.iter().enumerate() {
        if index == 0 {
            runs.push((vec![*point], above(point)));
            continue;
        }
        let previous = points[index - 1];
        if above(&previous) == above(point) {
            if let Some(run) = runs.last_mut() {
                run.0.push(*point);
            }
            continue;
        }
        let span = point.1 - previous.1;
        let ratio = if span.abs() < f64::EPSILON {
            0.0
        } else {
            (zero - previous.1) / span
        };
        let crossing = (previous.0 + ratio * (point.0 - previous.0), zero);
        if let Some(run) = runs.last_mut() {
            run.0.push(crossing);
        }
        runs.push((vec![crossing, *point], above(point)));
    }
    runs
}

/// 折线 path 的 `d`（`M x y L x y …`，与 charts-rs 的写法一致）
fn polyline_d(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(index, (x, y))| {
            format!(
                "{} {} {}",
                if index == 0 { "M" } else { "L" },
                fmt_coord(*x),
                fmt_coord(*y)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 一段折线闭合成"以 0 轴为底"的多边形（单点、竖直段没有面积，返回 `None`）
fn fill_d(run: &[(f64, f64)], zero: f64) -> Option<String> {
    let (first, last) = (run.first()?, run.last()?);
    if run.len() < 2 || (last.0 - first.0).abs() < f64::EPSILON {
        return None;
    }
    let mut d = polyline_d(run);
    // 收尾沿 0 轴折回起点；末点已经落在 0 轴上时不必多画一条重复的边
    if (last.1 - zero).abs() > f64::EPSILON {
        d.push_str(&format!(" L {} {}", fmt_coord(last.0), fmt_coord(zero)));
    }
    d.push_str(&format!(" L {} {}", fmt_coord(first.0), fmt_coord(zero)));
    Some(d)
}

/// 面积填充按 0 轴拆成多条 path，各自换成对应侧的颜色（`fill-opacity` 等属性原样保留）。
fn split_fill(tag: &str, zero: f64, above: &str, below: &str) -> Option<String> {
    let points = path_points(attr_value(tag, "d")?)?;
    // 最后三个点是 charts-rs 的收尾基线（见 `rewrite_fill_baseline`），只取折线本身
    let line = points.get(..points.len().checked_sub(3)?)?;
    let mut out = String::new();
    for (run, is_above) in split_at_zero(line, zero) {
        let Some(d) = fill_d(&run, zero) else {
            continue;
        };
        let colored = replace_attr(tag, "d", &d)?;
        out.push_str(&replace_attr(
            &colored,
            "fill",
            side_color(is_above, above, below),
        )?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 折线按 0 轴拆成多条 path，各自换成对应侧的颜色（颜色与面积、数据点一致）。
fn split_line(tag: &str, zero: f64, above: &str, below: &str) -> Option<String> {
    let points = path_points(attr_value(tag, "d")?)?;
    let mut out = String::new();
    for (run, is_above) in split_at_zero(&points, zero) {
        let colored = replace_attr(tag, "d", &polyline_d(&run))?;
        out.push_str(&replace_attr(
            &colored,
            "stroke",
            side_color(is_above, above, below),
        )?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 数据点按圆心相对 0 轴的位置着色（`stroke` 与 `fill` 一起换，边框与实心色一致）。
fn sign_dot(tag: &str, zero: f64, above: &str, below: &str) -> Option<String> {
    let cy = attr_value(tag, "cy")?.parse::<f64>().ok()?;
    let color = side_color(cy <= zero, above, below);
    let colored = replace_attr(tag, "stroke", color)?;
    replace_attr(&colored, "fill", color)
}

/// 取"0 轴之上 / 之下"对应的颜色。
fn side_color<'a>(above: bool, up: &'a str, down: &'a str) -> &'a str {
    if above {
        up
    } else {
        down
    }
}

/// 找出填充 path 的标签文本（判据见 [`FILL_OPACITY_ATTR`]）。
fn fill_tag(svg: &str) -> Option<&str> {
    svg.split(SVG_PATH_OPEN).skip(1).find_map(|tail| {
        let end = tail.find("/>")?;
        let tag = &tail[..end + 2];
        tag.contains(FILL_OPACITY_ATTR).then_some(tag)
    })
}

/// 面积 / 折线 / 数据点按 0 轴分色（红涨绿跌：之上红、之下绿）。
///
/// 只在自己算出的范围有效、且 0 轴确实落在绘图区内时改写（同 [`anchor_fill_at_zero`]）；
/// 算不出 0 轴就原样返回 —— 宁可保持单色，也不画一条位置错的分界线。
pub fn paint_by_sign(
    svg: String,
    axis_range: Option<(f64, f64, usize)>,
    above: &str,
    below: &str,
) -> String {
    let Some((min, max, _splits)) = axis_range else {
        return svg;
    };
    let Some(zero) = fill_tag(&svg)
        .and_then(|tag| attr_value(tag, "d"))
        .and_then(path_points)
        .and_then(|points| zero_axis_y(&points, min, max))
    else {
        return svg;
    };
    let painted = rewrite_tags(&svg, SVG_PATH_OPEN, |tag| {
        if tag.contains(FILL_OPACITY_ATTR) {
            split_fill(tag, zero, above, below)
        } else if tag.contains(FILL_NONE_ATTR) {
            split_line(tag, zero, above, below)
        } else {
            None
        }
    });
    rewrite_tags(&painted, "<circle", |tag| sign_dot(tag, zero, above, below))
}

/// 按可用宽度抽稀 X 轴类目标签（首尾必显）：不要的类目传空串，charts-rs 不画空标签。
pub fn thin_labels(categories: &[String], width: f64) -> Vec<String> {
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
pub fn display_value(value: f64, kind: ChartValueKind) -> f32 {
    match kind {
        ChartValueKind::Money => (value / 100.0) as f32,
        ChartValueKind::Percent | ChartValueKind::Ratio | ChartValueKind::Count => value as f32,
    }
}

/// 全部序列的显示值范围（与 charts-rs 自己算范围的口径一致：只看数据点）。
pub fn display_range(series: &[ChartSeries], slots: usize, kind: ChartValueKind) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for item in series {
        for slot in 0..slots {
            let value = f64::from(display_value(
                item.data.get(slot).copied().unwrap_or(0.0),
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

/// Y 轴范围：刻度必须是**大整数**，上下界**各自贴合数据**（不强制以 0 对称）。
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
/// **不做上下对称**：`-35..90` 给的是 `-50..100`（期望值那张图曾经被撑成 `±100`，
/// 底下 50 的空白里一个数据点都没有）。上下界都对齐到步长的整数倍，0 因此**仍然是**
/// 一条刻度（`0 = 0 × step`），涨跌照样从 0 轴读，只是不再各留一半空白。
///
/// `bounds` 是调用方给的天生上下界（见 [`ChartConfig::y_bounds`]）：数据落在里面就直接用，
/// 于是"胜率"这类百分比指标拿到的是 `0..100` 而不是被撑到 150。
///
/// 算不出（数据非有限、或极端量级下 6 段都放不下）时返回 `None`，
/// 交给 charts-rs 自己决定（见调用点的兜底）。
pub fn nice_axis_range(
    data_min: f64,
    data_max: f64,
    bounds: Option<(f64, f64)>,
) -> Option<(f64, f64, usize)> {
    if let Some((low, high)) = bounds {
        if high > low && data_min >= low && data_max <= high {
            // 上界必须**严格**大于数据最大值（charts-rs 只在 `axis_max > 数据最大值` 时才认它），
            // 数据正好贴着上界（胜率首笔就 100%）时抬一个**不会出现在刻度文字里**的微量：
            // 刻度由 `format_float` 按一位小数格式化。
            let high = if data_max >= high {
                high + AXIS_LIMIT_EPSILON
            } else {
                high
            };
            return Some((low, high, Y_SPLITS));
        }
    }
    let scale = data_min.abs().max(data_max.abs()).max(data_max - data_min);
    for step in nice_steps(scale) {
        // 上下界各自对齐到步长的整数倍；全正的数据从 0 起算（与 charts-rs 的默认一致）
        let low = if data_min > 0.0 { 0.0 } else { data_min };
        let low_units = (low / step).floor();
        let mut high_units = (low.max(data_max) / step).ceil();
        // 上界必须**严格**大于数据最大值：charts-rs 只在 `axis_max > 数据最大值` 时才认这个
        // 自定义上界，否则它退回自己那套阶梯（等于白算）。用 f32 比较（它就是 f32），
        // 所以可能要多抬一格。
        while (high_units * step) as f32 <= data_max as f32 {
            high_units += 1.0;
        }
        let (min, max) = (low_units * step, high_units * step);
        let splits = ((max - min) / step).round();
        if splits.is_finite() && splits >= 1.0 && splits <= Y_AXIS_MAX_SPLITS as f64 {
            return Some((min, max, splits as usize));
        }
    }
    None
}

// ==================================================================== 叠层几何

/// 从 DOM 量回来的数据点（每个序列一组 `(颜色, x, y)`，像素相对画布左上角）。
pub type MeasuredPoints = Vec<Vec<(String, f64, f64)>>;

/// 这个 `<g>` 分组算不算"一条可用的序列"（判定抽成纯函数，便于单测）。
///
/// ⚠ **不能要求至少两个圆**：只有一个类目时每个序列只有一个数据点、只有一个 `<circle>`，
/// 按"< 2 就跳过"会把整条序列丢掉 → 几何为空 → tooltip 永远不显示
/// （实测缺陷：「只有一组数据时图表上的 tooltip 不显示」）。非空即可。
pub fn group_is_series(paths: usize, circles: usize) -> bool {
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
    /// 取值语义。`value_axis` 存的是**画图值**，而 [`ChartGeometry::y_of`] 收的是
    /// **原始值**（金额是分），靠它换算。
    pub kind: ChartValueKind,
}

impl ChartGeometry {
    /// 把量回来的点与原始数据拼成完整几何。
    pub fn assemble(
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
            let first_value = display_value(data.data.first().copied().unwrap_or(0.0), kind) as f64;
            for (offset, (_, _, y)) in item.iter().enumerate().skip(1) {
                let value =
                    display_value(data.data.get(offset).copied().unwrap_or(0.0), kind) as f64;
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
                        let value = item.data.get(slot).copied().unwrap_or(0.0);
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
            kind,
        }
    }

    /// 参考值 → y 像素。
    ///
    /// 入参与虚线参考线（`ChartConfig::reference`）同单位：**金额是分**；
    /// `value_axis` 里存的是画图值（金额是元），所以这里先过一遍 [`display_value`]
    /// —— 否则金额图上的非零参考线会画到画面外（实测：`reference(10000)` 算出 -99 万）。
    pub fn y_of(&self, value: i64) -> Option<f64> {
        let (v0, y0, v1, y1) = self.value_axis?;
        if (v1 - v0).abs() < f64::EPSILON {
            return None;
        }
        let value = f64::from(display_value(value as f64, self.kind));
        Some(y0 + (value - v0) / (v1 - v0) * (y1 - y0))
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

/// 命中最近的类目。
pub fn hit_index(geometry: &ChartGeometry, offset_x: f64) -> Option<usize> {
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
/// * [`ChartValueKind::Money`]：分 → 元两位小数（`with_symbol` 为真时带 `¥`）
/// * [`ChartValueKind::Percent`]：两位小数 + `%`
/// * [`ChartValueKind::Ratio`]：两位小数（比值 / 倍数）
/// * [`ChartValueKind::Count`]：整数
pub fn format_value(value: f64, kind: ChartValueKind, with_symbol: bool) -> String {
    match kind {
        ChartValueKind::Money => {
            // 金额按分存、按元显示（两位小数）；`f64` 对分这个量级是精确的
            let yuan = format!("{:.2}", value / 100.0);
            if with_symbol {
                format!("¥{yuan}")
            } else {
                yuan
            }
        }
        ChartValueKind::Percent => format!("{value:.2}%"),
        ChartValueKind::Ratio => format!("{value:.2}"),
        ChartValueKind::Count => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实心点只碰 `<circle>`：折线路径同样是 `stroke="#色" fill="none"`，整串替换会把折线填成面积。
    /// （这条断言从前住在 `tr-ui` 的 `#[cfg(test)]` 里，而 `cargo test -p tr-ui --lib` 一条都不执行。）
    #[test]
    fn solid_dots_only_touches_circles() {
        let svg = concat!(
            r##"<path d="M0 0 L1 1" stroke="#DC2626" fill="none"/>"##,
            r##"<circle cx="1" cy="1" r="2.5" stroke-width="2" stroke="#DC2626" fill="none"/>"##,
            r##"<circle cx="3" cy="3" r="5.5" stroke-width="2" stroke="#16A34A" fill="#16A34A"/>"##,
        )
        .to_string();
        let hexes = ["#DC2626".to_string(), "#16A34A".to_string()];
        let out = solid_dots(svg, &hexes);
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
        // 认不出的色号不动它：序列色不在调色板里时保持原样，不会被填错色
        let untouched = solid_dots(
            r##"<circle cx="9" cy="9" r="2" stroke="#000000" fill="none"/>"##.to_string(),
            &hexes,
        );
        assert!(untouched.contains(r##"stroke="#000000" fill="none"/>"##));
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
        assert!((display_value(123_456.0, ChartValueKind::Money) - 1234.56).abs() < 0.01);
        assert!((display_value(50.0, ChartValueKind::Percent) - 50.0).abs() < f32::EPSILON);
        assert!((display_value(7.0, ChartValueKind::Count) - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tooltip_rows_are_formatted_per_kind() {
        assert_eq!(
            format_value(123_456.0, ChartValueKind::Money, true),
            "¥1234.56"
        );
        assert_eq!(
            format_value(12_500.0, ChartValueKind::Percent, true),
            "12500.00%"
        );
        // 百分数**不除 100**：后端给的就是"已是百分数"的值（`tr-service` 的 `percent_of`
        // 把 0.625 变成 62.5），所以只有 Money 那条分支做分 → 元换算。
        assert_eq!(
            format_value(125.0, ChartValueKind::Percent, true),
            "125.00%"
        );
        assert_eq!(format_value(3.0, ChartValueKind::Count, true), "3");
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
        // （浮层高度取 40：60px 处上方放得下，才走"在指针上方"那条分支；
        //   放不下要翻到下方，见本测试最后一段）
        let high = place_tooltip((400.0, 60.0), 800.0, 200.0, 40.0);
        let low = place_tooltip((400.0, 300.0), 800.0, 200.0, 40.0);
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
            ChartSeries::new("支出", "#DC2626", vec![10_400.0]),
            ChartSeries::new("收入", "#16A34A", vec![8_000.0]),
            ChartSeries::new("转账", "#3964FE", vec![0.0]),
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
        // 金额图：`value_axis` 存的是**元**（0 元 → y=100，100 元 → y=0），
        // 而 `y_of` 收的是**分**（50 元 = 5000 分 → y=50）。
        let geometry = ChartGeometry {
            value_axis: Some((0.0, 100.0, 100.0, 0.0)),
            kind: ChartValueKind::Money,
            ..Default::default()
        };
        assert_eq!(geometry.y_of(5_000), Some(50.0));
        assert_eq!(geometry.y_of(0), Some(100.0));
        assert_eq!(geometry.y_of(10_000), Some(0.0));
        assert_eq!(ChartGeometry::default().y_of(0), None);
    }

    #[test]
    fn geometry_assembles_rows_and_axis() {
        let categories = vec!["a".to_string(), "b".to_string()];
        let series = vec![ChartSeries::new("支出", "var(--x)", vec![100.0, 300.0])];
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
        let (min, max, splits) = nice_axis_range(0.0, 36_000.0, None).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![0.0, 10_000.0, 20_000.0, 30_000.0, 40_000.0]
        );
    }

    #[test]
    fn axis_range_hugs_the_data_when_it_crosses_zero() {
        // 跨零**不**做上下对称：期望值那张图（-35..90）曾经被撑成 ±100，底下 50 的空白里
        // 一个数据点都没有 —— 现在各自贴住数据两侧，0 依然落在刻度上。
        let (min, max, splits) = nice_axis_range(-35.0, 90.0, None).unwrap();
        assert_eq!(ticks(min, max, splits), vec![-50.0, 0.0, 50.0, 100.0]);

        // 数据自己对称时范围仍然对称（不是被强制的）
        let (min, max, splits) = nice_axis_range(-3_000.0, 3_000.0, None).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![-4_000.0, -2_000.0, 0.0, 2_000.0, 4_000.0]
        );

        // 一边高一边矮：各按自己的数据贴边（0 仍是一条刻度）
        let (min, max, splits) = nice_axis_range(-1_000.0, 5_000.0, None).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![-2_000.0, 0.0, 2_000.0, 4_000.0, 6_000.0]
        );

        // 只到 0（不跨零）时从 0 起算即可。
        // 这里顺带钉住"上界那格"的代价：数据最大值 5000 恰好是 1000 的整数倍，而自定义上界
        // 必须**严格**大于它，于是 1000 的步长要 6 段（0…6000）—— 超过 5 段的上限，
        // 这一档只能让位给 2000 的步长（宁可网格粗一档，也不超段数）。
        let (min, max, splits) = nice_axis_range(0.0, 5_000.0, None).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![0.0, 2_000.0, 4_000.0, 6_000.0]
        );
    }

    #[test]
    fn percent_bounds_hold_the_axis_within_one_hundred() {
        // 胜率：数据打到 100% 时上界也停在 100 —— 只抬一个刻度文字里看不见的微量
        // （charts-rs 要求 `axis_max` 严格大于数据最大值；刻度按一位小数格式化）。
        let (min, max, splits) = nice_axis_range(0.0, 100.0, Some((0.0, 100.0))).unwrap();
        assert_eq!(min, 0.0);
        assert!(max > 100.0 && max < 100.1, "上界不该跑出 100：{max}");
        assert_eq!(splits, Y_SPLITS);
        // 刻度按**一位小数**取整后必须是 25 的整数倍 —— charts-rs 就是这么格式化刻度文字的
        // （`format_float`：一位小数、去掉 `.0`），所以那点微量不会出现在轴上。
        let rounded: Vec<f64> = ticks(min, max, splits)
            .into_iter()
            .map(|value| (value * 10.0).round() / 10.0)
            .collect();
        assert_eq!(rounded, vec![0.0, 25.0, 50.0, 75.0, 100.0]);

        // 有负数（亏损笔）时给 -100..100
        let (min, max, splits) = nice_axis_range(-30.0, 40.0, Some((-100.0, 100.0))).unwrap();
        assert_eq!(
            ticks(min, max, splits),
            vec![-100.0, -50.0, 0.0, 50.0, 100.0]
        );

        // 数据超出给定上下界时不信边界（防御）：退回按数据挑
        let (_, max, _) = nice_axis_range(-30.0, 140.0, Some((0.0, 100.0))).unwrap();
        assert!(max > 140.0, "数据超界时必须按数据挑：{max}");
    }

    #[test]
    fn ratio_and_percent_keep_two_decimals() {
        // 用户报的那张图：盈亏比 0.81 被取整成 1（曲线一直是平的）
        assert_eq!(format_value(0.8129, ChartValueKind::Ratio, true), "0.81");
        assert_eq!(format_value(1.0, ChartValueKind::Ratio, true), "1.00");
        assert_eq!(
            format_value(83.333, ChartValueKind::Percent, true),
            "83.33%"
        );
        assert_eq!(
            format_value(12_345.0, ChartValueKind::Money, true),
            "¥123.45"
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
            let (min, max, splits) = nice_axis_range(data_min, data_max, None)
                .unwrap_or_else(|| panic!("算不出范围: {data_min}..{data_max}"));
            assert!(min <= data_min, "{min} 没盖住下界 {data_min}");
            assert!(
                (max as f32) > (data_max as f32),
                "{max} 没有严格大于上界 {data_max}"
            );
            assert!((1..=Y_AXIS_MAX_SPLITS).contains(&splits));
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

    // ---------------------------------------------------------------- 0 轴分色
    //
    // 用户报的那张「累计盈亏」：面积整块一个颜色，0 轴上下的部分看不出盈亏。
    // 探针 SVG 的三点跨零（-20000 / -5000 / 60000，0 轴在 y=147），正好覆盖这一档。

    #[test]
    fn fill_and_line_split_at_the_zero_axis() {
        let svg = paint_by_sign(
            PROBE_SVG.to_string(),
            Some((-100_000.0, 100_000.0, 4)),
            "#DC2626",
            "#16A34A",
        );
        // 面积：0 轴之下（y > 147）那段绿、之上（y < 147）那段红，各自闭到 0 轴
        let below_fill = concat!(
            r##"<path d="M 196.3 174.8 L 461 153.9 L 481.2 147 L 196.3 147""##,
            r##" fill="#16A34A" fill-opacity="0.4"/>"##
        );
        let above_fill = concat!(
            r##"<path d="M 481.2 147 L 725.7 63.6 L 725.7 147 L 481.2 147""##,
            r##" fill="#DC2626" fill-opacity="0.4"/>"##
        );
        assert!(svg.contains(below_fill), "{svg}");
        assert!(svg.contains(above_fill), "{svg}");
        // 折线同色：交点（481.2, 147）是两段共用的端点
        let below_line = concat!(
            r##"<path d="M 196.3 174.8 L 461 153.9 L 481.2 147""##,
            r##" stroke-width="2" fill="none" stroke="#16A34A"/>"##
        );
        let above_line = concat!(
            r##"<path d="M 481.2 147 L 725.7 63.6""##,
            r##" stroke-width="2" fill="none" stroke="#DC2626"/>"##
        );
        assert!(svg.contains(below_line), "{svg}");
        assert!(svg.contains(above_line), "{svg}");
        // 数据点按圆心落在哪一侧着色（`stroke` 与 `fill` 一起换，边框与实心色一致）
        let dot = concat!(
            r##"<circle cx="196.3" cy="174.8" r="2.5" stroke-width="2""##,
            r##" stroke="#16A34A" fill="#16A34A"/>"##
        );
        assert!(svg.contains(dot), "{svg}");
        // 网格线一个字节都不动
        assert!(svg.contains(r#"<line stroke-width="1" x1="64" y1="147" x2="858" y2="147"/>"#));
    }

    #[test]
    fn split_stays_untouched_without_a_trustworthy_zero_axis() {
        // 没有自己算出的范围 ⇒ 不知道 0 在哪儿，整条保持单色
        assert_eq!(
            paint_by_sign(PROBE_SVG.to_string(), None, "#DC2626", "#16A34A"),
            PROBE_SVG.to_string()
        );
        assert_eq!(
            paint_by_sign(
                PROBE_SVG.to_string(),
                Some((1.0, 1.0, 4)),
                "#DC2626",
                "#16A34A"
            ),
            PROBE_SVG.to_string()
        );
    }
}
