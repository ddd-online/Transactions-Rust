//! 页面取数 module —— 「拉什么、key 是什么、失败算什么」由页面声明，
//! 「什么时候发、要不要去重、变更后怎么作废、跨视图缓存什么时候保留、复核回来要不要写」
//! 全在这里。它是"谁在飞、什么时候作废、失败怎么提示"的唯一 owner。
//!
//! 与 [`crate::store`] 的分工：store 是**状态槽位**（账本 / 外观 / 开关），
//! 本模块是**取数协议**。
//!
//! ## interface 的六个语义轴
//!
//! | 轴 | 表达方式 |
//! |---|---|
//! | **结果类型**不限于是列表（记账 · 记录要的是「记录 + 统计」这一份整体结果） | `T` 任意 `Clone + PartialEq`；页面读 `value` 拿到整份结果 |
//! | **key** 不限于是字符串（账本 / 分页 / 筛选 / 日期各自是不同的 key） | `key` 闭包返回 `Option<K>`，`K` 只要 `Clone + PartialEq` |
//! | **失败三档**：提示 / 静默 / 把 `Err` 当业务空 | [`OnError`]：`Notify(前缀)` 弹提示并记进 `failed`；`Silent` 什么都不做（"不存在 = 空条目"） |
//! | **「加载过」** 一档（区分「空」与「还没回来」） | [`Query::loaded`] |
//! | **跨视图缓存**是一条显式策略 | [`Cache::Keep`]：视图重建时先显示缓存、再复核一次；[`Cache::Mount`] 随视图生灭 |
//! | **复合拉取** = 多条查询 + 一个合并状态（不新增"复合查询"形状） | 页面建多条 [`Query`]，整体 `loading` 取并集 |
//!
//! 纯决策（要不要发、去重、generation、复核判定）在 [`tr_draw::query`] 里，
//! native 上可断言（`cargo test -p tr-draw`）；这里是 effect 与 IPC adapter，
//! 页面只声明「拉什么、key 是什么」。
//!
//! ## 边界：什么时候**别**用它
//!
//! * **命令 + 本地状态**（应用设置那页的保存 / 导入 / 开关）：不是"取一份数据"，
//!   是"做一件事"，走 [`crate::ipc`] + 页面的本地状态，**不要**为了统一塞进来；
//! * **写入路径**（下单 / 改成交 / 同步到其他账本）：本模块只读不写；
//! * **一次性旁路读取**（例如弹窗里查一个股票名）：没有 key、没有缓存语义，
//!   直接用 `api::*`，别造一条只有一个使用面的形状；
//! * **按 key 的惰性缓存**（两个真实使用面：事件页"按日期拉图片与关联交易，命中就同步换上"、
//!   分析页"按图表 id 拉曲线数据"）：[`Cache::Keep`] 是"单槽位 + 挂载复核"，
//!   命中时不能同步换值，形状对不上 —— 这两段留在页面里，**等它们各自长出第三个同形状的
//!   使用面、或两者先合并成一个**，再抽成 module 的一条策略。
//!
//! 纪律：取数一律过这里，别再在页面里手写 `spawn_local` + `notify_error` 的组合 ——
//! 那种写法每多一处，就多一处"忘了置回 loading"或"忘了在变更后重拉"的机会。

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use leptos::prelude::*;
use tr_draw::query::{unchanged, QueryCore, Step};

use crate::error_handler::notify_error;
use crate::ipc::IpcError;

