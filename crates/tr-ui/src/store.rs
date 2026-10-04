//! 界面级共享状态。
//!
//! 四块界面级共享状态：
//! * 账本列表 + 当前账本
//! * 底部状态栏显示的收支统计
//! * 外观（浅色 / 深色）
//! * 功能开关（哪些顶级功能在侧边栏出现）
//!
//! 实现方式与 [`crate::notify`] 一致：`RwSignal` + 模块级 thread_local 全局槽位。
//! 原因相同——这些状态会在 `spawn_local` 的异步块与 window 事件回调里被读写，
//! 那些位置没有当前 reactive `Owner`，`use_context` 取不到。

use std::cell::RefCell;
use std::collections::BTreeMap;

use leptos::prelude::*;
use tr_domain::dto::LedgerDto;

use crate::api::desktop::FeatureFlags;

/// 账本切换请求里"全部"的语义值（`tr_domain::consts::ALL`）。
pub use tr_domain::consts::ALL;

/// 外观取值（与 `config_get` 的 `appearance` 一致）。
pub const APPEARANCE_LIGHT: &str = "light";
pub const APPEARANCE_DARK: &str = "dark";
pub const APPEARANCE_SYSTEM: &str = "system";

/// 全局共享状态。
#[derive(Clone, Copy)]
pub struct AppStores {
    /// 账本列表（已按 `createdAt` 升序）。
    pub ledgers: RwSignal<Vec<LedgerDto>>,
    /// 当前账本 id；空串表示尚未选中。
    pub current_ledger_id: RwSignal<String>,
    /// 账本列表加载中。
    pub ledgers_loading: RwSignal<bool>,
    /// 当前工作空间目录（`workspace_get`）。
    pub workspace_dir: RwSignal<String>,
    /// 当前列表的 `trStatistics`（分），由消费记录页写入、底部状态栏读取。
    pub statistics: RwSignal<BTreeMap<String, i64>>,
    /// 外观：light / dark / system。
    pub appearance: RwSignal<String>,
    /// 功能开关：哪些顶级功能在侧边栏出现（`config_get` 读入，设置页写入）。
    ///
    /// **默认全开**：首屏读到配置之前的短暂窗口里，侧边栏按"全开"渲染，与老配置一致。
    pub enabled_features: RwSignal<FeatureFlags>,
    /// 事件页右栏（关联交易列表）是否展开（`config_get` 读入，事件页开合时写回配置）。
    ///
    /// 放在全局状态里而不是页面局部：换页回来时不必再等一次 IPC 往返，
    /// 也就不会先按"展开"渲染一帧再收起来（闪一下）。
    pub key_event_linked_open: RwSignal<bool>,
    /// 侧边栏是否收起（只剩图标）。`config_get` 读入，点收起按钮时写回配置。
    ///
    /// 与事件页右栏同理放在全局：外壳、设置页、跳转逻辑都可能读到它。
    pub sidebar_collapsed: RwSignal<bool>,
    /// 侧栏**正在收放**（点击后挂 360ms 的那个短命标记）。
    ///
    /// 它只有一个用处：给 `.app-shell` 挂上 `is-sidebar-morphing`，
    /// 让里面那些"会重排的文字"演一次交接（见 app.css 的同名注释）。
    /// 放在共享状态而不是外壳局部信号：收起按钮在 [`crate::shell::AppLeftBar`] 里，
    /// 类名要挂在外壳最外层那层 —— 两边都得读它。
    pub sidebar_morphing: RwSignal<bool>,
    /// 当前工作空间自定义图标的 `trasset://` URL（**空串 = 用内置的应用标志**）。
    ///
    /// 存 URL 而不是"有没有图标"：`<img src>` 直接吃它，省掉每次渲染的往返。
    /// 值里带一个自增的查询串（缓存击穿）—— `trasset` 按路径取文件、忽略查询串，
    /// 但 WebView2 会按完整 URL 缓存，换图标后不加这一条就会一直显示上一张。
    pub workspace_icon: RwSignal<String>,
    /// 图标 URL 的缓存击穿计数（只增，见 [`AppStores::apply_workspace_icon`]）。
    pub workspace_icon_version: RwSignal<u32>,
    /// **还没选定工作空间**：由外壳（`shell.rs` 的 [`App`](crate::shell::App)）读写，
    /// [`crate::error_handler`] 也读它。
    ///
    /// 放在全局而不是外壳局部：那期间页面照常渲染（它们是选择屏的背景），而它们发出的
    /// 业务命令必然以"未打开工作空间"失败 —— 这些提示只会糊在背景上，收口里全部丢掉。
    pub workspace_required: RwSignal<bool>,
}

thread_local! {
    /// 全局唯一状态槽位。由 [`crate::shell::App`] 在挂载时装入。
    static GLOBAL: RefCell<Option<AppStores>> = const { RefCell::new(None) };
}

