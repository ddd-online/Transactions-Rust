//! 应用设置页（P6-a）：3 个分栏。
//!
//! 分栏与顺序（固定文案，改动即影响界面）：通用设置 / 日记配置 / 关于软件。
//!
//! 两个分栏已经迁走，别再往这里加回：
//! * 「消费模板」原是本页第 2 个分栏，现属记账事务 → 记账页的**模板**子功能
//!   （见 [`crate::pages::templates`]）；
//! * 「股票」（交易费用 / 交易标签 / 重置股票数据）原是本页第 3 个分栏，现属股票事务
//!   → 股票页的**设置**子功能（见 [`crate::pages::stock`]）。费用设置与标签都是按账本存的，
//!   放在股票页与下单、复盘、统计同屏更顺手。
//!
//! ## 本实现的设计取舍（逐条，均为有意为之）
//!
//! 1. **分栏导航形态**：顶部 `Tabs` + `TabPane`（本轮任务要求交付并验证这两个组件）。
//!    每个分栏内部是「分栏名标题 + 卡片列表」结构，
//!    卡片外观（白底 / 1px `--transactions-color-divider` / `--transactions-radius-md` /
//!    hover 底色 `--transactions-color-hover-bg` / padding 12px 16px）。
//! 2. **通用设置·工作空间**：按钮固定文案「切换」、弹窗标题「选择工作目录」、输入框占位
//!    「请输入或选择工作目录路径」（任务单里概括成「更换目录…」，未采用）。
//!    本仓库没有 `transactions-file-select` 组件，直接用 `dialog_open` 选目录。
//! 3. **通用设置不展示版本信息**：该分栏只有 4 张卡片；`app_info` 的 version
//!    只出现在「关于软件」。`config_get()` 仍一次取全
//!    （workspace_dir / close_behavior / appearance / config_path / is_dev），
//!    其中 `config_path` / `is_dev` 与 `app_info("isDev")` 在本页**都没有对应 UI**
//!    （曾经在关于页展示过"构建类型"，已按需求移除），只保留后端能力，界面不消费。
//! 4. **通用设置·外观/关闭行为**：持久化失败时提示并**回滚界面选中值**（任务单要求），
//!    不静默吞掉。
//!    **开发者工具不是开关而是按钮**：它的行为是"点击开一个新窗口"，没有可关闭的开关态
//!    （要关就在 DevTools 自己的窗口上关），所以界面给「打开」按钮，只发 `devtools_toggle(true)`；
//!    按钮带忙状态防止连点开出两个窗口。
//! 5. **日记配置**：**没有**「文件勾选」（扫描后顺序导入全部文件），只是每行状态；
//!    「浏览器 dev 模式降级」分支（手输路径）在 Tauri 下不存在，故不实现；
//!    导入完成后无需刷新日记页，故省略。
//! 5. **关于软件**：GitHub 链接是本轮任务要求增补；只展示**应用名 / 版本 / GitHub / 版权行**
//!    —— **构建时间与构建类型都不展示**（`app_info` 支持 `name` / `version` / `isDev`，
//!    但开发版/正式版这行按需求移除了）。
//!    更新说明照任务单按**纯文本 + 保留换行**渲染（本仓库没有 Markdown 解析器）；
//!    下载进度额外用小字显示 `speed`。
//! 6. 全页不使用 `unwrap` / `expect` 处理用户数据：解析失败、命令失败一律走通知。

use std::cell::RefCell;
use std::time::Duration;

use leptos::prelude::*;
use leptos::tachys::view::any_view::IntoAny;
use tr_domain::dto::DiaryExportFileError;
use tr_domain::proxy::{ProxySetting, PROXY_MODE_AUTO, PROXY_MODE_MANUAL, PROXY_MODE_OFF};

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, FeaturePage, Input, Progress, Segmented, SegmentedOption,
    Spin, SpinSize, TabItem, TabPane, Tabs, Tooltip,
};
use crate::error_handler::notify_error;
use crate::icons::{self, Icon};
use crate::ipc;
use crate::notify::Notifier;
use crate::store::{AppStores, APPEARANCE_DARK, APPEARANCE_LIGHT, APPEARANCE_SYSTEM};

