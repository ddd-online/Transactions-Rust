//! 应用外壳：左侧 200px 导航 + 内容区 + 顶部窗口控制 + 底部状态栏 + 工作空间选择。
//!
//! * 左侧 200px 导航（7 个页面：6 项导航 + 固定在底部的「设置」项）
//! * 顶部右上角窗口控制（最小化 / 最大化 / 关闭 → `window_control`）
//! * 账本切换：挂载时 `ledger_list { id: "all" }`，按 `createdAt` 升序，默认选第一个；
//!   支持新建账本与删除确认
//! * 底部状态栏：左侧工作空间/账本状态，右侧在「消费记录」页显示 `trStatistics` 统计
//!   （仅「消费记录」页渲染底部统计）
//! * `workspace-required` 事件（由 [`crate::ipc`] 在识别到"未打开工作空间"时派发）
//!   与"尚未配置工作空间"都会打开「选择工作空间」流程：`dialog_open` → `workspace_open`
//!
//! 本实现的设计取舍：
//! * 没有"内核状态指示灯"：Rust 版没有子进程内核，进程即应用，该指示灯无对应语义
//! * 没有内核重启后的恢复逻辑（同上），只有工作空间切换

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;

use crate::api;
use crate::components::ui::{IconButton, IconButtonVariant, Input, Modal};
use crate::error_handler::notify_error;
use crate::icons::{self, Icon};
use crate::notify::{Notice, NoticeKind, Notifier};
use crate::pages::{
    CategoryTagPage, DataAnalysisPage, DiaryPage, KeyEventPage, SettingsPage, StockPage,
    TransactionsPage,
};
use crate::store::{AppStores, APPEARANCE_SYSTEM};

/// 页面（共 7 个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 消费记录
    Transactions,
    /// 数据分析
    DataAnalysis,
    /// 股票交易
    Stock,
    /// 关键事件
    KeyEvent,
    /// 日记管理
    Diary,
    /// 分类标签
    CategoryTag,
    /// 应用设置
    Settings,
}

impl Page {
    /// 侧边栏顺序（顺序即渲染顺序）。
    pub const ALL: [Page; 7] = [
        Page::Transactions,
        Page::DataAnalysis,
        Page::Stock,
        Page::KeyEvent,
        Page::Diary,
        Page::CategoryTag,
        Page::Settings,
    ];

    /// 侧边栏中除「设置」之外的 6 项（「设置」固定在最底部）。
    pub const NAV_ITEMS: [Page; 6] = [
        Page::CategoryTag,
        Page::Transactions,
        Page::DataAnalysis,
        Page::Stock,
        Page::KeyEvent,
        Page::Diary,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Transactions => "消费记录",
            Page::DataAnalysis => "数据分析",
            Page::Stock => "股票交易",
            Page::KeyEvent => "关键事件",
            Page::Diary => "日记",
            Page::CategoryTag => "分类标签",
            Page::Settings => "应用设置",
        }
    }

    /// 页面的稳定标识（仅用于调试与占位页展示，不随界面改动）。
    pub fn route(self) -> &'static str {
        match self {
            Page::Transactions => "/tr_view",
            Page::DataAnalysis => "/da_view",
            Page::Stock => "/stock_view",
            Page::KeyEvent => "/key_event_view",
            Page::Diary => "/diary_view",
            Page::CategoryTag => "/category_tag_view",
            Page::Settings => "/settings_view",
        }
    }

    /// 侧边栏图标（与文案一一对应）。
    pub fn icon(self) -> Icon {
        match self {
            // 导航图标的排列：分类标签 / 消费记录 / 数据分析 / 股票交易 /
            // 关键事件 / 日记管理，底部是应用设置
            Page::CategoryTag => Icon::Tag,
            Page::Transactions => Icon::Transaction,
            Page::DataAnalysis => Icon::LineChart,
            Page::Stock => Icon::Stock,
            Page::KeyEvent => Icon::Star,
            Page::Diary => Icon::Read,
            Page::Settings => Icon::Setting,
        }
    }

    /// 页面主要依赖的命令（用于文档与排查；P6-b 之后**全部页面均已实现**）。
    pub fn planned_commands(self) -> &'static str {
        match self {
            Page::Transactions => "tr_query / tr_create / tr_delete / tr_link",
            Page::DataAnalysis => {
                "chart_list / chart_create / chart_update / chart_delete / tr_chart_data"
            }
            Page::Stock => {
                "stock_overview / stock_positions / stock_trades / stock_trade_create / \
                 stock_statistics / stock_history"
            }
            Page::KeyEvent => "key_event_list_by_year / key_event_upsert / key_event_image_add",
            Page::Diary => "diary_list_dates / diary_get / diary_upsert / diary_delete",
            Page::CategoryTag => {
                "category_list / category_create / tag_list / tag_create / template_list"
            }
            Page::Settings => "config_get / config_set_appearance / app_info / update_check",
        }
    }
}

