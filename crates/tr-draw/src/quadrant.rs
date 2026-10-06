//! 四象限（紧急 × 重要）的纯算法：落在哪个象限、弹窗里按什么顺序列、同坐标的点怎么错开。
//!
//! 界面上是待办页的一个子功能，但这里装的三件事都是**规则**：
//!
//! * [`Quadrant::of`]：两轴是六档（`-5 / -3 / -1 / 1 / 3 / 5`，**没有 0**），所以"正负"就是判据；
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((-10.0..=10.0).contains(&dx) && (-10.0..=10.0).contains(&dy));
        // 一小批真实规模的 id：不能全挤在同一点（否则点会叠成一坨）
        let points: Vec<(f64, f64)> = (0..24).map(|i| jitter_of(&format!("item-{i}"))).collect();
        let mut unique = points.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup();
        assert!(unique.len() > 12, "24 个 id 至少该散开一半：{unique:?}");
    }
}