/// 失败档次（interface 的一部分：页面不必自己发明失败语义）。
#[derive(Clone)]
pub enum OnError {
    /// 弹一句提示（标题 = 前缀，沿用各页原文案），并把 `"{前缀}: {msg}"` 记进
    /// [`Query::failed`]（空态里要显示那句话）。
    ///
    /// 前缀是一个**闭包**而不是字符串：有的页面前缀随 key / 交易类型变
    /// （「查询 支出 消费分类失败」），失败的那一刻现算才拿得到。
    Notify(Rc<dyn Fn() -> String>),
    /// 静默：既不弹提示也不记 `failed` —— "日记不存在 = 空条目"这类业务语义。
    Silent,
    /// 把 `Err` 当**业务上的空**：按 `fallback` 显示、不弹提示、不记 `failed`。
    /// 与 [`OnError::Silent`] 的差别在结果：Silent 是"这次没取到，手上留旧的"，
    /// BusinessEmpty 是"没有就是空"（日记不存在、事件没有图片）。
    BusinessEmpty,
}

impl OnError {
    /// 固定前缀的 [`OnError::Notify`]。
    pub fn notify(prefix: &'static str) -> Self {
        OnError::Notify(Rc::new(move || prefix.to_string()))
    }
}

/// 跨视图缓存策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cache {
    /// 随视图生灭：视图重建就重新取一次（绝大多数页面）。
    Mount,
    /// 跨视图保留：数据留在模块级槽位，视图重建时**先显示缓存、再复核一次**
    /// （`id` 必须全局唯一，且同一 id 只能用同一种结果类型）。
    ///
    /// 复核回来与缓存一致时**不写** [`Query::value`]（否则图表会重播入场动画）。
    Keep(&'static str),
}

/// 一条查询的句柄（字段都是 `RwSignal`，所以它像信号一样随便放进多个闭包）。
///
/// * `value` —— 最近一次的结果（失败 / 清空时回落成 `fallback`）；
/// * `loading` —— 有没有请求在飞；
/// * `loaded` —— 有没有**跑完过一次**（区分"还没回来"与"确实是空"）；
/// * `failed` —— 失败文案（按 [`OnError`] 决定记不记）；
/// * [`Query::reload`] —— 变更成功后强制重拉。
#[derive(Debug)]
pub struct Query<T: 'static> {
    pub value: RwSignal<T>,
    pub loading: RwSignal<bool>,
    pub loaded: RwSignal<bool>,
    pub failed: RwSignal<Option<String>>,
    /// 强制重拉的自增计数（变了就重发一次）
    generation: RwSignal<u64>,
}

// 四个字段都是 `RwSignal`（Copy），所以这个句柄可以像信号一样随便放进多个闭包。
// 手写而不是 derive：derive 会给 `T` 加 `Copy` / `Clone` 约束，而 `T` 不必是 Copy。
impl<T: 'static> Clone for Query<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Query<T> {}

impl<T: Default + 'static> Query<T> {
    /// 声明一条查询：`key` 返回 `None` 表示"现在不拉"（例如还没选账本），
    /// `fetch` 是真正发请求的那一步（签名与 `api::*` 的函数一致）。
    pub fn new<K, KF, F>(key: KF, fetch: F) -> QueryBuilder<T, K, KF, F> {
        QueryBuilder {
            cache: Cache::Mount,
            cache_scope: None,
            on_error: OnError::Silent,
            keep_stale: false,
            fallback: Rc::new(|_: Option<&K>| T::default()),
            key,
            fetch,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: 'static> Query<T> {
    /// 强制重拉：变更成功后调用（key 没变也会再发一次）。
    pub fn reload(&self) {
        self.generation.update(|generation| *generation += 1);
    }

    /// 丢掉一条跨视图缓存（切账本时用：不把上一个账本的数据与去重键带过去）。
    ///
    /// 对 [`Cache::Mount`] 的查询是空操作。
    pub fn forget(id: &'static str) {
        KEEP_SLOTS.with(|slots| {
            slots.borrow_mut().remove(id);
        });
    }
}

/// 声明中途的配置（`Query::new(..).on_error(..).start()`）。
pub struct QueryBuilder<T: 'static, K, KF, F> {
    cache: Cache,
    cache_scope: Option<Rc<dyn Fn() -> String>>,
    on_error: OnError,
    keep_stale: bool,
    fallback: Rc<dyn Fn(Option<&K>) -> T>,
    key: KF,
    fetch: F,
    _marker: std::marker::PhantomData<K>,
}

