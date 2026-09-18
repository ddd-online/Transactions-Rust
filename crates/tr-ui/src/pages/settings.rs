//! 应用设置页（P6-a）：5 个分栏。
//!
//! ## 对照的原 Vue 文件清单
//!
//! | 分栏 | 原组件 |
//! |---|---|
//! | （分栏标题与顺序） | `settings_view/SettingsView.vue` |
//! | 通用设置 | `settings_view/GeneralSetting.vue` |
//! | 消费模板 | `settings_view/TransactionsTemplateSetting.vue`（+ `tr_view/TransactionRecordModal.vue` 的「保存为模板」弹窗） |
//! | 日记配置 | `settings_view/DiarySetting.vue` |
//! | 股票交易 | `settings_view/StockTradingSetting.vue` + `stock_view/StockAccountView.vue` 的「交易费用设置」 |
//! | 关于软件 | `settings_view/AboutSetting.vue`（+ `stores/updateStore.ts` 的状态机） |
//!
//! ## 有意差异（逐条，均为有意为之）
//!
//! 1. **分栏导航形态**：原文是左侧 200px 竖向 `nav-item` 导航（`SettingsView.vue`），
//!    本实现改用顶部 `Tabs` + `TabPane`（本轮任务要求交付并验证这两个组件）。
//!    每个分栏内部仍按 `SettingsPageWrapper.vue` 的「分栏名标题 + 卡片列表」结构排布，
//!    卡片外观（白底 / 1px `--transactions-color-divider` / `--transactions-radius-md` /
//!    hover 底色 `--transactions-color-hover-bg` / padding 12px 16px）照抄原文。
//! 2. **通用设置·工作空间**：原文按钮文案是「切换」、弹窗标题「选择工作目录」、输入框占位
//!    「请输入或选择工作目录路径」（任务单里概括成「更换目录…」，未采用）。
//!    本仓库没有 `transactions-file-select` 组件，直接用 `dialog_open` 选目录。
//! 3. **通用设置不展示版本信息**：原文该分栏只有 4 张卡片；`app_info` 的 version/isDev
//!    只出现在「关于软件」（与原文一致）。`config_get()` 仍一次取全
//!    （workspace_dir / close_behavior / appearance / config_path / is_dev），
//!    其中 `config_path` 与 `is_dev` 按原文**不渲染**（原设置页没有对应 UI；
//!    构建类型改用关于页的 `app_info("isDev")` 展示）。
//! 4. **通用设置·外观/关闭行为/开发者工具**：原文 `appearanceStore` 静默吞掉持久化失败，
//!    本实现失败时提示并**回滚界面选中值**（任务单要求）；开发者工具以 `devtools_toggle`
//!    的返回布尔为准。
//! 5. **消费模板·新建模板是增补**：v0.27 的设置页**没有**新建入口（模板在「记一笔」弹窗里
//!    「保存为模板」）。本实现按任务单增补「新建模板」弹窗，文案沿用
//!    `TransactionRecordModal.vue`（模板名称 / 请输入模板名称 / 保存模板失败 / 保存模板成功）。
//! 6. **消费模板·列表不是 `Table`**：行由 `DragSortItem` 渲染成 `<div>`（整行 draggable），
//!    而 `<div>` 不能作为 `<tbody>` 的子节点，因此列宽/表头照抄原文但用 `st-table` 的 div 网格实现。
//! 7. **日记配置**：原文**没有**「文件勾选」（扫描后顺序导入全部文件），本实现照原文，
//!    只是每行状态；原文「浏览器 dev 模式降级」分支（手输路径）在 Tauri 下不存在，未移植；
//!    原文导入完成后的 `diaryStore.loadDates()` 只影响日记页，本页不需要，省略。
//! 8. **股票交易·费用设置卡片来自 `stock_view/StockAccountView.vue`**（设置页原文没有它）。
//!    换算照抄原文：佣金费率按「万分之」（×10000），印花税/过户费按「%」（×100）。
//!    原文把印花税/过户费 的 tooltip 内联在 label 里，本实现因 `FormItem` 的 label 是字符串，
//!    把问号图标挪到输入框行尾（语义不变）。
//!    最低佣金回填走 `tr_domain::money::cents_to_yuan`（任务要求的唯一金额换算入口），
//!    因此显示两位小数（`5.00`），原文用 `parseFloat((分/100).toFixed(2))` 会显示 `5`——
//!    这是**有意的**：金额换算一律过 `tr_domain::money`，不自行实现 `/100`。
//!    费率输入非法（非数字 / `NaN` / `inf`）时不发请求、只提示（不 panic）。
//! 9. **关于软件**：原文没有 GitHub 链接（本轮任务要求增补）；**构建时间后端未提供**
//!    （`app_info` 只支持 `name` / `version` / `isDev`），故未展示，改为展示构建类型
//!    （开发版 / 正式版）。更新说明照任务单按**纯文本 + 保留换行**渲染（原文是 MarkdownViewer，
//!    本仓库没有 Markdown 解析器）；下载进度原文只渲染 `percent`，本实现额外用小字显示 `speed`。
//! 10. 全页不使用 `unwrap` / `expect` 处理用户数据：解析失败、命令失败一律走通知。

use std::cell::RefCell;
use std::time::Duration;

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;
use tr_domain::dto::{DiaryExportFileError, TransactionTemplateDto};
use tr_domain::models::StockFeeSetting;
use tr_domain::money::{cents_to_yuan, yuan_to_cents};

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, CheckboxGroup, CheckboxOption, DragSortItem,
    DragSortState, Empty, Form, FormItem, FormLayout, Input, Modal, Popconfirm, Progress,
    Segmented, SegmentedOption, Select, SelectOption, Spin, SpinSize, Switch, TabItem, TabPane,
    Tabs, Tag, TagKind, Tooltip,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::ipc;
use crate::notify::Notifier;
use crate::store::{AppStores, APPEARANCE_DARK, APPEARANCE_LIGHT, APPEARANCE_SYSTEM};

/// 页面标题（原 `SettingsView.vue` 的「应用设置」）。
pub const PAGE_TITLE: &str = "应用设置";

const TAB_GENERAL: &str = "general";
const TAB_TEMPLATE: &str = "template";
const TAB_DIARY: &str = "diary";
const TAB_STOCK: &str = "stock";
const TAB_ABOUT: &str = "about";

/// 交易费用说明（`StockAccountView.vue` 的 tooltip 文案，逐字照抄；
/// 原文用 `<br />` 换行，这里用 `\n` + CSS `white-space: pre-line`）。
const FEE_TOOLTIP: &str = "佣金：委托成交总额 × 费率，不足最低佣金时按最低佣金收取（买卖双向）\n一笔委托分多笔成交时，费用按委托成交总额计算一次，再按各笔成交金额比例分摊\n买入实际成本 = 成交金额 + 佣金 + 过户费";
/// 印花税说明（逐字照抄）。
const STAMP_TOOLTIP: &str = "卖出时按成交金额 × 费率收取";
/// 过户费说明（逐字照抄）。
const TRANSFER_TOOLTIP: &str = "买卖双向收取，仅沪市（60/68 开头）适用";
/// 交易标签最多保存数量（原文 `tags.length >= 20`）。
const MAX_STOCK_TAGS: usize = 20;
/// 外部链接（本轮增补，原文没有）。
const GITHUB_URL: &str = "https://github.com/ddd-online/Transactions";

#[component]
pub fn SettingsPage() -> impl IntoView {
    let active = RwSignal::new(TAB_GENERAL.to_string());
    let items = vec![
        TabItem::new(TAB_GENERAL, "通用设置"),
        TabItem::new(TAB_TEMPLATE, "消费模板"),
        TabItem::new(TAB_DIARY, "日记配置"),
        TabItem::new(TAB_STOCK, "股票交易"),
        TabItem::new(TAB_ABOUT, "关于软件"),
    ];

    view! {
        <section class="page">
            <header class="page-header">
                <div class="page-header-text">
                    <h1 class="page-title">{PAGE_TITLE}</h1>
                    <p class="page-subtitle">"通用设置 · 消费模板 · 日记配置 · 股票交易 · 关于软件"</p>
                </div>
                <div class="app-top-bar-spacer"></div>
            </header>

            <div class="page-body">
                <Tabs active=active items=items class="st-tabs" />
                <div class="st-panes">
                    <TabPane active=active key=TAB_GENERAL>
                        <GeneralSetting />
                    </TabPane>
                    <TabPane active=active key=TAB_TEMPLATE>
                        <TemplateSetting />
                    </TabPane>
                    <TabPane active=active key=TAB_DIARY>
                        <DiarySetting />
                    </TabPane>
                    <TabPane active=active key=TAB_STOCK>
                        <StockSetting />
                    </TabPane>
                    <TabPane active=active key=TAB_ABOUT>
                        <AboutSetting />
                    </TabPane>
                </div>
            </div>
        </section>
    }
}

