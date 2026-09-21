//! 事件页（`/key_event_view`）—— P6-b 完整实现。
//!
//! ## 组成
//!
//! * [`KeyEventPage`]：年份导航 + 三栏编排 + 状态流转
//! * [`event_list`]：事件卡片（色条 / 短日期 / 30 字摘要 / 删除气泡）
//! * [`add_modal`]：日期 + 名称
//! * [`detail_panel`]：20 个颜色 + 描述（查看/编辑）+ 底部操作
//! * [`image_gallery`]：左大图 + 右侧 160px 缩略图列 + 另存/删除
//! * [`linked_panel`]：关联消费记录卡片 + 解除关联
//! * [`ImageUploadHost`] + [`crate::components::ui::UploadProgressBar`]：串行上传状态机与进度条，
//!   单张图片的内容由 [`crate::components::ui::read_as_data_url`] 读出
//!
//! ## 设计取舍
//!
//! 1. **切年不做 O(N) 预取**：切年时若对每个事件各发
//!    `key_event_images_list` + `tr_linked_by_date`，请求量会到 2N。这里改为选中时惰性加载
//!    （带缓存），请求量降到 2。
//! 2. **上传进度是分阶段的近似值**：Tauri IPC 是一次性 invoke，拿不到 XHR 字节进度。
//!    这里按「读取/转换 0→30→60」+「写库完成 100」上报；总进度仍是 `已完成/总数` 的阶梯式。
//! 3. **跳过的文件标为「已跳过」**：本实现把被跳过的行标为「已跳过」，比让它停在「上传中」更诚实。
//! 4. **灯箱**：ESC 关闭 + 上一张/下一张 + 缩放。
//! 5. `key_event_get`：本页没有调用点 —— 选中事件直接从 `list_by_year` 的结果里取。

use std::collections::{BTreeMap, BTreeSet};

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::dto::TransactionRecordDto;
use tr_domain::models::{KeyEvent, KeyEventImage};

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, DatePicker, FeaturePage, FileStatus, IconButton,
    IconButtonVariant, ImagePicker, Input, Modal, ModalSize, Popconfirm, Textarea,
    UploadFileProgress, UploadProgress, UploadProgressBar, UploadStatus,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{split_ymd, today_ymd};

/// 页面标题（固定文案，改动即影响界面）。
pub const PAGE_TITLE: &str = "事件";

/// 事件颜色（顺序即渲染顺序，共 20 个）。
const EVENT_COLORS: [&str; 20] = [
    "#D9705A", "#C25460", "#D07048", "#D48838", "#C6963A", "#A09040", "#5C9858", "#4A8E70",
    "#5C9E7C", "#3D8878", "#389098", "#4A78A0", "#5C8DB5", "#6070A0", "#7868A0", "#8C6B9E",
    "#A06088", "#B06078", "#8C7B6E", "#7E8890",
];

/// 事件标题上限（新建弹窗的 `maxlength` 与正文首行截断共用 200）。
const TITLE_MAX: usize = 200;
/// 描述上限（编辑区的 `maxlength`）。
const CONTENT_MAX: u32 = 5000;
/// 列表摘要长度（30 字）。
const SUMMARY_MAX: usize = 30;
/// 上传完成后进度条停留时长（2000ms）。
const PROGRESS_LINGER_MS: u64 = 2000;

/// 一次待上传的图片集合（`web_sys::File` 不是 `Send`，状态机留在界面线程）。
struct PendingUpload {
    target_date: String,
    files: Vec<web_sys::File>,
    index: usize,
}

/// 上传状态机的控制块（信号都是 `Copy`，可自由进闭包）。
#[derive(Clone, Copy)]
struct UploadControls {
    progress: RwSignal<UploadProgress>,
    pending: RwSignal<Option<std::rc::Rc<PendingUpload>>, leptos::prelude::LocalStorage>,
    images: RwSignal<Vec<KeyEventImage>>,
    asset_urls: RwSignal<BTreeMap<String, String>>,
}

// ==================================================================== 页面

