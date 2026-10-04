//! 待办页（`/todo_view`）—— 两个子功能共用一个版心与左侧图标条。
//!
//! | 组成 | 职责 |
//! |---|---|
//! | [`TodoPage`] | 只决定"当前是哪个子功能"（定义见 [`TodoSub`]） |
//! | [`TodoSubRail`] | 子功能图标条（`FeaturePage` 的 `rail` 插槽内容） |
//! | [`record_view`] | **记录**：工具栏 = 待办视图 / 四象限图 +「新建卡片」；卡片竖直排列 |
//! | [`history_view`] | **历史**：已完成的事项（含主题名）+ 进度记录弹窗（无工具栏） |
//! | 状态组织 | 本文件直接持有信号（不引入 store；与其余页面一致） |
//!
//! ## 两处口径写在这里，免得下次再猜
//!
//! * **「完成」不是搬运**：状态置成 `done` 之后，卡片视图（只取进行中）里没有它、历史里
//!   有它 —— 数据只有一份，所以历史里留了一条「退回进行中」的退路（点错了还能回来）。
//! * **时间只到日**（`YYYY-MM-DD`，与事件日期 / 股票委托时间同口径）；紧急度与重要度是**六档**
//!   （低 / 中低 / 次低 / 次高 / 中高 / 高 —— 存的是既有整数 `-5 -3 -1 1 3 5`，界面只显示文案），
//!   四象限图直接把这两个值当坐标：**+紧急向右、+重要向上，0 是原点**。
//!
//! 切子功能会**重建**视图（信号随组件 owner 释放），因此来回切会重新拉数据 —— 与"切页面"一致。

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::consts;
use tr_domain::dto::{TodoCardDto, TodoHistoryDto, TodoItemDto, TodoProgressDto};

use crate::api;
use crate::components::ui::{
    watch_canvas_size, Button, ButtonSize, ButtonVariant, DatePicker, DragSortItem, DragSortState,
    Empty, FeaturePage, IconButton, IconButtonVariant, Input, Modal, ModalSize, Popconfirm, Select,
    SelectOption, TabItem, TabPane, Tabs,
};
use crate::error_handler::notify_error;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{format_timestamp, today_ymd, ymd_to_seconds, DAY_SECONDS};

/// 页面标题（固定文案，改动即影响界面）。侧栏条目名也用这一个来源。
pub const PAGE_TITLE: &str = "待办";

/// 待办页的两个子功能（左侧图标条切换，顺序即渲染顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoSub {
    /// 记录：卡片 + 事项 + 进度记录（含四象限图）
    Record,
    /// 历史：已完成的事项
    History,
}

impl TodoSub {
    /// 图标条顺序（顺序即渲染顺序）。
    pub const ALL: [TodoSub; 2] = [Self::Record, Self::History];

    /// 子功能名 —— 同时用作悬停提示与 `aria-label`（也是 fixtures 点它的可访问名）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Record => "记录",
            Self::History => "历史",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::Record => Icon::FileText,
            Self::History => Icon::History,
        }
    }
}

/// 记录子功能里的两个分栏（工具栏左侧 `Tabs` 的 key，固定文案，改动即影响界面）。
const TAB_BOARD: &str = "board";
const TAB_QUADRANT: &str = "quadrant";

/// 待办页容器：只决定"当前是哪个子功能"。
#[component]
pub fn TodoPage() -> impl IntoView {
    let sub = RwSignal::new(TodoSub::Record);
    view! {
        {move || match sub.get() {
            TodoSub::Record => record_view(sub),
            TodoSub::History => history_view(sub),
        }}
    }
}

/// 子功能图标条 —— `FeaturePage` 的 `rail` 插槽内容（外层 `.page-rail` 由它渲染）。
#[component]
pub fn TodoSubRail(sub: RwSignal<TodoSub>) -> impl IntoView {
    view! {
        <nav class="page-rail-nav" aria-label="待办子功能">
            {TodoSub::ALL
                .iter()
                .map(|item| {
                    let value = *item;
                    view! {
                        <button
                            type="button"
                            class="page-rail-btn"
                            class:is-active=move || sub.get() == value
                            title=item.label()
                            aria-label=item.label()
                            on:click=move |_| sub.set(value)
                        >
                            <span class="page-rail-btn-icon">{icons::icon(item.icon())}</span>
                        </button>
                    }
                })
                .collect_view()}
        </nav>
    }
}

// ==================================================================== 子功能一：记录

