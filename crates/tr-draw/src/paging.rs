//! 分页页码的纯算法：哪些页要显示、哪里折成省略号。
//!
//! 规则（`components/ui/pagination.rs` 的界面壳只负责把它们画出来）：
//! 首页、末页、当前页 ±1 必显，其余折叠成 `…`；
//! 靠近两端时多显示几个，避免出现 `1 … 2` 这类空洞。
//!
//! 唯一的使用面是分页控件，但它是**规则**而不是渲染配置：页码收敛算错在界面上
//! 只表现为"少了一个能点的页码"，把它放这儿是为了能在 `cargo test` 里断言。

/// 页码槽位：`None` 表示省略号。
pub fn page_slots(current: i32, pages: i32) -> Vec<Option<i32>> {
    let pages = pages.max(1);
    let current = current.clamp(1, pages);
    let mut slots: Vec<Option<i32>> = Vec::new();
    for candidate in 1..=pages {
        let visible = candidate == 1
            || candidate == pages
            || (candidate - current).abs() <= 1
            || (current <= 3 && candidate <= 5)
            || (current >= pages - 2 && candidate >= pages - 4);
        if visible {
            slots.push(Some(candidate));
        } else if !slots.last().map(|slot| slot.is_none()).unwrap_or(false) {
            slots.push(None);
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把槽位还原成"页号列表"，好读断言。
    fn shape(slots: &[Option<i32>]) -> String {
        slots
            .iter()
            .map(|slot| match slot {
                Some(page) => page.to_string(),
                None => "…".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_window_moves_with_the_current_page() {
        assert_eq!(shape(&page_slots(1, 10)), "1 2 3 4 5 … 10");
        assert_eq!(shape(&page_slots(5, 10)), "1 … 4 5 6 … 10");
        assert_eq!(shape(&page_slots(10, 10)), "1 … 6 7 8 9 10");
        assert_eq!(shape(&page_slots(3, 7)), "1 2 3 4 5 … 7");
    }

    #[test]
    fn short_lists_show_every_page_without_ellipsis() {
        assert_eq!(shape(&page_slots(1, 1)), "1");
        assert_eq!(shape(&page_slots(1, 3)), "1 2 3");
        assert_eq!(shape(&page_slots(2, 5)), "1 2 3 4 5");
        // 越界的入参按边界收敛，不 panic
        assert_eq!(shape(&page_slots(0, 4)), "1 2 3 4");
        assert_eq!(shape(&page_slots(99, 4)), "1 2 3 4");
        assert_eq!(shape(&page_slots(1, 0)), "1");
        assert_eq!(shape(&page_slots(1, -5)), "1");
    }

    /// 不变量：首末页永远在、页号严格递增、省略号不连排、也不与首末页相邻成空洞。
    #[test]
    fn slots_stay_well_formed_for_every_page_count() {
        for pages in 1..=40 {
            for current in 1..=pages {
                let slots = page_slots(current, pages);
                let numbers: Vec<i32> = slots.iter().filter_map(|slot| *slot).collect();
                assert_eq!(numbers.first(), Some(&1), "首页必显（current={current}）");
                assert_eq!(
                    numbers.last(),
                    Some(&pages),
                    "末页必显（current={current}）"
                );
                assert!(
                    numbers.windows(2).all(|pair| pair[0] < pair[1]),
                    "页号必须严格递增：{}",
                    shape(&slots)
                );
                assert!(
                    !slots
                        .windows(2)
                        .any(|pair| pair[0].is_none() && pair[1].is_none()),
                    "省略号不连排：{}",
                    shape(&slots)
                );
                assert!(numbers.contains(&current), "当前页必显：{}", shape(&slots));
                // "1 … 2" 这类空洞：省略号两侧的页号至少差 2
                for pair in slots.windows(3) {
                    if pair[1].is_none() {
                        let (Some(left), Some(right)) = (pair[0], pair[2]) else {
                            panic!("省略号两侧都该是页号：{}", shape(&slots));
                        };
                        assert!(
                            right - left >= 2,
                            "空省略号（两侧页号相邻）：{}",
                            shape(&slots)
                        );
                    }
                }
            }
        }
    }
}