/// 页面根组件。
#[component]
pub fn KeyEventPage() -> impl IntoView {
    let stores = AppStores::global();

    // ---- 年份 / 列表 ----
    let year = RwSignal::new(
        split_ymd(&today_ymd())
            .map(|(year, _, _)| year)
            .unwrap_or(1970),
    );
    let events = RwSignal::new(Vec::<KeyEvent>::new());
    let event_dates = RwSignal::new(BTreeSet::<String>::new());
    let list_loading = RwSignal::new(false);

    // ---- 选中事件 ----
    let selected_date = RwSignal::new(String::new());
    let current_event = RwSignal::new(Option::<KeyEvent>::None);
    let is_editing = RwSignal::new(false);
    let draft_content = RwSignal::new(String::new());
    let detail_loading = RwSignal::new(false);

    // ---- 图片 ----
    let images = RwSignal::new(Vec::<KeyEventImage>::new());
    let selected_image_id = RwSignal::new(String::new());
    let preview_open = RwSignal::new(false);
    let asset_urls = RwSignal::new(BTreeMap::<String, String>::new());
    let image_cache = RwSignal::new(BTreeMap::<String, Vec<KeyEventImage>>::new());

    // ---- 关联消费记录 ----
    let linked = RwSignal::new(Vec::<TransactionRecordDto>::new());
    let tr_cache = RwSignal::new(BTreeMap::<String, Vec<TransactionRecordDto>>::new());

    // ---- 上传 ----
    let progress = RwSignal::new(UploadProgress::default());
    let pending =
        RwSignal::<Option<std::rc::Rc<PendingUpload>>, leptos::prelude::LocalStorage>::new_local(
            None,
        );
    let upload = UploadControls {
        progress,
        pending,
        images,
        asset_urls,
    };

    // ---- 新增事件弹窗 ----
    let add_open = RwSignal::new(false);
    let add_date = RwSignal::new(today_ymd());
    let add_title = RwSignal::new(String::new());
    let add_loading = RwSignal::new(false);

    // 账本或年份变化 → 清空选中并重拉该年事件
    Effect::new(move |_| {
        let ledger_id = stores.current_ledger_id.get();
        let current_year = year.get();
        clear_selection(
            stores,
            selected_date,
            current_event,
            is_editing,
            images,
            selected_image_id,
            preview_open,
            linked,
            progress,
            pending,
        );
        if ledger_id.is_empty() {
            events.set(Vec::new());
            event_dates.set(BTreeSet::new());
            return;
        }
        load_year(
            ledger_id,
            current_year,
            events,
            event_dates,
            list_loading,
            image_cache,
            tr_cache,
        );
    });

    // 选中一个日期：先取事件本体，再惰性加载图片与关联交易
    let select_event = move |date: String| {
        let found = events
            .get_untracked()
            .into_iter()
            .find(|event| event.date == date);
        match found {
            Some(event) => {
                draft_content.set(event.content.clone());
                current_event.set(Some(event));
            }
            None => {
                draft_content.set(String::new());
                current_event.set(None);
                return;
            }
        }
        select_date(
            date,
            stores,
            selected_date,
            is_editing,
            detail_loading,
            images,
            selected_image_id,
            preview_open,
            linked,
            asset_urls,
            image_cache,
            tr_cache,
            progress,
            pending,
        );
    };

    // ---- 保存（标题 / 正文 / 颜色一次性 upsert）----
    let save = move |payload: Option<(String, String, String)>| {
        let Some((title, content, color)) = payload else {
            return;
        };
        let ledger_id = stores.current_ledger_id.get_untracked();
        let date = selected_date.get_untracked();
        if ledger_id.is_empty() || date.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::key_event::upsert(&ledger_id, &date, &title, &content, &color).await {
                Ok(_) => {
                    Notifier::global().success("事件已保存".to_string(), None);
                    is_editing.set(false);
                    // 就地同步列表与当前事件
                    events.update(
                        |items| match items.iter_mut().find(|item| item.date == date) {
                            Some(existing) => {
                                existing.title = title.clone();
                                existing.content = content.clone();
                                existing.color = color.clone();
                            }
                            None => items.push(KeyEvent {
                                date: date.clone(),
                                title: title.clone(),
                                content: content.clone(),
                                color: color.clone(),
                                ledger_id: ledger_id.clone(),
                                ..KeyEvent::default()
                            }),
                        },
                    );
                    current_event.update(|slot| {
                        if let Some(event) = slot.as_mut() {
                            event.title = title.clone();
                            event.content = content.clone();
                            event.color = color.clone();
                        }
                    });
                    event_dates.update(|dates| {
                        dates.insert(date.clone());
                    });
                    // 标题变更会影响列表显示；只刷新列表，不动缓存
                    refresh_list(ledger_id, year.get_untracked(), events, event_dates);
                }
                Err(error) => notify_error("保存事件失败", &error),
            }
        });
    };

    // ---- 删除事件 ----
    let delete_event = move |date: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || date.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::key_event::delete(&date, &ledger_id).await {
                Ok(()) => {
                    Notifier::global().success("事件已删除".to_string(), None);
                    image_cache.update(|cache| {
                        cache.remove(&date);
                    });
                    tr_cache.update(|cache| {
                        cache.remove(&date);
                    });
                    if selected_date.get_untracked() == date {
                        clear_selection(
                            stores,
                            selected_date,
                            current_event,
                            is_editing,
                            images,
                            selected_image_id,
                            preview_open,
                            linked,
                            progress,
                            pending,
                        );
                    }
                    refresh_list(ledger_id, year.get_untracked(), events, event_dates);
                }
                Err(error) => notify_error("删除事件失败", &error),
            }
        });
    };

    // ---- 解除关联 ----
    let unlink = move |transaction_id: String| {
        if transaction_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::tr::unlink(&transaction_id).await {
                Ok(_) => {
                    Notifier::global().success("已解除关联".to_string(), None);
                    linked.update(|items| {
                        items.retain(|item| item.transaction_id != transaction_id);
                    });
                    let snapshot = linked.get_untracked();
                    let date = selected_date.get_untracked();
                    tr_cache.update(|cache| {
                        cache.insert(date, snapshot.clone());
                    });
                    publish_statistics(stores, &snapshot);
                }
                Err(error) => notify_error("解除关联失败", &error),
            }
        });
    };

    // ---- 新增事件 ----
    let confirm_add = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let date = add_date.get_untracked();
        let title = add_title.get_untracked().trim().to_string();
        if ledger_id.is_empty() {
            Notifier::global().warning("尚未选择账本".to_string(), None);
            return;
        }
        if date.is_empty() {
            Notifier::global().error("请选择日期".to_string(), None);
            return;
        }
        add_loading.set(true);
        leptos::task::spawn_local(async move {
            let ledger_for_refresh = ledger_id.clone();
            match api::key_event::upsert(&ledger_id, &date, &title, "", "").await {
                Ok(saved_date) => {
                    add_open.set(false);
                    add_title.set(String::new());
                    Notifier::global().success("事件已保存".to_string(), None);
                    refresh_list(
                        ledger_for_refresh,
                        year.get_untracked(),
                        events,
                        event_dates,
                    );
                    // 立即选中新事件
                    if let Ok(items) =
                        api::key_event::list_by_year(&year.get_untracked().to_string(), &ledger_id)
                            .await
                    {
                        if let Some(event) = items.into_iter().find(|item| item.date == saved_date)
                        {
                            draft_content.set(event.content.clone());
                            current_event.set(Some(event));
                        }
                    }
                    select_date(
                        saved_date,
                        stores,
                        selected_date,
                        is_editing,
                        detail_loading,
                        images,
                        selected_image_id,
                        preview_open,
                        linked,
                        asset_urls,
                        image_cache,
                        tr_cache,
                        progress,
                        pending,
                    );
                }
                Err(error) => notify_error("保存事件失败", &error),
            }
            add_loading.set(false);
        });
    };

    // 版心两块：工具栏 / 内容区各自建好视图再交给 `FeaturePage`（骨架见 components/ui/feature_page.rs）
    let toolbar = view! {
        <div class="key-event-yearbar">
            <button
                type="button"
                class="ui-icon-btn"
                title="上一年"
                aria-label="上一年"
                on:click=move |_| year.update(|value| *value -= 1)
            >
                {icons::icon(Icon::Left)}
            </button>
            <span class="key-event-yearbar__value">{move || year.get().to_string()}</span>
            <button
                type="button"
                class="ui-icon-btn"
                title="下一年"
                aria-label="下一年"
                on:click=move |_| year.update(|value| *value += 1)
            >
                {icons::icon(Icon::Right)}
            </button>
        </div>
    }
    .into_any();

    let content = view! {
        <div class="key-event-body">
            <div class="key-event-panel key-event-panel--left">
                {event_list(
                    events,
                    list_loading,
                    selected_date,
                    UnsyncCallback::new(move |date: String| select_event(date)),
                    UnsyncCallback::new(move |date: String| delete_event(date)),
                    UnsyncCallback::new(move |()| add_open.set(true)),
                )}
            </div>

            <div class="key-event-panel key-event-panel--center">
                <div class="key-event-detail">
                    <Show
                        when=move || current_event.get().is_some()
                        fallback=|| {
                            view! {
                                <div class="key-event-empty">
                                    <span class="key-event-empty__text">
                                        "选择左侧事件查看详情"
                                    </span>
                                </div>
                            }
                        }
                    >
                        <div class="key-event-detail__body">
                            <ColorToolbar
                                current_event=current_event
                                on_color=UnsyncCallback::new(move |color: String| {
                                    let Some(event) = current_event.get_untracked()
                                    else {
                                        return;
                                    };
                                    save(Some((
                                        event.title.clone(),
                                        event.content.clone(),
                                        color,
                                    )));
                                })
                            />

                            // 上传入口（`ImagePicker`）在详情底栏，见下方 `key-event-detail__footer`；
                            // 这里只留进度条与画廊本体。
                            <ImageUploadHost
                                images=images
                                selected_id=selected_image_id
                                preview_open=preview_open
                                asset_urls=asset_urls
                                progress=progress
                                pending=pending
                            />

                            <div class="key-event-description">
                                <Show
                                    when=move || !is_editing.get()
                                    fallback=move || {
                                        view! {
                                            <div class="key-event-description__edit">
                                                <Textarea
                                                    value=draft_content
                                                    maxlength=CONTENT_MAX
                                                    placeholder="输入描述内容…"
                                                    class="key-event-textarea"
                                                />
                                            </div>
                                        }
                                    }
                                >
                                    <div class="key-event-description__content">
                                        {move || {
                                            let content = current_event
                                                .get()
                                                .map(|event| event.content)
                                                .unwrap_or_default();
                                            if content.trim().is_empty() {
                                                view! {
                                                    <p class="key-event-description__placeholder">
                                                        "暂无描述"
                                                    </p>
                                                }
                                                    .into_any()
                                            } else {
                                                view! {
                                                    <crate::components::ui::Markdown
                                                        source=Signal::derive(move || {
                                                            content.clone()
                                                        })
                                                        class="key-event-markdown"
                                                    />
                                                }
                                                    .into_any()
                                            }
                                        }}
                                    </div>
                                </Show>
                            </div>

                            <div class="key-event-detail__footer">
                                <Show
                                    when=move || is_editing.get()
                                    fallback=move || {
                                        view! {
                                            // 「添加图片」放这里（**下方工具栏**）：与「编辑描述」同一行、
                                            // 同一档 Small 次要按钮；编辑态不显示（那时底栏是取消/保存）。
                                            // 进度条仍留在图片区下面（见 `ImageUploadHost` 的注释）。
                                            <ImagePicker
                                                label="添加图片"
                                                multiple=true
                                                disabled=Signal::derive(move || {
                                                    progress.get().status
                                                        == Some(UploadStatus::Uploading)
                                                })
                                                on_files=UnsyncCallback::new(move |files: Vec<
                                                    web_sys::File,
                                                >| {
                                                    start_upload(
                                                        files,
                                                        selected_date.get_untracked(),
                                                        upload,
                                                    )
                                                })
                                            />
                                            <Button
                                                variant=ButtonVariant::Secondary
                                                size=ButtonSize::Small
                                                on_click=move |_| {
                                                    if let Some(event) = current_event
                                                        .get_untracked()
                                                    {
                                                        draft_content.set(event.content);
                                                    }
                                                    is_editing.set(true);
                                                }
                                            >
                                                <span class="ui-btn__icon">
                                                    {icons::icon(Icon::Edit)}
                                                </span>
                                                "编辑描述"
                                            </Button>
                                        }
                                    }
                                >
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        disabled=Signal::derive(move || {
                                            progress.get().status
                                                == Some(UploadStatus::Uploading)
                                        })
                                        on_click=move |_| {
                                            is_editing.set(false);
                                            if let Some(event) = current_event
                                                .get_untracked()
                                            {
                                                draft_content.set(event.content);
                                            }
                                        }
                                    >
                                        "取消"
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Primary
                                        size=ButtonSize::Small
                                        on_click=move |_| {
                                            // 标题为空时用正文首行
                                            let (title, color) = match current_event
                                                .get_untracked()
                                            {
                                                Some(event)
                                                    if !event.title.trim().is_empty() =>
                                                {
                                                    (event.title.clone(), event.color)
                                                }
                                                Some(event) => {
                                                    let first_line = draft_content
                                                        .get_untracked()
                                                        .lines()
                                                        .next()
                                                        .unwrap_or_default()
                                                        .trim()
                                                        .to_string();
                                                    (
                                                        format::truncate(
                                                            &first_line,
                                                            TITLE_MAX,
                                                        ),
                                                        event.color,
                                                    )
                                                }
                                                None => return,
                                            };
                                            save(Some((
                                                title,
                                                draft_content.get_untracked(),
                                                color,
                                            )));
                                        }
                                    >
                                        "保存"
                                    </Button>
                                </Show>
                            </div>
                        </div>
                    </Show>

                    <Show when=move || detail_loading.get() && current_event.get().is_none()>
                        <div class="key-event-skeleton" aria-hidden="true">
                            <div class="key-event-skeleton__line is-wide"></div>
                            <div class="key-event-skeleton__line"></div>
                        </div>
                    </Show>
                </div>
            </div>

            <div class="key-event-panel key-event-panel--right">
                {linked_panel(
                    linked,
                    selected_date,
                    UnsyncCallback::new(move |id: String| unlink(id)),
                )}
            </div>
        </div>

        {add_modal(
            add_open,
            add_date,
            add_title,
            add_loading,
            UnsyncCallback::new(move |()| confirm_add()),
        )}
    }
    .into_any();

    view! {
        <FeaturePage title=PAGE_TITLE class="key-event-page" toolbar=toolbar content=content />
    }
}

