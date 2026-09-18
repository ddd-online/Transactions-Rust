//! 消息与通知队列。
//!
//! 对照原 `app/src/backend/notification.ts`（Ant Design Vue 的 `message` / `notification`）：
//!
//! | 原实现 | 语义 | 位置 |
//! |---|---|---|
//! | `message.*(text)` | 只有一句话，3 秒自动消失 | 屏幕底部（贴状态栏上方） |
//! | `notification.*(message, description)` | 有描述，4.5 秒自动消失 | 右上角，`top: 96px` |
//!
//! `NotificationUtil.success/error/warning(text, description?)` 的分支规则被逐条保留：
//! **有 description 走 notification，没有 description 走 message**。
//!
//! 队列实现：`RwSignal<Vec<Notice>>`（Leptos 信号）+ 模块级 thread_local 全局槽位。
//! 用全局槽位而不是 `provide_context`：通知会在 `spawn_local` 的异步块、window 事件
//! 回调里被触发，这些场景没有（也不应该有）当前的 reactive `Owner`，context 查不到。

use std::cell::RefCell;
use std::time::Duration;

use leptos::prelude::*;

/// 通知类型（对应原实现的四个入口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Info,
    Warning,
    Error,
}

impl NoticeKind {
    /// 语义色类名（`app.css` 的 `.notice--*`）。
    pub fn class(self) -> &'static str {
        match self {
            NoticeKind::Success => "notice--success",
            NoticeKind::Info => "notice--info",
            NoticeKind::Warning => "notice--warning",
            NoticeKind::Error => "notice--error",
        }
    }
}

/// 队列里的一条消息/通知。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u32,
    pub kind: NoticeKind,
    /// notification 的标题；message 模式下就是唯一的正文。
    pub title: String,
    /// notification 的描述；为空表示走 message 通道。
    pub description: String,
}

impl Notice {
    /// 有描述 → notification；无描述 → message。与 `notification.ts` 一致。
    pub fn is_notification(&self) -> bool {
        !self.description.is_empty()
    }
}

/// 自动消失时长：Ant Design 默认 message 3s / notification 4.5s。
const MESSAGE_DURATION: Duration = Duration::from_millis(3000);
const NOTIFICATION_DURATION: Duration = Duration::from_millis(4500);

/// 通知句柄（两个信号的轻量拷贝，可自由移动进闭包/异步块）。
#[derive(Clone, Copy)]
pub struct Notifier {
    items: RwSignal<Vec<Notice>>,
    next_id: RwSignal<u32>,
}

thread_local! {
    /// 全局唯一通知队列。由 [`crate::shell::App`] 在挂载时装入。
    static GLOBAL: RefCell<Option<Notifier>> = const { RefCell::new(None) };
}

impl Notifier {
    /// 新建队列（必须在某个 reactive owner 下调用，即组件渲染期间）。
    pub fn new() -> Self {
        Self {
            items: RwSignal::new(Vec::new()),
            next_id: RwSignal::new(1),
        }
    }

    /// 装入全局槽位。
    pub fn install(self) {
        GLOBAL.with(|slot| *slot.borrow_mut() = Some(self));
    }

    /// 取全局队列；未安装时返回一个降级实例（只打日志，不 panic）。
    ///
    /// 之所以降级而不是 `expect`：通知是纯展示副作用，任何调用时机问题都不该
    /// 让整个界面崩掉（原实现里 `message.error` 在任何时机都可调用）。
    pub fn global() -> Notifier {
        GLOBAL.with(|slot| {
            slot.borrow().unwrap_or_else(|| {
                leptos::logging::warn!("通知队列尚未安装，已丢弃该条通知");
                Notifier::new()
            })
        })
    }

    /// 队列本身（供渲染层订阅）。
    pub fn items(&self) -> RwSignal<Vec<Notice>> {
        self.items
    }

    pub fn success(&self, text: impl Into<String>, description: Option<String>) {
        self.push(NoticeKind::Success, text, description);
    }

    pub fn error(&self, text: impl Into<String>, description: Option<String>) {
        self.push(NoticeKind::Error, text, description);
    }

    pub fn warning(&self, text: impl Into<String>, description: Option<String>) {
        self.push(NoticeKind::Warning, text, description);
    }

    pub fn info(&self, text: impl Into<String>, description: Option<String>) {
        self.push(NoticeKind::Info, text, description);
    }

    /// 入队并安排自动消失，返回该条的 id。
    pub fn push(
        &self,
        kind: NoticeKind,
        text: impl Into<String>,
        description: Option<String>,
    ) -> u32 {
        let id = self.next_id.get_untracked();
        self.next_id.set(id.wrapping_add(1));

        let notice = Notice {
            id,
            kind,
            title: text.into(),
            description: description.unwrap_or_default(),
        };
        let duration = if notice.is_notification() {
            NOTIFICATION_DURATION
        } else {
            MESSAGE_DURATION
        };

        self.items.update(|items| items.push(notice));

        let this = *self;
        set_timeout(move || this.dismiss(id), duration);
        id
    }

    /// 立即移除某条。
    pub fn dismiss(&self, id: u32) {
        self.items
            .update(|items| items.retain(|item| item.id != id));
    }

    /// 清空队列。
    pub fn clear(&self) {
        self.items.update(Vec::clear);
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}
