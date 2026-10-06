//! 列表重排（拖拽排序）的纯算法：新顺序是什么、**哪些行需要落库**。
//!
//! 从分类 / 标签页搬来（ADR-0001 的第二波）。它决定的是"发几条写请求"：
//! 先把被拖动的项从 `from` 取出、插到 `to`，随后**全量重排** `sortOrder`，
//! 但只把 `sortOrder` 与新下标不一致的项放进结果 —— 调用方据此发请求。
//!
//! 两个取值函数由调用方给（`key` 用来告诉调用方"这一项是谁"，`sort_order` 读它当前的序号）：
//! 本模块不认识任何具体 DTO，也就不必为了排序去依赖页面类型。

/// 需要落库的一项：新下标 + 该项的键。
pub type SortChange = (usize, String);

/// 重排列表并算出"需要落库的项"。
///
/// 返回 `(新顺序, 需要落库的项)`；下标越界或原位返回 `None`（调用方据此什么都不做）。
pub fn reorder_with_changes<T, K, S>(
    list: &[T],
    from: usize,
    to: usize,
    key: K,
    sort_order: S,
) -> Option<(Vec<T>, Vec<SortChange>)>
where
    T: Clone,
    K: Fn(&T) -> String,
    S: Fn(&T) -> i32,
{
    if from == to || from >= list.len() || to >= list.len() {
        return None;
    }

    let mut reordered = list.to_vec();
    let moved = reordered.remove(from);
    reordered.insert(to, moved);

    let mut changed: Vec<SortChange> = Vec::new();
    for (index, item) in reordered.iter().enumerate() {
        if sort_order(item) != index as i32 {
            changed.push((index, key(item)));
        }
    }
    Some((reordered, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Row {
        name: &'static str,
        sort_order: i32,
    }

    fn rows(names: &[&'static str]) -> Vec<Row> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| Row {
                name,
                sort_order: index as i32,
            })
            .collect()
    }

    fn key(row: &Row) -> String {
        row.name.to_string()
    }

    fn order(row: &Row) -> i32 {
        row.sort_order
    }

    fn names(list: &[Row]) -> Vec<&'static str> {
        list.iter().map(|row| row.name).collect()
    }

    /// 把第一项拖到最后：新顺序对了，且**只有序号变了的那几项**要落库。
    #[test]
    fn dragging_reports_only_the_rows_whose_order_changed() {
        let list = rows(&["a", "b", "c", "d"]);
        let (reordered, changed) = reorder_with_changes(&list, 0, 3, key, order).unwrap();
        assert_eq!(names(&reordered), vec!["b", "c", "d", "a"]);
        // 新下标：b=0 c=1 d=2 a=3；旧序号里只有 a(0) 与 d(2) 对不上新下标之外……
        // b/c/d 的旧序号分别是 1/2/3 → 与 0/1/2 都不等，所以要落库的是四项
        assert_eq!(
            changed,
            vec![
                (0, "b".to_string()),
                (1, "c".to_string()),
                (2, "d".to_string()),
                (3, "a".to_string())
            ]
        );
    }

    /// 相邻交换：只有这两项的序号变了。
    #[test]
    fn swapping_neighbours_touches_only_those_two() {
        let list = rows(&["a", "b", "c", "d"]);
        let (reordered, changed) = reorder_with_changes(&list, 1, 2, key, order).unwrap();
        assert_eq!(names(&reordered), vec!["a", "c", "b", "d"]);
        assert_eq!(
            changed,
            vec![(1, "c".to_string()), (2, "b".to_string())],
            "a(0) 与 d(3) 的序号没变，不该发请求"
        );
    }

    /// 已经排好的列表：怎么拖都只动被拖的那几项；序号与下标原本就对得上时，
    /// 只有被挪动区间内那几项需要落库。
    #[test]
    fn a_list_that_was_already_renumbered_yields_no_changes() {
        // 先把顺序改成 b a c d 并"落库"（sort_order 跟着新下标）
        let list = vec![
            Row {
                name: "b",
                sort_order: 0,
            },
            Row {
                name: "a",
                sort_order: 1,
            },
            Row {
                name: "c",
                sort_order: 2,
            },
        ];
        assert!(
            reorder_with_changes(&list, 0, 0, key, order).is_none(),
            "原位拖拽什么都不做"
        );
        let (reordered, changed) = reorder_with_changes(&list, 0, 1, key, order).unwrap();
        assert_eq!(names(&reordered), vec!["a", "b", "c"]);
        assert_eq!(
            changed,
            vec![(0, "a".to_string()), (1, "b".to_string())],
            "c 的序号仍是 2，不必落库"
        );
    }

    /// 脏数据（序号与下标大面积不一致）会把不一致的全报上来 —— 这正是"顺手修好"的机会。
    #[test]
    fn a_dirty_list_reports_every_mismatch() {
        let list = vec![
            Row {
                name: "a",
                sort_order: 5,
            },
            Row {
                name: "b",
                sort_order: 5,
            },
            Row {
                name: "c",
                sort_order: 5,
            },
        ];
        let (_, changed) = reorder_with_changes(&list, 0, 1, key, order).unwrap();
        assert_eq!(changed.len(), 3, "三项的序号都对不上新下标");
    }

    /// 越界 / 原位：`None`（调用方据此不发请求，也不改本地列表）。
    #[test]
    fn out_of_range_drags_do_nothing() {
        let list = rows(&["a", "b", "c"]);
        assert!(reorder_with_changes(&list, 0, 0, key, order).is_none());
        assert!(reorder_with_changes(&list, 3, 1, key, order).is_none());
        assert!(reorder_with_changes(&list, 1, 3, key, order).is_none());
        assert!(reorder_with_changes(&[], 0, 0, key, order).is_none());
        // 单项列表：任何拖动都是原位
        assert!(reorder_with_changes(&rows(&["only"]), 0, 0, key, order).is_none());
    }
}