// ==================================================================== 状态辅助

/// 清空选中，并把底部状态统计归零。
#[allow(clippy::too_many_arguments)]
fn clear_selection(
    stores: AppStores,
    selected_date: RwSignal<String>,
    current_event: RwSignal<Option<KeyEvent>>,
    is_editing: RwSignal<bool>,
    images: RwSignal<Vec<KeyEventImage>>,
    selected_image_id: RwSignal<String>,
    preview_open: RwSignal<bool>,
    linked: RwSignal<Vec<TransactionRecordDto>>,
    progress: RwSignal<UploadProgress>,
    pending: RwSignal<Option<std::rc::Rc<PendingUpload>>, leptos::prelude::LocalStorage>,
) {
    selected_date.set(String::new());
    current_event.set(None);
    is_editing.set(false);
    images.set(Vec::new());
    selected_image_id.set(String::new());
    preview_open.set(false);
    linked.set(Vec::new());
    progress.set(UploadProgress::default());
    pending.set(None);
    publish_statistics(stores, &[]);
}

/// 把关联交易的收支汇总写进全局统计（底部状态栏的「收入/支出/转账」）。
fn publish_statistics(stores: AppStores, records: &[TransactionRecordDto]) {
    let mut totals = BTreeMap::new();
    for record in records {
        let entry = totals
            .entry(record.transaction_type.clone())
            .or_insert(0_i64);
        *entry = entry.saturating_add(record.price);
    }
    stores.statistics.set(totals);
}

