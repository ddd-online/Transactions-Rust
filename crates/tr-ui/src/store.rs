//! 界面级共享状态。
//!
//! 三块界面级共享状态：
//! * 账本列表 + 当前账本
//! * 底部状态栏显示的收支统计
//! * 外观（浅色 / 深色）
//!
//! 实现方式与 [`crate::notify`] 一致：`RwSignal` + 模块级 thread_local 全局槽位。
//! 原因相同——这些状态会在 `spawn_local` 的异步块与 window 事件回调里被读写，
//! 那些位置没有当前 reactive `Owner`，`use_context` 取不到。

use std::cell::RefCell;
use std::collections::BTreeMap;

use leptos::prelude::*;
use tr_domain::dto::LedgerDto;

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
}

impl Default for AppStores {
    fn default() -> Self {
        Self::new()
    }
}