impl AppStores {
    pub fn new() -> Self {
        Self {
            ledgers: RwSignal::new(Vec::new()),
            current_ledger_id: RwSignal::new(String::new()),
            ledgers_loading: RwSignal::new(false),
            workspace_dir: RwSignal::new(String::new()),
            statistics: RwSignal::new(BTreeMap::new()),
            appearance: RwSignal::new(APPEARANCE_SYSTEM.to_string()),
            enabled_features: RwSignal::new(FeatureFlags::defaults()),
            key_event_linked_open: RwSignal::new(true),
            sidebar_collapsed: RwSignal::new(false),
            sidebar_morphing: RwSignal::new(false),
            workspace_icon: RwSignal::new(String::new()),
            workspace_icon_version: RwSignal::new(0),
            workspace_required: RwSignal::new(false),
        }
    }

    pub fn install(self) {
        GLOBAL.with(|slot| *slot.borrow_mut() = Some(self));
    }

    /// 取全局状态；未安装时返回降级实例（只打日志，不 panic）。
    pub fn global() -> AppStores {
        GLOBAL.with(|slot| {
            slot.borrow().unwrap_or_else(|| {
                leptos::logging::warn!("共享状态尚未安装，已使用临时实例");
                AppStores::new()
            })
        })
    }

    /// 当前账本名（未选中时为空）。
    pub fn current_ledger_name(&self) -> String {
        let id = self.current_ledger_id.get();
        if id.is_empty() {
            return String::new();
        }
        self.ledgers.with(|ledgers| {
            ledgers
                .iter()
                .find(|ledger| ledger.id == id)
                .map(|ledger| ledger.name.clone())
                .unwrap_or_default()
        })
    }

    /// 写入账本列表：按 `createdAt` 升序排序，并在**未选中**时默认选第一个。
    ///
    /// 当前选中项若已不在列表中（例如被删除），回落到第一个。
    pub fn set_ledgers(&self, mut ledgers: Vec<LedgerDto>) {
        ledgers.sort_by_key(|ledger| ledger.created_at);
        let current = self.current_ledger_id.get_untracked();
        let still_exists = !current.is_empty() && ledgers.iter().any(|item| item.id == current);
        let next = if still_exists {
            current
        } else {
            ledgers
                .first()
                .map(|item| item.id.clone())
                .unwrap_or_default()
        };
        self.ledgers.set(ledgers);
        self.current_ledger_id.set(next);
    }

    /// 切换当前账本。
    pub fn select_ledger(&self, id: impl Into<String>) {
        let id = id.into();
        if self.current_ledger_id.get_untracked() != id {
            self.statistics.set(BTreeMap::new());
        }
        self.current_ledger_id.set(id);
    }

    /// 把外观写入 `<html data-theme>`：
    /// * `light` / `dark` → 显式设置 `data-theme`
    /// * `system` → **移除**属性，交回 `tokens.css` 的 `prefers-color-scheme` 兜底
    pub fn apply_appearance(&self) {
        let mode = self.appearance.get_untracked();
        let Some(element) = document().document_element() else {
            return;
        };
        match mode.as_str() {
            APPEARANCE_LIGHT | APPEARANCE_DARK => {
                let _ = element.set_attribute("data-theme", &mode);
            }
            _ => {
                let _ = element.remove_attribute("data-theme");
            }
        }
    }

    /// 某个顶级功能是否启用（未知名 = 启用：宁可想多了显示出来，也别让功能"凭空消失"）。
    ///
    /// ⚠ 这是**响应式读取**：在 `view!` 的闭包里调它，开关一变侧边栏就会重渲染。
    pub fn feature_enabled(&self, feature: &str) -> bool {
        self.enabled_features.get().get(feature).unwrap_or(true)
    }

    /// 用后端返回的开关集合覆盖本地状态（`config_set_feature` 的返回值）。
    pub fn set_enabled_features(&self, features: FeatureFlags) {
        self.enabled_features.set(features);
    }

    /// 工作空间图标变化：把相对路径翻成 `<img src>` 可用的 URL 并写入共享状态。
    ///
    /// `relative` 为空 = 用户清掉了图标（或这个工作空间从没设过）→ 状态清空，
    /// 侧栏回落成内置标志。
    pub fn apply_workspace_icon(&self, relative: &str) {
        if relative.is_empty() {
            self.workspace_icon.set(String::new());
            return;
        }
        let relative = relative.to_string();
        leptos::task::spawn_local(async move {
            let stores = AppStores::global();
            match crate::api::desktop::asset_url(&relative).await {
                Ok(url) => {
                    let version = stores.workspace_icon_version.get_untracked() + 1;
                    stores.workspace_icon_version.set(version);
                    stores.workspace_icon.set(format!("{url}?v={version}"));
                }
                Err(error) => {
                    stores.workspace_icon.set(String::new());
                    crate::error_handler::notify_error("读取工作空间图标", &error);
                }
            }
        });
    }
}

impl Default for AppStores {
    fn default() -> Self {
        Self::new()
    }
}