impl<T, K, KF, F> QueryBuilder<T, K, KF, F>
where
    T: Clone + Send + Sync + PartialEq + Default + 'static,
    K: Clone + PartialEq + 'static,
    KF: Fn() -> Option<K> + 'static,
{
    /// 失败档次（默认 [`OnError::Silent`]）。
    pub fn on_error(mut self, on_error: OnError) -> Self {
        self.on_error = on_error;
        self
    }

    /// 跨视图缓存策略（默认 [`Cache::Mount`]）。
    pub fn cache(mut self, cache: Cache) -> Self {
        self.cache = cache;
        self
    }

    /// 缓存**作用域**：作用域变了（例如切了账本），这一条缓存立刻作废 ——
    /// 不把上一个作用域的数据与去重键带过去。只对 [`Cache::Keep`] 有意义。
    ///
    /// 闭包在渲染期与每次观察时各读一次（都用 `untrack` 读，不会把作用域里的信号
    /// 变成上层视图的依赖）。
    pub fn cache_scope(mut self, scope: impl Fn() -> String + 'static) -> Self {
        self.cache_scope = Some(Rc::new(scope));
        self
    }

    /// 失败 / 清空时对外显示什么（默认 `T::default()`）。
    ///
    /// 闭包拿到的是"当前 key"：`None` 表示这次是被清空（例如还没选账本），
    /// `Some(&key)` 表示失败时手上那一份 key（「不存在 = 某日期的空条目」要它）。
    pub fn fallback(mut self, fallback: impl Fn(Option<&K>) -> T + 'static) -> Self {
        self.fallback = Rc::new(fallback);
        self
    }

    /// 失败时**保留手上那一份**（默认按 `fallback` 回落）。
    ///
    /// 只对 [`OnError::Notify`] 有意义：统计曲线 / 账户总览这类"手上本来就有旧数据"的地方，
    /// 一次失败不该把图清空 —— 提示一句、把旧数字留在那儿比一片空白有用。
    pub fn keep_stale(mut self, keep_stale: bool) -> Self {
        self.keep_stale = keep_stale;
        self
    }

    /// 建好句柄并立刻挂上（父组件渲染时创建一次）。
    pub fn start<Fut>(self) -> Query<T>
    where
        F: Fn(K) -> Fut + 'static,
        Fut: Future<Output = Result<T, IpcError>> + 'static,
    {
        let QueryBuilder {
            cache,
            cache_scope,
            on_error,
            keep_stale,
            fallback,
            key,
            fetch,
            ..
        } = self;

        // Keep 策略：数据与决策状态都住在模块级槽位里 —— 视图被反复重建时
        // （统计视图是普通函数，父级每次重渲染都会重新调用它）新挂载的那份
        // 既能先看到缓存，也不会把同一条请求再发一遍。
        let keep = match cache {
            Cache::Mount => None,
            Cache::Keep(id) => {
                let scope = cache_scope.as_ref().map(|scope| untrack(scope.as_ref()));
                Some(keep_slot::<K, T>(id, scope, &fallback))
            }
        };

        let (value, loading, loaded, failed) = match &keep {
            Some(slot) => (
                // 起手就带上缓存：视图重建时先显示上次的数据，不闪空白
                RwSignal::new(slot.value.borrow().clone()),
                RwSignal::new(slot.loading.get()),
                RwSignal::new(slot.loaded.get()),
                RwSignal::new(None::<String>),
            ),
            None => (
                RwSignal::new(fallback(None)),
                RwSignal::new(false),
                RwSignal::new(false),
                RwSignal::new(None::<String>),
            ),
        };
        let generation = RwSignal::new(0_u64);

        // 决策状态：Keep 时跨视图重建保留（"同一条请求在飞时去重"靠的就是它）
        let core = match &keep {
            Some(slot) => Rc::clone(&slot.core),
            None => Rc::new(RefCell::new(QueryCore::new())),
        };
        let fetch = Rc::new(fetch);

        Effect::new(move |prev: Option<(Option<K>, u64)>| {
            let current_key = key();
            let generation_now = generation.get();
            // 缓存作用域变了（切账本）→ 这一条缓存作废：数据与去重状态一起丢
            if let Some(slot) = keep.as_ref() {
                let scope = cache_scope.as_ref().map(|scope| untrack(scope.as_ref()));
                slot.ensure_scope(scope, &fallback);
            }
            let step = match prev {
                // 首次运行 = 视图挂载：确保当前 key 有一份数据（缓存先显示、静默复核）
                None => core.borrow_mut().mount(current_key.clone()),
                // reload() 把 generation 推进一步：key 没变也重发
                Some((_, previous)) if previous != generation_now => {
                    core.borrow_mut().reload(current_key.clone())
                }
                Some(_) => core.borrow_mut().observe(current_key.clone()),
            };

            match step {
                Step::Idle => {}
                Step::Clear => {
                    value.set(fallback(None));
                    failed.set(None);
                    loading.set(false);
                    loaded.set(true);
                    if let Some(slot) = keep.as_ref() {
                        slot.loading.set(false);
                        slot.loaded.set(true);
                    }
                }
                Step::Fetch { key, generation } => {
                    loading.set(true);
                    if let Some(slot) = keep.as_ref() {
                        slot.loading.set(true);
                    }
                    let core = Rc::clone(&core);
                    let fetch = Rc::clone(&fetch);
                    let fallback = Rc::clone(&fallback);
                    let on_error = on_error.clone();
                    let keep = keep.clone();
                    leptos::task::spawn_local(async move {
                        let result = fetch(key.clone()).await;
                        // 请求结束（成功失败都算）：结果已经过期就整份丢掉
                        let current = core.borrow_mut().settle(&key, generation);
                        if !current {
                            return;
                        }
                        if let Some(slot) = keep.as_ref() {
                            slot.loading.set(false);
                        }
                        match result {
                            Ok(data) => {
                                failed.set(None);
                                // 复核结果与手上一致就不写信号（图表不重播入场动画）
                                let same = unchanged(Some(&value.get_untracked()), &data);
                                if !same {
                                    value.set(data.clone());
                                }
                                loaded.set(true);
                                loading.set(false);
                                if let Some(slot) = keep.as_ref() {
                                    *slot.value.borrow_mut() = data;
                                    slot.loaded.set(true);
                                }
                            }
                            Err(error) => {
                                let fallback_value = fallback(Some(&key));
                                match on_error {
                                    OnError::Notify(ref prefix) => {
                                        let prefix = prefix();
                                        if !keep_stale {
                                            value.set(fallback_value);
                                        }
                                        failed.set(Some(error.prefixed(&prefix)));
                                        notify_error(&prefix, &error);
                                    }
                                    OnError::Silent => {
                                        // 手上留着上一次的结果（不发请求的语义是"这次没取到"）
                                        failed.set(None);
                                    }
                                    OnError::BusinessEmpty => {
                                        // 没有就是空：按 fallback 显示，且算"已经跑完一次"
                                        value.set(fallback_value);
                                        failed.set(None);
                                    }
                                }
                                loaded.set(true);
                                loading.set(false);
                                // 失败不留"已发过"：没有刷新按钮的页面也要能再试一次
                                core.borrow_mut().failed(&key, generation);
                            }
                        }
                    });
                }
            }

            (current_key, generation_now)
        });

        Query {
            value,
            loading,
            loaded,
            failed,
            generation,
        }
    }
}