/// 拉取某年的事件列表与日期集合，并写入各信号。
fn load_year(
    ledger_id: String,
    year: i32,
    events: RwSignal<Vec<KeyEvent>>,
    event_dates: RwSignal<BTreeSet<String>>,
    list_loading: RwSignal<bool>,
    image_cache: RwSignal<BTreeMap<String, Vec<KeyEventImage>>>,
    tr_cache: RwSignal<BTreeMap<String, Vec<TransactionRecordDto>>>,
) {
    list_loading.set(true);
    leptos::task::spawn_local(async move {
        let year_text = year.to_string();
        let list = api::key_event::list_by_year(&year_text, &ledger_id).await;
        let dates = api::key_event::dates_by_year(&year_text, &ledger_id).await;
        // 账本在请求途中被切换时丢弃过期结果
        if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
            return;
        }
        match list {
            Ok(items) => {
                // 年份换了就丢掉旧年份的缓存（避免无限增长）
                let year_prefix = format!("{year:04}-");
                image_cache.update(|cache| cache.retain(|key, _| key.starts_with(&year_prefix)));
                tr_cache.update(|cache| cache.retain(|key, _| key.starts_with(&year_prefix)));
                events.set(items);
            }
            Err(error) => {
                events.set(Vec::new());
                notify_error("查询事件失败", &error);
            }
        }
        match dates {
            Ok(items) => event_dates.set(items.into_iter().collect()),
            Err(_) => {
                // 日期集合只是列表的点缀，失败时用事件列表自行推导，不打扰用户
                let derived = events
                    .get_untracked()
                    .into_iter()
                    .map(|event| event.date)
                    .collect::<BTreeSet<_>>();
                event_dates.set(derived);
            }
        }
        list_loading.set(false);
    });
}

/// 只刷新列表（保存/删除后），保留缓存。
fn refresh_list(
    ledger_id: String,
    year: i32,
    events: RwSignal<Vec<KeyEvent>>,
    event_dates: RwSignal<BTreeSet<String>>,
) {
    load_year(
        ledger_id,
        year,
        events,
        event_dates,
        RwSignal::new(false),
        RwSignal::new(BTreeMap::new()),
        RwSignal::new(BTreeMap::new()),
    );
}

/// 选中日期后的惰性加载（图片 + 关联交易，命中缓存则跳过）。
#[allow(clippy::too_many_arguments)]
fn select_date(
    date: String,
    stores: AppStores,
    selected_date: RwSignal<String>,
    is_editing: RwSignal<bool>,
    detail_loading: RwSignal<bool>,
    images: RwSignal<Vec<KeyEventImage>>,
    selected_image_id: RwSignal<String>,
    preview_open: RwSignal<bool>,
    linked: RwSignal<Vec<TransactionRecordDto>>,
    asset_urls: RwSignal<BTreeMap<String, String>>,
    image_cache: RwSignal<BTreeMap<String, Vec<KeyEventImage>>>,
    tr_cache: RwSignal<BTreeMap<String, Vec<TransactionRecordDto>>>,
    progress: RwSignal<UploadProgress>,
    pending: RwSignal<Option<std::rc::Rc<PendingUpload>>, leptos::prelude::LocalStorage>,
) {
    selected_date.set(date.clone());
    is_editing.set(false);
    preview_open.set(false);
    selected_image_id.set(String::new());
    progress.set(UploadProgress::default());
    pending.set(None);

    let ledger_id = stores.current_ledger_id.get_untracked();
    if ledger_id.is_empty() {
        return;
    }

    // 缓存命中：立即显示，不再重新请求
    let cached_images = image_cache.get_untracked().get(&date).cloned();
    let cached_trs = tr_cache.get_untracked().get(&date).cloned();
    match &cached_images {
        Some(items) => {
            images.set(items.clone());
            let items = items.clone();
            leptos::task::spawn_local(async move {
                resolve_asset_urls(&items, asset_urls).await;
            });
        }
        None => images.set(Vec::new()),
    }
    match &cached_trs {
        Some(items) => {
            linked.set(items.clone());
            publish_statistics(stores, items);
        }
        None => linked.set(Vec::new()),
    }

    let needs_images = cached_images.is_none();
    let needs_trs = cached_trs.is_none();
    if !needs_images && !needs_trs {
        return;
    }

    detail_loading.set(true);
    leptos::task::spawn_local(async move {
        if needs_images {
            match api::key_event::images_list(&date, &ledger_id).await {
                Ok(items) => {
                    resolve_asset_urls(&items, asset_urls).await;
                    if selected_date.get_untracked() == date {
                        images.set(items.clone());
                    }
                    image_cache.update(|cache| {
                        cache.insert(date.clone(), items);
                    });
                }
                Err(error) => notify_error("加载图片失败", &error),
            }
        }
        if needs_trs {
            match api::tr::linked_by_date(&date, &ledger_id).await {
                Ok(items) => {
                    if selected_date.get_untracked() == date {
                        linked.set(items.clone());
                        publish_statistics(stores, &items);
                    }
                    tr_cache.update(|cache| {
                        cache.insert(date.clone(), items);
                    });
                }
                Err(error) => notify_error("查询关联交易失败", &error),
            }
        }
        if selected_date.get_untracked() == date {
            detail_loading.set(false);
        }
    });
}