/// 记录子功能：工具栏 = 两个分栏 + 新建卡片；内容 = 卡片（竖直排列）或四象限图。
fn record_view(sub: RwSignal<TodoSub>) -> AnyView {
    let stores = AppStores::global();

    let tab = RwSignal::new(TAB_BOARD.to_string());
    let cards = RwSignal::new(Vec::<TodoCardDto>::new());
    let loading = RwSignal::new(false);

    // ---- 新建卡片 ----
    let card_open = RwSignal::new(false);
    let card_title = RwSignal::new(String::new());
    let card_saving = RwSignal::new(false);
    // 删卡片确认（弹窗说清"事项与进度记录一起删"）
    let card_delete = RwSignal::new(Option::<(String, String)>::None);
    let card_deleting = RwSignal::new(false);

    // ---- 事项弹窗（新建 / 编辑共用一份表单；`item_editing` 为空 = 新建）----
    let item_open = RwSignal::new(false);
    let item_card = RwSignal::new(String::new());
    let item_editing = RwSignal::new(String::new());
    let item_title = RwSignal::new(String::new());
    let item_start = RwSignal::new(String::new());
    let item_due = RwSignal::new(String::new());
    let item_urgency = RwSignal::new(consts::TODO_LEVEL_DEFAULT.to_string());
    let item_importance = RwSignal::new(consts::TODO_LEVEL_DEFAULT.to_string());
    let item_saving = RwSignal::new(false);

    // ---- 进度记录：同一时刻只展开一条（列表本来就是一屏），因此输入框只留一份 ----
    let expanded = RwSignal::new(String::new());
    let progress_text = RwSignal::new(String::new());
    let progress_saving = RwSignal::new(false);

    let no_ledger = move || stores.current_ledger_id.get().is_empty();

    // ---- 卡片拖动排序：先把新顺序写进信号，再逐条落库 ----
    let card_drag = DragSortState::new();
    let reorder_cards = move |from: usize, to: usize| {
        let current = cards.get_untracked();
        if from >= current.len() || to >= current.len() || from == to {
            return;
        }
        let mut reordered = current.clone();
        let moved = reordered.remove(from);
        reordered.insert(to, moved);
        // 拖动会挪动一整段，所以按新下标整体写回（卡片数量很小，不做"只发改动项"的优化）
        let payload: Vec<(String, i32)> = reordered
            .iter()
            .enumerate()
            .map(|(index, card)| (card.id.clone(), index as i32))
            .collect();
        cards.set(reordered);
        leptos::task::spawn_local(async move {
            for (id, index) in payload {
                if let Err(error) = api::todo::card_sort(&id, index).await {
                    notify_error("保存卡片排序失败", &error);
                }
            }
        });
    };
    let drop_card: UnsyncCallback<(usize, usize)> =
        UnsyncCallback::new(move |(from, to)| reorder_cards(from, to));

    // 拉卡片视图。每次改动后整表重拉：一个账本的待办量级很小，局部更新只会更啰嗦。
    let load = move |_: ()| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            cards.set(Vec::new());
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::todo::cards(&ledger_id).await {
                Ok(list) => cards.set(list),
                Err(error) => notify_error("读取待办失败", &error),
            }
            loading.set(false);
        });
    };

    // 账本变化（含首屏）→ 重新拉；顺手收起进度展开
    Effect::new(move |_: Option<()>| {
        stores.current_ledger_id.get();
        expanded.set(String::new());
        load(());
    });

    let open_card_modal = move || {
        if no_ledger() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        card_title.set(String::new());
        card_open.set(true);
    };

    let save_card = move || {
        if card_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        let title = card_title.get_untracked().trim().to_string();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        if title.is_empty() {
            Notifier::global().error("请输入卡片主题", None);
            return;
        }
        card_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::todo::card_create(&ledger_id, &title).await {
                Ok(_) => {
                    card_open.set(false);
                    card_title.set(String::new());
                    Notifier::global().success(format!("卡片「{title}」已创建"), None);
                    load(());
                }
                Err(error) => notify_error("创建卡片失败", &error),
            }
            card_saving.set(false);
        });
    };

    let confirm_delete_card = move || {
        let Some((id, _)) = card_delete.get_untracked() else {
            return;
        };
        if card_deleting.get_untracked() {
            return;
        }
        card_deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::todo::card_delete(&id).await {
                Ok(()) => {
                    card_delete.set(None);
                    Notifier::global().success("卡片已删除", None);
                    load(());
                }
                Err(error) => notify_error("删除卡片失败", &error),
            }
            card_deleting.set(false);
        });
    };

    // 回调统一收成 `UnsyncCallback`（Copy），好往下面的纯函数里传。
    let open_new_item: UnsyncCallback<String> = UnsyncCallback::new(move |card_id: String| {
        open_item_form(
            item_open,
            item_card,
            item_editing,
            item_title,
            item_start,
            item_due,
            item_urgency,
            item_importance,
            card_id,
            None,
        )
    });
    let open_edit_item: UnsyncCallback<(String, TodoItemDto)> =
        UnsyncCallback::new(move |(card_id, item): (String, TodoItemDto)| {
            open_item_form(
                item_open,
                item_card,
                item_editing,
                item_title,
                item_start,
                item_due,
                item_urgency,
                item_importance,
                card_id,
                Some(item),
            )
        });
    let ask_delete_card: UnsyncCallback<(String, String)> =
        UnsyncCallback::new(move |payload: (String, String)| card_delete.set(Some(payload)));

    let save_item = move || {
        if item_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        let title = item_title.get_untracked().trim().to_string();
        if title.is_empty() {
            Notifier::global().error("请输入事项", None);
            return;
        }
        // 下拉里的值是字符串；解析失败回落到默认档（选项只有六档，正常不会失败）
        let urgency = item_urgency
            .get_untracked()
            .parse::<i32>()
            .unwrap_or(consts::TODO_LEVEL_DEFAULT);
        let importance = item_importance
            .get_untracked()
            .parse::<i32>()
            .unwrap_or(consts::TODO_LEVEL_DEFAULT);
        let card_id = item_card.get_untracked();
        let editing = item_editing.get_untracked();
        let start = item_start.get_untracked();
        let due = item_due.get_untracked();
        item_saving.set(true);
        leptos::task::spawn_local(async move {
            let result = if editing.is_empty() {
                api::todo::item_create(
                    &ledger_id, &card_id, &title, &start, &due, urgency, importance,
                )
                .await
                .map(|_| ())
            } else {
                api::todo::item_update(
                    &ledger_id, &editing, &title, &start, &due, urgency, importance,
                )
                .await
                .map(|_| ())
            };
            match result {
                Ok(()) => {
                    item_open.set(false);
                    Notifier::global().success("事项已保存", None);
                    load(());
                }
                Err(error) => notify_error("保存事项失败", &error),
            }
            item_saving.set(false);
        });
    };

    let complete: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::todo::item_status(&ledger_id, &item_id, consts::TODO_STATUS_DONE).await {
                Ok(_) => {
                    Notifier::global().success("已完成，已移到历史", None);
                    load(());
                }
                Err(error) => notify_error("更新状态失败", &error),
            }
        });
    });

    let remove_item: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        leptos::task::spawn_local(async move {
            match api::todo::item_delete(&item_id).await {
                Ok(()) => {
                    Notifier::global().success("事项已删除", None);
                    load(());
                }
                Err(error) => notify_error("删除事项失败", &error),
            }
        });
    });

    let toggle_progress: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        expanded.update(|current| {
            if *current == item_id {
                current.clear();
            } else {
                *current = item_id.clone();
            }
        });
        progress_text.set(String::new());
    });

    let add_progress: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        if progress_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        let content = progress_text.get_untracked().trim().to_string();
        if ledger_id.is_empty() {
            return;
        }
        if content.is_empty() {
            Notifier::global().error("请输入进度", None);
            return;
        }
        progress_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::todo::progress_add(&ledger_id, &item_id, &content).await {
                Ok(_) => {
                    progress_text.set(String::new());
                    load(());
                }
                Err(error) => notify_error("记录进度失败", &error),
            }
            progress_saving.set(false);
        });
    });

    let remove_progress: UnsyncCallback<String> =
        UnsyncCallback::new(move |progress_id: String| {
            leptos::task::spawn_local(async move {
                match api::todo::progress_delete(&progress_id).await {
                    Ok(()) => load(()),
                    Err(error) => notify_error("删除进度失败", &error),
                }
            });
        });

    // 进度打勾 / 取消打勾（打勾后正文加删除线）
    let toggle_progress_done: UnsyncCallback<(String, bool)> =
        UnsyncCallback::new(move |(progress_id, done): (String, bool)| {
            leptos::task::spawn_local(async move {
                match api::todo::progress_done(&progress_id, done).await {
                    Ok(()) => load(()),
                    Err(error) => notify_error("更新进度状态失败", &error),
                }
            });
        });

    let toolbar = view! {
        <Tabs
            active=tab
            items=vec![
                TabItem::new(TAB_BOARD, "待办视图"),
                TabItem::new(TAB_QUADRANT, "四象限图"),
            ]
        />
        // 「新建卡片」是**待办视图**那张表的主动作（卡片 = 主题）：四象限图上的点全部来自
        // 已有卡片，切过去时把它收起来。用常驻 DOM + `is-hidden` 而不是 `Show` —— 后者会把
        // 闭包逼成 `FnOnce`（见 CSS 里那条通用隐藏类的注释）。
        <div class="todo-toolbar-actions" class:is-hidden=move || tab.get() == TAB_QUADRANT>
            <Button
                variant=ButtonVariant::Primary
                size=ButtonSize::Small
                disabled=Signal::derive(no_ledger)
                on_click=move |_| open_card_modal()
            >
                "新建卡片"
            </Button>
        </div>
    }
    .into_any();

    let content = view! {
        <div class="todo-body">
            // ---- 待办视图：竖直排列的卡片 ----
            <TabPane active=tab key=TAB_BOARD class="todo-pane">
                {move || {
                    let list = cards.get();
                    if list.is_empty() {
                        if loading.get() {
                            return view! { <div class="todo-loading">"正在加载…"</div> }
                                .into_any();
                        }
                        return view! {
                            <Empty
                                title="还没有卡片"
                                description="卡片是一个「主题」。先建一张，再往里加事项。"
                                icon=Icon::CheckCircle
                            />
                        }
                            .into_any();
                    }
                    view! {
                        <div class="todo-cards">
                            {list
                                .into_iter()
                                .enumerate()
                                .map(|(index, card)| {
                                    card_view(
                                        card,
                                        index,
                                        card_drag,
                                        drop_card,
                                        expanded,
                                        progress_text,
                                        progress_saving,
                                        open_new_item,
                                        open_edit_item,
                                        ask_delete_card,
                                        complete,
                                        remove_item,
                                        toggle_progress,
                                        add_progress,
                                        remove_progress,
                                        toggle_progress_done,
                                    )
                                })
                                .collect_view()}
                        </div>
                    }
                        .into_any()
                }}
            </TabPane>

            // ---- 四象限图 ----
            <TabPane active=tab key=TAB_QUADRANT class="todo-pane">
                {move || quadrant_view(&cards.get())}
            </TabPane>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="todo-page"
            rail=view! { <TodoSubRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
        />

        // 新建卡片
        <Modal
            open=card_open
            title="新建卡片"
            size=ModalSize::Small
            ok_text="创建"
            cancel_text="取消"
            ok_loading=card_saving
            on_close=move || card_open.set(false)
            on_ok=move || save_card()
        >
            <div class="modal-form-item">
                <p class="modal-form-label">"卡片主题"</p>
                <Input
                    value=card_title
                    placeholder="如：需求开发"
                    maxlength=200
                    on_enter=move || save_card()
                />
            </div>
        </Modal>

        // 删卡片（危险确认：说清连带删掉什么）
        <Modal
            open=Signal::derive(move || card_delete.get().is_some())
            title="删除卡片"
            size=ModalSize::Small
            ok_text="删除"
            cancel_text="取消"
            ok_danger=true
            ok_loading=card_deleting
            on_close=move || card_delete.set(None)
            on_ok=move || confirm_delete_card()
        >
            <p class="todo-modal-text">
                {move || match card_delete.get() {
                    Some((_, title)) => {
                        format!(
                            "删除卡片「{title}」？它下面的事项与进度记录会一起删掉，历史里属于它的事项也会消失。此操作不可恢复。",
                        )
                    }
                    None => String::new(),
                }}
            </p>
        </Modal>

        // 新建 / 编辑事项
        <Modal
            open=item_open
            title=Signal::derive(move || {
                if item_editing.get().is_empty() {
                    "添加事项".to_string()
                } else {
                    "编辑事项".to_string()
                }
            })
            size=ModalSize::Medium
            ok_text="保存"
            cancel_text="取消"
            ok_loading=item_saving
            on_close=move || item_open.set(false)
            on_ok=move || save_item()
        >
            <div class="modal-form-item">
                <p class="modal-form-label">"事项"</p>
                <Input value=item_title placeholder="如：AI辅助研发" maxlength=500 />
            </div>
            <div class="todo-form-row">
                <div class="modal-form-item">
                    <p class="modal-form-label">"开始时间"</p>
                    <DatePicker value=item_start placeholder="可选" allow_clear=true />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"截止时间"</p>
                    <DatePicker value=item_due placeholder="可选" allow_clear=true />
                </div>
            </div>
            <div class="todo-form-row">
                <div class="modal-form-item">
                    <p class="modal-form-label">"紧急度"</p>
                    // 六档：面板 260px 装得下全部选项，不需要搜索框
                    <Select value=item_urgency options=level_options() />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"重要度"</p>
                    <Select value=item_importance options=level_options() />
                </div>
            </div>
        </Modal>
    }
    .into_any()
}

