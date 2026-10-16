//! 记录页（已清仓轮次）的三处**落点 DOM id**：左栏股票卡片、某一轮的卡片、那一轮的轮头。
//!
//! 从 `pages/stock.rs` 抽出来（ADR-0001 的口径：能被 native 断言的东西放这里）。
//! 抽出来的理由与 [`crate::diary_tree`] 的三条树节点 id 完全一样：
//! **渲染侧与"滚动定位"侧必须用同一份 id 规则** —— 定位靠 `get_element_by_id` 反查，
//! 两边各写一份字符串，改一处忘一处就表现为"跳过去没反应"，而那种缺陷在界面上只是一次
//! 什么都没发生（UIA 也看不见"没滚过去"）。
//!
//! 谁在用：统计页「逐笔结算明细」的**结算点**格子是一颗按钮，点了就切到记录子功能、
//! 选中那只股票、把那一轮滚进视野并把焦点放到轮头（见 `pages/stock.rs` 的 `RoundLocate`）。
//!
//! 三处 id 各自解决一件事：
//! * [`history_node_id`] —— 左栏那张卡片要滚进视野（股票多时它可能在列表外）；
//! * [`round_node_id`] —— 那一轮的卡片滚到**居中**（上下留出邻轮的上下文）；
//! * [`round_head_node_id`] —— 焦点落在轮头（那颗 `<button aria-expanded>`）上，
//!   键盘与读屏用户才算真的"到了"。
//!
//! `code` 只做最小清洗（空白 → `-`）：`id` 里出现空白是非法 HTML，而股票代码本身
//! 由 `tr_domain::stock` 校验；清洗只为了让任何历史脏数据也不会产出非法 id。

/// 清洗进 id 的片段：空白换成 `-`（其余原样，代码是 `[0-9A-Za-z]`）。
fn slug(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_whitespace() { '-' } else { ch })
        .collect()
}

/// 左栏「某只已清仓股票」卡片的 DOM id。
pub fn history_node_id(code: &str) -> String {
    format!("stock-history-{}", slug(code))
}

/// 「某只股票的第 N 轮」卡片的 DOM id。
pub fn round_node_id(code: &str, round_no: i64) -> String {
    format!("stock-round-{}-{round_no}", slug(code))
}

/// 「第 N 轮」卡片里**轮头那颗按钮**的 DOM id（跳转的焦点落点）。
pub fn round_head_node_id(code: &str, round_no: i64) -> String {
    format!("stock-round-head-{}-{round_no}", slug(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一只股票的不同轮、不同股票的同号轮，三种 id 都互不相同 —— 定位靠唯一性。
    #[test]
    fn the_three_ids_are_unique_per_stock_and_round() {
        assert_ne!(history_node_id("600519"), history_node_id("601595"));
        assert_ne!(round_node_id("600519", 1), round_node_id("600519", 2));
        assert_ne!(round_node_id("600519", 1), round_node_id("601595", 1));
        assert_ne!(round_node_id("600519", 2), round_head_node_id("600519", 2));
    }

    /// 同一对 (代码, 轮号) 每次都是同一个 id：渲染侧与定位侧因此能对上。
    #[test]
    fn the_same_target_always_maps_to_the_same_id() {
        assert_eq!(round_node_id("510300", 7), round_node_id("510300", 7));
        assert_eq!(round_node_id("510300", 7), "stock-round-510300-7");
        assert_eq!(history_node_id("510300"), "stock-history-510300");
        assert_eq!(round_head_node_id("510300", 7), "stock-round-head-510300-7");
    }

    /// 脏代码（带空白）也要产出合法 id：`id` 里有空白时 `getElementById` 会取不到。
    #[test]
    fn whitespace_in_a_code_never_reaches_the_id() {
        for id in [
            history_node_id("60 0519"),
            round_node_id("60 0519", 1),
            round_head_node_id("60\t0519", 1),
        ] {
            assert!(!id.contains(char::is_whitespace), "{id}");
        }
    }
}
