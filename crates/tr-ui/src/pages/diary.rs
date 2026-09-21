//! 日记页（`/diary_view`）—— P6-b 完整实现。
//!
//! ## 组成
//!
//! * [`DiaryPage`]：工具栏（今天 / 收起全部 / 日期选择器）+ 两栏编排
//! * [`DiaryTree`]：年 → 月 → 日三级折叠树（年/月降序、日降序）
//! * [`DiaryEditor`]：心情 + 字数 + **始终可编辑**（无预览/Markdown 渲染）+ 1500ms 防抖自动保存 + 删除
//! * 状态直接由本文件持有信号（与其余页面一致）
//! * 正文按**纯文本**呈现（换行原样保留）：日记页不做 Markdown 渲染
//!
//! ## 关键行为
//!
//! * **日记不存在也能写**：`diary_get` 对不存在的日期会报错，因此读取失败时按
//!   "空条目"兜底，否则新日期永远打不开编辑器。
//! * **自动保存、无保存按钮**：输入或切心情后 1500ms 防抖保存；`Ctrl+S`/`Cmd+S` 立即保存。
//! * **保存状态如实反映**：输入/切心情先把状态置为 `Editing`（「编辑中」），
//!   防抖到点保存时短暂 `Saving`（「保存中…」），成功后 `Saved`（「已保存」）；
//!   失败为「保存失败」。换日期时状态回到 `Idle`（不把上一条的状态带过来）。
//!   保存期间又发生改动时，完成回调会再排一次防抖保存，因此「编辑中」一定会收敛到「已保存」。
//! * **左右两处字数口径不同**：编辑器按本地草稿的码点数实时算，左树用服务端的 `wordCount`。
//! * **折叠状态**：首次拿到非空数据时全部年份收起；月份默认全收起；「收起全部」把所有年份收起。
//!
//! ## 设计取舍
//!
//! * **关键词过滤未实现**：本页没有路由、没有 UI、IPC 也没有该参数，后端也没有等价命令，
//!   因此不做过滤。
//! * 单栏编辑区，没有分栏。
//! * 编辑器的时间防抖用 `set_timeout` + 句柄，切日期时显式清掉。

use std::collections::{BTreeMap, BTreeSet};

use leptos::prelude::*;
use tr_domain::models::{DiaryDateItem, DiaryEntry};

use crate::api;
use crate::components::ui::{
    Button, ButtonVariant, DatePicker, FeaturePage, Modal, ModalSize, Textarea,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{format_ymd_cn, split_ymd, today_ymd, weekday_cn};

/// 页面标题（固定文案，改动即影响界面）。
pub const PAGE_TITLE: &str = "日记";

/// 自动保存防抖时长（1500ms）。
const AUTOSAVE_DEBOUNCE_MS: u64 = 1500;

/// 心情选项（emoji 本身就是入库值；label 只用于 tooltip / `aria-label`）。
const MOODS: [(&str, &str); 6] = [
    ("", "无"),
    ("😊", "开心"),
    ("😐", "平静"),
    ("😢", "难过"),
    ("😤", "生气"),
    ("😰", "焦虑"),
];

/// 保存状态（驱动「编辑中 / 保存中… / 已保存 / 保存失败」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveStatus {
    /// 已落库（或刚载入，尚无改动）
    Idle,
    /// **有未落库的改动**：输入/切心情后、防抖窗口内（这就是「编辑中」）
    Editing,
    Saving,
    Saved,
    Error,
}

impl SaveStatus {
    fn label(self) -> &'static str {
        match self {
            SaveStatus::Idle | SaveStatus::Saved => "已保存",
            SaveStatus::Editing => "编辑中",
            SaveStatus::Saving => "保存中…",
            SaveStatus::Error => "保存失败",
        }
    }

    fn class(self) -> &'static str {
        match self {
            SaveStatus::Editing => "is-editing",
            SaveStatus::Saving => "is-saving",
            SaveStatus::Saved => "is-saved",
            SaveStatus::Error => "is-error",
            SaveStatus::Idle => "is-idle",
        }
    }
}

// ==================================================================== 页面