/// 页面标题（固定文案「应用设置」，改动即影响界面）。
pub const PAGE_TITLE: &str = "应用设置";

const TAB_GENERAL: &str = "general";
const TAB_DIARY: &str = "diary";
const TAB_ABOUT: &str = "about";

/// 外部链接（本仓库地址，改动即影响「关于软件」）。
const GITHUB_URL: &str = "https://github.com/ddd-online/Transactions-Rust";

#[component]
pub fn SettingsPage() -> impl IntoView {
    let active = RwSignal::new(TAB_GENERAL.to_string());
    let items = vec![
        TabItem::new(TAB_GENERAL, "通用设置"),
        TabItem::new(TAB_DIARY, "日记配置"),
        TabItem::new(TAB_ABOUT, "关于软件"),
    ];

    // 版心两块：工具栏 / 内容区各自建好视图再交给 `FeaturePage`（骨架见 components/ui/feature_page.rs）
    let toolbar = view! {
        <Tabs active=active items=items class="st-tabs" />
    }
    .into_any();

    let content = view! {
        <div class="st-panes">
            <TabPane active=active key=TAB_GENERAL>
                <GeneralSetting />
            </TabPane>
            <TabPane active=active key=TAB_DIARY>
                <DiarySetting />
            </TabPane>
            <TabPane active=active key=TAB_ABOUT>
                <AboutSetting />
            </TabPane>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage title=PAGE_TITLE toolbar=toolbar content=content />
    }
}

// ---------------------------------------------------------------- 通用设置