// ---------------------------------------------------------------- 通用设置

/// 通用设置（`GeneralSetting.vue`）：工作空间 / 外观 / 关闭行为 / 开发者工具。
#[component]
fn GeneralSetting() -> impl IntoView {
    let stores = AppStores::global();

    let close_behavior = RwSignal::new(String::new());
    let appearance = RwSignal::new(stores.appearance.get_untracked());
    let devtools = RwSignal::new(false);
    let switching = RwSignal::new(false);

    // DevTools 真实状态同步（原 `onDevToolsStateChanged`）：开关始终跟随主进程，
    // 避免"启动时自动打开 / 从 DevTools 自身按钮关闭"导致的状态脱节。
    ipc::listen::<bool, _>(api::desktop::EVENT_DEVTOOLS_STATE_CHANGED, move |opened| {
        devtools.set(opened)
    });

    // 首屏：`config_get()` 一次拿全（workspace_dir / close_behavior / appearance /
    // config_path / is_dev），再取 DevTools 初值。
    leptos::task::spawn_local(async move {
        match api::desktop::config_get().await {
            Ok(config) => {
                stores.workspace_dir.set(config.workspace_dir);
                close_behavior.set(config.close_behavior);
                let mode = if config.appearance.is_empty() {
                    APPEARANCE_SYSTEM.to_string()
                } else {
                    config.appearance
                };
                appearance.set(mode.clone());
                stores.appearance.set(mode);
                stores.apply_appearance();
            }
            Err(error) => notify_error("读取配置", &error),
        }

        match api::desktop::devtools_get_state().await {
            Ok(state) => devtools.set(state),
            Err(error) => notify_error("读取开发者工具状态", &error),
        }
    });

    // ---- 工作空间：dialog_open → workspace_open → 重新拉账本 ----
    let switch_workspace = move || {
        if switching.get_untracked() {
            return;
        }
        switching.set(true);
        leptos::task::spawn_local(async move {
            let current = stores.workspace_dir.get_untracked();
            match api::desktop::dialog_open("选择工作目录", &current).await {
                Ok(response) => {
                    if let Some(directory) = response.first_path().map(str::to_string) {
                        match api::desktop::workspace_open(&directory).await {
                            Ok(()) => {
                                stores.workspace_dir.set(directory);
                                match api::ledger::list_all().await {
                                    Ok(ledgers) => stores.set_ledgers(ledgers),
                                    Err(error) => notify_error("查询账本", &error),
                                }
                                Notifier::global().success("切换工作空间成功", None);
                            }
                            Err(error) => notify_error("切换工作空间失败", &error),
                        }
                    } else if let Some(message) = response.error {
                        Notifier::global().error("切换工作空间失败", Some(message));
                    }
                }
                Err(error) => notify_error("选择工作目录", &error),
            }
            switching.set(false);
        });
    };

    // ---- 外观：立即生效；失败回滚选中值 ----
    let change_appearance = move |mode: String| {
        let previous = stores.appearance.get_untracked();
        leptos::task::spawn_local(async move {
            match api::desktop::config_set_appearance(&mode).await {
                Ok(()) => {
                    stores.appearance.set(mode);
                    stores.apply_appearance();
                }
                Err(error) => {
                    appearance.set(previous);
                    notify_error("设置外观", &error);
                }
            }
        });
    };

    // ---- 关闭行为 ----
    let change_close_behavior = move |mode: String| {
        let previous = close_behavior.get_untracked();
        leptos::task::spawn_local(async move {
            match api::desktop::config_set_close_behavior(&mode).await {
                Ok(()) => {}
                Err(error) => {
                    close_behavior.set(previous);
                    notify_error("设置关闭行为", &error);
                }
            }
        });
    };

    // ---- 开发者工具：以后端返回的布尔为准 ----
    let toggle_devtools = move |enabled: bool| {
        leptos::task::spawn_local(async move {
            match api::desktop::devtools_toggle(enabled).await {
                Ok(state) => devtools.set(state),
                Err(error) => {
                    devtools.set(!enabled);
                    notify_error("切换开发者工具", &error);
                }
            }
        });
    };

    let workspace_text = move || {
        let directory = stores.workspace_dir.get();
        if directory.is_empty() {
            "未设置工作空间".to_string()
        } else {
            directory
        }
    };

    view! {
        <div class="st-list">
            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"工作空间"</span>
                    <span
                        class="st-card-desc st-card-desc--mono"
                        class:st-card-desc--empty=move || stores.workspace_dir.get().is_empty()
                    >
                        {workspace_text}
                    </span>
                </div>
                <div class="st-card-action">
                    <Button
                        variant=ButtonVariant::Secondary
                        loading=switching
                        on_click=move || switch_workspace()
                    >
                        "切换"
                    </Button>
                </div>
            </div>

            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"外观"</span>
                    <span class="st-card-desc">"界面颜色方案，可跟随系统"</span>
                </div>
                <div class="st-card-action">
                    <Segmented
                        value=appearance
                        options=vec![
                            SegmentedOption::new(APPEARANCE_LIGHT, "浅色"),
                            SegmentedOption::new(APPEARANCE_DARK, "深色"),
                            SegmentedOption::new(APPEARANCE_SYSTEM, "跟随系统"),
                        ]
                        on_change=move |mode: String| change_appearance(mode)
                    />
                </div>
            </div>

            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"关闭行为"</span>
                    <span class="st-card-desc">"点击关闭按钮时的操作"</span>
                </div>
                <div class="st-card-action">
                    <Segmented
                        value=close_behavior
                        options=vec![
                            SegmentedOption::new("quit", "直接关闭"),
                            SegmentedOption::new("tray", "缩小到托盘"),
                        ]
                        on_change=move |mode: String| change_close_behavior(mode)
                    />
                </div>
            </div>

            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"开发者工具"</span>
                    <span class="st-card-desc">"打开 Chromium DevTools，用于调试前端代码"</span>
                </div>
                <div class="st-card-action">
                    <Switch
                        checked=devtools
                        on_change=move |enabled: bool| toggle_devtools(enabled)
                    />
                </div>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------- 消费模板

/// 消费模板（`TransactionsTemplateSetting.vue`）：列表 + 删除 + 拖拽排序 + （增补）新建。
#[component]
fn TemplateSetting() -> impl IntoView {
    let stores = AppStores::global();

    let templates = RwSignal::new(Vec::<TransactionTemplateDto>::new());
    let loading = RwSignal::new(false);
    let drag = DragSortState::new();

    // 新建模板弹窗（增补，见文件头注释 5）
    let create_open = RwSignal::new(false);
    let creating = RwSignal::new(false);
    let form_name = RwSignal::new(String::new());
    let form_type = RwSignal::new("expense".to_string());
    let form_category = RwSignal::new(String::new());
    let form_tags = RwSignal::new(Vec::<String>::new());
    let form_description = RwSignal::new(String::new());
    let form_outlier = RwSignal::new(false);
    let categories = RwSignal::new(Vec::<String>::new());
    let tag_names = RwSignal::new(Vec::<String>::new());

    let reset_form = move || {
        form_name.set(String::new());
        form_type.set("expense".to_string());
        form_category.set(String::new());
        form_tags.set(Vec::new());
        form_description.set(String::new());
        form_outlier.set(false);
    };

    // ---- 加载：`template_list(ledgerId)`（错误前缀「查询模板失败」，回落空数组） ----
    let load = move |ledger_id: String| {
        if ledger_id.is_empty() {
            templates.set(Vec::new());
            loading.set(false);
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::template::list(&ledger_id).await {
                Ok(list) => templates.set(list),
                Err(error) => {
                    templates.set(Vec::new());
                    notify_error("查询模板失败", &error);
                }
            }
            loading.set(false);
        });
    };

    // 账本变化 → 重新加载
    Effect::new(move |_: Option<()>| load(stores.current_ledger_id.get()));

    // ---- 拖拽排序：只对 `sort_order` 变化的项发请求，且逐条互不中断 ----
    let reorder = move |from: usize, to: usize| {
        let mut list = templates.get_untracked();
        if from >= list.len() || to >= list.len() || from == to {
            return;
        }
        let previous: Vec<(String, i32)> = list
            .iter()
            .map(|item| (item.template_id.clone(), item.sort_order))
            .collect();

        let moved = list.remove(from);
        list.insert(to, moved);
        for (index, item) in list.iter_mut().enumerate() {
            item.sort_order = index as i32;
        }

        let ledger_id = stores.current_ledger_id.get_untracked();
        let pending: Vec<(String, i32)> = list
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                previous
                    .iter()
                    .any(|(id, order)| id == &item.template_id && *order != *index as i32)
            })
            .map(|(index, item)| (item.template_id.clone(), index as i32))
            .collect();

        templates.set(list);

        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            for (id, sort_order) in pending {
                if let Err(error) = api::template::update_sort(&id, &ledger_id, sort_order).await {
                    notify_error("更新模板排序失败", &error);
                }
            }
        });
    };

    // ---- 删除 ----
    let delete_template = move |template_id: String| {
        leptos::task::spawn_local(async move {
            match api::template::delete(&template_id).await {
                Ok(()) => {
                    Notifier::global().success("删除模板成功", None);
                    load(stores.current_ledger_id.get_untracked());
                }
                Err(error) => notify_error("删除模板失败", &error),
            }
        });
    };

    // ---- 新建模板 ----
    let open_create = move || {
        reset_form();
        create_open.set(true);
    };

    // 分类：随「弹窗打开 / 交易类型 / 账本」变化重新拉取
    Effect::new(move |_: Option<()>| {
        let open = create_open.get();
        let transaction_type = form_type.get();
        let ledger_id = stores.current_ledger_id.get();
        if !open {
            return;
        }
        form_category.set(String::new());
        form_tags.set(Vec::new());
        if ledger_id.is_empty() {
            categories.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            match api::category::list(&transaction_type, &ledger_id).await {
                Ok(list) => categories.set(list.into_iter().map(|item| item.name).collect()),
                Err(error) => {
                    categories.set(Vec::new());
                    notify_error("查询分类失败", &error);
                }
            }
        });
    });

    // 标签：`tag_list("{分类}:{类型}", ledgerId)`
    Effect::new(move |_: Option<()>| {
        let open = create_open.get();
        let category = form_category.get();
        let transaction_type = form_type.get();
        let ledger_id = stores.current_ledger_id.get();
        form_tags.set(Vec::new());
        if !open || category.is_empty() || ledger_id.is_empty() {
            tag_names.set(Vec::new());
            return;
        }
        let key = format!("{category}:{transaction_type}");
        leptos::task::spawn_local(async move {
            match api::tag::list(&key, &ledger_id).await {
                Ok(list) => tag_names.set(list.into_iter().map(|item| item.name).collect()),
                Err(error) => {
                    tag_names.set(Vec::new());
                    notify_error("查询标签失败", &error);
                }
            }
        });
    });

    let submit_create = move || {
        if creating.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先打开工作空间", None);
            return;
        }
        // 前端先挡一道后端 `validate()` 的三条错误（文案与后端一致，中文）
        let name = form_name.get_untracked().trim().to_string();
        if name.is_empty() {
            Notifier::global().error("模板名称不能为空", None);
            return;
        }
        let category = form_category.get_untracked();
        if category.is_empty() {
            Notifier::global().error("分类不能为空", None);
            return;
        }

        let dto = TransactionTemplateDto {
            template_id: String::new(),
            ledger_id: ledger_id.clone(),
            template_name: name,
            transaction_type: form_type.get_untracked(),
            category,
            tags: form_tags.get_untracked(),
            // 原文 `trForm.flags.join(',')`：勾选离群值时写入标记名
            flags: if form_outlier.get_untracked() {
                "outlier".to_string()
            } else {
                String::new()
            },
            description: form_description.get_untracked(),
            sort_order: 0,
        };

        creating.set(true);
        leptos::task::spawn_local(async move {
            match api::template::create(dto).await {
                Ok(_id) => {
                    Notifier::global().success("保存模板成功", None);
                    create_open.set(false);
                    reset_form();
                    load(ledger_id);
                }
                Err(error) => notify_error("保存模板失败", &error),
            }
            creating.set(false);
        });
    };

    view! {
        <div class="st-pane">
            <div class="st-toolbar">
                <h2 class="st-pane-title">"消费模板"</h2>
                <Button variant=ButtonVariant::Primary on_click=move || open_create()>
                    "新建模板"
                </Button>
            </div>

            <div class="st-table">
                <div class="st-thead">
                    <div class="st-th st-th--drag"></div>
                    <div class="st-th">"模板名称"</div>
                    <div class="st-th st-th--center">"交易类型"</div>
                    <div class="st-th">"分类"</div>
                    <div class="st-th">"标签"</div>
                    <div class="st-th">"标记"</div>
                    <div class="st-th">"描述"</div>
                    <div class="st-th st-th--center">"操作"</div>
                </div>

                <div class="st-tbody">
                    {move || {
                        let list = templates.get();
                        if list.is_empty() {
                            if loading.get() {
                                view! {
                                    <div class="st-loading">
                                        <Spin spinning=true size=SpinSize::Small />
                                        <span>"正在加载模板…"</span>
                                    </div>
                                }
                                    .into_any()
                            } else {
                                view! { <Empty title="暂无模板" /> }.into_any()
                            }
                        } else {
                            list
                                .into_iter()
                                .enumerate()
                                .map(|(index, template)| {
                                    template_row(template, index, drag, reorder, delete_template)
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </div>
            </div>

            <Modal
                open=create_open
                title="新建模板"
                width=520
                ok_text="保存"
                cancel_text="取消"
                ok_loading=creating
                on_close=move || create_open.set(false)
                on_ok=move || submit_create()
            >
                <Form layout=FormLayout::Vertical>
                    <FormItem label="模板名称">
                        <Input value=form_name placeholder="请输入模板名称" maxlength=20 />
                    </FormItem>
                    <FormItem label="交易类型">
                        <Segmented
                            value=form_type
                            options=vec![
                                SegmentedOption::new("expense", "支出"),
                                SegmentedOption::new("income", "收入"),
                                SegmentedOption::new("transfer", "转账"),
                            ]
                        />
                    </FormItem>
                    <FormItem label="分类">
                        {move || {
                            let options = categories
                                .get()
                                .into_iter()
                                .map(SelectOption::same)
                                .collect::<Vec<SelectOption>>();
                            view! {
                                <Select
                                    value=form_category
                                    options=options
                                    placeholder="请选择分类"
                                    searchable=true
                                />
                            }
                        }}
                    </FormItem>
                    <FormItem label="标签">
                        {move || {
                            let options = tag_names
                                .get()
                                .into_iter()
                                .map(CheckboxOption::same)
                                .collect::<Vec<CheckboxOption>>();
                            if options.is_empty() {
                                view! { <span class="st-hint">"该分类下暂无标签"</span> }.into_any()
                            } else {
                                view! { <CheckboxGroup values=form_tags options=options /> }.into_any()
                            }
                        }}
                    </FormItem>
                    <FormItem label="描述">
                        <Input value=form_description placeholder="请输入描述" maxlength=50 />
                    </FormItem>
                    <FormItem label="标记">
                        <Checkbox checked=form_outlier label="离群值" />
                    </FormItem>
                </Form>
            </Modal>
        </div>
    }
}

/// 一行模板：`DragSortItem` 作整行容器（原 `SortableJS` 的 `.drag-handle` 是视觉元素）。
fn template_row(
    template: TransactionTemplateDto,
    index: usize,
    drag: DragSortState,
    on_drop: impl Fn(usize, usize) + Copy + 'static,
    on_delete: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let template_id = template.template_id.clone();
    let name = template.template_name.clone();
    let name_title = name.clone();
    let type_text = format::transaction_type_text(&template.transaction_type);
    let tag_kind = TagKind::from_transaction_type(&template.transaction_type);
    let category = if template.category.is_empty() {
        "-".to_string()
    } else {
        template.category.clone()
    };
    let tags = template.tags.clone();
    let tags_empty = tags.is_empty();
    let has_flags = !template.flags.is_empty();
    let description_raw = template.description.clone();
    let description = if description_raw.is_empty() {
        "-".to_string()
    } else {
        description_raw.clone()
    };
    let delete_title = format!("删除模板「{}」？此操作不可恢复。", template.template_name);
    let id_for_delete = template_id.clone();

    let drop_handler = UnsyncCallback::new(move |(from, to): (usize, usize)| on_drop(from, to));
    let delete_handler = UnsyncCallback::new(move |id: String| on_delete(id));

    view! {
        <DragSortItem index=index state=drag on_drop=drop_handler class="st-tr">
            <div class="st-td st-td--drag">
                <span class="st-drag-handle" title="拖动排序">
                    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
                        <circle cx="5" cy="3" r="1.5"></circle>
                        <circle cx="11" cy="3" r="1.5"></circle>
                        <circle cx="5" cy="8" r="1.5"></circle>
                        <circle cx="11" cy="8" r="1.5"></circle>
                        <circle cx="5" cy="13" r="1.5"></circle>
                        <circle cx="11" cy="13" r="1.5"></circle>
                    </svg>
                </span>
            </div>

            <div class="st-td">
                <span class="st-cell-ellipsis" title=name_title>
                    {name}
                </span>
            </div>

            <div class="st-td st-td--center">
                <Tag kind=tag_kind>{type_text}</Tag>
            </div>

            <div class="st-td">{category}</div>

            <div class="st-td st-td--tags">
                {if tags_empty {
                    view! { <span class="st-dash">"-"</span> }.into_any()
                } else {
                    tags
                        .into_iter()
                        .map(|tag| view! { <Tag>{tag}</Tag> })
                        .collect_view()
                        .into_any()
                }}
            </div>

            <div class="st-td">
                {if has_flags {
                    view! { <Tag kind=TagKind::Outlier>"离群值"</Tag> }.into_any()
                } else {
                    view! { <span class="st-dash">"-"</span> }.into_any()
                }}
            </div>

            <div class="st-td">
                <span class="st-cell-ellipsis" title=description_raw>
                    {description}
                </span>
            </div>

            <div class="st-td st-td--center">
                <Popconfirm
                    title=delete_title
                    ok_text="删除"
                    cancel_text="取消"
                    class="ui-popconfirm--end"
                    on_confirm=move || delete_handler.run(id_for_delete.clone())
                >
                    <Button
                        variant=ButtonVariant::TextDanger
                        size=ButtonSize::Small
                        icon_only=true
                        title="删除"
                    >
                        {icons::icon(Icon::Trash)}
                    </Button>
                </Popconfirm>
            </div>
        </DragSortItem>
    }
}

// ---------------------------------------------------------------- 日记配置

/// 一个待导入文件的界面状态（原 `ImportFileItem`）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiaryImportRow {
    date: String,
    status: &'static str,
    error: String,
}

impl DiaryImportRow {
    fn status_text(&self) -> String {
        match self.status {
            "importing" => "导入中".to_string(),
            "done" => "已完成".to_string(),
            "error" if self.error.is_empty() => "失败".to_string(),
            "error" => self.error.clone(),
            _ => "等待中".to_string(),
        }
    }

    fn status_class(&self) -> &'static str {
        match self.status {
            "importing" => "importing",
            "done" => "done",
            "error" => "error",
            _ => "pending",
        }
    }
}

/// 日记配置（`DiarySetting.vue`）：导入 + 导出。
#[component]
fn DiarySetting() -> impl IntoView {
    // ---- 导入状态 ----
    let import_status = RwSignal::new("idle".to_string());
    let import_rows = RwSignal::new(Vec::<DiaryImportRow>::new());
    let import_total = RwSignal::new(0_usize);
    let import_completed = RwSignal::new(0_usize);

    // ---- 导出状态 ----
    let export_status = RwSignal::new("idle".to_string());
    let export_total = RwSignal::new(0_i32);
    let export_success = RwSignal::new(0_i32);
    let export_failed = RwSignal::new(Vec::<DiaryExportFileError>::new());
    let export_scope = RwSignal::new("all".to_string());
    let today = crate::time::today_ymd();
    let export_year = RwSignal::new(today.get(0..4).unwrap_or_default().to_string());
    let export_month = RwSignal::new(today.get(0..7).unwrap_or_default().to_string());

    let set_row = move |index: usize, status: &'static str, error: String| {
        import_rows.update(|rows| {
            if let Some(row) = rows.get_mut(index) {
                row.status = status;
                row.error = error;
            }
        });
    };

    // ---- 导入：dialog_open → import_scan → 顺序 import_file ----
    let run_import = move |directory: String| {
        import_status.set("scanning".to_string());
        import_rows.set(Vec::new());
        import_total.set(0);
        import_completed.set(0);

        leptos::task::spawn_local(async move {
            let scan = match api::diary::import_scan(&directory).await {
                Ok(response) => response,
                Err(error) => {
                    Notifier::global().error(format!("扫描目录失败：{}", error.message()), None);
                    import_status.set("idle".to_string());
                    return;
                }
            };

            if scan.files.is_empty() {
                Notifier::global().info("未找到符合格式的日记文件（YYYY-MM-DD.txt / .md）", None);
                import_status.set("idle".to_string());
                return;
            }

            let files = scan.files;
            import_rows.set(
                files
                    .iter()
                    .map(|file| DiaryImportRow {
                        date: file.date.clone(),
                        status: "pending",
                        error: String::new(),
                    })
                    .collect(),
            );
            import_total.set(files.len());
            import_status.set("importing".to_string());

            let mut has_error = false;
            let mut completed = 0_usize;

            for (index, file) in files.iter().enumerate() {
                set_row(index, "importing", String::new());
                match api::diary::import_file(&file.path, &file.date).await {
                    Ok(_) => {
                        set_row(index, "done", String::new());
                        completed += 1;
                        import_completed.set(completed);
                    }
                    Err(error) => {
                        let message = if error.message().is_empty() {
                            "未知错误".to_string()
                        } else {
                            error.message().to_string()
                        };
                        set_row(index, "error", message);
                        has_error = true;
                    }
                }
            }

            if has_error {
                import_status.set("error".to_string());
            } else {
                import_status.set("done".to_string());
                Notifier::global().success(format!("成功导入 {completed} 篇日记"), None);
                // 1.5 秒后自动复位（原 `setTimeout(..., 1500)`）
                set_timeout(
                    move || {
                        import_status.set("idle".to_string());
                        import_rows.set(Vec::new());
                        import_total.set(0);
                        import_completed.set(0);
                    },
                    Duration::from_millis(1500),
                );
            }
        });
    };

    let pick_import_directory = move || {
        leptos::task::spawn_local(async move {
            match api::desktop::dialog_open("选择目录导入", "").await {
                Ok(response) => {
                    if let Some(directory) = response.first_path().map(str::to_string) {
                        run_import(directory);
                    }
                }
                Err(error) => notify_error("选择目录导入", &error),
            }
        });
    };

    // ---- 导出：dialog_open → diary_export ----
    let run_export = move |directory: String| {
        let scope = export_scope.get_untracked();
        let year_text = export_year.get_untracked();
        let month_text = export_month.get_untracked();

        let (year, month) = match scope.as_str() {
            "year" => (parse_year(&year_text), None),
            "month" => parse_year_month(&month_text),
            _ => (None, None),
        };

        let scope_label = match scope.as_str() {
            "year" => format!("（{}年）", year_text.trim()),
            "month" => format!("（{}）", month_text.trim()),
            _ => String::new(),
        };

        export_status.set("exporting".to_string());

        leptos::task::spawn_local(async move {
            match api::diary::export(&directory, year, month).await {
                Ok(result) => {
                    export_status.set("done".to_string());
                    export_total.set(result.total);
                    export_success.set(result.success);
                    let failed = result.failed;
                    let failed_count = failed.len();
                    export_failed.set(failed);

                    if result.total == 0 {
                        Notifier::global().info(format!("没有可导出的日记{scope_label}"), None);
                    } else if failed_count > 0 {
                        Notifier::global().warning(
                            format!(
                                "导出完成{scope_label}，成功 {} 篇，失败 {failed_count} 篇",
                                result.success
                            ),
                            None,
                        );
                    } else {
                        Notifier::global().success(
                            format!("成功导出 {} 篇日记{scope_label}", result.success),
                            None,
                        );
                    }

                    // 3 秒后自动复位（原 `setTimeout(..., 3000)`）
                    set_timeout(
                        move || {
                            export_status.set("idle".to_string());
                            export_total.set(0);
                            export_success.set(0);
                            export_failed.set(Vec::new());
                        },
                        Duration::from_millis(3000),
                    );
                }
                Err(error) => {
                    export_status.set("idle".to_string());
                    Notifier::global().error(format!("导出失败：{}", error.message()), None);
                }
            }
        });
    };

    let pick_export_directory = move || {
        if export_status.get_untracked() == "exporting" {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::desktop::dialog_open("选择目录导出", "").await {
                Ok(response) => {
                    if let Some(directory) = response.first_path().map(str::to_string) {
                        run_export(directory);
                    }
                }
                Err(error) => notify_error("选择目录导出", &error),
            }
        });
    };

    let import_percent = move || {
        let total = import_total.get();
        if total == 0 {
            0.0_f64
        } else {
            (import_completed.get() as f64 / total as f64 * 100.0).round()
        }
    };

    view! {
        <div class="st-pane">
            <h2 class="st-pane-title">"日记配置"</h2>

            <div class="st-list">
                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"导入日记"</span>
                        <span class="st-card-desc">
                            "从本地目录批量导入，文件名需为 YYYY-MM-DD.txt 或 YYYY-MM-DD.md 格式"
                        </span>
                    </div>
                    <div class="st-card-action">
                        <Tooltip title="从本地目录批量导入，文件名需为 YYYY-MM-DD.txt 或 YYYY-MM-DD.md 格式">
                            <Button
                                variant=ButtonVariant::Secondary
                                on_click=move || pick_import_directory()
                            >
                                {icons::icon(Icon::Inbox)}
                                "选择目录导入"
                            </Button>
                        </Tooltip>
                    </div>
                </div>

                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"导出日记"</span>
                        <span class="st-card-desc">
                            "将日记导出为 Markdown 文件（YYYY-MM-DD.md），可重新导入"
                        </span>
                        <div class="st-scope-row">
                            <Segmented
                                value=export_scope
                                options=vec![
                                    SegmentedOption::new("all", "全部"),
                                    SegmentedOption::new("year", "按年"),
                                    SegmentedOption::new("month", "按月"),
                                ]
                            />
                            <Show when=move || export_scope.get() == "year">
                                <div class="st-scope-input">
                                    <Input value=export_year placeholder="如 2026" maxlength=4 />
                                </div>
                            </Show>
                            <Show when=move || export_scope.get() == "month">
                                <div class="st-scope-input">
                                    <Input value=export_month placeholder="如 2026-01" maxlength=7 />
                                </div>
                            </Show>
                        </div>
                    </div>
                    <div class="st-card-action">
                        <Tooltip title="将全部日记导出为 Markdown 文件（YYYY-MM-DD.md）">
                            <Button
                                variant=ButtonVariant::Secondary
                                loading=Signal::derive(move || {
                                    export_status.get() == "exporting"
                                })
                                on_click=move || pick_export_directory()
                            >
                                {icons::icon(Icon::Read)}
                                "选择目录导出"
                            </Button>
                        </Tooltip>
                    </div>
                </div>
            </div>

            // 导入进度卡（status != idle）
            <Show when=move || import_status.get() != "idle">
                <div class="st-progress-card">
                    <div class="st-summary-row">
                        <span class="st-summary-text">
                            <Show when=move || import_status.get() == "scanning">
                                <Spin spinning=true size=SpinSize::Small />
                            </Show>
                            <Show when=move || import_status.get() == "scanning">
                                <span>"正在扫描目录…"</span>
                            </Show>
                            <Show when=move || import_status.get() == "importing">
                                <span>
                                    {move || {
                                        format!(
                                            "导入中 {}/{}",
                                            import_completed.get(),
                                            import_total.get(),
                                        )
                                    }}
                                </span>
                            </Show>
                            <Show when=move || import_status.get() == "done">
                                <span class="st-icon-done">
                                    {icons::icon(Icon::CheckCircle)}
                                </span>
                                <span>
                                    {move || format!("{} 篇导入完成", import_total.get())}
                                </span>
                            </Show>
                            <Show when=move || import_status.get() == "error">
                                <span class="st-icon-error">
                                    {icons::icon(Icon::CloseCircle)}
                                </span>
                                <span>
                                    {move || {
                                        format!(
                                            "导入中断，已完成 {}/{}",
                                            import_completed.get(),
                                            import_total.get(),
                                        )
                                    }}
                                </span>
                            </Show>
                        </span>
                        <span class="st-summary-percent">
                            {move || format!("{:.0}%", import_percent())}
                        </span>
                    </div>

                    <div class="st-bar-track">
                        <div
                            class="st-bar-fill"
                            class:st-bar-fill--done=move || import_status.get() == "done"
                            class:st-bar-fill--error=move || import_status.get() == "error"
                            style=move || format!("transform: scaleX({});", import_percent() / 100.0)
                        ></div>
                    </div>

                    <div class="st-file-list">
                        {move || {
                            import_rows
                                .get()
                                .into_iter()
                                .map(diary_import_row)
                                .collect_view()
                        }}
                    </div>
                </div>
            </Show>

            // 导出失败明细（status == done 且 failed 非空）
            <Show when=move || {
                export_status.get() == "done" && !export_failed.with(Vec::is_empty)
            }>
                <div class="st-progress-card">
                    <div class="st-summary-title">
                        {move || {
                            format!(
                                "导出完成，{}/{} 篇成功，{} 篇失败：",
                                export_success.get(),
                                export_total.get(),
                                export_failed.with(Vec::len),
                            )
                        }}
                    </div>
                    <div class="st-file-list">
                        {move || export_failed.get().into_iter().map(diary_failed_row).collect_view()}
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// 导入进度里的一行文件（原 `.file-row`）。
fn diary_import_row(row: DiaryImportRow) -> impl IntoView {
    let status_text = row.status_text();
    let row_class = format!("st-file-row st-file-row--{}", row.status_class());
    let status_class = format!("st-file-status st-file-status--{}", row.status_class());
    let date = row.date.clone();
    let error = row.error.clone();
    let title = if error.is_empty() { None } else { Some(error) };
    let status = row.status;

    view! {
        <div class=row_class title=title>
            <span class="st-file-dot">
                <Show when=move || status == "done">
                    <span class="st-icon-done">{icons::icon(Icon::CheckCircle)}</span>
                </Show>
                <Show when=move || status == "importing">
                    <Spin spinning=true size=SpinSize::Small />
                </Show>
                <Show when=move || status == "error">
                    <span class="st-icon-error">{icons::icon(Icon::CloseCircle)}</span>
                </Show>
                <Show when=move || status == "pending">
                    <span class="st-dot"></span>
                </Show>
            </span>
            <span class="st-file-date">{date}</span>
            <span class=status_class>{status_text}</span>
        </div>
    }
}

/// 导出失败明细里的一行。
fn diary_failed_row(item: DiaryExportFileError) -> impl IntoView {
    let title = item.error.clone();
    let text = item.error;
    let date = item.date;
    view! {
        <div class="st-file-row st-file-row--error" title=title>
            <span class="st-file-dot">
                <span class="st-icon-error">{icons::icon(Icon::CloseCircle)}</span>
            </span>
            <span class="st-file-date">{date}</span>
            <span class="st-file-status st-file-status--error">{text}</span>
        </div>
    }
}

// ---------------------------------------------------------------- 股票交易

/// 股票交易（`StockTradingSetting.vue` + `StockAccountView.vue` 的费用设置）。
#[component]
fn StockSetting() -> impl IntoView {
    let stores = AppStores::global();

    // ---- 费用设置 ----
    let fee_commission = RwSignal::new(String::new());
    let fee_min = RwSignal::new(String::new());
    let fee_stamp = RwSignal::new(String::new());
    let fee_transfer = RwSignal::new(String::new());
    let fee_saving = RwSignal::new(false);

    // ---- 交易标签 ----
    let tags = RwSignal::new(Vec::<String>::new());
    let default_tag = RwSignal::new(String::new());
    let tags_loading = RwSignal::new(false);
    let tags_saving = RwSignal::new(false);
    let new_tag = RwSignal::new(String::new());

    // ---- 重置 ----
    let confirm_open = RwSignal::new(false);
    let resetting = RwSignal::new(false);

    let no_ledger = move || stores.current_ledger_id.get().is_empty();

    // 回填：佣金 ×10000、最低佣金（分→元）、印花税/过户费 ×100（逐字照抄原 `fillFeeForm`）
    let fill_fee_form = move |setting: &StockFeeSetting| {
        fee_commission.set(format_scaled(setting.commission_rate, 10_000.0, 4));
        fee_min.set(cents_to_yuan(setting.min_commission));
        fee_stamp.set(format_scaled(setting.stamp_duty_rate, 100.0, 3));
        fee_transfer.set(format_scaled(setting.transfer_fee_rate, 100.0, 3));
    };

    let load_fee = move |ledger_id: String| {
        if ledger_id.is_empty() {
            fee_commission.set(String::new());
            fee_min.set(String::new());
            fee_stamp.set(String::new());
            fee_transfer.set(String::new());
            return;
        }
        leptos::task::spawn_local(async move {
            match api::stock::fee_settings_get(&ledger_id).await {
                Ok(setting) => fill_fee_form(&setting),
                Err(error) => notify_error("读取费用设置失败", &error),
            }
        });
    };

    let save_fee = move || {
        if fee_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先打开工作空间", None);
            return;
        }

        // 校验顺序与文案逐字照抄原 `handleSaveFeeSettings`
        let Some(commission) = parse_number(&fee_commission.get_untracked()) else {
            Notifier::global().error("请输入大于 0 的佣金费率", None);
            return;
        };
        if commission <= 0.0 {
            Notifier::global().error("请输入大于 0 的佣金费率", None);
            return;
        }
        let min_text = fee_min.get_untracked();
        let Some(min_commission_yuan) = parse_number(&min_text) else {
            Notifier::global().error("请输入不小于 0 的最低佣金", None);
            return;
        };
        if min_commission_yuan < 0.0 {
            Notifier::global().error("请输入不小于 0 的最低佣金", None);
            return;
        }
        let Some(stamp_duty) = parse_number(&fee_stamp.get_untracked()) else {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        };
        let Some(transfer_fee) = parse_number(&fee_transfer.get_untracked()) else {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        };
        if stamp_duty < 0.0 || transfer_fee < 0.0 {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        }

        // 元 → 分必须走 `tr_domain::money`
        let min_commission = match yuan_to_cents(&min_text) {
            Ok(cents) => cents,
            Err(_) => {
                Notifier::global().error("请输入不小于 0 的最低佣金", None);
                return;
            }
        };

        fee_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::fee_settings_put(
                &ledger_id,
                commission / 10_000.0,
                min_commission,
                stamp_duty / 100.0,
                transfer_fee / 100.0,
            )
            .await
            {
                Ok(setting) => {
                    // 保存后以后端返回为准回填
                    fill_fee_form(&setting);
                    Notifier::global().success("费用设置已保存", None);
                }
                Err(error) => notify_error("保存费用设置失败", &error),
            }
            fee_saving.set(false);
        });
    };

    // ---- 交易标签 ----
    let load_tags = move |ledger_id: String| {
        if ledger_id.is_empty() {
            tags.set(Vec::new());
            default_tag.set(String::new());
            tags_loading.set(false);
            return;
        }
        tags_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_get(&ledger_id).await {
                Ok(setting) => {
                    tags.set(setting.tags);
                    default_tag.set(setting.default_tag);
                }
                Err(error) => notify_error("读取交易标签失败", &error),
            }
            tags_loading.set(false);
        });
    };

    let save_tags = move |next: Vec<String>, success: Option<String>| {
        if tags_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先打开工作空间", None);
            return;
        }
        tags_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_put(&ledger_id, next).await {
                Ok(setting) => {
                    // 保存后以后端返回的 tags 为准
                    tags.set(setting.tags);
                    default_tag.set(setting.default_tag);
                    if let Some(text) = success {
                        Notifier::global().success(text, None);
                        new_tag.set(String::new());
                    }
                }
                Err(error) => notify_error("保存交易标签失败", &error),
            }
            tags_saving.set(false);
        });
    };

    let add_tag = move || {
        if tags_saving.get_untracked() {
            return;
        }
        if stores.current_ledger_id.get_untracked().is_empty() {
            Notifier::global().error("请先打开工作空间", None);
            return;
        }
        let next = new_tag.get_untracked().trim().to_string();
        if next.is_empty() {
            return;
        }
        if tags.with(|list| list.contains(&next)) {
            Notifier::global().error(format!("标签「{next}」已存在"), None);
            return;
        }
        if tags.with(Vec::len) >= MAX_STOCK_TAGS {
            Notifier::global().error("最多保存 20 个标签，请先删除不再需要的标签", None);
            return;
        }
        let mut list = tags.get_untracked();
        list.push(next.clone());
        save_tags(list, Some(format!("标签「{next}」已添加")));
    };

    let remove_tag = move |tag: String| {
        if tags_saving.get_untracked() || tag == default_tag.get_untracked() {
            return;
        }
        if stores.current_ledger_id.get_untracked().is_empty() {
            Notifier::global().error("请先打开工作空间", None);
            return;
        }
        let next: Vec<String> = tags
            .get_untracked()
            .into_iter()
            .filter(|item| item != &tag)
            .collect();
        save_tags(next, Some(format!("标签「{tag}」已删除")));
    };

    // ---- 重置 ----
    let do_reset = move || {
        if resetting.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("重置股票交易数据失败", Some("请先打开工作空间".to_string()));
            return;
        }
        resetting.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::reset(&ledger_id).await {
                Ok(_) => {
                    confirm_open.set(false);
                    // 重置会清掉费用设置与交易标签，重新拉一遍（原实现 reloadAll）
                    load_fee(ledger_id.clone());
                    load_tags(ledger_id);
                    Notifier::global().success("股票交易数据已重置", None);
                }
                Err(error) => notify_error("重置股票交易数据失败", &error),
            }
            resetting.set(false);
        });
    };

    // 账本变化 → 重新加载费用设置与交易标签
    Effect::new(move |_: Option<()>| {
        let ledger_id = stores.current_ledger_id.get();
        load_fee(ledger_id.clone());
        load_tags(ledger_id);
    });

    let tag_empty_text = move || {
        if tags_loading.get() {
            "正在加载标签…".to_string()
        } else if stores.current_ledger_id.get().is_empty() {
            "请先打开工作空间后配置交易标签".to_string()
        } else {
            "暂无标签".to_string()
        }
    };

    view! {
        <div class="st-pane">
            <h2 class="st-pane-title">"股票交易"</h2>

            <div class="st-list">
                // ---- 交易标签 ----
                <div class="st-card st-card--block">
                    <div class="st-tag-head">
                        <div class="st-card-info">
                            <span class="st-card-title">"交易标签"</span>
                            <span class="st-card-desc">
                                "清仓或交易历史中为每轮交易选择，可增删；「分析」默认不可删除。删除不影响历史记录，单个不超过 8 字，最多保存 20 个。"
                            </span>
                        </div>
                        <div class="st-tag-action">
                            <div class="st-tag-input">
                                <Input
                                    value=new_tag
                                    placeholder="如：低吸"
                                    maxlength=8
                                    allow_clear=true
                                    on_enter=move || add_tag()
                                />
                            </div>
                            <Button
                                variant=ButtonVariant::Primary
                                loading=tags_saving
                                disabled=Signal::derive(move || {
                                    new_tag.get().trim().is_empty()
                                        || tags.with(Vec::len) >= MAX_STOCK_TAGS
                                })
                                on_click=move || add_tag()
                            >
                                "添加"
                            </Button>
                        </div>
                    </div>

                    <div class="st-tag-list">
                        {move || {
                            let list = tags.get();
                            if list.is_empty() {
                                view! {
                                    <div class="st-tag-empty">{tag_empty_text()}</div>
                                }
                                    .into_any()
                            } else {
                                let default = default_tag.get();
                                list
                                    .into_iter()
                                    .map(|tag| {
                                        let is_default = tag == default;
                                        let for_title = tag.clone();
                                        let name = tag.clone();
                                        let aria = format!("删除标签 {tag}");
                                        // 回调先建好：`UnsyncCallback` 是 Copy，
                                        // `Show` 的 children 要求 `Fn`，不能把 String move 进去
                                        let remove_click =
                                            UnsyncCallback::new(move |()| remove_tag(tag.clone()));
                                        view! {
                                            <span
                                                class="st-tag-item"
                                                class:st-tag-item--default=is_default
                                                title=if is_default {
                                                    "默认标签，不可删除".to_string()
                                                } else {
                                                    for_title
                                                }
                                            >
                                                <span class="st-tag-item-name">{name}</span>
                                                <Show when=move || is_default>
                                                    <span class="st-tag-item-default">"默认"</span>
                                                </Show>
                                                <Show when=move || !is_default>
                                                    <button
                                                        type="button"
                                                        class="st-tag-item-remove"
                                                        disabled=move || tags_saving.get()
                                                        aria-label=aria.clone()
                                                        on:click=move |_| remove_click.run(())
                                                    >
                                                        {icons::icon(Icon::Close)}
                                                    </button>
                                                </Show>
                                            </span>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </div>
                </div>

                // ---- 交易费用设置（来自 StockAccountView.vue） ----
                <div class="st-card st-card--block">
                    <div class="st-panel-head">
                        <div class="st-panel-title-row">
                            <h3 class="st-panel-title">"交易费用设置"</h3>
                            <Tooltip title=FEE_TOOLTIP class="st-fee-tip">
                                <span class="st-panel-tip" aria-label="查看交易费用说明">
                                    {icons::icon(Icon::InfoCircle)}
                                </span>
                            </Tooltip>
                        </div>
                        <Button
                            variant=ButtonVariant::Primary
                            loading=fee_saving
                            disabled=Signal::derive(no_ledger)
                            on_click=move || save_fee()
                        >
                            "保存"
                        </Button>
                    </div>

                    <Form layout=FormLayout::Vertical class="st-fee-form">
                        <FormItem label="佣金费率">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_commission
                                    placeholder="如 2.354"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"万分之"</span>
                            </div>
                        </FormItem>
                        <FormItem label="最低佣金">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_min
                                    placeholder="如 5"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"元/委托"</span>
                            </div>
                        </FormItem>
                        <FormItem label="印花税">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_stamp
                                    placeholder="如 0.05"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"%"</span>
                                <Tooltip title=STAMP_TOOLTIP class="st-fee-tip">
                                    <span class="st-panel-tip" aria-label="印花税说明">
                                        {icons::icon(Icon::InfoCircle)}
                                    </span>
                                </Tooltip>
                            </div>
                        </FormItem>
                        <FormItem label="过户费">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_transfer
                                    placeholder="如 0.001"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"%"</span>
                                <Tooltip title=TRANSFER_TOOLTIP class="st-fee-tip">
                                    <span class="st-panel-tip" aria-label="过户费说明">
                                        {icons::icon(Icon::InfoCircle)}
                                    </span>
                                </Tooltip>
                            </div>
                        </FormItem>
                    </Form>

                    <Show when=move || no_ledger()>
                        <p class="st-hint">"请先打开工作空间"</p>
                    </Show>
                </div>

                // ---- 重置 ----
                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"重置"</span>
                        <span class="st-card-desc">
                            "清空当前账本的股票数据（账户本金、持仓、交易记录、资金记录、费用设置与交易标签），此操作不可恢复。"
                        </span>
                    </div>
                    <div class="st-card-action">
                        <Button
                            variant=ButtonVariant::PrimaryDanger
                            disabled=Signal::derive(no_ledger)
                            on_click=move || confirm_open.set(true)
                        >
                            "重置"
                        </Button>
                    </div>
                </div>
            </div>

            <Modal
                open=confirm_open
                title="重置股票交易数据"
                width=440
                ok_text="确认重置"
                cancel_text="取消"
                ok_danger=true
                ok_loading=resetting
                on_close=move || confirm_open.set(false)
                on_ok=move || do_reset()
            >
                <p class="st-modal-text">
                    "将清空当前账本的账户本金、持仓、交易记录、资金记录、费用设置与交易标签。此操作不可恢复，确定继续吗？"
                </p>
            </Modal>
        </div>
    }
}

// ---------------------------------------------------------------- 关于软件

/// 「关于软件」的更新状态（对应原 `stores/updateStore.ts` 的 Pinia store）。
///
/// 为什么放在模块级 `thread_local` 而不是组件内信号：原实现的 updateStore 是**全局 store**，
/// 跨路由常驻；本实现的 `TabPane` 切走会卸载组件，组件内信号会随之丢失
/// （"下载中切到别的分栏再切回来"会看到状态归零），且每次重新挂载都会重复注册事件监听。
/// 这里把状态与监听都收敛到模块级槽位，语义与原文一致。
#[derive(Clone, Copy)]
struct UpdateState {
    app_name: RwSignal<String>,
    version: RwSignal<String>,
    is_dev: RwSignal<bool>,
    /// idle / checking / available / no-update / downloading / downloaded / error
    status: RwSignal<String>,
    latest_version: RwSignal<String>,
    download_url: RwSignal<String>,
    digest: RwSignal<String>,
    release_body: RwSignal<String>,
    error_message: RwSignal<String>,
    download_percent: RwSignal<f64>,
    download_speed: RwSignal<String>,
}

thread_local! {
    /// 全局唯一的更新状态槽位（首次使用时创建并注册事件监听）。
    static UPDATE_STATE: RefCell<Option<UpdateState>> = const { RefCell::new(None) };
}

impl UpdateState {
    fn initial() -> Self {
        Self {
            app_name: RwSignal::new(String::new()),
            version: RwSignal::new(String::new()),
            is_dev: RwSignal::new(false),
            status: RwSignal::new("idle".to_string()),
            latest_version: RwSignal::new(String::new()),
            download_url: RwSignal::new(String::new()),
            digest: RwSignal::new(String::new()),
            release_body: RwSignal::new(String::new()),
            error_message: RwSignal::new(String::new()),
            download_percent: RwSignal::new(0.0_f64),
            download_speed: RwSignal::new(String::new()),
        }
    }

    /// 取全局状态；首次调用时创建并注册三个下载事件监听（只注册一次）。
    ///
    /// 注意 `RefCell` 的借用必须各自独立成句：`if let Some(x) = *slot.borrow()`
    /// 的临时借用会活到整个 `if let` 语句结束，紧接着再 `borrow_mut()` 会 panic。
    fn global() -> Self {
        let existing = UPDATE_STATE.with(|slot| slot.borrow().as_ref().copied());
        if let Some(state) = existing {
            return state;
        }
        let state = Self::initial();
        state.register_listeners();
        UPDATE_STATE.with(|slot| *slot.borrow_mut() = Some(state));
        state
    }

    /// 订阅下载事件（`ipc::listen` 内部保活闭包，界面进程即应用进程，无需退订）。
    fn register_listeners(self) {
        ipc::listen::<api::update::UpdateProgress, _>(
            api::update::EVENT_DOWNLOAD_PROGRESS,
            move |progress| {
                self.download_percent.set(progress.percent as f64);
                self.download_speed.set(progress.speed.clone());
                if self.status.get_untracked() != "downloading" {
                    self.status.set("downloading".to_string());
                }
            },
        );
        ipc::listen::<api::update::UpdateComplete, _>(
            api::update::EVENT_DOWNLOAD_COMPLETE,
            move |_complete| self.status.set("downloaded".to_string()),
        );
        ipc::listen::<api::update::UpdateError, _>(
            api::update::EVENT_DOWNLOAD_ERROR,
            move |error| {
                self.error_message.set(error.message.clone());
                self.status.set("error".to_string());
                Notifier::global().error("下载更新失败", Some(error.message));
            },
        );
    }
}

/// 关于软件（`AboutSetting.vue` + `stores/updateStore.ts`）。
///
/// **构建时间后端未提供**（`app_info` 只支持 `name` / `version` / `isDev`），
/// 因此这里只展示「应用名 / 版本 / 构建类型（开发版 / 正式版）」，不显示构建时间。
#[component]
fn AboutSetting() -> impl IntoView {
    let state = UpdateState::global();
    let app_name = state.app_name;
    let version = state.version;
    let is_dev = state.is_dev;
    let status = state.status;
    let latest_version = state.latest_version;
    let download_url = state.download_url;
    let digest = state.digest;
    let release_body = state.release_body;
    let error_message = state.error_message;
    let download_percent = state.download_percent;
    let download_speed = state.download_speed;

    // 首屏：应用信息 + 自动检查一次更新
    leptos::task::spawn_local(async move {
        match api::desktop::app_info("name").await {
            Ok(value) => app_name.set(value),
            Err(error) => notify_error("读取应用信息", &error),
        }
        match api::desktop::app_info("version").await {
            Ok(value) => version.set(value),
            Err(error) => notify_error("读取应用信息", &error),
        }
        match api::desktop::app_info("isDev").await {
            // isDev 是字符串 "true" / "false"
            Ok(value) => is_dev.set(value == "true"),
            Err(error) => notify_error("读取应用信息", &error),
        }
    });

    let check_for_update = move || {
        let current = status.get_untracked();
        if current == "downloading" || current == "downloaded" {
            return;
        }
        status.set("checking".to_string());
        error_message.set(String::new());

        leptos::task::spawn_local(async move {
            match api::update::check().await {
                Ok(result) => {
                    // 注意：`update_check` 不会 reject；hasUpdate == false 且有 error
                    // 表示"检查失败"，不能当成"已是最新"。
                    if let Some(message) = result.error.clone() {
                        status.set("error".to_string());
                        error_message.set(message.clone());
                        Notifier::global().error("检查更新失败", Some(message));
                        return;
                    }
                    if result.has_update {
                        status.set("available".to_string());
                        latest_version.set(result.latest_version.clone());
                        download_url.set(result.download_url.clone());
                        digest.set(result.digest.clone());
                        release_body.set(result.body.clone());
                    } else {
                        status.set("no-update".to_string());
                    }
                }
                Err(error) => {
                    status.set("error".to_string());
                    error_message.set(error.message().to_string());
                    notify_error("检查更新失败", &error);
                }
            }
        });
    };

    // 挂载时自动检查一次（原文 `onMounted` 里的 `checkForUpdate()`）
    Effect::new(move |_: Option<()>| check_for_update());

    let download_update = move || {
        let url = download_url.get_untracked();
        if url.is_empty() {
            return;
        }
        status.set("downloading".to_string());
        download_percent.set(0.0);
        download_speed.set(String::new());

        leptos::task::spawn_local(async move {
            let digest = digest.get_untracked();
            match api::update::download(&url, &digest).await {
                Ok(response) => {
                    if response.success {
                        // 正常路径由 `update:download-complete` 事件置为 downloaded
                        if status.get_untracked() == "downloading" {
                            status.set("downloaded".to_string());
                        }
                    } else if response.is_cancelled() {
                        status.set("available".to_string());
                        download_percent.set(0.0);
                        download_speed.set(String::new());
                    } else {
                        let message = response.error.unwrap_or_else(|| "下载失败".to_string());
                        if status.get_untracked() != "error" {
                            status.set("error".to_string());
                            error_message.set(message.clone());
                        }
                        Notifier::global().error("下载更新失败", Some(message));
                    }
                }
                Err(error) => {
                    status.set("error".to_string());
                    error_message.set(error.message().to_string());
                    notify_error("下载更新失败", &error);
                }
            }
        });
    };

    let cancel_download = move || {
        leptos::task::spawn_local(async move {
            match api::update::cancel().await {
                Ok(()) => {
                    status.set("available".to_string());
                    download_percent.set(0.0);
                    download_speed.set(String::new());
                }
                Err(error) => notify_error("取消下载", &error),
            }
        });
    };

    let install_update = move || {
        leptos::task::spawn_local(async move {
            match api::update::install().await {
                Ok(response) => {
                    if response.success {
                        // 成功后应用会自行退出，界面只需提示
                        Notifier::global().success("正在启动安装程序", None);
                    } else {
                        let message = response.error.unwrap_or_else(|| "安装启动失败".to_string());
                        status.set("error".to_string());
                        error_message.set(message.clone());
                        Notifier::global().error("安装更新失败", Some(message));
                    }
                }
                Err(error) => {
                    status.set("error".to_string());
                    error_message.set(error.message().to_string());
                    notify_error("安装更新失败", &error);
                }
            }
        });
    };

    let version_text = move || {
        let value = version.get();
        if value.is_empty() {
            "版本 …".to_string()
        } else {
            format!("版本 {value}")
        }
    };

    let build_type = move || {
        if is_dev.get() {
            "构建类型：开发版"
        } else {
            "构建类型：正式版"
        }
    };

    // 更新说明在 available / downloading / downloaded / error 阶段保留展示
    let show_release_body = move || {
        let status_now = status.get();
        !release_body.get().is_empty()
            && matches!(
                status_now.as_str(),
                "available" | "downloading" | "downloaded" | "error"
            )
    };

    view! {
        <div class="st-pane st-about">
            <div class="st-about-main">
                <div class="st-about-header">
                    <div class="st-app-logo">
                        <svg width="1024" height="1024" viewBox="0 0 1024 1024">
                            <rect
                                x="0"
                                y="0"
                                width="1024"
                                height="1024"
                                rx="200"
                                ry="200"
                                style="fill: var(--transactions-color-primary)"
                            ></rect>
                            <path
                                transform="translate(220.909996509552 49.1699676513672)"
                                fill="var(--transactions-color-text-inverse)"
                                d="M363.01 322.09 L236.22 322.09 L236.22 685.1 L135.78 685.1 L135.78 322.09 L9.61 322.09 L9.61 240.56 L363.01 240.56 Z M572.57 456.01 C560.79 449.6 547.05 446.4 531.34 446.4 C510.05 446.4 493.42 454.2 481.43 469.81 C469.44 485.41 463.45 506.64 463.45 533.51 L463.45 685.1 L365.49 685.1 L365.49 367.66 L463.45 367.66 L463.45 426.56 L464.69 426.56 C480.19 383.57 508.09 362.08 548.39 362.08 C558.72 362.08 566.78 363.32 572.57 365.8 Z"
                            ></path>
                        </svg>
                    </div>
                    <h2 class="st-app-name">
                        {move || {
                            let name = app_name.get();
                            if name.is_empty() { "Transactions".to_string() } else { name }
                        }}
                    </h2>
                    <p class="st-app-version">{version_text}</p>
                    <p class="st-app-build">{build_type}</p>
                </div>

                <div class="st-about-update">
                    {move || match status.get().as_str() {
                        "checking" => {
                            view! {
                                <div class="st-update-row">
                                    <Spin spinning=true size=SpinSize::Small />
                                    <span class="st-update-text">"正在检查更新…"</span>
                                </div>
                            }
                                .into_any()
                        }
                        "no-update" => {
                            view! {
                                <div class="st-update-row st-update-success">
                                    <span class="st-update-icon">
                                        {icons::icon(Icon::CheckCircle)}
                                    </span>
                                    <span class="st-update-text">"已是最新版本"</span>
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        on_click=move || check_for_update()
                                    >
                                        "重新检查"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                        "available" => {
                            view! {
                                <div class="st-update-row st-update-available">
                                    <span class="st-update-text">
                                        "发现新版本 "
                                        <strong>{move || latest_version.get()}</strong>
                                    </span>
                                    <Button
                                        variant=ButtonVariant::Primary
                                        size=ButtonSize::Small
                                        on_click=move || download_update()
                                    >
                                        "立即更新"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                        "downloading" => {
                            view! {
                                <div class="st-update-row">
                                    <Progress
                                        percent=Signal::derive(move || download_percent.get())
                                        class="st-update-progress"
                                    />
                                    <span class="st-update-text">
                                        {move || format!("{:.0}%", download_percent.get())}
                                    </span>
                                    // 原实现只渲染 percent；速度是本轮任务要求的增补
                                    <span class="st-update-speed">
                                        {move || download_speed.get()}
                                    </span>
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        on_click=move || cancel_download()
                                    >
                                        "取消下载"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                        "downloaded" => {
                            view! {
                                <div class="st-update-row st-update-success">
                                    <span class="st-update-icon">
                                        {icons::icon(Icon::CheckCircle)}
                                    </span>
                                    <span class="st-update-text">"下载完成"</span>
                                    <Button
                                        variant=ButtonVariant::Primary
                                        size=ButtonSize::Small
                                        on_click=move || install_update()
                                    >
                                        "安装并退出"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                        "error" => {
                            view! {
                                <div class="st-update-row st-update-error">
                                    <span class="st-update-icon">
                                        {icons::icon(Icon::CloseCircle)}
                                    </span>
                                    <span class="st-update-text">
                                        {move || {
                                            let message = error_message.get();
                                            if message.is_empty() {
                                                "检查失败，请稍后重试".to_string()
                                            } else {
                                                message
                                            }
                                        }}
                                    </span>
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        on_click=move || check_for_update()
                                    >
                                        "重试"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                        _ => {
                            view! {
                                <div class="st-update-row">
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        on_click=move || check_for_update()
                                    >
                                        "检查更新"
                                    </Button>
                                </div>
                            }
                                .into_any()
                        }
                    }}
                </div>

                <Show when=show_release_body>
                    <div class="st-release-body">{move || release_body.get()}</div>
                </Show>

                <div class="st-about-footer">
                    <a
                        class="st-about-link"
                        href=GITHUB_URL
                        target="_blank"
                        rel="noreferrer"
                    >
                        "GitHub"
                    </a>
                    <p class="st-about-copyright">
                        {move || {
                            format!(
                                "© {} Transactions. All rights reserved.",
                                crate::time::format_timestamp(
                                    crate::time::now_seconds(),
                                    "YYYY",
                                ),
                            )
                        }}
                    </p>
                </div>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------- 工具函数

/// 等价原前端的 `String(parseFloat((value * scale).toFixed(digits)))`：
/// 先按 `scale` 换算，再四舍五入到 `digits` 位小数，最后去掉多余的 0。
fn format_scaled(value: f64, scale: f64, digits: i32) -> String {
    let factor = 10_f64.powi(digits);
    let rounded = (value * scale * factor).round() / factor;
    format!("{rounded}")
}

/// 解析用户输入的数值：非数字 / 非有限值（`NaN` / `inf`）视为非法。
fn parse_number(input: &str) -> Option<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

/// `YYYY` → 年；空串或非法一律 `None`（表示"不限"）。
fn parse_year(input: &str) -> Option<i64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i64>().ok().filter(|value| *value > 0)
}

/// `YYYY-MM` → (年, 月)；空串或非法一律 `(None, None)`（表示"不限"）。
fn parse_year_month(input: &str) -> (Option<i64>, Option<i64>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    match trimmed.split_once('-') {
        Some((year, month)) => (parse_year(year), parse_year(month)),
        // 只填年份时按"按年"处理
        None => (parse_year(trimmed), None),
    }
}