/// 打开事项表单；`item` 为空 = 新建。
#[allow(clippy::too_many_arguments)]
fn open_item_form(
    item_open: RwSignal<bool>,
    item_card: RwSignal<String>,
    item_editing: RwSignal<String>,
    item_title: RwSignal<String>,
    item_start: RwSignal<String>,
    item_due: RwSignal<String>,
    item_urgency: RwSignal<String>,
    item_importance: RwSignal<String>,
    card_id: String,
    item: Option<TodoItemDto>,
) {
    item_card.set(card_id);
    match item {
        Some(item) => {
            item_editing.set(item.id);
            item_title.set(item.title);
            item_start.set(item.start_date);
            item_due.set(item.due_date);
            item_urgency.set(item.urgency.to_string());
            item_importance.set(item.importance.to_string());
        }
        None => {
            item_editing.set(String::new());
            item_title.set(String::new());
            item_start.set(String::new());
            item_due.set(String::new());
            item_urgency.set(consts::TODO_LEVEL_DEFAULT.to_string());
            item_importance.set(consts::TODO_LEVEL_DEFAULT.to_string());
        }
    }
    item_open.set(true);
}

/// 六档的下拉选项（紧急度与重要度共用）：值是既有整数，界面显示档位文案。
fn level_options() -> Vec<SelectOption> {
    consts::TODO_LEVELS
        .iter()
        .zip(consts::TODO_LEVEL_LABELS)
        .map(|(level, label)| SelectOption::new(level.to_string(), label))
        .collect()
}

/// 开始时间「已过 N 天」（`None` = 没填开始时间，或开始日期还没到）。
fn elapsed_days(start_date: &str) -> Option<i64> {
    if start_date.is_empty() {
        return None;
    }
    let start = ymd_to_seconds(start_date)?;
    let today = ymd_to_seconds(&today_ymd())?;
    let days = (today - start) / DAY_SECONDS;
    (days >= 0).then_some(days)
}

