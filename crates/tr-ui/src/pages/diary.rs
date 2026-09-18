//! 日记页（`/diary_view`）—— P6-b 完整实现。
//!
//! ## 对照的原 Vue 文件
//!
//! | 原文件 | 本文件对应部分 |
//! |---|---|
//! | `diary_view/DiaryView.vue` | [`DiaryPage`]：工具栏（今天 / 收起全部 / 跳转日期）+ 两栏编排 |
//! | `diary_view/DiaryTree.vue` | [`DiaryTree`]：年 → 月 → 日三级折叠树（年/月降序、日降序） |
//! | `diary_view/DiaryEditor.vue` | [`DiaryEditor`]：心情 + 字数 + 编辑/预览切换 + 1500ms 防抖自动保存 + 删除 |
//! | `stores/diaryStore.ts` | 本文件直接持有信号（与其余页面一致） |
//! | `utils/markdown.ts` | [`crate::components::ui::Markdown`]（纯 Rust，先转义再拼标签） |
//!
//! ## 关键行为（逐条照抄原实现）
//!
//! * **日记不存在也能写**：`diary_get` 对不存在的日期会报错，因此读取失败时按
//!   "空条目"兜底（原 `loadEntry` 的 `tryOrFallback`），否则新日期永远打不开编辑器。
//! * **自动保存、无保存按钮**：输入或切心情后 1500ms 防抖保存；`Ctrl+S`/`Cmd+S` 立即保存。
//! * **保存状态不回落 idle**：「已保存」会一直显示（原实现如此）。
//! * **左右两处字数口径不同**：编辑器按本地草稿的码点数实时算，左树用服务端的 `wordCount`。
//! * **折叠状态**：首次拿到非空数据时全部年份收起；月份默认全收起；「收起全部」把所有年份收起。
//!
//! ## 有意与原实现的差异（详见汇报）
//!
//! * **关键词过滤未实现**：原 Go 侧的 `ListDatesByKeyword` 没有路由、没有 UI、IPC 也没有该参数
//!   （死代码），后端也没有等价命令，因此本页不做过滤（任务单允许在汇报里说明取舍）。
//! * 编辑/预览是**单栏切换**（与原实现一致，没有分栏）。
//! * 编辑器的时间防抖用 `set_timeout` + 句柄，切日期时显式清掉（原实现用 `clearTimeout`）。

use std::collections::{BTreeMap, BTreeSet};

use leptos::prelude::*;
use tr_domain::models::{DiaryDateItem, DiaryEntry};

use crate::api;
use crate::components::ui::{Button, ButtonSize, ButtonVariant, DatePicker, Modal, Textarea};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::time::{format_ymd_cn, split_ymd, today_ymd, weekday_cn};

/// 页面标题（与原 `AppLeftBar.vue` 文案一致）。
pub const PAGE_TITLE: &str = "日记管理";

/// 自动保存防抖时长（原 `setTimeout(() => doSave(), 1500)`）。
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

/// 保存状态（驱动「保存中… / 已保存 / 保存失败」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveStatus {
    Idle,
    Saving,
    Saved,
    Error,
}

