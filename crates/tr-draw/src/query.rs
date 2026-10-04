//! 页面取数的**决策核心**。
//!
//! 界面里每个页面都有一份取数逻辑，但真正属于"决策"的只有四件事：
//! 这次要不要发、同一个 key 要不要去重、什么时候作废重来、复核回来要不要写出去。
//! 这四条过去散在九个页面里各写一遍（顺带各写一遍 loading / notify_error 样板），
//! 而且都住在只编 wasm32 的 `tr-ui` 里 —— native 一句也测不到。这里把它们收成
//! 一个纯状态机，于是 `cargo test -p tr-draw` 就能真跑（见 `docs/adr/0001-pure-draw-crate.md`
//! 那句"再遇到界面里想被 native 断言的纯算法，直接放 tr-draw"）。
//!
//! 本模块**不知道** key 长什么样（只要求能比较相等）、不知道结果类型、不知道失败怎么提示；
//! 那些属于界面侧 [`crate::query`](../../tr-ui/src/query.rs) 的 [adapter]。这里只有
//! "喂事件、收下一步"这一条 seam：
//!
//! * [`QueryCore::mount`] —— 视图（重新）挂载：确保当前 key 有一份数据（在飞则去重）；
//! * [`QueryCore::observe`] —— 之后的每次观察（key 信号变化 / Effect 重跑）；
//! * [`QueryCore::reload`] —— 变更成功后强制重拉（key 没变也发）；
//! * [`QueryCore::settle`] —— 请求结束（成功或失败都算）;
//! * [`QueryCore::failed`] —— 请求失败：把这一条从"已发给过"里撤掉，下一次观察允许重试。
//!
//! 产出只有三种：什么都不做、清空对外状态、发一次请求。

/// 核心给出的下一步。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<K> {
    /// 什么都不用做：同一份 key 已经取过、或已经在飞。
    Idle,
    /// key 变成"无"（例如还没选账本）：清空对外状态，不发请求。
    Clear,
    /// 发一次请求。
    Fetch { key: K, generation: u64 },
}

/// 一条查询的决策状态。
///
/// `K` 是去重的依据（账本 id / 年份 / 筛选条件的快照…），只要求 `Clone + PartialEq`。
/// `generation` 是 [`QueryCore::reload`] 的自增计数：变了就表示"这一份 key 要重算"，
/// 用它去比对[`QueryCore::settle`]回来的那一份，过期结果自然落空。
#[derive(Debug, Clone)]
pub struct QueryCore<K> {
    /// 当前观察到的 key（`None` = 现在不拉）
    key: Option<K>,
    /// 是否已经清空过一次对外状态（避免每次重渲染都重复写同一份空值）
    cleared: bool,
    /// 强制重拉的自增计数
    generation: u64,
    /// 已经"发过"的 `(key, generation)`（含正在飞的）
    served: Option<(K, u64)>,
    /// 正在飞的 `(key, generation)`
    in_flight: Option<(K, u64)>,
}

impl<K: Clone + PartialEq> Default for QueryCore<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + PartialEq> QueryCore<K> {
    pub fn new() -> Self {
        Self {
            key: None,
            cleared: false,
            generation: 0,
            served: None,
            in_flight: None,
        }
    }

    /// 视图（重新）挂载：即使这一份 key 已经取过，也**复核一次**（跨视图缓存的语义：
    /// 先显示手上的缓存、再静默复核）。仍然不给正在飞的同一份 key 再发一份。
    pub fn mount(&mut self, key: Option<K>) -> Step<K> {
        self.plan(key, true)
    }

    /// 之后的每次观察（key 信号变化 / Effect 重跑）：
    /// 同一份 key 取过就不再发 —— 视图重建不该变成请求风暴。
    pub fn observe(&mut self, key: Option<K>) -> Step<K> {
        self.plan(key, false)
    }

    /// 强制重拉：变更成功后调用（key 没变也会再发一次）。
    pub fn reload(&mut self, key: Option<K>) -> Step<K> {
        self.generation += 1;
        self.plan(key, false)
    }

    /// 请求结束（成功或失败都算）：清掉"正在飞"。
    ///
    /// 返回这一份是不是**当前**那一份 —— 不是（请求期间 key 换了 / 又重拉了）
    /// 就说明结果已经过期，调用方应当丢弃它。
    pub fn settle(&mut self, key: &K, generation: u64) -> bool {
        let current = self.in_flight.as_ref() == Some(&(key.clone(), generation));
        if current {
            self.in_flight = None;
        }
        current
    }

    /// 请求失败：把这一条从"已发过"里撤掉，让下一次观察能重试
    /// （没有"刷新"按钮的页面靠这一条，否则一次失败会永久卡住）。
    pub fn failed(&mut self, key: &K, generation: u64) {
        if self.served.as_ref() == Some(&(key.clone(), generation)) {
            self.served = None;
        }
    }

    /// 当前的重复计数（仅用于断言 / 排障）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn plan(&mut self, key: Option<K>, force: bool) -> Step<K> {
        let Some(key) = key else {
            // key 变成"无"：清空对外状态。已经清干净了就什么都不做 ——
            // 每次重渲染都返回 Clear 会让界面反复写同一份空值。
            let already_cleared = self.cleared;
            self.key = None;
            self.cleared = true;
            self.served = None;
            self.in_flight = None;
            return if already_cleared {
                Step::Idle
            } else {
                Step::Clear
            };
        };
        self.cleared = false;