/// 一张卡片：主题 + 进行中的事项 + 「添加事项」。
///
/// 卡片是这一页唯一的面（Paper + 发丝线）；事项行**不再各自成卡**，只用发丝线分隔
/// （嵌套卡片是页面里最容易读成"一堆盒子"的写法）。
///
/// 拖动排序的抓手是**表头**：`DragSortItem` 只包住表头那一行，卡片里的输入框
/// （进度记录）因此不会落在可拖区域内 —— 否则在输入框里框选文字会被浏览器当成"拖卡片"。
#[allow(clippy::too_many_arguments)]
fn card_view(
    card: TodoCardDto,
    index: usize,
    drag: DragSortState,
    on_drop: UnsyncCallback<(usize, usize)>,
    expanded: RwSignal<String>,
    progress_text: RwSignal<String>,
    progress_saving: RwSignal<bool>,
    open_new_item: UnsyncCallback<String>,
    open_edit_item: UnsyncCallback<(String, TodoItemDto)>,
    ask_delete_card: UnsyncCallback<(String, String)>,
    complete: UnsyncCallback<String>,
    remove_item: UnsyncCallback<String>,
    toggle_progress: UnsyncCallback<String>,
    add_progress: UnsyncCallback<String>,
    remove_progress: UnsyncCallback<String>,
    toggle_progress_done: UnsyncCallback<(String, bool)>,
) -> AnyView {
    let count = card.items.len();
    let delete_id = card.id.clone();
    let delete_title = card.title.clone();
    let new_item_card = card.id.clone();
    let items = card.items;
    view! {
        <section class="todo-card">
            <DragSortItem index=index state=drag on_drop=on_drop class="todo-card__head">
                <span class="ui-drag-handle" title="拖动排序">
                    {icons::icon(Icon::DragHandle)}
                </span>
                <h3 class="todo-card__title">{card.title.clone()}</h3>
                <span class="todo-card__count">{format!("{count} 项进行中")}</span>
                <div class="todo-card__actions">
                    <Button
                        variant=ButtonVariant::Secondary
                        size=ButtonSize::Small
                        on_click=move |_| open_new_item.run(new_item_card.clone())
                    >
                        "添加事项"
                    </Button>
                    <IconButton
                        variant=IconButtonVariant::Danger
                        label="删除卡片"
                        on_click=UnsyncCallback::new(move |()| {
                            ask_delete_card.run((delete_id.clone(), delete_title.clone()))
                        })
                    >
                        {icons::icon(Icon::Trash)}
                    </IconButton>
                </div>
            </DragSortItem>
            {if items.is_empty() {
                view! { <p class="todo-card__empty">"这张卡片下还没有进行中的事项。"</p> }
                    .into_any()
            } else {
                view! {
                    <ul class="todo-items">
                        {items
                            .into_iter()
                            .map(|item| item_row(
                                item,
                                expanded,
                                progress_text,
                                progress_saving,
                                open_edit_item,
                                complete,
                                remove_item,
                                toggle_progress,
                                add_progress,
                                remove_progress,
                                toggle_progress_done,
                            ))
                            .collect_view()}
                    </ul>
                }
                    .into_any()
            }}
        </section>
    }
    .into_any()
}

/// 一条事项：勾选 + 事项 + 起止与强弱 + 进度展开 + 编辑/删除。
///
/// **点整行也能收起/展开进度**（进度按钮在行尾，行本身是更大的目标）；
/// 勾选与行尾动作都在自己的 `on:click` 里 `stop_propagation`，否则点它们会顺带折叠进度。
#[allow(clippy::too_many_arguments)]
fn item_row(
    item: TodoItemDto,
    expanded: RwSignal<String>,
    progress_text: RwSignal<String>,
    progress_saving: RwSignal<bool>,
    open_edit_item: UnsyncCallback<(String, TodoItemDto)>,
    complete: UnsyncCallback<String>,
    remove_item: UnsyncCallback<String>,
    toggle_progress: UnsyncCallback<String>,
    add_progress: UnsyncCallback<String>,
    remove_progress: UnsyncCallback<String>,
    toggle_progress_done: UnsyncCallback<(String, bool)>,
) -> AnyView {
    let id = item.id.clone();
    let card_id = item.card_id.clone();
    let progress_count = item.progress.len();
    let progress = item.progress.clone();
    let for_expand = id.clone();
    let for_complete = id.clone();
    let for_delete = id.clone();
    let for_row = id.clone();
    let for_toggle = id.clone();
    let for_add = id.clone();
    let for_show = id.clone();
    let for_label = id.clone();
    let for_edit_item = item.clone();

    let dates = match (item.start_date.is_empty(), item.due_date.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{} 起", item.start_date),
        (true, false) => format!("{} 截止", item.due_date),
        (false, false) => format!("{} → {}", item.start_date, item.due_date),
    };
    // 有开始时间才配「已过 N 天」这个标签（开始日期还没到就不显示）
    let elapsed = elapsed_days(&item.start_date);

    view! {
        <li
            class="todo-item"
            class:is-expanded=move || expanded.get() == for_expand
            on:click=move |_| toggle_progress.run(for_row.clone())
        >
            <button
                type="button"
                class="todo-item__check"
                title="标记为已完成"
                aria-label="标记为已完成"
                on:click=move |event: web_sys::MouseEvent| {
                    event.stop_propagation();
                    complete.run(for_complete.clone())
                }
            >
                {icons::icon(Icon::Check)}
            </button>

            <div class="todo-item__main">
                // 两行：第一行事项描述，第二行两档标签 + 已过天数 + 时间区间
                // （与四象限图那个弹窗里的行同一套写法）
                <span class="todo-item__title">{item.title.clone()}</span>
                <span class="todo-item__levels">
                    <span class="todo-level" title="紧急度">
                        {format!("紧急 {}", consts::todo_level_label(item.urgency))}
                    </span>
                    <span class="todo-level" title="重要度">
                        {format!("重要 {}", consts::todo_level_label(item.importance))}
                    </span>
                    {elapsed
                        .map(|days| {
                            view! {
                                <span class="todo-level todo-level--elapsed">
                                    {format!("已过 {days} 天")}
                                </span>
                            }
                        })}
                    {(!dates.is_empty())
                        .then(|| view! { <span class="todo-item__dates">{dates.clone()}</span> })}
                </span>
                <Show when=move || expanded.get() == for_show>
                    {progress_panel(
                        progress.clone(),
                        progress_text,
                        progress_saving,
                        for_add.clone(),
                        add_progress,
                        remove_progress,
                        toggle_progress_done,
                    )}
                </Show>
            </div>

            <div
                class="todo-item__actions"
                on:click=move |event: web_sys::MouseEvent| event.stop_propagation()
            >
                <Button
                    variant=ButtonVariant::Text
                    size=ButtonSize::Small
                    class="todo-item__progress-btn"
                    on_click=move |_| toggle_progress.run(for_toggle.clone())
                >
                    {move || {
                        if expanded.get() == for_label {
                            format!("收起（{progress_count}）")
                        } else {
                            format!("进度（{progress_count}）")
                        }
                    }}
                </Button>
                <IconButton
                    label="编辑事项"
                    on_click=UnsyncCallback::new(move |()| {
                        open_edit_item.run((card_id.clone(), for_edit_item.clone()))
                    })
                >
                    {icons::icon(Icon::Edit)}
                </IconButton>
                <Popconfirm
                    title="删除这条事项？"
                    description="它的进度记录会一起删掉。"
                    ok_text="删除"
                    // 行尾的动作簇贴着卡片右缘：气泡必须右对齐，否则整个面板跑到窗口外点不到
                    class="ui-popconfirm--end"
                    on_confirm=UnsyncCallback::new(move |()| remove_item.run(for_delete.clone()))
                >
                    <IconButton variant=IconButtonVariant::Danger label="删除事项">
                        {icons::icon(Icon::Trash)}
                    </IconButton>
                </Popconfirm>
            </div>
        </li>
    }
    .into_any()
}