// ---------------------------------------------------------------- 跨视图缓存槽位

/// Keep 槽位：**数据与决策状态**（不只是数据 —— 去重要靠 in_flight）。
struct KeepSlot<K, T> {
    core: Rc<RefCell<QueryCore<K>>>,
    value: RefCell<T>,
    loading: Cell<bool>,
    loaded: Cell<bool>,
    /// 这份缓存属于哪个作用域（空 `None` = 不区分作用域）
    scope: RefCell<Option<String>>,
}

impl<K: Clone + PartialEq, T: Clone> KeepSlot<K, T> {
    /// 作用域对不上就把这份缓存整个作废（数据 + 去重状态）。
    fn ensure_scope(&self, scope: Option<String>, fallback: &Rc<dyn Fn(Option<&K>) -> T>) {
        if *self.scope.borrow() == scope {
            return;
        }
        *self.scope.borrow_mut() = scope;
        *self.value.borrow_mut() = fallback(None);
        *self.core.borrow_mut() = QueryCore::new();
        self.loading.set(false);
        self.loaded.set(false);
    }
}

thread_local! {
    /// Keep 槽位表（`id → Rc<KeepSlot<K, T>>`，类型擦除后用 `Any` 取回）。
    static KEEP_SLOTS: RefCell<HashMap<&'static str, Rc<dyn Any>>> =
        RefCell::new(HashMap::new());
}

