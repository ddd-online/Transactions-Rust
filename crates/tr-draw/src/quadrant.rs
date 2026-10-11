//! 四象限（紧急 × 重要）的纯算法：落在哪个象限、点在画布上的位置、弹窗里按什么顺序列、
//! 同坐标的点怎么错开。
//!
//! 界面上是待办页的一个子功能，但这里装的几件事都是**规则**：
//!
//! * [`Quadrant::of`]：两轴是六档（`-5 / -3 / -1 / 1 / 3 / 5`，**没有 0**），所以"正负"就是判据；
//! * [`point_of`]：坐标就是这两个档位，但**绘图区四周留白**（[`PLOT_INSET`]）——
//!   六档线性铺满画布的话，最外那两档正好压在画布边线上，半径 9px 的点有一半在画布外，
//!   再叠上抖动就可能整颗都不见了（用户报的正是"紧急、重要都最高的事项在象限图上找不到"）；
//! * [`rows`]：该象限的进行中事项，先按紧急度降序、再按重要度降序；
//!   用**稳定**排序，同紧急同重要的两条保持卡片序 → 同一份数据每次打开弹窗都排成一个样；
//! * [`jitter_of`]：同一坐标上的两颗点按 id 各带一个固定偏移（`DefaultHasher`，固定种子，
//!   不是随机数）—— 重渲染不会让点自己抖。
//!
//! 象限的**名字与说明文案**留在界面侧（只有那一页用得到）。

/// 落在哪个象限：纵轴 = 重要度、横轴 = 紧急度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quadrant {
    /// 重要且紧急
    Now,
    /// 重要但不紧急
    Plan,
    /// 不重要且不紧急
    Less,
    /// 紧急但不重要
    Delegate,
}

impl Quadrant {
    /// 两轴是六档、没有 0 ⇒ "正负"就是判据，不必处理"正好压在轴上"。
    pub fn of(urgency: i32, importance: i32) -> Self {
        match (importance > 0, urgency > 0) {
            (true, true) => Self::Now,
            (true, false) => Self::Plan,
            (false, false) => Self::Less,
            (false, true) => Self::Delegate,
        }
    }

    /// 四个象限（固定顺序：从"马上做"顺时针到"授权做"）。
    pub const ALL: [Quadrant; 4] = [
        Quadrant::Now,
        Quadrant::Plan,
        Quadrant::Less,
        Quadrant::Delegate,
    ];
}

/// 一个能落进象限的事项：只要这两个档位。
pub trait QuadrantItem {
    fn urgency(&self) -> i32;
    fn importance(&self) -> i32;
}

impl<T: QuadrantItem> QuadrantItem for &T {
    fn urgency(&self) -> i32 {
        (*self).urgency()
    }
    fn importance(&self) -> i32 {
        (*self).importance()
    }
}

/// 某个事项落在哪个象限。
pub fn quadrant_of(item: &impl QuadrantItem) -> Quadrant {
    Quadrant::of(item.urgency(), item.importance())
}

/// 弹窗里列哪些、按什么顺序：该象限的事项，先按紧急度降序、再按重要度降序。
///
/// `sort_by` 是稳定排序，所以同档位的两条保持传入顺序（卡片序 → 事项序）。
pub fn rows<T: QuadrantItem + Clone>(items: &[T], quad: Quadrant) -> Vec<T> {
    let mut rows: Vec<T> = items
        .iter()
        .filter(|item| quadrant_of(*item) == quad)
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        b.urgency()
            .cmp(&a.urgency())
            .then(b.importance().cmp(&a.importance()))
    });
    rows
}

/// 按 id 给每个点一个固定的小偏移，远近只够分开两颗点、不改变它落在哪个象限。
///
/// 用 `DefaultHasher`（标准库、固定种子）而不是随机数：同一份数据每次渲染都落在同一处。
pub fn jitter_of(id: &str) -> (f64, f64) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let bits = hasher.finish();
    // ±10px（点半径 9px）
    let dx = (bits % 21) as f64 - 10.0;
    let dy = ((bits >> 21) % 21) as f64 - 10.0;
    (dx, dy)
}

/// 数据点的半径（px）：界面上的 `<circle r=…>` 与这里用的是**同一个数**
/// （改了半径就得跟着改留白，否则点又会被画布裁掉）。
pub const POINT_RADIUS: f64 = 9.0;

/// 抖动偏移的绝对值上限（px）：[`jitter_of`] 的输出恒落在 `±JITTER_MAX` 之内。
pub const JITTER_MAX: f64 = 10.0;

/// 最外档位的点与画布边线之间还要留出的空隙（px）：与轴端标注那 8px 同一档。
pub const PLOT_MARGIN: f64 = 8.0;

/// 绘图区四周的留白（px）= 点半径 + 白色描边的一半（`stroke-width: 2`）+ 抖动上限 + 空隙。
///
/// 六个档位是**线性铺进绘图区**的，所以最外那两档离画布边线正好一个留白 —— 连"最大抖动"
/// 和"描边"都还在画布内。留白只改变"画布边线 ↔ 最外档位"这段距离（约 28px）：
/// 中间的档位、十字轴、象限底与水印都不动。
pub const PLOT_INSET: f64 = POINT_RADIUS + 1.0 + JITTER_MAX + PLOT_MARGIN;