/// 应用根组件。
#[component]
pub fn App() -> impl IntoView {
    // 全局队列与共享状态必须在任何子组件之前安装（子组件的克隆都从全局槽位取）
    let notifier = Notifier::new();
    notifier.install();
    let stores = AppStores::new();
    stores.install();
    // 「关于软件」的更新状态同理：信号必须建在**根 owner** 下，否则会挂在页签组件上，
    // 切走再切回时已 dispose → 该面板整块空白（见 `init_update_state` 的注释）。
    crate::pages::settings::init_update_state();

    let current_page = RwSignal::new(Page::Transactions);
    let workspace_modal_open = RwSignal::new(false);
    let workspace_picking = RwSignal::new(false);

    // 监听 `workspace-required`：由 ipc 层在"未打开工作空间"时派发
    let workspace_required = window_event_listener_untyped("workspace-required", move |_| {
        workspace_modal_open.set(true);
    });
    on_cleanup(move || workspace_required.remove());

    // 首屏初始化：读配置（外观 + 工作空间）→ 有工作空间则打开并拉账本，否则弹选择框。
    // 组件体只执行一次，因此直接 spawn 即可（不需要 Effect 去重）。
    leptos::task::spawn_local(async move {
        match api::desktop::config_get().await {
            Ok(config) => {
                let appearance = if config.appearance.is_empty() {
                    APPEARANCE_SYSTEM.to_string()
                } else {
                    config.appearance.clone()
                };
                stores.appearance.set(appearance);
                stores.apply_appearance();
                stores.workspace_dir.set(config.workspace_dir.clone());

                if config.workspace_dir.is_empty() {
                    workspace_modal_open.set(true);
                } else {
                    open_workspace(config.workspace_dir).await;
                }
            }
            Err(error) => {
                notify_error("读取配置", &error);
                workspace_modal_open.set(true);
            }
        }
    });

    view! {
        <div class="app-shell">
            <div class="app-shell-body">
                <aside class="app-sidebar">
                    <AppLeftBar current_page=current_page />
                </aside>

                <main class="app-content">
                    <TopBar />
                    <NoticeOverlay />
                    <div class="app-router-view">
                        {move || match current_page.get() {
                            Page::Transactions => view! { <TransactionsPage /> }.into_any(),
                            Page::DataAnalysis => view! { <DataAnalysisPage /> }.into_any(),
                            Page::Stock => view! { <StockPage /> }.into_any(),
                            Page::KeyEvent => view! { <KeyEventPage /> }.into_any(),
                            Page::Diary => view! { <DiaryPage /> }.into_any(),
                            Page::CategoryTag => view! { <CategoryTagPage /> }.into_any(),
                            Page::Settings => view! { <SettingsPage /> }.into_any(),
                        }}
                    </div>
                    // 外壳**没有**全局底部栏：底部的收支统计只属于消费记录页，
                    // 由该页把它渲染在功能卡片内部（这样卡片能一直触达窗口底边）。
                    // 工作空间与账本名也不再占用底栏 —— 账本在侧栏顶部、工作空间在设置页。
                </main>
            </div>

            <Modal
                open=workspace_modal_open
                title="新建或打开工作空间"
                ok_text="选择目录…"
                ok_loading=workspace_picking
                on_close=move || workspace_modal_open.set(false)
                on_ok=move || pick_workspace(workspace_modal_open, workspace_picking)
            >
                <p class="workspace-picker-text">
                    "选择一个目录作为工作空间，应用会在其中创建 transactions.db 数据库与 data/assets 资产目录。"
                </p>
                <div class="workspace-picker-path">
                    {move || {
                        let dir = stores.workspace_dir.get();
                        if dir.is_empty() { "（尚未选择）".to_string() } else { dir }
                    }}
                </div>
                <p class="workspace-picker-text">
                    "若目录里已有当前格式的 transactions.db，会直接打开（只读校验，不做迁移）。"
                </p>
            </Modal>
        </div>
    }
}