fn keep_slot<K, T>(
    id: &'static str,
    scope: Option<String>,
    fallback: &Rc<dyn Fn(Option<&K>) -> T>,
) -> Rc<KeepSlot<K, T>>
where
    K: Clone + PartialEq + 'static,
    T: Clone + 'static,
{
    KEEP_SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        if let Some(existing) = slots.get(id) {
            match Rc::clone(existing).downcast::<KeepSlot<K, T>>() {
                Ok(slot) => {
                    // 作用域对不上 = 上一个作用域的缓存，整份丢掉重建
                    if *slot.scope.borrow() == scope {
                        return slot;
                    }
                }
                Err(_) => {
                    leptos::logging::warn!("取数缓存槽位 {id} 的结果类型与上次不一致，已重建");
                }
            }
        }
        let slot = Rc::new(KeepSlot {
            core: Rc::new(RefCell::new(QueryCore::new())),
            value: RefCell::new(fallback(None)),
            loading: Cell::new(false),
            loaded: Cell::new(false),
            scope: RefCell::new(scope),
        });
        slots.insert(id, Rc::clone(&slot) as Rc<dyn Any>);
        slot
    })
}

// ---------------------------------------------------------------- 兼容层：ListQuery

/// 按 key 拉一份列表 —— 老 `ListQuery` 的语义原样保留（分析 / 模板 / 待办三页在用），
/// 实现已经换成上面那个 module 的薄壳：key 变了就重拉、失败提示并回落空列表、
/// `reload()` 强制重拉、空账本清空且不发。
///
/// 新代码直接用 [`Query`]（`loaded` / 失败档次 / 跨视图缓存都在那边）。
#[derive(Debug)]
pub struct ListQuery<T: Send + Sync + 'static> {
    /// 最近一次成功的结果
    pub value: RwSignal<Vec<T>>,
    /// 是否有请求在飞行中
    pub loading: RwSignal<bool>,
    /// 有没有**跑完过一次**（区分"还没回来"与"确实是空"）
    pub loaded: RwSignal<bool>,
    /// 失败文案（`"{前缀}: {msg}"`）；成功或静默失败时是 `None`
    pub failed: RwSignal<Option<String>>,
    inner: Query<Vec<T>>,
}

impl<T: PartialEq + Send + Sync + 'static> Clone for ListQuery<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: PartialEq + Send + Sync + 'static> Copy for ListQuery<T> {}

impl<T: Clone + PartialEq + Send + Sync + 'static> ListQuery<T> {
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
        let inner = Query::<Vec<T>>::new(key, fetch)
            .on_error(OnError::notify(prefix))
            .start();
        Self {
            value: inner.value,
            loading: inner.loading,
            loaded: inner.loaded,
            failed: inner.failed,
            inner,
        }
    }

    /// 强制重拉：变更成功后调用（key 没变也会再发一次）。
    pub fn reload(&self) {
        self.inner.reload();
    }
}
