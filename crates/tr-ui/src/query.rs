//! 页面取数的统一走法。
//!
//! 每个页面原先都自己拼一遍"置 loading → `spawn_local` → 成功写信号 / 失败 `notify_error`
//! 并回落空值"，顺带各自决定"什么时候重拉、同一个 key 要不要重复发、账本为空时显示什么"。
//! 这条协议现在只有这里一份（[`ListQuery`]）—— 页面只声明**拉什么、key 是什么**。
//!
//! 与 [`crate::store`] 的分工：store 是**状态槽位**（账本 / 外观 / 开关），
//! 本模块是**取数协议**（谁在飞、什么时候作废、失败怎么提示）。
//!
//! 纪律：取数一律过这里，别再在页面里手写 `spawn_local` + `notify_error` 的组合 ——
//! 那种写法每多一处，就多一处"忘了置回 loading"或"忘了在变更后重拉"的机会。
//!
//! ## 边界：什么时候**别**用它
//!
//! 它管的是**一种**形状：一个 key → 一个列表，失败就按静态前缀提示并回落空列表，
//! 变更后用 `reload()` 重拉。下面这些语义**不在这里**，也**不要为了"统一"而塞进来** ——
//! 一个调用方只是假想 seam，等**第二个**同样形状的需求出现时再抽（各自的理由写在那一页的注释里）：
//!
//! * 复合拉取（一次加载要拉好几个列表）：`pages/category_tag.rs`、`pages/key_event.rs`；
//! * 静默失败、或把 `Err` 当业务语义（"不存在 = 空条目"）：`pages/diary.rs`；
//! * 分页 + 多条件，且要区分"还没回来"与"确实是空"：`pages/transactions.rs`；
//! * 一页多条彼此独立的查询 + 跨视图缓存：`pages/stock.rs`；
//! * 根本不是列表查询（命令 + 本地状态）：`pages/settings.rs`。

use std::future::Future;
use std::rc::Rc;

use leptos::prelude::*;

use crate::error_handler::notify_error;
use crate::ipc::IpcError;

/// 按 key 拉一份列表：key 变了就重拉，变更成功后用 [`Self::reload`] 强制重拉。
///
/// 语义（与页面原先手写的那套逐条对齐）：
/// * `key` 返回 `None`（例如还没选账本）→ 清空列表、不发请求；
/// * 拉取中**保留上一次的值**（不闪空白），`loading` 为真；
/// * 失败 → `notify_error(prefix, …)` 并回落成空列表（与各页原行为一致）；
/// * 同一个 `(key, generation)` 只发一次请求 —— `key` 的信号被无关重渲染触发时不会重发。
///
/// `T: Send + Sync` 是 `RwSignal` 默认存储（`SyncStorage`）的要求 —— 列表元素都是
/// `tr_domain::dto` 里的 DTO，满足；哪天要放 JS 句柄（`!Send`）再换 `LocalStorage`。
#[derive(Debug)]
pub struct ListQuery<T: Send + Sync + 'static> {
    /// 最近一次成功的结果
    pub value: RwSignal<Vec<T>>,
    /// 是否有请求在飞行中
    pub loading: RwSignal<bool>,
    /// `reload()` 的自增计数：变了就强制重发（key 没变也发）
    generation: RwSignal<u32>,
}

// 三个字段都是 `RwSignal`（Copy），所以这个句柄可以像信号一样随便放进多个闭包。
// 手写而不是 derive：derive 会给 `T` 加 `Copy`/`Clone` 约束，而 `T` 不必是 Copy。
impl<T: Send + Sync + 'static> Clone for ListQuery<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync + 'static> Copy for ListQuery<T> {}

impl<T: Clone + Send + Sync + 'static> ListQuery<T> {
    /// 建一个查询并立刻挂上（父组件渲染时创建一次）。
    ///
    /// * `prefix` —— 失败通知的标题（沿用各页原来的文案，例如「查询模板失败」）；
    /// * `key` —— 拉取的依据（账本 id / 年份 / 筛选条件的拼接串）；返回 `None` 表示"现在不拉"；
    /// * `fetch` —— 真正发请求的那一步，签名与 `api::*` 的函数一致。
    pub fn new<K, F, Fut>(prefix: &'static str, key: K, fetch: F) -> Self
    where
        K: Fn() -> Option<String> + 'static,
        F: Fn(String) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<T>, IpcError>> + 'static,
    {
        let value = RwSignal::new(Vec::new());
        let loading = RwSignal::new(false);
        let generation = RwSignal::new(0_u32);
        // 已经为哪一次 (key, generation) 发过请求 —— 同一个 key 不重发
        let served = RwSignal::new(None::<(String, u32)>);
        // `fetch` 要能在 Effect 的每次运行里各用一次：Effect 是 FnMut，不能把 fetch 本身
        // 移进异步块（那会让它变成 FnOnce），所以套一层 Rc，每次调用点克隆一个句柄。
        let fetch = Rc::new(fetch);

        Effect::new(move |_: Option<()>| {
            let generation = generation.get();
            let Some(key) = key() else {
                served.set(None);
                value.set(Vec::new());
                loading.set(false);
                return;
            };
            if served.get_untracked().as_ref() == Some(&(key.clone(), generation)) {
                return;
            }
            served.set(Some((key.clone(), generation)));
            loading.set(true);
            let fetch = Rc::clone(&fetch);
            leptos::task::spawn_local(async move {
                match fetch(key).await {
                    Ok(items) => value.set(items),
                    Err(error) => {
                        value.set(Vec::new());
                        notify_error(prefix, &error);
                    }
                }
                loading.set(false);
            });
        });

        Self {
            value,
            loading,
            generation,
        }
    }

    /// 强制重拉：变更成功后调用（key 没变也会再发一次）。
    pub fn reload(&self) {
        self.generation.update(|generation| *generation += 1);
    }
}