/// 两轴档位的跨度（最外两档之差；六档 `-5..=5` ⇒ 10）。
///
/// 与 `tr_domain::consts::TODO_LEVELS` 是否一致由单测盯着（档位表加了更外的档而这里没跟上，
/// 那颗点就会跑到画布外）。
pub const LEVEL_SPAN: f64 = 10.0;

/// 十字轴（两轴的 0 档）在画布上的位置：绘图区的留白是**对称**的，所以轴恒在画布中心 ——
/// 图上的十字与"正负就是象限"（[`Quadrant::of`]）永远对齐。
pub fn axis_of(w: f64, h: f64) -> (f64, f64) {
    (w / 2.0, h / 2.0)
}

/// 一个事项在画布（`w × h`，px）上的**中心点**：档位线性铺进留白后的绘图区，
/// 0 档压在十字轴上（[`axis_of`]），最后叠上这个 id 自己的固定抖动（[`jitter_of`]）。
///
/// **不变量**（`points_never_leave_the_canvas` 单测盯着）：任何档位、任何 id，
/// 点连同半径与描边都完整落在画布内；且点落在十字轴的哪一侧，与档位的正负一致。
pub fn point_of(urgency: i32, importance: i32, id: &str, w: f64, h: f64) -> (f64, f64) {
    let (dx, dy) = jitter_of(id);
    let (axis_x, axis_y) = axis_of(w, h);
    // 绘图区比画布小一圈（两侧各一个留白）。画布小到装不下留白时收成 0（界面上到不了：
    // 卡片有 220px 的最小高度、宽度是整个版心），那时点会叠在中心。
    let plot_w = (w - 2.0 * PLOT_INSET).max(0.0);
    let plot_h = (h - 2.0 * PLOT_INSET).max(0.0);
    (
        axis_x + f64::from(urgency) / LEVEL_SPAN * plot_w + dx,
        // 纵轴向上为正：重要度越高，y 越小（越靠近画布顶边）
        axis_y - f64::from(importance) / LEVEL_SPAN * plot_h + dy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    // 只在测试里用：坐标轴的跨度要跟 `TODO_LEVELS` 对得上（见 `level_span_matches_the_level_table`）
    use tr_domain::consts;

    #[derive(Clone, Debug, PartialEq)]
    struct Item {
        id: &'static str,
        urgency: i32,
        importance: i32,
    }

    impl QuadrantItem for Item {
        fn urgency(&self) -> i32 {
            self.urgency
        }
        fn importance(&self) -> i32 {
            self.importance
        }
    }

    fn item(id: &'static str, urgency: i32, importance: i32) -> Item {
        Item {
            id,
            urgency,
            importance,
        }
    }

    /// 六档两两组合都要落到对的那一格（没有任何一档落在"轴上"）。
    #[test]
    fn every_level_pair_lands_in_exactly_one_quadrant() {
        let levels = [-5, -3, -1, 1, 3, 5];
        for urgency in levels {
            for importance in levels {
                let expected = match (importance > 0, urgency > 0) {
                    (true, true) => Quadrant::Now,
                    (true, false) => Quadrant::Plan,
                    (false, false) => Quadrant::Less,
                    (false, true) => Quadrant::Delegate,
                };
                assert_eq!(
                    Quadrant::of(urgency, importance),
                    expected,
                    "紧急 {urgency} / 重要 {importance}"
                );
            }
        }
        // 四格都被覆盖到，且 ALL 不重复
        let mut seen: Vec<Quadrant> = Quadrant::ALL.to_vec();
        seen.sort_by_key(|quad| format!("{quad:?}"));
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn rows_filter_by_quadrant_and_sort_by_urgency_then_importance() {
        let items = vec![
            item("a", 1, 1),
            item("b", 5, 1),
            item("c", 5, 5),
            item("d", 1, 3),
            item("e", -5, 5),  // Plan
            item("f", -1, -1), // Less
        ];
        let now: Vec<&str> = rows(&items, Quadrant::Now)
            .iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(
            now,
            vec!["c", "b", "d", "a"],
            "紧急降序 → 重要降序（d 的重要度 3 高于 a 的 1）"
        );

        let plan: Vec<&str> = rows(&items, Quadrant::Plan)
            .iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(plan, vec!["e"]);
        assert!(rows(&items, Quadrant::Delegate).is_empty());
    }

    /// 同档位保持传入顺序（稳定排序）—— 弹窗每次打开都是同一个样。
    #[test]
    fn equal_levels_keep_their_input_order() {
        let items = vec![item("x", 3, 3), item("y", 3, 3), item("z", 3, 3)];
        let ids: Vec<&str> = rows(&items, Quadrant::Now)
            .iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(ids, vec!["x", "y", "z"]);
    }

    #[test]
    fn jitter_is_stable_bounded_and_spreads_ids() {
        let (dx, dy) = jitter_of("todo-1");
        assert_eq!(jitter_of("todo-1"), (dx, dy), "同一个 id 永远同一处");
        assert!(
            (-JITTER_MAX..=JITTER_MAX).contains(&dx) && (-JITTER_MAX..=JITTER_MAX).contains(&dy),
            "抖动必须落在 ±{JITTER_MAX} 之内（留白是按这个上限算的）"
        );
        // 一小批真实规模的 id：不能全挤在同一点（否则点会叠成一坨）
        let points: Vec<(f64, f64)> = (0..24).map(|i| jitter_of(&format!("item-{i}"))).collect();
        let mut unique = points.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup();
        assert!(unique.len() > 12, "24 个 id 至少该散开一半：{unique:?}");
    }

    /// 一批 id（事项的 id 是 uuid，哈希出来的抖动落在整个区间里）。
    fn ids() -> Vec<String> {
        (0..24).map(|i| format!("item-{i}")).collect()
    }

    /// 档位表与坐标轴的跨度必须一致：`TODO_LEVELS` 加了更外的档（比如 7）而这里没跟上，
    /// 那颗点就会跑到画布外 —— 这条比"点越界"那条更早红，指得出原因。
    #[test]
    fn level_span_matches_the_level_table() {
        let widest = consts::TODO_LEVELS
            .iter()
            .map(|level| level.abs())
            .max()
            .unwrap_or(0);
        assert_eq!(
            f64::from(widest) * 2.0,
            LEVEL_SPAN,
            "档位表 {:?} 与 LEVEL_SPAN 对不上",
            consts::TODO_LEVELS
        );
    }

    /// **用户报的那个缺陷的护栏**：任何档位、任何 id，点连半径与白色描边都完整落在画布内。
    ///
    /// 修之前：两轴铺满画布，最外档位（±5）正好压在画布边线上 ⇒ 半径 9px 的点有一半在
    /// 画布外，抖动再把它推出去，"紧急高 + 重要高"那颗经常整颗都看不到。
    #[test]
    fn points_never_leave_the_canvas() {
        let reach = POINT_RADIUS + 1.0;
        // 界面上可能出现的画布尺寸：宽是整个版心（几百到两千多），高最小 220（CSS 的 min-height）
        for (w, h) in [(1306.0, 923.0), (640.0, 220.0), (2400.0, 1400.0)] {
            for urgency in consts::TODO_LEVELS {
                for importance in consts::TODO_LEVELS {
                    for id in ids() {
                        let (x, y) = point_of(urgency, importance, &id, w, h);
                        assert!(
                            x - reach >= 0.0 && x + reach <= w,
                            "紧急 {urgency} / 重要 {importance} / id {id} 的点横向越界：\
                             x={x:.1}（画布宽 {w}）"
                        );
                        assert!(
                            y - reach >= 0.0 && y + reach <= h,
                            "紧急 {urgency} / 重要 {importance} / id {id} 的点纵向越界：\
                             y={y:.1}（画布高 {h}）"
                        );
                    }
                }
            }
        }
    }

    /// 点落在十字轴的哪一侧必须与档位的正负一致：否则"图上看到的"与象限名的计数
    /// （[`Quadrant::of`] 拿正负判）会自相矛盾。
    #[test]
    fn a_point_is_on_the_same_side_of_the_axis_as_its_level() {
        let (w, h) = (1306.0, 923.0);
        let (axis_x, axis_y) = axis_of(w, h);
        for urgency in consts::TODO_LEVELS {
            for importance in consts::TODO_LEVELS {
                for id in ids() {
                    let (x, y) = point_of(urgency, importance, &id, w, h);
                    assert_eq!(x > axis_x, urgency > 0, "紧急 {urgency} 落在轴的错误一侧");
                    assert_eq!(
                        y < axis_y,
                        importance > 0,
                        "重要 {importance} 落在轴的错误一侧"
                    );
                }
            }
        }
    }

    /// 留白只吃"画布边线 ↔ 最外档位"这一段：0 档仍然压在画布中心（十字轴上），
    /// 最外档位离边线正好一个留白（把抖动减掉就是"理想落点"）；纵轴向上为正。
    #[test]
    fn the_inset_only_shifts_the_outermost_level() {
        let (w, h) = (1306.0, 923.0);
        assert_eq!(axis_of(w, h), (w / 2.0, h / 2.0), "十字轴恒在画布中心");
        let (dx, dy) = jitter_of("level-probe");
        for (level, expected_x) in [(-5, PLOT_INSET), (0, w / 2.0), (5, w - PLOT_INSET)] {
            let (x, _) = point_of(level, -1, "level-probe", w, h);
            assert!(
                (x - dx - expected_x).abs() < 1e-9,
                "{level} 档的理想横坐标 {} ≠ {expected_x}",
                x - dx
            );
        }
        for (level, expected_y) in [(-5, h - PLOT_INSET), (0, h / 2.0), (5, PLOT_INSET)] {
            let (_, y) = point_of(-1, level, "level-probe", w, h);
            assert!(
                (y - dy - expected_y).abs() < 1e-9,
                "{level} 档的理想纵坐标 {} ≠ {expected_y}（向上为正）",
                y - dy
            );
        }
    }
}