/// 打开（或切换）工作空间：打开数据库 → 记住目录 → 刷新账本列表。
async fn open_workspace(directory: String) {
    let stores = AppStores::global();
    match api::desktop::workspace_open(&directory).await {
        Ok(()) => {
            stores.workspace_dir.set(directory);
            refresh_ledgers().await;
        }
        Err(error) => {
            notify_error("打开工作空间", &error);
            // 失败时把目录清掉，避免状态栏显示一个并未打开的工作空间
            stores.workspace_dir.set(String::new());
        }
    }
}

/// 弹出目录选择对话框并打开所选目录。
fn pick_workspace(modal_open: RwSignal<bool>, picking: RwSignal<bool>) {
    let stores = AppStores::global();
    if picking.get_untracked() {
        return;
    }
    picking.set(true);
    leptos::task::spawn_local(async move {
        let default_path = stores.workspace_dir.get_untracked();
        match api::desktop::dialog_open("新建或打开工作空间", &default_path).await {
            Ok(response) => {
                if let Some(path) = response.first_path() {
                    open_workspace(path.to_string()).await;
                    modal_open.set(false);
                }
            }
            Err(error) => notify_error("选择工作空间", &error),
        }
        picking.set(false);
    });
}

/// 拉取全部账本（按 `createdAt` 升序、默认选第一个由 [`AppStores::set_ledgers`] 负责）。
async fn refresh_ledgers() {
    let stores = AppStores::global();
    stores.ledgers_loading.set(true);
    let result = api::ledger::list_all().await;
    stores.ledgers_loading.set(false);
    match result {
        Ok(ledgers) => stores.set_ledgers(ledgers),
        Err(error) => notify_error("查询账本", &error),
    }
}

// ---------------------------------------------------------------- 左侧导航