/// 把图片路径解析成 `trasset://` URL 并写进缓存（只查未命中项）。
pub(crate) async fn resolve_asset_urls(
    images: &[KeyEventImage],
    asset_urls: RwSignal<BTreeMap<String, String>>,
) {
    let mut wanted: Vec<String> = Vec::new();
    {
        let cache = asset_urls.get_untracked();
        for image in images {
            for path in [&image.file_path, &image.thumb_path] {
                if !path.is_empty() && !cache.contains_key(path) && !wanted.contains(path) {
                    wanted.push(path.clone());
                }
            }
        }
    }
    for path in wanted {
        if let Ok(url) = api::desktop::asset_url(&path).await {
            asset_urls.update(|cache| {
                cache.insert(path.clone(), url);
            });
        }
    }
}

// ==================================================================== 上传状态机

/// 开始一轮上传。
fn start_upload(files: Vec<web_sys::File>, target_date: String, controls: UploadControls) {
    if files.is_empty() || target_date.is_empty() {
        Notifier::global().warning("请先选择事件日期".to_string(), None);
        return;
    }
    let total = files.len();
    controls.progress.set(UploadProgress {
        files: files
            .iter()
            .map(|file| UploadFileProgress {
                name: file.name(),
                percent: 0,
                status: FileStatus::Pending,
                error_message: String::new(),
            })
            .collect(),
        total,
        completed: 0,
        status: Some(UploadStatus::Uploading),
        error_message: String::new(),
    });
    controls.pending.set(Some(std::rc::Rc::new(PendingUpload {
        target_date,
        files,
        index: 0,
    })));
    drive_upload(controls);
}

/// 顺序处理下一个文件。
fn drive_upload(controls: UploadControls) {
    let Some(state) = controls.pending.get_untracked() else {
        return;
    };
    let Some(file) = state.files.get(state.index).cloned() else {
        finish_upload(controls, state);
        return;
    };

    let index = state.index;
    mark_file(controls, index, |entry| {
        entry.status = FileStatus::Uploading;
        entry.percent = 0;
    });

    let ledger_id = AppStores::global().current_ledger_id.get_untracked();
    let date = state.target_date.clone();

    leptos::task::spawn_local(async move {
        let progress_signal = controls.progress;
        let set_percent = move |percent: u8| {
            progress_signal.update(|snapshot| {
                if let Some(entry) = snapshot.files.get_mut(index) {
                    entry.percent = percent;
                }
            });
        };

        // 阶段 1：读取（HEIC/HEIF 会先转 JPEG）
        let data = match crate::components::ui::read_as_data_url(&file, &set_percent).await {
            Ok(data) => data,
            Err(message) => {
                fail_upload(controls, index, message);
                return;
            }
        };
        set_percent(70);

        if ledger_id.is_empty() {
            fail_upload(controls, index, "尚未选择账本".to_string());
            return;
        }

        // 阶段 2：写库（Tauri IPC 一次性调用，返回即成功）
        match api::key_event::image_add(&date, &ledger_id, &data).await {
            Ok(_) => {
                mark_file(controls, index, |entry| {
                    entry.status = FileStatus::Done;
                    entry.percent = 100;
                });
                controls.progress.update(|snapshot| {
                    snapshot.completed = snapshot.completed.saturating_add(1);
                });
                let Some(state) = controls.pending.get_untracked() else {
                    return;
                };
                controls.pending.set(Some(std::rc::Rc::new(PendingUpload {
                    target_date: state.target_date.clone(),
                    files: state.files.clone(),
                    index: index + 1,
                })));
                drive_upload(controls);
            }
            Err(error) => fail_upload(controls, index, error.prefixed("添加图片失败")),
        }
    });
}

/// 一轮上传结束：`total` 改写为 done 数、状态置 done、2 秒后回到 idle。
fn finish_upload(controls: UploadControls, state: std::rc::Rc<PendingUpload>) {
    let done = controls
        .progress
        .get_untracked()
        .files
        .iter()
        .filter(|file| file.status == FileStatus::Done)
        .count();
    controls.progress.update(|snapshot| {
        snapshot.completed = done;
        snapshot.total = done;
        snapshot.status = Some(UploadStatus::Done);
    });
    controls.pending.set(None);

    let progress = controls.progress;
    set_timeout(
        move || {
            progress.update(|snapshot| {
                if snapshot.status == Some(UploadStatus::Done) {
                    *snapshot = UploadProgress::default();
                }
            });
        },
        std::time::Duration::from_millis(PROGRESS_LINGER_MS),
    );

    // 刷新该日图片列表（一次请求，多张图也只多一次）
    let date = state.target_date.clone();
    let ledger_id = AppStores::global().current_ledger_id.get_untracked();
    if ledger_id.is_empty() {
        return;
    }
    let images = controls.images;
    let asset_urls = controls.asset_urls;
    leptos::task::spawn_local(async move {
        if let Ok(items) = api::key_event::images_list(&date, &ledger_id).await {
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            images.set(items.clone());
            resolve_asset_urls(&items, asset_urls).await;
        }
    });
}

/// 单文件失败：整体置 error 并停下（等待「重试 / 跳过」）。
fn fail_upload(controls: UploadControls, index: usize, message: String) {
    mark_file(controls, index, |entry| {
        entry.status = FileStatus::Error;
        entry.percent = 0;
        entry.error_message = message.clone();
    });
    controls.progress.update(|snapshot| {
        snapshot.status = Some(UploadStatus::Error);
        snapshot.error_message = message;
    });
}

/// 就地修改某个文件行。
fn mark_file(controls: UploadControls, index: usize, mutate: impl FnOnce(&mut UploadFileProgress)) {
    controls.progress.update(|snapshot| {
        if let Some(entry) = snapshot.files.get_mut(index) {
            mutate(entry);
        }
    });
}

// ==================================================================== 子视图