/// 页面根组件。
#[component]
pub fn DiaryPage() -> impl IntoView {
    let stores = AppStores::global();
    let today = today_ymd();
    let selected_date = RwSignal::new(today.clone());
    let dates = RwSignal::new(Vec::<DiaryDateItem>::new());
    let entry = RwSignal::new(Option::<DiaryEntry>::None);
    let draft = RwSignal::new(String::new());
    let mood = RwSignal::new(String::new());
    let save_status = RwSignal::new(SaveStatus::Idle);
    // 补存闸门：保存完成时若草稿又变了就 +1，交给下面的 Effect 再排一次保存
    let save_retry = RwSignal::new(0u64);
    let saving = RwSignal::new(false);
    let delete_open = RwSignal::new(false);
    let deleting = RwSignal::new(false);
    let jump_date = RwSignal::new(String::new());
    // **草稿所属账本**：在 `schedule_save` 时快照。切账本时草稿仍写回这个账本，
    // 既不会丢字，也不会把 A 账本的正文写进 B 账本。
    let draft_ledger = RwSignal::new(String::new());

    // 折叠状态：年份集合 + `${year}-${month}` 集合
    let collapsed_years = RwSignal::new(BTreeSet::<i32>::new());
    let expanded_months = RwSignal::new(BTreeSet::<String>::new());
    let tree_initialized = RwSignal::new(false);

    // 防抖句柄
    let timer = StoredValue::new(Option::<leptos::leptos_dom::helpers::TimeoutHandle>::None);

    let clear_pending_save = move || {
        timer.update_value(|slot| {
            if let Some(handle) = slot.take() {
                handle.clear();
            }
        });
    };

    let load_dates = move |ledger_id: String| {
        leptos::task::spawn_local(async move {
            if let Ok(items) = api::diary::list_dates(&ledger_id).await {
                // 陈旧响应守卫：账本已经切走就丢弃，别把上一个账本的日期树画出来
                if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                    return;
                }
                dates.set(items);
            }
        });
    };

    let load_entry = move |ledger_id: String, date: String| {
        selected_date.set(date.clone());
        // 换日期后状态从零起算：否则上一条的「编辑中 / 保存中… / 保存失败」会被带过来
        save_status.set(SaveStatus::Idle);
        leptos::task::spawn_local(async move {
            let loaded = api::diary::get(&date, &ledger_id).await;
            // 陈旧响应守卫：账本已经切走就丢弃
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            match loaded {
                Ok(item) => {
                    draft.set(item.content.clone());
                    mood.set(item.mood.clone());
                    entry.set(Some(item));
                }
                Err(_) => {
                    // 不存在 → 空条目（可编辑）
                    draft.set(String::new());
                    mood.set(String::new());
                    entry.set(Some(DiaryEntry {
                        date: date.clone(),
                        ledger_id: ledger_id.clone(),
                        ..DiaryEntry::default()
                    }));
                }
            }
        });
    };

    let do_save = move || {
        // 草稿属于哪个账本就写哪个账本（切账本后仍在飞的这次保存不会串账本）
        let ledger_id = draft_ledger.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let Some(current) = entry.get_untracked() else {
            return;
        };
        if saving.get_untracked() {
            // 上一次保存还在飞：不并发写（避免旧内容后落库）。
            // 这次调用不会被丢掉——下面的完成回调会发现草稿已经变了，再排一次防抖保存。
            return;
        }
        saving.set(true);
        save_status.set(SaveStatus::Saving);
        let date = current.date.clone();
        let content = draft.get_untracked();
        let mood_value = mood.get_untracked();
        // 快照：用来判断"保存期间草稿又变了没有"
        let content_at_start = content.clone();
        let mood_at_start = mood_value.clone();
        leptos::task::spawn_local(async move {
            let saved = api::diary::upsert(&date, &content, &mood_value, &ledger_id).await;
            saving.set(false);
            // 账本已经切走：这次写入属于**旧**账本，界面此刻是新账本的数据，不能合并回去
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            match saved {
                Ok(saved) => {
                    save_status.set(SaveStatus::Saved);
                    // 用返回的条目覆盖（含服务端 wordCount）
                    let summary = DiaryDateItem {
                        date: saved.date.clone(),
                        word_count: saved.word_count,
                        mood: saved.mood.clone(),
                    };
                    entry.set(Some(saved));
                    dates.update(|items| {
                        match items.iter_mut().find(|item| item.date == summary.date) {
                            Some(existing) => *existing = summary.clone(),
                            None => items.push(summary.clone()),
                        }
                        items.sort_by(|left, right| right.date.cmp(&left.date));
                    });
                }
                Err(error) => {
                    save_status.set(SaveStatus::Error);
                    notify_error("保存日记失败", &error);
                }
            }
            // 保存期间草稿又变了（在飞的请求撞上了一波输入）→ 补排一次防抖保存。
            // 没有这一步的话「编辑中」会一直挂着，最新内容也永远不会落库。
            let same_entry = entry.get_untracked().is_some_and(|item| item.date == date);
            if same_entry
                && (draft.get_untracked() != content_at_start
                    || mood.get_untracked() != mood_at_start)
            {
                save_retry.update(|tick| *tick += 1);
            }
        });
    };

    let schedule_save = move || {
        // 「编辑中」：改动已进草稿、还没落库（防抖窗口内就显示这个）
        save_status.set(SaveStatus::Editing);
        // 记下这次草稿属于哪个账本（切账本时按它写回）
        draft_ledger.set(stores.current_ledger_id.get_untracked());
        // 先清掉上一次的待触发任务
        clear_pending_save();
        let handle = set_timeout_with_handle(
            do_save,
            std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS),
        );
        if let Ok(handle) = handle {
            timer.set_value(Some(handle));
        }
    };

    // 补存闸门：保存完成时若发现草稿又变了，就再走一次（带防抖的）保存，
    // 保证「编辑中」最终一定收敛到「已保存」。
    Effect::new(move |_| {
        if save_retry.get() == 0 {
            return;
        }
        schedule_save();
    });

    // 账本变化 → 拉该账本的日期树与选中那天的日记；切账本前先把待落库的草稿写回**旧**账本。
    //
    // 只读信号、只做副作用（不在这里创建任何响应式值 —— 那会被随后的重跑 dispose）。
    Effect::new(move |_| {
        let ledger_id = stores.current_ledger_id.get();
        // ① 取消防抖；有未落库的草稿 → 立刻写回草稿所属账本（旧账本）
        clear_pending_save();
        if save_status.get_untracked() == SaveStatus::Editing
            && !draft_ledger.get_untracked().is_empty()
        {
            do_save();
        }
        // ② 换账本后让日期树对新数据重新做一次"首次收起"
        tree_initialized.set(false);
        // ③ 一个账本都没有 → 清空并展示「未选择账本」引导
        if ledger_id.is_empty() {
            dates.set(Vec::new());
            entry.set(None);
            draft.set(String::new());
            mood.set(String::new());
            save_status.set(SaveStatus::Idle);
            draft_ledger.set(String::new());
            return;
        }
        // ④ 账本内换数据：日期树重拉，**保持同一天**（同记账页保留时间范围的做法）
        load_dates(ledger_id.clone());
        load_entry(ledger_id, selected_date.get_untracked());
    });

    let go_to_date = move |date: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if date.is_empty() || ledger_id.is_empty() {
            return;
        }
        clear_pending_save();
        load_entry(ledger_id, date);
    };

    // 日期选择器**选到某天就直接跳过去**（不再需要「跳转日期」按钮）。
    // `DatePicker` 只在"选中某天 / 清空"时写 `jump_date`，因此这个 Effect 不会在挂载时误触发；
    // 点「今天」会把 `jump_date` 清空 → 空串在这里直接返回，不会被带着重复跳一次。
    // （Effect 体内只读信号、只做副作用；不在这里创建任何响应式值 —— 那会被随后的重跑 dispose。）
    Effect::new(move |_| {
        let date = jump_date.get();
        if date.is_empty() {
            return;
        }
        go_to_date(date);
    });

    let go_to_today = move || {
        jump_date.set(String::new());
        let today = today_ymd();
        go_to_date(today.clone());
        // 展开今天所在的年与月
        if let Some((year, month, _)) = split_ymd(&today) {
            collapsed_years.update(|years| {
                years.remove(&year);
            });
            expanded_months.update(|months| {
                months.insert(format!("{year}-{month}"));
            });
        }
    };

    let confirm_delete = move || {
        // 显示中的条目必属当前账本；账本为空时无从删起
        let ledger_id = stores.current_ledger_id.get_untracked();
        let Some(current) = entry.get_untracked() else {
            return;
        };
        let date = current.date.clone();
        if date.is_empty() || ledger_id.is_empty() {
            return;
        }
        deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::diary::delete(&date, &ledger_id).await {
                Ok(()) => {
                    Notifier::global().success("日记已删除".to_string(), None);
                    dates.update(|items| items.retain(|item| item.date != date));
                    entry.set(None);
                    draft.set(String::new());
                    mood.set(String::new());
                    save_status.set(SaveStatus::Idle);
                    delete_open.set(false);
                    selected_date.set(String::new());
                }
                Err(error) => notify_error("删除日记失败", &error),
            }
            deleting.set(false);
        });
    };

    // 版心两块：工具栏 / 内容区各自建好视图再交给 `FeaturePage`（骨架见 components/ui/feature_page.rs）
    let toolbar = view! {
        <div class="diary-tools">
            <Button
                variant=ButtonVariant::Secondary
                on_click=move |_| go_to_today()
            >
                "今天"
            </Button>
            <Button
                variant=ButtonVariant::Secondary
                on_click=move |_| {
                    let years = dates
                        .get_untracked()
                        .iter()
                        .filter_map(|item| {
                            split_ymd(&item.date).map(|(year, _, _)| year)
                        })
                        .collect::<BTreeSet<_>>();
                    collapsed_years.set(years);
                    expanded_months.set(BTreeSet::new());
                }
            >
                "全部收起"
            </Button>
            <div class="diary-jump">
                <DatePicker value=jump_date placeholder="选择日期" />
            </div>
        </div>

        // 已保存状态与「删除」跟着工具栏走（原来在编辑器底部的那条栏里）
        <div class="diary-tools-right">
            <span class=move || {
                format!("diary-save-status {}", save_status.get().class())
            }>{move || save_status.get().label()}</span>
            <button
                type="button"
                class="ui-btn ui-btn--text-danger ui-btn--sm"
                on:click=move |_| delete_open.set(true)
            >
                <span class="ui-btn__icon">{icons::icon(Icon::Trash)}</span>
                "删除"
            </button>
        </div>
    }
    .into_any();

    // 内容区：有账本 → 日期树 + 编辑器；一个账本都没有 → 「未选择账本」引导
    // （与记账页同一套结构，复用既有 `.empty-guide*` 样式）
    let content = view! {
        <Show
            when=move || !stores.current_ledger_id.get().is_empty()
            fallback=move || {
                view! {
                    <div class="empty-guide">
                        <span class="empty-guide-icon">{icons::icon(Icon::Book)}</span>
                        <p class="empty-guide-title">"未选择账本"</p>
                        <p class="empty-guide-text">
                            "请在左上角「选择账本」里新建或选择一个账本；日记按账本分开记录。"
                        </p>
                    </div>
                }
            }
        >
            <div class="diary-body">
                <div class="diary-panel diary-panel--left">
                    <DiaryTree
                        dates=dates
                        selected_date=selected_date
                        collapsed_years=collapsed_years
                        expanded_months=expanded_months
                        initialized=tree_initialized
                        on_select=UnsyncCallback::new(move |date: String| go_to_date(date))
                    />
                </div>
                <div class="diary-panel diary-panel--right">
                    <DiaryEditor
                        entry=entry
                        draft=draft
                        mood=mood
                        on_schedule_save=UnsyncCallback::new(move |()| schedule_save())
                        on_save_now=UnsyncCallback::new(move |()| {
                            clear_pending_save();
                            do_save();
                        })
                        on_mood=UnsyncCallback::new(move |_| schedule_save())
                    />
                </div>
            </div>
        </Show>
    }
    .into_any();

    view! {
        <FeaturePage title=PAGE_TITLE toolbar=toolbar content=content />

        <Modal
            open=Signal::derive(move || delete_open.get())
            title="确认删除"
            size=ModalSize::Small
            ok_text="删除"
            cancel_text="取消"
            ok_danger=true
            ok_loading=Signal::derive(move || deleting.get())
            on_close=move || delete_open.set(false)
            on_ok=move || confirm_delete()
        >
            <p class="workspace-picker-text">
                {move || match entry.get() {
                    Some(current) => {
                        format!("确定要删除「{}」的日记吗？", current.date)
                    }
                    None => String::new(),
                }}
            </p>
        </Modal>
    }
}