/// 展开的进度记录：历史条目（打勾 + 时间 + 正文 + 删除）+ 一条新的输入。
///
/// 面板自己吃掉点击：行本身是"点一下收起进度"，而面板里有输入框与按钮 ——
/// 点它们不该顺手把面板收起来。
fn progress_panel(
    progress: Vec<TodoProgressDto>,
    progress_text: RwSignal<String>,
    progress_saving: RwSignal<bool>,
    item_id: String,
    add_progress: UnsyncCallback<String>,
    remove_progress: UnsyncCallback<String>,
    toggle_done: UnsyncCallback<(String, bool)>,
) -> AnyView {
    let item_for_add = item_id.clone();
    let empty = progress.is_empty();
    view! {
        <div
            class="todo-progress"
            on:click=move |event: web_sys::MouseEvent| event.stop_propagation()
        >
            {if empty {
                view! { <p class="todo-progress__empty">"还没有进度记录。"</p> }.into_any()
            } else {
                view! {
                    <ol class="todo-progress__list">
                        {progress
                            .into_iter()
                            .map(|row| {
                                let remove_id = row.id.clone();
                                let done_id = row.id.clone();
                                let done = row.done;
                                let row_class = if done {
                                    "todo-progress__row is-done"
                                } else {
                                    "todo-progress__row"
                                };
                                view! {
                                    <li class=row_class>
                                        <button
                                            type="button"
                                            class="todo-progress__check"
                                            title=if done { "取消打勾" } else { "标记这条进度已完成" }
                                            aria-label=if done { "取消打勾" } else { "标记这条进度已完成" }
                                            on:click=move |event: web_sys::MouseEvent| {
                                                event.stop_propagation();
                                                toggle_done.run((done_id.clone(), !done))
                                            }
                                        >
                                            {icons::icon(Icon::Check)}
                                        </button>
                                        <span class="todo-progress__time">
                                            {format_timestamp(row.created_at, "MM-DD HH:mm")}
                                        </span>
                                        <span class="todo-progress__text">{row.content.clone()}</span>
                                        <IconButton
                                            variant=IconButtonVariant::Danger
                                            compact=true
                                            label="删除进度"
                                            on_click=UnsyncCallback::new(move |()| {
                                                remove_progress.run(remove_id.clone())
                                            })
                                        >
                                            {icons::icon(Icon::Close)}
                                        </IconButton>
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ol>
                }
                    .into_any()
            }}
            <div class="todo-progress__add">
                <Input
                    value=progress_text
                    placeholder="写一条进度…"
                    maxlength=2000
                    on_enter=move || add_progress.run(item_for_add.clone())
                />
                <Button
                    variant=ButtonVariant::Secondary
                    size=ButtonSize::Small
                    loading=progress_saving
                    on_click=move |_| add_progress.run(item_id.clone())
                >
                    "添加"
                </Button>
            </div>
        </div>
    }
    .into_any()
}

// ==================================================================== 四象限图

/// 四个象限（四象限管理法则）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quadrant {
    /// 重要且紧急
    Now,
    /// 重要但不紧急
    Plan,
    /// 不重要且不紧急
    Less,
    /// 紧急但不重要
    Delegate,
}

impl Quadrant {
    /// 象限名（画面上的水印字，也是弹窗标题的前半）
    fn name(self) -> &'static str {
        match self {
            Self::Now => "马上做",
            Self::Plan => "计划做",
            Self::Less => "减少做",
            Self::Delegate => "授权做",
        }
    }

    /// 象限的含义（弹窗标题的后半）：图上不再写这行说明，弹窗里要写 —— 点开就是为了看它。
    fn meaning(self) -> &'static str {
        match self {
            Self::Now => "重要且紧急",
            Self::Plan => "重要但不紧急",
            Self::Less => "不重要且不紧急",
            Self::Delegate => "紧急但不重要",
        }
    }

    /// 事项落在哪个象限：纵轴 = 重要度、横轴 = 紧急度。
    /// 两轴是六档（`TODO_LEVELS`：-5 / -3 / -1 / 1 / 3 / 5），**没有 0** —— 所以"正负"
    /// 就是判据，不用处理"正好压在轴上"这类边界。
    fn of(urgency: i32, importance: i32) -> Self {
        match (importance > 0, urgency > 0) {
            (true, true) => Self::Now,
            (true, false) => Self::Plan,
            (false, false) => Self::Less,
            (false, true) => Self::Delegate,
        }
    }

    /// 水印里唯一带色的那一格（见 CSS 的 `.todo-quadrant__name--now`）
    fn is_accent(self) -> bool {
        matches!(self, Self::Now)
    }
}

/// 四象限图里的一个事项：图上是一个点，弹窗里是一行。
#[derive(Debug, Clone, PartialEq, Eq)]
struct QuadrantItem {
    /// 事项 id（只用来给点取一个稳定的抖动偏移）
    id: String,
    /// 主题（卡片名）—— 图上没有它的位置，弹窗里补上
    card_title: String,
    title: String,
    start_date: String,
    due_date: String,
    urgency: i32,
    importance: i32,
}

impl QuadrantItem {
    /// 它落在哪个象限
    fn quadrant(&self) -> Quadrant {
        Quadrant::of(self.urgency, self.importance)
    }
}

/// 弹窗里列哪些、按什么顺序：该象限的**进行中事项**，先按紧急度降序、再按重要度降序。
///
/// `sort_by` 是**稳定**排序 —— 同紧急同重要的两条保持卡片视图里的顺序（卡片序 → 事项序），
/// 同一份数据每次打开弹窗都排成一个样。
fn quadrant_rows(items: &[QuadrantItem], open: Option<Quadrant>) -> Vec<QuadrantItem> {
    let Some(quad) = open else {
        return Vec::new();
    };
    let mut rows: Vec<QuadrantItem> = items
        .iter()
        .filter(|item| item.quadrant() == quad)
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        b.urgency
            .cmp(&a.urgency)
            .then(b.importance.cmp(&a.importance))
    });
    rows
}