        // 同一份 key 正在飞 → 去重（来回切子功能时最常见：上一次还没回来就又挂载了）。
        if self.in_flight.as_ref().is_some_and(|(k, _)| *k == key) {
            self.key = Some(key);
            return Step::Idle;
        }

        // 同一份 `(key, generation)` 已经发过 → 除非是挂载复核，否则不必再发。
        let served_same = self
            .served
            .as_ref()
            .is_some_and(|(k, g)| *k == key && *g == self.generation);
        if !force && served_same {
            self.key = Some(key);
            return Step::Idle;
        }

        self.key = Some(key.clone());
        self.served = Some((key.clone(), self.generation));
        self.in_flight = Some((key.clone(), self.generation));
        Step::Fetch {
            key,
            generation: self.generation,
        }
    }
}

/// 复核回来的结果与手上缓存是否**一致**。
///
/// 一致就不该写信号：统计视图的图表是"读信号即重建"的，同值通知会让入场动画重播
/// （真实缺陷：曲线一直在闪）。这条判据过去写在股票页的注释里，现在是一条能断言的决定。
pub fn unchanged<T: PartialEq>(current: Option<&T>, fresh: &T) -> bool {
    matches!(current, Some(current) if current == fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(step: Step<&'static str>) -> Option<&'static str> {
        match step {
            Step::Fetch { key, .. } => Some(key),
            _ => None,
        }
    }

    #[test]
    fn mount_fetches_and_observe_dedupes_the_same_key() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        // 视图重建：同一份 key 不再发
        assert_eq!(core.observe(Some("A")), Step::Idle);
        assert_eq!(core.observe(Some("A")), Step::Idle);
        // 换 key → 发
        assert_eq!(fetch(core.observe(Some("B"))), Some("B"));
        assert_eq!(core.observe(Some("B")), Step::Idle);
    }

    #[test]
    fn mount_revalidates_even_for_the_same_key() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        assert!(core.settle(&"A", 0));
        // 从别的子功能切回来：同一份 key 也复核一次
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
    }

    #[test]
    fn in_flight_same_key_is_deduped() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        // 还没回来就又挂载（视图重建）→ 不再发第二份
        assert_eq!(core.mount(Some("A")), Step::Idle);
        assert_eq!(core.observe(Some("A")), Step::Idle);
        assert!(core.settle(&"A", 0));
        // 回来之后再挂载 → 复核
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
    }

    #[test]
    fn reload_forces_one_more_request() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        assert!(core.settle(&"A", 0));
        assert_eq!(core.observe(Some("A")), Step::Idle);
        // 变更后重拉：key 没变也发一次
        assert_eq!(fetch(core.reload(Some("A"))), Some("A"));
        assert!(core.settle(&"A", 1));
        assert_eq!(core.observe(Some("A")), Step::Idle);
        // 不会连着发两次
        assert_eq!(core.observe(Some("A")), Step::Idle);
    }

    #[test]
    fn none_key_clears_once_and_never_fetches() {
        let mut core = QueryCore::new();
        // 一开始就没有 key（还没选账本）：清空一次对外状态 —— 界面据此显示
        // "确实是空"，而不是永远停在"正在加载"。
        assert_eq!(core.mount(None), Step::Clear);
        // 之后的每次重渲染不再重复清
        assert_eq!(core.observe(None), Step::Idle);
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        assert!(core.settle(&"A", 0));
        // 账本被清空 → 清空对外状态
        assert_eq!(core.observe(None), Step::Clear);
        // 清完再来一次就不再重复清
        assert_eq!(core.observe(None), Step::Idle);
    }

    #[test]
    fn stale_result_is_rejected_after_key_change() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        // 请求还在飞，key 就换了
        assert_eq!(fetch(core.observe(Some("B"))), Some("B"));
        // A 的结果回来时已经过期
        assert!(!core.settle(&"A", 0));
        // B 的才是当前那一份
        assert!(core.settle(&"B", 0));
    }

    #[test]
    fn failure_clears_served_so_the_next_observe_retries() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        assert!(core.settle(&"A", 0));
        core.failed(&"A", 0);
        // 失败后没有"刷新"按钮也要能再试一次
        assert_eq!(fetch(core.observe(Some("A"))), Some("A"));
    }

    #[test]
    fn failure_after_key_change_does_not_clobber_the_new_key() {
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        assert_eq!(fetch(core.observe(Some("B"))), Some("B"));
        // A 的失败不该把 B 的"已发过"撤掉
        core.failed(&"A", 0);
        assert!(!core.settle(&"A", 0));
        assert!(core.settle(&"B", 0));
        assert_eq!(core.observe(Some("B")), Step::Idle);
    }

    #[test]
    fn view_rebuild_storm_issues_at_most_one_request_per_round() {
        // 自激循环的回归：视图被反复重建（每轮一次 mount），若没有去重就是请求风暴。
        let mut core = QueryCore::new();
        assert_eq!(fetch(core.mount(Some("A"))), Some("A"));
        // 请求还没回来，视图又重建了 30 次
        for _ in 0..30 {
            assert_eq!(core.mount(Some("A")), Step::Idle);
        }
        assert!(core.settle(&"A", 0));
    }

    #[test]
    fn unchanged_detects_identical_revalidation() {
        let current = Some(vec![1, 2, 3]);
        assert!(unchanged(current.as_ref(), &vec![1, 2, 3]));
        assert!(!unchanged(current.as_ref(), &vec![1, 2]));
        assert!(!unchanged(None, &vec![1]));
    }
}
