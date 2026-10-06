//! 日记目录树：把日期列表按 年 → 月 → 日 分组，并给出三种树节点的 DOM id。
//!
//! 从 `pages/diary.rs` 搬来（ADR-0001 的第二波）。两条规则值得能被断言：
//!
//! * **分组与排序**：年 / 月是 `BTreeMap`（降序 = 新的在前），同一天内的多个日期按
//!   `date` 字符串降序（`YYYY-MM-DD` 的字典序就是时间序）；解析不出的日期直接跳过；
//! * **节点 id**：渲染侧与"滚动定位"侧必须用**同一份**规则（滚动靠 id 反查元素），
//!   从前这三条规则分居两个文件，改一处忘一处就表现为"定位不到那天"。

use std::collections::BTreeMap;

use tr_domain::models::DiaryDateItem;

use crate::calendar::parse_ymd;

/// 一个日期节点。
#[derive(Debug, Clone, PartialEq)]
pub struct DayNode {
    pub date: String,
    pub day: u32,
    pub word_count: i64,
    pub mood: String,
}

/// 年 → 月 → 日的分组结果。
pub type TreeMap = BTreeMap<i32, BTreeMap<u32, Vec<DayNode>>>;

/// 把日期列表按年/月分组（年降序、月降序、日降序）。
pub fn build_tree(items: &[DiaryDateItem]) -> TreeMap {
    let mut map: TreeMap = BTreeMap::new();
    for item in items {
        let Some(date) = parse_ymd(&item.date) else {
            continue;
        };
        map.entry(date.year)
            .or_default()
            .entry(date.month)
            .or_default()
            .push(DayNode {
                date: item.date.clone(),
                day: date.day,
                word_count: item.word_count,
                mood: item.mood.clone(),
            });
    }
    for months in map.values_mut() {
        for days in months.values_mut() {
            days.sort_by(|left, right| right.date.cmp(&left.date));
        }
    }
    map
}

/// 年节点的 DOM id。
pub fn year_node_id(year: i32) -> String {
    format!("diary-tree-year-{year}")
}

/// 月节点的 DOM id。
pub fn month_node_id(year: i32, month: u32) -> String {
    format!("diary-tree-month-{year}-{month}")
}

/// 日节点的 DOM id。
pub fn day_node_id(date: &str) -> String {
    format!("diary-tree-day-{date}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(date: &str, word_count: i64, mood: &str) -> DiaryDateItem {
        DiaryDateItem {
            date: date.to_string(),
            word_count,
            mood: mood.to_string(),
        }
    }

    #[test]
    fn days_group_under_their_year_and_month() {
        let tree = build_tree(&[
            item("2026-06-19", 10, "开心"),
            item("2026-06-01", 3, ""),
            item("2026-05-31", 7, "平静"),
            item("2025-12-31", 1, ""),
        ]);
        let years: Vec<i32> = tree.keys().copied().collect();
        assert_eq!(years, vec![2025, 2026], "年降序（新的在前）");
        let june: Vec<&str> = tree[&2026][&6]
            .iter()
            .map(|node| node.date.as_str())
            .collect();
        assert_eq!(june, vec!["2026-06-19", "2026-06-01"], "同月内日降序");
        assert_eq!(tree[&2026][&6][0].day, 19);
        assert_eq!(tree[&2026][&6][0].word_count, 10);
        assert_eq!(tree[&2026][&6][0].mood, "开心");
        let months: Vec<u32> = tree[&2026].keys().copied().collect();
        assert_eq!(months, vec![5, 6], "月降序");
    }

    /// 解析不出的日期直接跳过（宁可少一个节点，也不能 panic 或塞进错的那一年）。
    #[test]
    fn unparsable_dates_are_skipped() {
        let tree = build_tree(&[
            item("2026-06-19", 10, ""),
            item("坏日期", 5, ""),
            item("", 1, ""),
            item("2026-13-01", 2, ""),
        ]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[&2026][&6].len(), 1);
    }

    #[test]
    fn an_empty_list_yields_an_empty_tree() {
        assert!(build_tree(&[]).is_empty());
    }

    /// id 规则是渲染与滚动共用的那一份（改这里就是改两处）。
    #[test]
    fn node_ids_are_stable() {
        assert_eq!(year_node_id(2026), "diary-tree-year-2026");
        assert_eq!(month_node_id(2026, 6), "diary-tree-month-2026-6");
        assert_eq!(day_node_id("2026-06-19"), "diary-tree-day-2026-06-19");
    }
}