/// 一个象限名：静置是水印，**鼠标悬停 / 键盘聚焦时亮起**（见 CSS），点击打开该象限的事项列表。
///
/// 它是 SVG 里的 `<text role="button">` 而不是叠一层 HTML 按钮：hover 的正是"这几个字"本身，
/// 也不用把象限名的排布算两遍。代价是 `<svg>` 不能再挂 `role="img"`（img 的子元素在无障碍树里
/// 是装饰、拿不到焦点），改成 `role="group"` + 同一句 `aria-label`。
fn quadrant_name(
    quad: Quadrant,
    count: usize,
    x: f64,
    y: f64,
    name_px: f64,
    name_dy: f64,
    open: RwSignal<Option<Quadrant>>,
) -> AnyView {
    // 悬停/聚焦的"亮起"色由 CSS 给：这里只管挂类名（「马上做」是唯一带色的那格）
    let class = if quad.is_accent() {
        "todo-quadrant__name todo-quadrant__name--now"
    } else {
        "todo-quadrant__name"
    };
    view! {
        <text
            class=class
            x=x
            y=y + name_dy
            font-size=name_px
            text-anchor="middle"
            role="button"
            tabindex="0"
            aria-label=format!("{} · {} 项进行中", quad.name(), count)
            on:click=move |_| open.set(Some(quad))
            on:keydown=move |event: web_sys::KeyboardEvent| {
                // 键盘激活：Enter / Space 与按钮一致（`role=button` 的约定）
                let key = event.key();
                if key == "Enter" || key == " " {
                    event.prevent_default();
                    open.set(Some(quad));
                }
            }
        >
            <title>
                {format!("{} · {}（{} 项进行中，点击查看列表）", quad.name(), quad.meaning(), count)}
            </title>
            {quad.name()}
        </text>
    }
    .into_any()
}

/// 弹窗里的一行：**第一行**是事项描述，**第二行**是主题 + 紧急 / 重要两档 + 起止日期。
fn quadrant_item_row(item: &QuadrantItem) -> AnyView {
    let dates = match (item.start_date.is_empty(), item.due_date.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{} 起", item.start_date),
        (true, false) => format!("{} 截止", item.due_date),
        (false, false) => format!("{} → {}", item.start_date, item.due_date),
    };
    view! {
        <li class="todo-quadrant-modal__row">
            <span class="todo-quadrant-modal__title">{item.title.clone()}</span>
            <div class="todo-quadrant-modal__meta">
                <span class="todo-quadrant-modal__card">{item.card_title.clone()}</span>
                <span class="todo-item__levels">
                    <span class="todo-level" title="紧急度">
                        {format!("紧急 {}", consts::todo_level_label(item.urgency))}
                    </span>
                    <span class="todo-level" title="重要度">
                        {format!("重要 {}", consts::todo_level_label(item.importance))}
                    </span>
                </span>
                {(!dates.is_empty())
                    .then(|| {
                        view! { <span class="todo-quadrant-modal__dates">{dates.clone()}</span> }
                    })}
            </div>
        </li>
    }
    .into_any()
}