/// 通用设置：工作空间 / 外观 / 关闭行为 / 代理 / 开发者工具。
#[component]
fn GeneralSetting() -> impl IntoView {
    let stores = AppStores::global();

    let close_behavior = RwSignal::new(String::new());
    let appearance = RwSignal::new(stores.appearance.get_untracked());
    // 代理：模式 + 手动地址（切换模式立即保存；地址靠回车或「保存」按钮提交）
    let proxy_mode = RwSignal::new(PROXY_MODE_AUTO.to_string());
    let proxy_url = RwSignal::new(String::new());
    // 「当前生效：…」那一行（由 `proxy_detect` 填；切换模式/保存后刷新）
    let proxy_note = RwSignal::new(String::new());
    let proxy_detecting = RwSignal::new(false);
    // 开发者工具的按钮忙状态（防连点开出两个窗口）
    let devtools_opening = RwSignal::new(false);
    let switching = RwSignal::new(false);

    // ---- 代理 ----
    // 「当前生效」那一行：后端按**当前设置**解析（`auto` 会现场探测环境变量/系统代理），
    // 所以模式一改就得重新问一次，否则界面显示的还是上一次的结论。
    let refresh_proxy_note = move || {
        proxy_detecting.set(true);
        leptos::task::spawn_local(async move {
            match api::desktop::proxy_detect().await {
                Ok(response) => proxy_note.set(response.message),
                Err(error) => notify_error("检测代理", &error),
            }
            proxy_detecting.set(false);
        });
    };

    // 保存失败时把界面拉回**磁盘上的真实值**。
    // ⚠ 不能"记住点击前的值"：`Segmented` 在触发 `on_change` **之前**就已经把绑定信号改成新值了，
    // 回调里读到的"之前"其实就是刚点的那一项（回滚会变成空操作）——只有重读配置才权威。
    let reload_proxy = move || {
        leptos::task::spawn_local(async move {
            match api::desktop::config_get().await {
                Ok(config) => {
                    proxy_mode.set(config.proxy.effective_mode().to_string());
                    proxy_url.set(config.proxy.url);
                }
                Err(error) => notify_error("读取配置", &error),
            }
        });
    };

    // 提交给后端：成功后用**归一化的值**回显并刷新「当前生效」。
    // `rollback` 只在"切模式"这条路径上为真 —— 手动地址保存失败时**不能**回滚，
    // 否则输入框会消失，用户没法改那个地址。
    let submit_proxy = move |mode: String, url: String, rollback: bool| {
        leptos::task::spawn_local(async move {
            let setting = ProxySetting { mode, url };
            match api::desktop::config_set_proxy(setting).await {
                Ok(saved) => {
                    proxy_mode.set(saved.mode);
                    proxy_url.set(saved.url);
                    refresh_proxy_note();
                }
                Err(error) => {
                    if rollback {
                        reload_proxy();
                    }
                    notify_error("设置代理", &error);
                }
            }
        });
    };

    // 模式切换：「手动」只把输入框露出来（地址还没填，提交必然失败），其余两种立即保存。
    let switch_proxy_mode = move |mode: String| {
        if mode == PROXY_MODE_MANUAL {
            proxy_mode.set(mode);
        } else {
            submit_proxy(mode, proxy_url.get_untracked(), true);
        }
    };

    // 首屏：`config_get()` 一次拿全（workspace_dir / close_behavior / appearance /
    // config_path / is_dev / proxy）。
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
                // 空/未知模式一律回落到「自动探测」（与 `ProxySetting::effective_mode` 同口径）
                proxy_mode.set(config.proxy.effective_mode().to_string());
                proxy_url.set(config.proxy.url);
                // 首屏就把"当前到底走不走代理"显示出来（auto 模式尤其需要）
                refresh_proxy_note();
            }
            Err(error) => notify_error("读取配置", &error),
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

    // ---- 开发者工具：**一次性动作，不是开关** ----
    // 点击就是"开一个新窗口"，没有"关"这一态（关它在它自己的窗口上）；所以界面给按钮而不是开关，
    // 按钮只负责发 `true`。忙状态防连点开出两个窗口。
    let open_devtools = move || {
        if devtools_opening.get_untracked() {
            return;
        }
        devtools_opening.set(true);
        leptos::task::spawn_local(async move {
            if let Err(error) = api::desktop::devtools_toggle(true).await {
                notify_error("打开开发者工具", &error);
            }
            devtools_opening.set(false);
        });
    };

    let workspace_text = move || {
        let directory = stores.workspace_dir.get();
        if directory.is_empty() {
            "尚未选择工作空间".to_string()
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
                    <span class="st-card-desc">"界面配色，可跟随系统"</span>
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
                    <span class="st-card-desc">"点击右上角关闭按钮时的行为"</span>
                </div>
                <div class="st-card-action">
                    <Segmented
                        value=close_behavior
                        options=vec![
                            SegmentedOption::new("quit", "直接退出"),
                            SegmentedOption::new("tray", "最小化到托盘"),
                        ]
                        on_change=move |mode: String| change_close_behavior(mode)
                    />
                </div>
            </div>

            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"代理"</span>
                    <span class="st-card-desc">
                        "网络请求（行情、更新检查）走 HTTP 代理；自动探测读取环境变量与 Windows 系统代理"
                    </span>
                    <span class="st-card-desc st-card-desc--mono">{proxy_note}</span>
                </div>
                <div class="st-card-action">
                    <Show when=move || proxy_mode.get() == PROXY_MODE_MANUAL>
                        <div class="st-proxy-url">
                            <Input
                                value=proxy_url
                                placeholder="http://127.0.0.1:7890"
                                on_enter=move || {
                                    submit_proxy(
                                        PROXY_MODE_MANUAL.to_string(),
                                        proxy_url.get_untracked(),
                                        false,
                                    )
                                }
                            />
                        </div>
                    </Show>
                    <Segmented
                        value=proxy_mode
                        options=vec![
                            SegmentedOption::new(PROXY_MODE_OFF, "不使用"),
                            SegmentedOption::new(PROXY_MODE_AUTO, "自动探测"),
                            SegmentedOption::new(PROXY_MODE_MANUAL, "手动"),
                        ]
                        on_change=move |mode: String| switch_proxy_mode(mode)
                    />
                    <Show when=move || proxy_mode.get() == PROXY_MODE_MANUAL>
                        <Button
                            variant=ButtonVariant::Secondary
                            on_click=move |_| {
                                submit_proxy(
                                    PROXY_MODE_MANUAL.to_string(),
                                    proxy_url.get_untracked(),
                                    false,
                                )
                            }
                        >
                            "保存"
                        </Button>
                    </Show>
                    <Button
                        variant=ButtonVariant::Secondary
                        loading=proxy_detecting
                        on_click=move |_| refresh_proxy_note()
                    >
                        "检测"
                    </Button>
                </div>
            </div>

            <div class="st-card">
                <div class="st-card-info">
                    <span class="st-card-title">"开发者工具"</span>
                    <span class="st-card-desc">"打开开发者工具，用于调试界面代码"</span>
                </div>
                <div class="st-card-action">
                    // 行为是"开一个新窗口"，没有可关闭的开关态 → 用按钮（与「切换」同一套动作按钮样式）
                    <Button
                        variant=ButtonVariant::Secondary
                        loading=devtools_opening
                        on_click=move || open_devtools()
                    >
                        "打开"
                    </Button>
                </div>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------- 日记配置

/// 一个待导入文件的界面状态。
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

/// 日记配置：导入 + 导出（**都作用于当前账本**）。
#[component]
fn DiarySetting() -> impl IntoView {
    let stores = AppStores::global();
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

    // ---- 导入：dialog_open → import_scan → 顺序 import_file（落到**当前账本**）----
    let run_import = move |directory: String| {
        // 账本在开始导入时就定下来：整批文件都进这个账本，中途切账本也不改目标
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().info("请先选择一个账本，再导入日记", None);
            return;
        }
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
                match api::diary::import_file(&file.path, &file.date, &ledger_id).await {
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
                // 1.5 秒后自动复位
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
            match api::desktop::dialog_open("选择导入目录", "").await {
                Ok(response) => {
                    if let Some(directory) = response.first_path().map(str::to_string) {
                        run_import(directory);
                    }
                }
                Err(error) => notify_error("选择导入目录", &error),
            }
        });
    };

    // ---- 导出：dialog_open → diary_export（只导**当前账本**）----
    let run_export = move |directory: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().info("请先选择一个账本，再导出日记", None);
            return;
        }
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
            match api::diary::export(&directory, year, month, &ledger_id).await {
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

                    // 3 秒后自动复位
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
            match api::desktop::dialog_open("选择导出目录", "").await {
                Ok(response) => {
                    if let Some(directory) = response.first_path().map(str::to_string) {
                        run_export(directory);
                    }
                }
                Err(error) => notify_error("选择导出目录", &error),
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
        <div class="page-pane">
            // 分区标题去掉：页签已经说明这是哪一页（见「消费模板」处的同一条说明）
            <div class="st-list">
                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"导入日记"</span>
                        <span class="st-card-desc">
                            "从本地目录批量导入当前账本，文件名需为 YYYY-MM-DD.txt 或 YYYY-MM-DD.md"
                        </span>
                    </div>
                    <div class="st-card-action">
                        // 触发器在卡片右列（≈版心右缘）：气泡改为右对齐，否则居中的长文案会顶出窗口被裁
                        <Tooltip
                            title="从本地目录批量导入当前账本，文件名需为 YYYY-MM-DD.txt 或 YYYY-MM-DD.md"
                            class="ui-tooltip--end"
                        >
                            <Button
                                variant=ButtonVariant::Secondary
                                disabled=Signal::derive(move || {
                                    stores.current_ledger_id.get().is_empty()
                                })
                                on_click=move || pick_import_directory()
                            >
                                {icons::icon(Icon::Inbox)}
                                "批量导入"
                            </Button>
                        </Tooltip>
                    </div>
                </div>

                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"导出日记"</span>
                        <span class="st-card-desc">
                            "把当前账本的日记导出为 Markdown 文件（YYYY-MM-DD.md），可重新导入"
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
                        // 同上：右对齐展开，长文案不越出窗口
                        <Tooltip
                            title="把当前账本的日记导出为 Markdown 文件（YYYY-MM-DD.md），可在别的账本重新导入"
                            class="ui-tooltip--end"
                        >
                            <Button
                                variant=ButtonVariant::Secondary
                                disabled=Signal::derive(move || {
                                    stores.current_ledger_id.get().is_empty()
                                })
                                loading=Signal::derive(move || {
                                    export_status.get() == "exporting"
                                })
                                on_click=move || pick_export_directory()
                            >
                                {icons::icon(Icon::Read)}
                                "批量导出"
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

/// 导入进度里的一行文件。
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

// ---------------------------------------------------------------- 关于软件

/// 「关于软件」的更新状态（模块级全局状态，见下）。
///
/// 为什么放在模块级 `thread_local` 而不是组件内信号：更新状态需要跨路由常驻；
/// 本实现的 `TabPane` 切走会卸载组件，组件内信号会随之丢失
/// （"下载中切到别的分栏再切回来"会看到状态归零），且每次重新挂载都会重复注册事件监听。
/// 这里把状态与监听都收敛到模块级槽位。
#[derive(Clone, Copy)]
struct UpdateState {
    app_name: RwSignal<String>,
    version: RwSignal<String>,
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

/// 在**应用根组件**（`shell::App`）里调用一次，把更新状态建在根 owner 下。
///
/// 必须早于任何组件挂载：`UpdateState` 里全是 `RwSignal`，信号的归属是**创建时所在的
/// reactive owner**。若等 `AboutSetting` 首次渲染时才创建，它们就挂在那个页签组件上，
/// 切走「关于软件」→ owner 销毁 → 信号变"已 dispose" → 再切回来渲染访问即 panic，
/// 表现为**整个面板空白**（控制台：`you tried to access a reactive value … has already been disposed`）。
/// 这与 `AppStores` 在根组件里 `new()` + `install()` 是同一条理由。
pub fn init_update_state() {
    let _ = UpdateState::global();
}

impl UpdateState {
    fn initial() -> Self {
        Self {
            app_name: RwSignal::new(String::new()),
            version: RwSignal::new(String::new()),
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
    /// ⚠ **首次创建必须发生在应用根 owner 下**（见 [`init_update_state`]，由 `shell::App` 调用）：
    /// 若等到 `AboutSetting` 第一次渲染时才落到这里，这些信号就挂在**那个组件**的 owner 上，
    /// 切走「关于软件」时 owner 被销毁、信号变成"已 dispose"，
    /// 再切回来渲染访问它们就 panic —— 现象是**整个面板空白**，控制台报
    /// `you tried to access a reactive value ... but it has already been disposed`（实测踩过）。
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

/// 关于软件：应用信息 + 更新检查 + 下载进度 + GitHub 链接。
///
/// 只展示**应用名 / 版本 / GitHub / 版权行**：构建时间后端未提供
/// （`app_info` 只支持 `name` / `version` / `isDev`），构建类型那行按需求移除。
#[component]
fn AboutSetting() -> impl IntoView {
    let state = UpdateState::global();
    let app_name = state.app_name;
    let version = state.version;
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

    // 挂载时自动检查一次
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
            "正在读取…".to_string()
        } else {
            format!("版本 {value}")
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
        <div class="page-pane st-about">
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
                                    // 百分比之外额外显示速度（本轮任务要求的增补）
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
                        {icons::icon(Icon::GitHub)}
                        <span class="st-about-link-text">"GitHub"</span>
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