/// 左栏：事件卡片列表 + 底部「新增事件」。
fn event_list(
    events: RwSignal<Vec<KeyEvent>>,
    loading: RwSignal<bool>,
    selected_date: RwSignal<String>,
    on_select: UnsyncCallback<String>,
    on_delete: UnsyncCallback<String>,
    on_add: UnsyncCallback<()>,
) -> AnyView {
    let sorted = move || {
        let mut items = events.get();
        items.sort_by(|left, right| right.date.cmp(&left.date));
        items
    };

    view! {
        <div class="key-event-list">
            <Show
                when=move || !sorted().is_empty()
                fallback=move || {
                    view! {
                        <div class="key-event-empty">
                            <span class="key-event-empty__text">
                                {move || {
                                    if loading.get() { "正在加载…" } else { "暂无事件" }
                                }}
                            </span>
                        </div>
                    }
                        .into_any()
                }
            >
                <div class="key-event-cards">
                    {move || {
                        sorted()
                            .into_iter()
                            .map(|event| {
                                let date = event.date.clone();
                                let is_active = date == selected_date.get();
                                let label = if event.title.is_empty() {
                                    event.date.clone()
                                } else {
                                    event.title.clone()
                                };
                                let color = if event.color.is_empty() {
                                    "var(--transactions-color-primary)".to_string()
                                } else {
                                    event.color.clone()
                                };
                                let summary = format::truncate(&event.content, SUMMARY_MAX);
                                // 每个会捕获同一 `String` 的闭包各用一份克隆：`String` 不是 `Copy`，
                                // 两处 move 捕获会让 `view!` 生成的闭包退化成 `FnOnce`。
                                let summary_for_show = summary.clone();
                                let summary_for_text = summary.clone();
                                let click_date = date.clone();
                                let click_date_for_key = date.clone();
                                let delete_date = date.clone();
                                let label_for_title = label.clone();
                                // 回调先建好（`UnsyncCallback` 是 Copy）：`view!` 的 children
                                // 可能多次求值，直接 move 捕获 `String` 会让闭包退化成 FnOnce。
                                let delete_click = UnsyncCallback::new(move |()| {
                                    on_delete.run(delete_date.clone())
                                });
                                view! {
                                    <div
                                        class="key-event-card"
                                        class:is-active=is_active
                                        role="button"
                                        tabindex="0"
                                        aria-selected=is_active
                                        on:click=move |_| on_select.run(click_date.clone())
                                        on:keydown=move |event: leptos::ev::KeyboardEvent| {
                                            if event.key() == "Enter" || event.key() == " " {
                                                event.prevent_default();
                                                on_select.run(click_date_for_key.clone());
                                            }
                                        }
                                    >
                                        <div
                                            class="key-event-card__bar"
                                            style=format!("background-color: {color}")
                                        ></div>
                                        <div class="key-event-card__body">
                                            <div class="key-event-card__name">{label}</div>
                                            <div class="key-event-card__date">
                                                {format::short_date(&date)}
                                            </div>
                                            <Show when=move || !summary_for_show.is_empty()>
                                                <div class="key-event-card__desc">
                                                    {summary_for_text.clone()}
                                                </div>
                                            </Show>
                                        </div>
                                        <Popconfirm
                                            title=format!(
                                                "删除事件「{label_for_title}」？",
                                            )
                                            ok_text="删除"
                                            cancel_text="取消"
                                            on_confirm=move || delete_click.run(())
                                        >
                                            <IconButton
                                                variant=IconButtonVariant::Danger
                                                label="删除事件"
                                                class="key-event-card__delete"
                                                // 删除动作**只挂在 `on_confirm` 上**：这个按钮只负责
                                                // "打开确认气泡"。曾经这里挂着 `on_click=<删除>`，
                                                // 于是第一下点击就直接删掉了 —— 二次确认形同虚设。
                                                // `stop_propagation` 让点删除不顺带选中整行；它**不影响**
                                                // 气泡开关（`Popconfirm` 的触发在**捕获阶段**，先于冒泡）。
                                                stop_propagation=true
                                            >
                                                {icons::icon(Icon::Close)}
                                            </IconButton>
                                        </Popconfirm>
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </Show>

            <div class="key-event-list__footer">
                <Button variant=ButtonVariant::Primary block=true on_click=move || on_add.run(())>
                    "新增事件"
                </Button>
            </div>
        </div>
    }
    .into_any()
}

/// 颜色工具栏（20 色 + 一个「使用默认颜色」的虚线圆）。
#[component]
fn ColorToolbar(
    current_event: RwSignal<Option<KeyEvent>>,
    on_color: UnsyncCallback<String>,
) -> impl IntoView {
    view! {
        <div class="key-event-colors">
            {EVENT_COLORS
                .iter()
                .map(|color| {
                    let value = color.to_string();
                    let value_for_click = value.clone();
                    let value_for_class = value.clone();
                    view! {
                        <button
                            type="button"
                            class="key-event-swatch"
                            class:is-selected=move || {
                                current_event
                                    .get()
                                    .map(|event| event.color == value_for_class)
                                    .unwrap_or(false)
                            }
                            style=format!("background-color: {value_for_click}")
                            title=value_for_click.clone()
                            aria-label=value_for_click
                            on:click=move |_| on_color.run(value.clone())
                        ></button>
                    }
                })
                .collect_view()}
            <button
                type="button"
                class="key-event-swatch key-event-swatch--empty"
                class:is-selected=move || {
                    current_event
                        .get()
                        .map(|event| event.color.is_empty())
                        .unwrap_or(false)
                }
                title="使用默认颜色"
                aria-label="使用默认颜色"
                on:click=move |_| on_color.run(String::new())
            ></button>
        </div>
    }
}

/// 画廊 + 上传入口 + 上传进度 + 灯箱。
///
/// `progress` / `pending` 由**页面**持有（上传状态机也由页面的 `start_upload` 驱动），
/// 本组件只负责渲染与转发「重试 / 跳过」。
#[component]
fn ImageUploadHost(
    images: RwSignal<Vec<KeyEventImage>>,
    selected_id: RwSignal<String>,
    preview_open: RwSignal<bool>,
    asset_urls: RwSignal<BTreeMap<String, String>>,
    progress: RwSignal<UploadProgress>,
    pending: RwSignal<Option<std::rc::Rc<PendingUpload>>, leptos::prelude::LocalStorage>,
) -> impl IntoView {
    let controls = UploadControls {
        progress,
        pending,
        images,
        asset_urls,
    };

    // 默认选中第一张；选中项失效时回落
    Effect::new(move |_| {
        let items = images.get();
        let current = selected_id.get_untracked();
        if items.is_empty() {
            selected_id.set(String::new());
            preview_open.set(false);
        } else if !items.iter().any(|image| image.id == current) {
            selected_id.set(
                items
                    .first()
                    .map(|image| image.id.clone())
                    .unwrap_or_default(),
            );
        }
    });

    let on_retry = UnsyncCallback::new(move |()| {
        let Some(state) = pending.get_untracked() else {
            return;
        };
        mark_file(controls, state.index, |entry| {
            entry.status = FileStatus::Pending;
            entry.percent = 0;
        });
        progress.update(|snapshot| {
            snapshot.status = Some(UploadStatus::Uploading);
        });
        drive_upload(controls);
    });
    let on_skip = UnsyncCallback::new(move |()| {
        let Some(state) = pending.get_untracked() else {
            return;
        };
        let index = state.index;
        // 本实现把被跳过的行标为「已跳过」，比让它停在「上传中」更诚实
        mark_file(controls, index, |entry| {
            entry.status = FileStatus::Error;
            entry.error_message = "已跳过".to_string();
        });
        pending.set(Some(std::rc::Rc::new(PendingUpload {
            target_date: state.target_date.clone(),
            files: state.files.clone(),
            index: index + 1,
        })));
        progress.update(|snapshot| {
            snapshot.status = Some(UploadStatus::Uploading);
        });
        drive_upload(controls);
    });

    view! {
        <div class="key-event-gallery">
            <div class="key-event-gallery__view">
                {image_gallery(images, selected_id, preview_open, asset_urls)}
            </div>
            <div class="key-event-gallery__actions">
                // 「添加图片」已移到详情底栏（与「编辑描述」同一行、同一档按钮风格），
                // 这里只剩上传进度：进度条紧贴它要描述的图片区。
                <Show when=move || !progress.get().is_idle()>
                    <div class="key-event-gallery__progress">
                        <UploadProgressBar
                            progress=Signal::derive(move || progress.get())
                            on_retry=on_retry
                            on_skip=on_skip
                        />
                    </div>
                </Show>
            </div>
            {lightbox(images, selected_id, preview_open, asset_urls)}
        </div>
    }
}

/// 画廊本体（左大图 + 右侧 160px 缩略图列 + 另存/删除）。
fn image_gallery(
    images: RwSignal<Vec<KeyEventImage>>,
    selected_id: RwSignal<String>,
    preview_open: RwSignal<bool>,
    asset_urls: RwSignal<BTreeMap<String, String>>,
) -> AnyView {
    let url_of = move |path: &str| -> String {
        if path.is_empty() {
            return String::new();
        }
        asset_urls
            .get()
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_string())
    };

    let main_url = move || {
        let current = selected_id.get();
        let path = images
            .get()
            .into_iter()
            .find(|image| image.id == current)
            .map(|image| image.file_path)
            .unwrap_or_default();
        url_of(&path)
    };

    let save_as = move |_| {
        let current = selected_id.get_untracked();
        let path = images
            .get_untracked()
            .into_iter()
            .find(|image| image.id == current)
            .map(|image| image.file_path)
            .unwrap_or_default();
        if path.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::desktop::file_save_image(&path).await {
                Ok(response) => {
                    if response.canceled == Some(true) {
                        return;
                    }
                    if response.success {
                        Notifier::global().success("图片已保存".to_string(), None);
                    } else {
                        Notifier::global().error(
                            response.error.unwrap_or_else(|| "保存失败".to_string()),
                            None,
                        );
                    }
                }
                Err(error) => notify_error("保存失败", &error),
            }
        });
    };

    let delete_image = move |id: String| {
        if id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::key_event::image_delete(&id).await {
                Ok(()) => {
                    images.update(|items| items.retain(|image| image.id != id));
                }
                Err(error) => notify_error("删除图片失败", &error),
            }
        });
    };

    view! {
        <Show
            when=move || !images.get().is_empty()
            fallback=|| {
                view! {
                    <div class="key-event-gallery__empty">
                        <span>"暂无图片"</span>
                    </div>
                }
            }
        >
            <div class="key-event-gallery__main">
                <img
                    class="key-event-gallery__img"
                    src=main_url
                    alt=""
                    loading="lazy"
                    on:click=move |_| preview_open.set(true)
                />
                <IconButton
                    variant=IconButtonVariant::OnMedia
                    label="下载图片"
                    class="key-event-gallery__download"
                    on_click=move |_| save_as(())
                >
                    {icons::icon(Icon::Download)}
                </IconButton>
            </div>

            <div class="key-event-gallery__thumbs">
                {move || {
                    images
                        .get()
                        .into_iter()
                        .map(|image| {
                            let id = image.id.clone();
                            let is_selected = id == selected_id.get();
                            let thumb = url_of(&image.thumb_path);
                            let click_id = id.clone();
                            let delete_id = id.clone();
                            view! {
                                <div class="key-event-thumb" class:is-selected=is_selected>
                                    <img
                                        class="key-event-thumb__img"
                                        src=thumb
                                        alt=""
                                        loading="lazy"
                                        on:click=move |_| selected_id.set(click_id.clone())
                                    />
                                    // 删除图片也走**二次确认**（全站删除都确认；这里原来是一点就删）
                                    <Popconfirm
                                        title="删除这张图片？"
                                        ok_text="删除"
                                        cancel_text="取消"
                                        on_confirm=move || delete_image(delete_id.clone())
                                    >
                                        <IconButton
                                            variant=IconButtonVariant::Danger
                                            compact=true
                                            label="删除图片"
                                            class="key-event-thumb__delete"
                                            // 点删除别把缩略图也选中（气泡开关在捕获阶段，不受影响）
                                            stop_propagation=true
                                        >
                                            {icons::icon(Icon::Close)}
                                        </IconButton>
                                    </Popconfirm>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </Show>
    }
    .into_any()
}

/// 灯箱：ESC 关闭、←/→ 切换、按钮缩放。
fn lightbox(
    images: RwSignal<Vec<KeyEventImage>>,
    selected_id: RwSignal<String>,
    preview_open: RwSignal<bool>,
    asset_urls: RwSignal<BTreeMap<String, String>>,
) -> AnyView {
    let scale = RwSignal::new(1.0_f64);

    let close = move || {
        preview_open.set(false);
        scale.set(1.0);
    };

    let step = move |delta: i32| {
        let items = images.get_untracked();
        if items.is_empty() {
            return;
        }
        let current = selected_id.get_untracked();
        let index = items
            .iter()
            .position(|image| image.id == current)
            .unwrap_or(0) as i32;
        let next = (index + delta).rem_euclid(items.len() as i32) as usize;
        if let Some(image) = items.get(next) {
            selected_id.set(image.id.clone());
        }
    };

    view! {
        <Show when=move || preview_open.get()>
            <div
                class="key-event-lightbox"
                role="dialog"
                aria-modal="true"
                aria-label="图片预览"
                on:keydown=move |event: leptos::ev::KeyboardEvent| match event.key().as_str() {
                    "Escape" => close(),
                    "ArrowLeft" => step(-1),
                    "ArrowRight" => step(1),
                    _ => {}
                }
                on:click=move |_| close()
            >
                <div
                    class="key-event-lightbox__toolbar"
                    on:click=move |event| event.stop_propagation()
                >
                    <button
                        type="button"
                        class="ui-btn ui-btn--sm ui-btn--icon-only ui-btn--secondary"
                        title="上一张"
                        aria-label="上一张"
                        on:click=move |_| step(-1)
                    >
                        {icons::icon(Icon::Left)}
                    </button>
                    <button
                        type="button"
                        class="ui-btn ui-btn--sm ui-btn--icon-only ui-btn--secondary"
                        title="缩小"
                        aria-label="缩小"
                        on:click=move |_| scale.update(|value| *value = (*value - 0.25).max(0.25))
                    >
                        {icons::icon(Icon::ZoomOut)}
                    </button>
                    <button
                        type="button"
                        class="ui-btn ui-btn--sm ui-btn--icon-only ui-btn--secondary"
                        title="放大"
                        aria-label="放大"
                        on:click=move |_| scale.update(|value| *value = (*value + 0.25).min(4.0))
                    >
                        {icons::icon(Icon::ZoomIn)}
                    </button>
                    <button
                        type="button"
                        class="ui-btn ui-btn--sm ui-btn--icon-only ui-btn--secondary"
                        title="下一张"
                        aria-label="下一张"
                        on:click=move |_| step(1)
                    >
                        {icons::icon(Icon::Right)}
                    </button>
                    <button
                        type="button"
                        class="ui-btn ui-btn--sm ui-btn--icon-only ui-btn--secondary"
                        title="关闭"
                        aria-label="关闭"
                        on:click=move |_| close()
                    >
                        {icons::icon(Icon::Close)}
                    </button>
                </div>
                <img
                    class="key-event-lightbox__img"
                    alt=""
                    src=move || {
                        let current = selected_id.get();
                        let path = images
                            .get()
                            .into_iter()
                            .find(|image| image.id == current)
                            .map(|image| image.file_path)
                            .unwrap_or_default();
                        if path.is_empty() {
                            String::new()
                        } else {
                            asset_urls.get().get(&path).cloned().unwrap_or(path)
                        }
                    }
                    style=move || format!("transform: scale({})", scale.get())
                    on:click=move |event| event.stop_propagation()
                />
            </div>
        </Show>
    }
    .into_any()
}

/// 右栏：关联消费记录。
fn linked_panel(
    linked: RwSignal<Vec<TransactionRecordDto>>,
    selected_date: RwSignal<String>,
    on_delete: UnsyncCallback<String>,
) -> AnyView {
    view! {
        <div class="key-event-linked">
            <Show
                when=move || !selected_date.get().is_empty()
                fallback=|| {
                    view! {
                        <div class="key-event-empty">
                            <span class="key-event-empty__text">"选择事件查看关联交易"</span>
                        </div>
                    }
                }
            >
                <Show
                    when=move || !linked.get().is_empty()
                    fallback=|| {
                        view! {
                            <div class="key-event-empty">
                                <span class="key-event-empty__text">"暂无关联交易"</span>
                            </div>
                        }
                    }
                >
                    <div class="key-event-linked__cards">
                        {move || {
                            linked
                                .get()
                                .into_iter()
                                .map(|record| {
                                    let id = record.transaction_id.clone();
                                    let amount_class = match record.transaction_type.as_str() {
                                        "income" => "amount-income",
                                        "expense" => "amount-expense",
                                        _ => "amount-transfer",
                                    };
                                    let tags = record.tags.clone();
                                    let tags_for_show = tags.clone();
                                    let description = record.description.clone();
                                    let description_for_show = description.clone();
                                    let category = record.category.clone();
                                    let delete_click = UnsyncCallback::new(move |()| on_delete.run(id.clone()));
                                    let amount = format::signed_amount(
                                        &record.transaction_type,
                                        record.price,
                                    );
                                    view! {
                                        <div class="key-event-linked__card">
                                            <div class="key-event-linked__body">
                                                <div class="key-event-linked__row">
                                                    <span class="key-event-linked__value">
                                                        {category}
                                                    </span>
                                                    <span class=format!(
                                                        "key-event-linked__amount {amount_class}",
                                                    )>{amount}</span>
                                                </div>
                                                <div class="key-event-linked__meta">
                                                    <Show when=move || !tags_for_show.is_empty()>
                                                        <div class="key-event-linked__tags">
                                                            {tags
                                                                .clone()
                                                                .into_iter()
                                                                .map(|tag| {
                                                                    view! {
                                                                        <span class="key-event-linked__tag">
                                                                            {tag}
                                                                        </span>
                                                                    }
                                                                })
                                                                .collect_view()}
                                                        </div>
                                                    </Show>
                                                    <Show when=move || !description_for_show.is_empty()>
                                                        <span class="key-event-linked__desc">
                                                            {description.clone()}
                                                        </span>
                                                    </Show>
                                                </div>
                                            </div>
                                            <Popconfirm
                                                title="删除这条关联交易？"
                                                ok_text="删除"
                                                cancel_text="取消"
                                                on_confirm=move || delete_click.run(())
                                            >
                                                <IconButton
                                                    variant=IconButtonVariant::Danger
                                                    label="删除交易"
                                                    class="key-event-linked__delete"
                                                    // 同上：删除只走 `on_confirm`，这里只管开气泡
                                                    stop_propagation=true
                                                >
                                                    {icons::icon(Icon::Trash)}
                                                </IconButton>
                                            </Popconfirm>
                                        </div>
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                </Show>
            </Show>
        </div>
    }
    .into_any()
}

/// 「新增事件」弹窗（固定文案：标题「新增事件」、ok「新增」、cancel「取消」、宽 360）。
fn add_modal(
    open: RwSignal<bool>,
    date: RwSignal<String>,
    title: RwSignal<String>,
    loading: RwSignal<bool>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="新增事件"
            size=ModalSize::Small
            ok_text="新增"
            cancel_text="取消"
            ok_loading=Signal::derive(move || loading.get())
            on_close=move || open.set(false)
            on_ok=move || on_ok.run(())
        >
            <div class="modal-form-item">
                <p class="modal-form-label">"日期"</p>
                <DatePicker value=date />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"名称"</p>
                <Input value=title placeholder="事件名称（可选）" maxlength=200 />
            </div>
        </Modal>
    }
    .into_any()
}
