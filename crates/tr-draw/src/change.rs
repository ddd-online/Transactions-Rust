//! 写路径的**判据核心**：一次写意图出发时记下的东西，回来时用来判断"还该不该落地"。
//!
//! 界面侧的读半边有 [`crate::query`]（去重 / generation / 复核判定），写半边从前没有对应的东西 ——
//! "账本在写的过程中被切走了"这件事由**每个页面各写一遍**：全仓 9 处
//! `if stores.current_ledger_id.get_untracked() != ledger_id { return; }`
//! （`diary.rs` / `key_event.rs` / `transactions.rs`）。这些判断**比读半边那份更弱**：
//! 它们只比账本，而读的 `QueryCore::settle` 比的是整个 `(key, generation)` —— 翻页/改筛选之后
//! 第 3 页的应答会在第 1 页落地。
//!
//! 这里先把**写意图的目标**收成一个可断言的值：出发时记下是谁的活，回来时问一句还该不该落地。
//! 规则只有一条但值得钉住 —— **空目标（还没选账本）永远算过期**：那种写迟早会被后端拒绝，
//! 落在界面上只会让空态闪出旧数据。
//!
//! 这只是候选 2（写路径 module，见 #34）的第一片：把"谁过期"这条判据从 9 个调用点的
//! 手写比较里搬出来。接下来才是"成功之后哪些读作废"那张表。

/// 一次写意图的目标账本。
///
/// 出发时 `WriteTarget::new(当时的账本 id)`，异步回来后 `is_stale(现在的账本 id)`：
/// `true` 表示这次写属于**旧**账本 —— 本地不落地、也不提示（用户已经在看别的账本了）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTarget {
    ledger_id: String,
}

impl WriteTarget {
    /// 记下这次写是替哪个账本做的。
    pub fn new(ledger_id: impl Into<String>) -> Self {
        Self {
            ledger_id: ledger_id.into(),
        }
    }

    /// 出发时记下的账本 id（空串表示"还没选账本"）。
    pub fn ledger_id(&self) -> &str {
        &self.ledger_id
    }

    /// 账本已经切走（或压根没选）→ 这次写的结果**不该落地**。
    pub fn is_stale(&self, current_ledger_id: &str) -> bool {
        self.ledger_id.is_empty() || self.ledger_id != current_ledger_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_ledger_is_not_stale() {
        let target = WriteTarget::new("ledger-a");
        assert!(!target.is_stale("ledger-a"));
        assert_eq!(target.ledger_id(), "ledger-a");
    }

    #[test]
    fn switching_the_ledger_makes_the_write_stale() {
        let target = WriteTarget::new("ledger-a");
        assert!(target.is_stale("ledger-b"), "旧账本的写不该落到新账本上");
        assert!(target.is_stale(""), "切到「没有账本」也算过期");
    }

    /// 还没选账本就发出去的写永远算过期（后端迟早会拒，落地只会闪出旧数据）。
    #[test]
    fn a_write_without_a_ledger_is_always_stale() {
        let target = WriteTarget::new("");
        assert!(target.is_stale(""));
        assert!(
            target.is_stale("ledger-a"),
            "连事后选中了别的账本也不算它的"
        );
    }

    /// 判定是"纯比较"：同一对入参永远同一个答案（调用点可以放心在异步块里用）。
    #[test]
    fn the_rule_is_pure() {
        let target = WriteTarget::new("ledger-a");
        for _ in 0..3 {
            assert!(!target.is_stale("ledger-a"));
            assert!(target.is_stale("ledger-b"));
        }
    }
}