/// 左侧导航：账本切换 + 6 项导航 + 底部设置。
#[component]
fn AppLeftBar(current_page: RwSignal<Page>) -> impl IntoView {
    let stores = AppStores::global();
    let menu_open = RwSignal::new(false);
    let create_open = RwSignal::new(false);
    let create_name = RwSignal::new(String::new());
    let create_description = RwSignal::new(String::new());
    let creating = RwSignal::new(false);
    let delete_target = RwSignal::new(Option::<(String, String)>::None);
    let deleting = RwSignal::new(false);

    let current_ledger_name = move || {
        let name = stores.current_ledger_name();
        if name.is_empty() {
            "选择账本".to_string()
        } else {
            name
        }
    };

    let confirm_create = move || {
        let name = create_name.get_untracked().trim().to_string();
        if name.is_empty() {
            Notifier::global().error("请输入账本名称".to_string(), None);
            return;
        }
        let description = create_description.get_untracked();
        creating.set(true);
        leptos::task::spawn_local(async move {
            match api::ledger::create(&name, &description).await {
                Ok(id) => {
                    refresh_ledgers().await;
                    AppStores::global().select_ledger(id);
                    create_open.set(false);
                    create_name.set(String::new());
                    create_description.set(String::new());
                }
                Err(error) => notify_error("创建账本", &error),
            }
            creating.set(false);
        });
    };

    let confirm_delete = move || {
        let Some((id, name)) = delete_target.get_untracked() else {
            return;
        };
        deleting.set(true);
        leptos::task::spawn_local(async move {
            match api::ledger::delete(&id).await {
                Ok(()) => {
                    delete_target.set(None);
                    refresh_ledgers().await;
                    let _ = name;
                }
                Err(error) => notify_error("删除账本", &error),
            }
            deleting.set(false);
        });
    };

    view! {
        <div class="app-left-bar">
            <div class="sidebar-ledger">
                // 账本切换：触发器 + 下拉菜单共用一个定位锚点，
                // 菜单因此**贴齐触发器**、只隔 4px，不会像以前那样压在触发器上。
                <div class="ledger-anchor">
                    <button
                        type="button"
                        class="ledger-btn"
                        class:is-open=move || menu_open.get()
                        title="切换账本"
                        aria-haspopup="menu"
                        aria-expanded=move || if menu_open.get() { "true" } else { "false" }
                        on:click=move |_| menu_open.update(|open| *open = !*open)
                        on:keydown=move |ev| {
                            if ev.key() == "Escape" {
                                menu_open.set(false);
                            }
                        }
                    >
                        <span class=move || {
                            format!(
                                "ledger-dot ledger-dot--{}",
                                ledger_tone(&stores.current_ledger_id.get()),
                            )
                        }></span>
                        <span class="ledger-btn-name">{current_ledger_name}</span>
                        <span class="ledger-btn-arrow">{icons::icon(Icon::Down)}</span>
                    </button>

                    <Show when=move || menu_open.get()>
                        <div class="ledger-menu-layer">
                            <div
                                class="ui-select__backdrop"
                                on:click=move |_| menu_open.set(false)
                            ></div>
                            <div class="ledger-menu" role="menu">
                                {move || {
                                    let ledgers = stores.ledgers.get();
                                    let current = stores.current_ledger_id.get();
                                    if ledgers.is_empty() {
                                        view! {
                                            <div class="ledger-menu-empty">
                                                {move || {
                                                    if stores.ledgers_loading.get() {
                                                        "正在加载账本…"
                                                    } else {
                                                        "暂无账本"
                                                    }
                                                }}
                                            </div>
                                        }
                                            .into_any()
                                    } else {
                                        ledgers
                                            .into_iter()
                                            .map(|ledger| {
                                                let id = ledger.id.clone();
                                                let name = ledger.name.clone();
                                                let is_active = id == current;
                                                let tone = ledger_tone(&id);
                                                let select_id = id.clone();
                                                let delete_id = id.clone();
                                                let delete_name = name.clone();
                                                view! {
                                                    <div class="ledger-menu-row" class:is-active=is_active>
                                                        <button
                                                            type="button"
                                                            class="ledger-menu-name"
                                                            role="menuitem"
                                                            on:click=move |_| {
                                                                stores.select_ledger(select_id.clone());
                                                                menu_open.set(false);
                                                            }
                                                        >
                                                            <span class=format!(
                                                                "ledger-dot ledger-dot--{tone}",
                                                            )></span>
                                                            <span class="ledger-menu-name-text">{name}</span>
                                                        </button>
                                                        <IconButton
                                                            variant=IconButtonVariant::Danger
                                                            label="删除账本"
                                                            class="ledger-menu-delete"
                                                            on_click=move |_| {
                                                                delete_target
                                                                    .set(
                                                                        Some((
                                                                            delete_id.clone(),
                                                                            delete_name.clone(),
                                                                        )),
                                                                    );
                                                                menu_open.set(false);
                                                            }
                                                        >
                                                            {icons::icon(Icon::Trash)}
                                                        </IconButton>
                                                    </div>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                }}
                                <div class="ledger-menu-divider"></div>
                                // 「创建账本」放在列表**下方**（发丝线隔开）：这一列首先是"选一个账本"，
                                // 新建是次要动作；放顶部会让每次打开都先看到动作而不是内容。
                                <button
                                    type="button"
                                    class="ledger-menu-create"
                                    role="menuitem"
                                    on:click=move |_| {
                                        menu_open.set(false);
                                        create_open.set(true);
                                    }
                                >
                                    <span class="ledger-menu-create-icon">
                                        {icons::icon(Icon::Plus)}
                                    </span>
                                    <span>"创建账本"</span>
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>
            </div>

            <nav class="sidebar-nav" aria-label="主导航">
                {Page::NAV_ITEMS
                    .iter()
                    .map(|page| nav_button(*page, current_page))
                    .collect_view()}
            </nav>

            <div class="sidebar-spacer"></div>

            <div class="sidebar-bottom">
                {nav_button(Page::Settings, current_page)}
            </div>

            <Modal
                open=create_open
                title="创建账本"
                width=400
                ok_text="创建"
                ok_loading=creating
                on_close=move || create_open.set(false)
                on_ok=move || confirm_create()
            >
                <div class="modal-form-item">
                    <p class="modal-form-label">"名称"</p>
                    <Input value=create_name placeholder="请输入账本名称" maxlength=20 />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"描述"</p>
                    <Input value=create_description placeholder="请输入账本描述" maxlength=50 />
                </div>
            </Modal>

            <Modal
                open=Signal::derive(move || delete_target.get().is_some())
                title="确认删除"
                width=400
                ok_text="删除"
                ok_loading=deleting
                on_close=move || delete_target.set(None)
                on_ok=move || confirm_delete()
            >
                <p class="workspace-picker-text">
                    {move || match delete_target.get() {
                        Some((_, name)) => {
                            format!("确定要删除账本「{name}」吗？")
                        }
                        None => String::new(),
                    }}
                </p>
            </Modal>
        </div>
    }
}

/// 账本的**身份色**索引（0..8）。
///
/// DESIGN.md 的 "Ledger identity" 给了八个自然色、说它们"给每个账本一个稳定身份"，
/// 但此前从未被使用过。这里用账本 id 做 FNV-1a 哈希取色：同一个账本永远同一个颜色，
/// **纯展示、不落库**（换版本、换机器都一样，因为只依赖 id）。
fn ledger_tone(id: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 8) as usize
}

/// 一个导航按钮。
fn nav_button(page: Page, current_page: RwSignal<Page>) -> impl IntoView {
    let is_secondary = page == Page::Settings;
    let mut classes = String::from("nav-btn");
    if is_secondary {
        classes.push_str(" nav-btn-secondary");
    }
    view! {
        <button
            type="button"
            class=classes
            class:active=move || current_page.get() == page
            title=page.route()
            aria-label=page.label()
            on:click=move |_| current_page.set(page)
        >
            <span class="nav-btn-icon">{icons::icon(page.icon())}</span>
            <span class="nav-btn-text">{page.label()}</span>
        </button>
    }
}

// ---------------------------------------------------------------- 顶部窗口控制

/// 当前窗口是否最大化（仅用于切换图标；Rust 版没有窗口状态事件通道，
/// 因此这里按点击次数维护一个本地状态，与 `window_control` 的 toggle 语义一致）。
#[component]
fn TopBar() -> impl IntoView {
    let maximized = RwSignal::new(false);

    // 订阅外壳在每次 `Resized` 时广播的**真实**窗口状态
    // （`src-tauri/src/shell.rs` 发 `window-state-changed`，载荷是 `bool`）。
    //
    // 为什么必须有它：双击标题栏 / Win+↑ / 拖到屏幕顶部贴靠都不会经过
    // `window_control`，只靠下面的"乐观取反"会让图标与真实状态漂移，
    // 于是"点一下"的文案与动作不一致。
    // 乐观取反保留（点击立刻有反馈），但事件到来后以事件值为准。
    crate::ipc::listen::<bool, _>("window-state-changed", move |value| maximized.set(value));

    let send = move |action: api::desktop::WindowAction| {
        leptos::task::spawn_local(async move {
            if let Err(error) = api::desktop::window_control(action).await {
                notify_error("窗口控制", &error);
            }
        });
    };

    view! {
        <div class="app-top-bar">
            <div class="window-controls">
                <button
                    type="button"
                    class="window-btn"
                    aria-label="最小化"
                    title="最小化"
                    on:click=move |_| send(api::desktop::WindowAction::Minimize)
                >
                    {icons::icon(Icon::WindowMinimize)}
                </button>
                <button
                    type="button"
                    class="window-btn"
                    aria-label="最大化"
                    title=move || if maximized.get() { "还原" } else { "最大化" }
                    on:click=move |_| {
                        maximized.update(|value| *value = !*value);
                        send(api::desktop::WindowAction::Maximize);
                    }
                >
                    {move || {
                        if maximized.get() {
                            icons::icon(Icon::WindowRestore)
                        } else {
                            icons::icon(Icon::WindowMaximize)
                        }
                    }}
                </button>
                <button
                    type="button"
                    class="window-btn window-btn--close"
                    aria-label="关闭"
                    title="关闭"
                    on:click=move |_| send(api::desktop::WindowAction::Close)
                >
                    {icons::icon(Icon::Close)}
                </button>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------- 消息与通知

/// 消息（底部）与通知（右上角，`top: 96px`）两个队列。
///
/// 有 description 走通知（右上角），否则走消息（底部）。
#[component]
fn NoticeOverlay() -> impl IntoView {
    let notifier = Notifier::global();
    let items = notifier.items();

    let messages = move || {
        items
            .get()
            .into_iter()
            .filter(|notice| !notice.is_notification())
            .collect::<Vec<Notice>>()
    };
    let notifications = move || {
        items
            .get()
            .into_iter()
            .filter(Notice::is_notification)
            .collect::<Vec<Notice>>()
    };

    view! {
        <div class="notice-stack-message">
            {move || {
                messages()
                    .into_iter()
                    .map(|notice| {
                        let id = notice.id;
                        view! {
                            <div class=format!("notice-message {}", notice.kind.class())>
                                <span class="notice-message__icon">
                                    {icons::icon(notice_kind_icon(notice.kind))}
                                </span>
                                <span>{notice.title}</span>
                                <button
                                    type="button"
                                    class="notice__close"
                                    title="关闭"
                                    aria-label="关闭"
                                    on:click=move |_| notifier.dismiss(id)
                                >
                                    {icons::icon(Icon::Close)}
                                </button>
                            </div>
                        }
                    })
                    .collect_view()
            }}
        </div>

        <div class="notice-stack-notification">
            {move || {
                notifications()
                    .into_iter()
                    .map(|notice| {
                        let id = notice.id;
                        view! {
                            <div class="notice-notification">
                                <span class=format!(
                                    "notice-notification__icon {}",
                                    notice.kind.class(),
                                )>{icons::icon(notice_kind_icon(notice.kind))}</span>
                                <div class="notice-notification__body">
                                    <p class="notice-notification__title">{notice.title}</p>
                                    <p class="notice-notification__description">
                                        {notice.description}
                                    </p>
                                </div>
                                <button
                                    type="button"
                                    class="notice__close"
                                    title="关闭"
                                    aria-label="关闭"
                                    on:click=move |_| notifier.dismiss(id)
                                >
                                    {icons::icon(Icon::Close)}
                                </button>
                            </div>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
}

fn notice_kind_icon(kind: NoticeKind) -> Icon {
    match kind {
        NoticeKind::Success => Icon::CheckCircle,
        NoticeKind::Info => Icon::InfoCircle,
        NoticeKind::Warning => Icon::WarningCircle,
        NoticeKind::Error => Icon::CloseCircle,
    }
}

// ---------------------------------------------------------------- 底部（已内嵌到页面）
//
// 外壳不再持有全局底部栏：
//   * 工作空间与账本名不再常驻屏幕（账本在侧栏顶部，工作空间在设置页）；
//   * 收支统计只属于「消费记录」页，由 `pages/transactions.rs` 渲染在功能卡片内部，
//     于是卡片能一直触达窗口底边。见该文件里的 `StatisticsFooter`。