/// 四象限图：把**进行中**的事项按（紧急度, 重要度）画成点。
///
/// 坐标：`x = 紧急度`、`y = 重要度`，各 `-5..=5`，0 落在十字轴上（+紧急向右、+重要向上）。
/// 四个象限用**同一支色的四种深浅**分档（浅 → 深 = 优先级低 → 高）：设计系统里只有一支强调色，
/// 靠明度分档而不是引入第二种颜色；「马上做」的象限名用强调色点出来。
///
/// 版面：卡片（`.todo-quadrant__canvas`）撑满工具栏以下的高度，SVG 再撑满卡片 ——
/// viewBox 用 `watch_canvas_size` 量到的**真实像素**（1 单位 = 1px），所以四象限是等比贴合、
/// 不会变形；象限名摆在各自象限的中心（水平垂直居中）。
///
/// 象限名是一块**水印**：字号按象限宽度的三成算（四个名字都恰好 3 个字），
/// 颜色由 CSS 压成低浓度的墨 —— 图上的"重量"留给数据点，文字只做方位提示。象限的语义不必
/// 在象限里复述一遍：「重要」那一行 + 「不紧急」那一列 已经就是"计划做"。
/// 四个名字同时是**按钮**：悬停亮起、点击打开该象限的事项列表（见 [`quadrant_name`]）。
/// 落在同一坐标的事项按 id 各带一个固定的微小偏移（[`jitter_of`]），否则会叠成一坨。
fn quadrant_view(cards: &[TodoCardDto]) -> AnyView {
    let items: Vec<QuadrantItem> = cards
        .iter()
        .flat_map(|card| {
            card.items.iter().map(|item| QuadrantItem {
                id: item.id.clone(),
                card_title: card.title.clone(),
                title: item.title.clone(),
                start_date: item.start_date.clone(),
                due_date: item.due_date.clone(),
                urgency: item.urgency,
                importance: item.importance,
            })
        })
        .collect();

    if items.is_empty() {
        return view! {
            <Empty
                title="还没有进行中的事项"
                description="把事项的紧急度与重要度填上，这里就会把它们摆进四个象限。"
                icon=Icon::Aim
            />
        }
        .into_any();
    }

    // 画布按**实测像素**出图（1 个用户单位 = 1px）：固定 viewBox 缩放到卡片宽度时，
    // 四象限会在卡片里留出大片空白（宽高比不匹配就必然有一边空着），量到的宽高才对得上。
    // 象限底**铺满**卡片（不留边距）：轴端那四个字改在四角各留 8px（见下面的标注），
    // 圆角由 CSS 的 `overflow: hidden` 裁 —— 方块底自己不知道卡片是圆的。
    let canvas = NodeRef::<leptos::html::Div>::new();
    // 还没量到（首帧 / 面板刚挂载）时的兜底尺寸：沿用旧的 720×460
    let size = RwSignal::new((720.0_f64, 460.0_f64));
    watch_canvas_size(canvas, size);
    // 点开了哪个象限（`None` = 没有弹窗）
    let open_quad = RwSignal::new(Option::<Quadrant>::None);
    // 每个象限各有多少项（水印名的 tooltip / 无障碍名要用）。四个 `usize` 是 Copy —— 下面
    // 那两个 `move` 闭包各自带走一份，不用把 `items` 借出去（闭包要活在函数返回之后）。
    let count = |quad: Quadrant| items.iter().filter(|item| item.quadrant() == quad).count();
    let (now_count, plan_count, less_count, delegate_count) = (
        count(Quadrant::Now),
        count(Quadrant::Plan),
        count(Quadrant::Less),
        count(Quadrant::Delegate),
    );
    // 画点与列事项各要一份数据（同上：闭包得拿走所有权）
    let items_for_points = items.clone();
    // 弹窗要列的行：由"点开的是哪个象限"推出来。这份数据藏在 `Memo` 里，视图里的闭包只捕获
    // 它的句柄（Copy）—— 否则整份 Vec 会被 move 进弹窗的 children 工厂，那个工厂要能被
    // 求值多次（每次打开弹窗），move 出去就退化成 `FnOnce` 编不过。
    let items_for_rows = items.clone();
    let rows_of = Memo::new(move |_| quadrant_rows(&items_for_rows, open_quad.get()));

    view! {
        <div class="todo-quadrant">
            <div class="todo-quadrant__head">
                <h3 class="todo-quadrant__title">"四象限图"</h3>
                <span class="todo-quadrant__hint">
                    {format!("{} 项进行中；横轴紧急度，纵轴重要度（各六档：低 → 高）", items.len())}
                </span>
            </div>
            <div class="todo-quadrant__canvas" node_ref=canvas>
                {move || {
                    let (w, h) = size.get();
                    let sx = |urgency: i32| (f64::from(urgency) + 5.0) / 10.0 * w;
                    let sy = |importance: i32| {
                        h - (f64::from(importance) + 5.0) / 10.0 * h
                    };
                    let axis_x = sx(0);
                    let axis_y = sy(0);
                    // 四个象限的中心：水印名各自摆在自己的中心上
                    let mid = |a: f64, b: f64| (a + b) / 2.0;
                    let (plan_x, plan_y) = (mid(0.0, axis_x), mid(0.0, axis_y));
                    let (now_x, now_y) = (mid(axis_x, w), mid(0.0, axis_y));
                    let (less_x, less_y) = (mid(0.0, axis_x), mid(axis_y, h));
                    let (delegate_x, delegate_y) = (mid(axis_x, w), mid(axis_y, h));
                    // 水印字号：名称占象限宽度的三成（四个名字都是 3 个字）
                    let name_px = (axis_x * 0.30 / 3.0).clamp(22.0, 72.0);
                    // 中文字形的墨高约 0.96em（基线以上 0.84、以下 0.12）：要让这块墨**整体**
                    // 落在象限中心，基线得压在中心下方 (0.84 − 0.12) / 2 = 0.36em。
                    let name_dy = name_px * 0.36;
                    view! {
                        <svg
                            class="todo-quadrant__svg"
                            viewBox=format!("0 0 {w} {h}")
                            // `group` 而不是 `img`：`img` 的子元素在无障碍树里是装饰 —— 象限名
                            // 挂在上面的 `role=button` / 焦点就都没了。名字本身还是那句 aria-label。
                            role="group"
                            aria-label="进行中事项的四象限分布"
                        >
                            // 四个象限的底（明度分档：马上做最深、减少做最浅）
                            <rect
                                class="todo-quadrant__ground todo-quadrant__ground--now"
                                x=axis_x
                                y=0.0
                                width=w - axis_x
                                height=axis_y
                            ></rect>
                            <rect
                                class="todo-quadrant__ground todo-quadrant__ground--plan"
                                x=0.0
                                y=0.0
                                width=axis_x
                                height=axis_y
                            ></rect>
                            <rect
                                class="todo-quadrant__ground todo-quadrant__ground--less"
                                x=0.0
                                y=axis_y
                                width=axis_x
                                height=h - axis_y
                            ></rect>
                            <rect
                                class="todo-quadrant__ground todo-quadrant__ground--delegate"
                                x=axis_x
                                y=axis_y
                                width=w - axis_x
                                height=h - axis_y
                            ></rect>
                            // 十字轴
                            <line
                                class="todo-quadrant__axis"
                                x1=axis_x
                                y1=0.0
                                x2=axis_x
                                y2=h
                            ></line>
                            <line
                                class="todo-quadrant__axis"
                                x1=0.0
                                y1=axis_y
                                x2=w
                                y2=axis_y
                            ></line>
                            // 象限名（各自象限的**中心**：水平垂直都居中；可点，点开看这一格的事项）
                            {quadrant_name(Quadrant::Now, now_count, now_x, now_y, name_px, name_dy, open_quad)}
                            {quadrant_name(Quadrant::Plan, plan_count, plan_x, plan_y, name_px, name_dy, open_quad)}
                            {quadrant_name(Quadrant::Less, less_count, less_x, less_y, name_px, name_dy, open_quad)}
                            {quadrant_name(Quadrant::Delegate, delegate_count, delegate_x, delegate_y, name_px, name_dy, open_quad)}
                            // 轴端标注（紧急 / 不紧急 / 重要 / 不重要）：贴着各自的轴，但**四个
                            // 都要离卡片的边线 8px 以上**（象限底已经铺满卡片，边线就是卡片的边）。
                            <text
                                class="todo-quadrant__axis-label"
                                x=w - 8.0
                                y=axis_y - 10.0
                                text-anchor="end"
                            >
                                "紧急"
                            </text>
                            <text class="todo-quadrant__axis-label" x=8.0 y=axis_y - 10.0>
                                "不紧急"
                            </text>
                            // 纵轴这两个：标注 16px（见 CSS 的 `.todo-quadrant__axis-label`），
                            // 中文字形高约 0.88em、下缘只越出基线一点点 —— 所以顶部基线落在
                            // 21、底部落在 h−10，两侧各留 8px（直接写 0 / h 的话字正好骑在边线上，
                            // 顶部的字顶冒到线外侧、底部的字底沉到线外侧，看着像被截断）。
                            <text class="todo-quadrant__axis-label" x=axis_x + 10.0 y=21.0>
                                "重要"
                            </text>
                            <text class="todo-quadrant__axis-label" x=axis_x + 10.0 y=h - 10.0>
                                "不重要"
                            </text>
                            // 事项点：位置就是数据本身（同时落在同一点上的会重叠，靠悬停看名字）
                                {items_for_points
                                    .iter()
                                    .map(|item| {
                                        let (dx, dy) = jitter_of(&item.id);
                                        view! {
                                            <circle
                                                class="todo-quadrant__point"
                                                cx=sx(item.urgency) + dx
                                                cy=sy(item.importance) + dy
                                                r=9.0
                                            >
                                            <title>
                                                {format!(
                                                    "{}\n{} · 紧急 {} · 重要 {}",
                                                    item.title,
                                                    item.card_title,
                                                    consts::todo_level_label(item.urgency),
                                                    consts::todo_level_label(item.importance),
                                                )}
                                            </title>
                                        </circle>
                                    }
                                    })
                                    .collect_view()}
                        </svg>
                    }
                }}
            </div>
        </div>

        // 象限弹窗：这一格里的事项，按**紧急度降序、再按重要度降序**
        <Modal
            open=Signal::derive(move || open_quad.get().is_some())
            title=Signal::derive(move || match open_quad.get() {
                Some(quad) => format!("{} · {}", quad.name(), quad.meaning()),
                None => String::new(),
            })
            size=ModalSize::Medium
            footer=false
            on_close=move || open_quad.set(None)
        >
            {move || {
                let rows = rows_of.get();
                if rows.is_empty() {
                    view! { <p class="todo-progress__empty">"这个象限里还没有进行中的事项。"</p> }
                        .into_any()
                } else {
                    view! {
                        <ul class="todo-quadrant-modal__list">
                            {rows.iter().map(quadrant_item_row).collect_view()}
                        </ul>
                    }
                        .into_any()
                }
            }}
            <div class="todo-modal-actions">
                <Button variant=ButtonVariant::Secondary on_click=move |_| open_quad.set(None)>
                    "关闭"
                </Button>
            </div>
        </Modal>
    }
    .into_any()
}