// ==================================================================== 日期树

/// 一个日期节点。
#[derive(Debug, Clone, PartialEq)]
struct DayNode {
    date: String,
    day: u32,
    word_count: i64,
    mood: String,
}

/// 年 → 月 → 日的分组结果。
type TreeMap = BTreeMap<i32, BTreeMap<u32, Vec<DayNode>>>;

/// 把日期列表按年/月分组（年降序、月降序、日降序）。
fn build_tree(items: &[DiaryDateItem]) -> TreeMap {
    let mut map: TreeMap = BTreeMap::new();
    for item in items {
        let Some((year, month, day)) = split_ymd(&item.date) else {
            continue;
        };
        map.entry(year)
            .or_default()
            .entry(month)
            .or_default()
            .push(DayNode {
                date: item.date.clone(),
                day,
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

/// 左栏：年 → 月 → 日三级折叠树。
#[component]
fn DiaryTree(
    dates: RwSignal<Vec<DiaryDateItem>>,
    selected_date: RwSignal<String>,
    collapsed_years: RwSignal<BTreeSet<i32>>,
    expanded_months: RwSignal<BTreeSet<String>>,
    initialized: RwSignal<bool>,
    on_select: UnsyncCallback<String>,
) -> impl IntoView {
    // 首次拿到非空数据时把所有年份收起（只触发一次）
    Effect::new(move |_| {
        let items = dates.get();
        if items.is_empty() || initialized.get_untracked() {
            return;
        }
        let years = items
            .iter()
            .filter_map(|item| split_ymd(&item.date).map(|(year, _, _)| year))
            .collect::<BTreeSet<_>>();
        collapsed_years.set(years);
        initialized.set(true);
    });

    view! {
        <Show
            when=move || !dates.get().is_empty()
            fallback=|| {
                view! {
                    <div class="diary-tree__empty">
                        <span class="diary-tree__empty-title">"暂无日记"</span>
                        <span class="diary-tree__empty-hint">"点击「今天」开始写第一篇"</span>
                    </div>
                }
            }
        >
            <div class="diary-tree">
                {move || {
                    let tree = build_tree(&dates.get());
                    // 年降序
                    let mut years: Vec<(i32, BTreeMap<u32, Vec<DayNode>>)> =
                        tree.into_iter().collect();
                    years.sort_by_key(|(year, _)| std::cmp::Reverse(*year));
                    years
                        .into_iter()
                        .map(|(year, months)| {
                            let is_collapsed = collapsed_years.get().contains(&year);
                            let year_count: usize =
                                months.values().map(|days| days.len()).sum();
                            let mut month_entries: Vec<(u32, Vec<DayNode>)> =
                                months.into_iter().collect();
                            month_entries.sort_by_key(|(month, _)| std::cmp::Reverse(*month));
                            view! {
                                <div class="diary-tree__year">
                                    <button
                                        type="button"
                                        class="diary-tree__node diary-tree__year-node"
                                        aria-expanded=!is_collapsed
                                        on:click=move |_| {
                                            collapsed_years.update(|years| {
                                                if !years.remove(&year) {
                                                    years.insert(year);
                                                }
                                            });
                                        }
                                    >
                                        <span
                                            class="diary-tree__caret"
                                            class:is-open=!is_collapsed
                                        >
                                            {icons::icon(Icon::CaretRight)}
                                        </span>
                                        <span class="diary-tree__label">
                                            {format!("{year}年")}
                                        </span>
                                        <span class="diary-tree__count">
                                            {format!("{year_count}篇")}
                                        </span>
                                    </button>
                                    <Show when=move || !collapsed_years.get().contains(&year)>
                                        <div class="diary-tree__months">
                                            {month_entries
                                                .clone()
                                                .into_iter()
                                                .map(|(month, days)| {
                                                    let key = format!("{year}-{month}");
                                                    let is_open = expanded_months
                                                        .get()
                                                        .contains(&key);
                                                    let count = days.len();
                                                    let key_for_click = key.clone();
                                                    view! {
                                                        <div class="diary-tree__month">
                                                            <button
                                                                type="button"
                                                                class="diary-tree__node diary-tree__month-node"
                                                                aria-expanded=is_open
                                                                on:click=move |_| {
                                                                    expanded_months
                                                                        .update(|months| {
                                                                            if !months
                                                                                .remove(&key_for_click)
                                                                            {
                                                                                months
                                                                                    .insert(
                                                                                        key_for_click.clone(),
                                                                                    );
                                                                            }
                                                                        });
                                                                }
                                                            >
                                                                <span
                                                                    class="diary-tree__caret diary-tree__caret--sm"
                                                                    class:is-open=is_open
                                                                >
                                                                    {icons::icon(Icon::CaretRight)}
                                                                </span>
                                                                <span class="diary-tree__label">
                                                                    {format!("{month}月")}
                                                                </span>
                                                                <span class="diary-tree__count">
                                                                    {format!("{count}篇")}
                                                                </span>
                                                            </button>
                                                            <Show when=move || {
                                                                expanded_months.get().contains(&key)
                                                            }>
                                                                <div class="diary-tree__days">
                                                                    {days
                                                                        .clone()
                                                                        .into_iter()
                                                                        .map(|day| {
                                                                            let date = day.date.clone();
                                                                            let is_active = date
                                                                                == selected_date.get();
                                                                            let date_for_click = date
                                                                                .clone();
                                                                            let has_mood = !day
                                                                                .mood
                                                                                .is_empty();
                                                                            let mood_text = day
                                                                                .mood
                                                                                .clone();
                                                                            let day_number = day
                                                                                .day
                                                                                .to_string();
                                                                            let words = format!(
                                                                                "{}字",
                                                                                day.word_count,
                                                                            );
                                                                            view! {
                                                                                <button
                                                                                    type="button"
                                                                                    class="diary-tree__node diary-tree__day-node"
                                                                                    class:is-active=is_active
                                                                                    on:click=move |_| {
                                                                                        on_select
                                                                                            .run(date_for_click.clone())
                                                                                    }
                                                                                >
                                                                                    <span class="diary-tree__day">
                                                                                        {day_number}
                                                                                    </span>
                                                                                    <Show when=move || has_mood>
                                                                                        <span class="diary-tree__mood">
                                                                                            {mood_text.clone()}
                                                                                        </span>
                                                                                    </Show>
                                                                                    <span class="diary-tree__words">
                                                                                        {words.clone()}
                                                                                    </span>
                                                                                </button>
                                                                            }
                                                                        })
                                                                        .collect_view()}
                                                                </div>
                                                            </Show>
                                                        </div>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </Show>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </Show>
    }
}

// ==================================================================== 编辑器

/// 右栏：日记编辑器（心情 + 字数 + **始终可编辑** + 1500ms 防抖自动保存 + 删除）。
#[component]
fn DiaryEditor(
    entry: RwSignal<Option<DiaryEntry>>,
    draft: RwSignal<String>,
    mood: RwSignal<String>,

    on_schedule_save: UnsyncCallback<()>,
    on_save_now: UnsyncCallback<()>,
    on_mood: UnsyncCallback<()>,
) -> impl IntoView {
    view! {
        <Show
            when=move || entry.get().is_some()
            fallback=|| {
                view! {
                    <div class="diary-editor__empty">
                        <span class="diary-editor__empty-icon">"📖"</span>
                        <span class="diary-editor__empty-text">"选择左侧日期开始写作"</span>
                        <span class="diary-editor__empty-hint">
                            "或点上方「今天」写今天的日记"
                        </span>
                    </div>
                }
            }
        >
            <div class="diary-editor">
                <div class="diary-editor__header">
                    <div class="diary-editor__date-group">
                        <span class="diary-editor__date">
                            {move || {
                                entry.get().map(|item| format_ymd_cn(&item.date)).unwrap_or_default()
                            }}
                        </span>
                        <span class="diary-editor__weekday">
                            {move || {
                                entry.get().map(|item| weekday_cn(&item.date)).unwrap_or_default()
                            }}
                        </span>
                    </div>
                    <div class="diary-editor__meta">
                        <div class="diary-mood" role="radiogroup" aria-label="心情">
                            {MOODS
                                .iter()
                                .map(|(value, label)| {
                                    let value = value.to_string();
                                    let value_for_click = value.clone();
                                    let value_for_active = value.clone();
                                    view! {
                                        <button
                                            type="button"
                                            class="diary-mood__btn"
                                            class:is-active=move || mood.get() == value_for_active
                                            class:is-none=value.is_empty()
                                            title=*label
                                            aria-label=*label
                                            aria-pressed=move || mood.get() == value
                                            on:click=move |_| {
                                                mood.set(value_for_click.clone());
                                                on_mood.run(());
                                            }
                                        >
                                            {if value_for_click.is_empty() {
                                                "—".to_string()
                                            } else {
                                                value_for_click.clone()
                                            }}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <span class="diary-editor__words" aria-live="polite">
                            {move || format!("{}字", format::char_count(&draft.get()))}
                        </span>
                    </div>
                </div>

                // 永远是可编辑的文本域：不做 Markdown 渲染、也没有"预览/编辑"切换。
                // 正文按纯文本看待（换行原样保留），输入即触发 1500ms 防抖自动保存。
                <div class="diary-editor__body">
                    <div class="diary-editor__textarea-wrap">
                        <Textarea
                            value=draft
                            placeholder="写下今天的日记…"
                            class="diary-textarea"
                            on_input=UnsyncCallback::new(move |_| on_schedule_save.run(()))
                            on_save_shortcut=on_save_now
                        />
                    </div>
                </div>


            </div>
        </Show>
    }
}