impl SaveStatus {
    fn label(self) -> &'static str {
        match self {
            SaveStatus::Idle | SaveStatus::Saved => "已保存",
            SaveStatus::Saving => "保存中…",
            SaveStatus::Error => "保存失败",
        }
    }

    fn class(self) -> &'static str {
        match self {
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
    let today = today_ymd();
    let selected_date = RwSignal::new(today.clone());
    let dates = RwSignal::new(Vec::<DiaryDateItem>::new());
    let entry = RwSignal::new(Option::<DiaryEntry>::None);
    let draft = RwSignal::new(String::new());
    let mood = RwSignal::new(String::new());
    let mode = RwSignal::new(true); // true = 编辑，false = 预览
    let save_status = RwSignal::new(SaveStatus::Idle);
    let saving = RwSignal::new(false);
    let delete_open = RwSignal::new(false);
    let deleting = RwSignal::new(false);
    let jump_date = RwSignal::new(String::new());

    // 折叠状态：年份集合 + `${year}-${month}` 集合
    let collapsed_years = RwSignal::new(BTreeSet::<i32>::new());
    let expanded_months = RwSignal::new(BTreeSet::<String>::new());
    let tree_initialized = RwSignal::new(false);

    // 防抖句柄
    let timer = StoredValue::new(Option::<leptos::leptos_dom::helpers::TimeoutHandle>::None);

    let load_entry = move |date: String| {
        selected_date.set(date.clone());
        // 原实现：切换日期时把模式重置为预览
        mode.set(false);
        leptos::task::spawn_local(async move {
            match api::diary::get(&date).await {
                Ok(item) => {
                    draft.set(item.content.clone());
                    mood.set(item.mood.clone());
                    entry.set(Some(item));
                }
                Err(_) => {
                    // 不存在 → 空条目（可编辑），与原 `tryOrFallback` 的兜底一致
                    draft.set(String::new());
                    mood.set(String::new());
                    entry.set(Some(DiaryEntry {
                        date: date.clone(),
                        ..DiaryEntry::default()
                    }));
                }
            }
        });
    };

    // 首次挂载：并发拉日期表与今天的日记
    leptos::task::spawn_local(async move {
        if let Ok(items) = api::diary::list_dates().await {
            dates.set(items);
        }
    });
    load_entry(today.clone());

    let do_save = move || {
        let Some(current) = entry.get_untracked() else {
            return;
        };
        if saving.get_untracked() {
            return;
        }
        saving.set(true);
        save_status.set(SaveStatus::Saving);
        let date = current.date.clone();
        let content = draft.get_untracked();
        let mood_value = mood.get_untracked();
        leptos::task::spawn_local(async move {
            match api::diary::upsert(&date, &content, &mood_value).await {
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
            saving.set(false);
        });
    };

    let schedule_save = move || {
        // 先清掉上一次的待触发任务
        timer.update_value(|slot| {
            if let Some(handle) = slot.take() {
                handle.clear();
            }
        });
        let handle = set_timeout_with_handle(
            do_save,
            std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS),
        );
        if let Ok(handle) = handle {
            timer.set_value(Some(handle));
        }
    };

    let go_to_date = move |date: String| {
        if date.is_empty() {
            return;
        }
        timer.update_value(|slot| {
            if let Some(handle) = slot.take() {
                handle.clear();
            }
        });
        load_entry(date);
    };

    let go_to_today = move || {
        jump_date.set(String::new());
        let today = today_ymd();
        go_to_date(today.clone());
        // 展开今天所在的年与月（原 `treeRef.goToToday()`）
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
        let Some(current) = entry.get_untracked() else {
            return;
        };
        let date = current.date.clone();
        if date.is_empty() {
            return;
        }
        deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::diary::delete(&date).await {
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

    view! {
        <section class="page diary-page">
            <header class="page-header">
                <div class="diary-toolbar-left">
                    <Button size=ButtonSize::Small on_click=move |_| go_to_today()>
                        "今天"
                    </Button>
                    <Button
                        size=ButtonSize::Small
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
                        "收起全部"
                    </Button>
                    <div class="diary-jump">
                        <DatePicker value=jump_date placeholder="跳转到日期" />
                    </div>
                    <Button
                        size=ButtonSize::Small
                        on_click=move |_| {
                            let date = jump_date.get_untracked();
                            go_to_date(date);
                        }
                    >
                        "跳转"
                    </Button>
                </div>
                <div class="app-top-bar-spacer"></div>
            </header>

            <div class="page-body">
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
                            mode=mode
                            save_status=save_status
                            on_schedule_save=UnsyncCallback::new(move |()| schedule_save())
                            on_save_now=UnsyncCallback::new(move |()| {
                                timer.update_value(|slot| {
                                    if let Some(handle) = slot.take() {
                                        handle.clear();
                                    }
                                });
                                do_save();
                            })
                            on_mood=UnsyncCallback::new(move |_| schedule_save())
                            on_delete=UnsyncCallback::new(move |()| delete_open.set(true))
                        />
                    </div>
                </div>
            </div>

            <Modal
                open=Signal::derive(move || delete_open.get())
                title="确认删除"
                width=400
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
                            format!("确定要删除「{}」的日记吗？此操作不可恢复。", current.date)
                        }
                        None => String::new(),
                    }}
                </p>
            </Modal>
        </section>
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

/// 把日期列表按年/月分组（年降序、月降序、日降序 —— 与原 `DiaryTree.vue` 一致）。
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
    // 首次拿到非空数据时把所有年份收起（原 `initialized` 只触发一次）
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

/// 右栏：日记编辑器（心情 + 字数 + 编辑/预览 + 自动保存 + 删除）。
#[component]
fn DiaryEditor(
    entry: RwSignal<Option<DiaryEntry>>,
    draft: RwSignal<String>,
    mood: RwSignal<String>,
    mode: RwSignal<bool>,
    save_status: RwSignal<SaveStatus>,
    on_schedule_save: UnsyncCallback<()>,
    on_save_now: UnsyncCallback<()>,
    on_mood: UnsyncCallback<()>,
    on_delete: UnsyncCallback<()>,
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
                            "或点击工具栏「今天」开始今天的日记"
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

                <div class="diary-editor__body">
                    <Show
                        when=move || mode.get()
                        fallback=move || {
                            view! {
                                <div class="diary-editor__preview">
                                    <crate::components::ui::Markdown
                                        source=Signal::derive(move || draft.get())
                                        class="diary-markdown"
                                    />
                                </div>
                            }
                        }
                    >
                        <div class="diary-editor__textarea-wrap">
                            <Textarea
                                value=draft
                                placeholder="写下今天的日记…"
                                class="diary-textarea"
                                on_input=UnsyncCallback::new(move |_| on_schedule_save.run(()))
                                on_save_shortcut=on_save_now
                            />
                        </div>
                    </Show>
                </div>

                <div class="diary-editor__footer">
                    <div class="diary-editor__footer-left">
                        <button
                            type="button"
                            class="ui-btn ui-btn--text ui-btn--sm"
                            title="Ctrl+S 保存"
                            on:click=move |_| mode.update(|value| *value = !*value)
                        >
                            <span class="ui-btn__icon">
                                {move || {
                                    if mode.get() {
                                        icons::icon(Icon::Eye)
                                    } else {
                                        icons::icon(Icon::Edit)
                                    }
                                }}
                            </span>
                            {move || if mode.get() { "预览" } else { "编辑" }}
                        </button>
                    </div>
                    <div class="diary-editor__footer-right">
                        <span class=move || {
                            format!("diary-save-status {}", save_status.get().class())
                        }>{move || save_status.get().label()}</span>
                        <button
                            type="button"
                            class="ui-btn ui-btn--text-danger ui-btn--sm"
                            on:click=move |_| on_delete.run(())
                        >
                            <span class="ui-btn__icon">{icons::icon(Icon::Trash)}</span>
                            "删除"
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// 让 `ButtonVariant` 的引用不被裁剪（本页按钮统一走 [`Button`]）。
#[allow(dead_code)]
fn primary_variant() -> ButtonVariant {
    ButtonVariant::Primary
}