/// 事项点的抖动偏移（单位：px）：紧急度与重要度都相同的事项会叠成一坨，
/// 按 id 给每个点一个固定的小偏移，远近只够分开两颗点、不改变它落在哪个象限。
///
/// 用 `DefaultHasher`（标准库、固定种子）而不是随机数：同一份数据每次渲染都落在同一处，
/// 重渲染不会让点自己抖。
fn jitter_of(id: &str) -> (f64, f64) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let bits = hasher.finish();
    // ±10px（点半径 9px）
    let dx = (bits % 21) as f64 - 10.0;
    let dy = ((bits >> 21) % 21) as f64 - 10.0;
    (dx, dy)
}

// ==================================================================== 子功能二：历史

/// 历史子功能：已完成的事项（按完成时刻倒序）+ 进度记录弹窗。
fn history_view(sub: RwSignal<TodoSub>) -> AnyView {
    let stores = AppStores::global();
    let rows = RwSignal::new(Vec::<TodoHistoryDto>::new());
    let loading = RwSignal::new(false);
    // 进度记录弹窗（历史里进度记录收进按钮，不占行内空间）
    let progress_open = RwSignal::new(Option::<TodoHistoryDto>::None);

    let load = move |_: ()| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            rows.set(Vec::new());
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::todo::history(&ledger_id).await {
                Ok(list) => rows.set(list),
                Err(error) => notify_error("读取历史失败", &error),
            }
            loading.set(false);
        });
    };

    Effect::new(move |_: Option<()>| {
        stores.current_ledger_id.get();
        load(());
    });

    // 退回进行中（误点的退路：状态是同一个字段，改回去就行）。
    let restore: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::todo::item_status(&ledger_id, &item_id, consts::TODO_STATUS_DOING).await {
                Ok(_) => {
                    Notifier::global().success("已退回进行中", None);
                    load(());
                }
                Err(error) => notify_error("退回失败", &error),
            }
        });
    });

    // 删除（历史里也留着退路：错记的条目直接删掉）
    let remove: UnsyncCallback<String> = UnsyncCallback::new(move |item_id: String| {
        leptos::task::spawn_local(async move {
            match api::todo::item_delete(&item_id).await {
                Ok(()) => {
                    Notifier::global().success("事项已删除", None);
                    load(());
                }
                Err(error) => notify_error("删除事项失败", &error),
            }
        });
    });

    let content = view! {
        {move || {
            let list = rows.get();
            let total = list.len();
            // 分节标题**任何数据状态下都在**：空态也看得见"这是历史"，而不是一片空白
            // （ui-smoke 的页面标记就挂在它上面）。
            let head = view! {
                <div class="todo-history__head">
                    <h3 class="todo-history__title">"已完成的事项"</h3>
                    <span class="todo-history__count">{format!("共 {total} 条")}</span>
                </div>
            };
            if list.is_empty() {
                return view! {
                    <div class="todo-history">
                        {head}
                        {if loading.get() {
                            view! { <div class="todo-loading">"正在加载…"</div> }.into_any()
                        } else {
                            view! {
                                <Empty
                                    title="还没有已完成的事项"
                                    description="在「记录」里把事项标记为已完成，它就会落到这里。"
                                    icon=Icon::History
                                />
                            }
                                .into_any()
                        }}
                    </div>
                }
                    .into_any();
            }
            view! {
                <div class="todo-history">
                    {head}
                    <table class="todo-table">
                        <thead>
                            <tr>
                                <th>"完成时间"</th>
                                <th>"主题"</th>
                                <th>"事项"</th>
                                <th class="is-center">"紧急"</th>
                                <th class="is-center">"重要"</th>
                                <th class="is-center">"进度记录"</th>
                                <th class="is-center">"操作"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {list
                                .into_iter()
                                .map(|row| {
                                    let open_row = row.clone();
                                    let restore_id = row.id.clone();
                                    let delete_id = row.id.clone();
                                    let progress_count = row.progress.len();
                                    view! {
                                        <tr>
                                            <td class="todo-table__time">
                                                {format_timestamp(row.completed_at, "YYYY-MM-DD HH:mm")}
                                            </td>
                                            <td>{row.card_title.clone()}</td>
                                            <td class="todo-table__title">{row.title.clone()}</td>
                                            <td class="is-center">
                                                {consts::todo_level_label(row.urgency)}
                                            </td>
                                            <td class="is-center">
                                                {consts::todo_level_label(row.importance)}
                                            </td>
                                            <td class="is-center">
                                                <Button
                                                    variant=ButtonVariant::Text
                                                    size=ButtonSize::Small
                                                    on_click=move |_| progress_open.set(Some(open_row.clone()))
                                                >
                                                    {format!("查看（{progress_count}）")}
                                                </Button>
                                            </td>
                                            <td class="is-center">
                                                <div class="todo-table__actions">
                                                    <Button
                                                        variant=ButtonVariant::Text
                                                        size=ButtonSize::Small
                                                        on_click=move |_| restore.run(restore_id.clone())
                                                    >
                                                        "退回进行中"
                                                    </Button>
                                                    <Popconfirm
                                                        title="删除这条事项？"
                                                        description="它的进度记录会一起删掉。"
                                                        ok_text="删除"
                                                        cancel_text="取消"
                                                        class="ui-popconfirm--end"
                                                        on_confirm=UnsyncCallback::new(move |()| {
                                                            remove.run(delete_id.clone())
                                                        })
                                                    >
                                                        <Button
                                                            variant=ButtonVariant::TextDanger
                                                            size=ButtonSize::Small
                                                        >
                                                            "删除"
                                                        </Button>
                                                    </Popconfirm>
                                                </div>
                                            </td>
                                        </tr>
                                    }
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>
            }
                .into_any()
        }}
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="todo-page"
            rail=view! { <TodoSubRail sub=sub /> }.into_any()
            content=content
        />

        // 进度记录弹窗：标题带事项名，正文按时间列出
        <Modal
            open=Signal::derive(move || progress_open.get().is_some())
            title=Signal::derive(move || match progress_open.get() {
                Some(row) => format!("进度记录 · {}", row.title),
                None => "进度记录".to_string(),
            })
            size=ModalSize::Medium
            footer=false
            on_close=move || progress_open.set(None)
        >
            {move || match progress_open.get() {
                Some(row) if row.progress.is_empty() => {
                    view! { <p class="todo-progress__empty">"这条事项没有进度记录。"</p> }
                        .into_any()
                }
                Some(row) => {
                    view! {
                        <ol class="todo-progress__list">
                            {row
                                .progress
                                .iter()
                                .map(|item| {
                                    view! {
                                        <li class="todo-progress__row">
                                            <span class="todo-progress__time">
                                                {format_timestamp(item.created_at, "YYYY-MM-DD HH:mm")}
                                            </span>
                                            <span class="todo-progress__text">{item.content.clone()}</span>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ol>
                    }
                        .into_any()
                }
                None => view! { <p></p> }.into_any(),
            }}
            <div class="todo-modal-actions">
                <Button variant=ButtonVariant::Secondary on_click=move |_| progress_open.set(None)>
                    "关闭"
                </Button>
            </div>
        </Modal>
    }
    .into_any()
}
