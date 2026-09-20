//! tr-ui —— Transactions 的界面层（Leptos CSR，编译为 WASM 后由 Tauri 窗口加载）。
//!
//! 模块结构与职责：
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`ipc`] | Tauri 命令调用桥（错误信封、`workspace-required` 事件） |
//! | [`error_handler`] | 通知 + 兜底 / 重新抛出 |
//! | [`notify`] | message / notification 全局队列 |
//! | [`api`] | 按业务域的命令封装 |
//! | [`store`] | 账本 / 统计 / 外观 的界面级共享状态 |
//! | [`format`] | 金额与类型文案（金额换算走 `tr_domain::money`） |
//! | [`time`] | 秒级时间戳 → 本地时间字符串 |
//! | [`icons`] | 内联 SVG 图标集 |
//! | [`components::ui`] | 通用组件套件 |
//! | [`pages`] | 七个页面（消费记录 / 数据分析 / 股票 / 关键事件 / 日记管理 / 分类标签 / 应用设置） |
//! | [`shell`] | 应用外壳（导航 / 窗口控制 / 底部状态栏 / 工作空间选择） |
//!
//! 设计令牌与样式在 `static/css/`（`tokens.css` 是颜色与尺寸令牌的唯一来源）。
//!
//! 本 crate 只对 wasm32 编译；native 侧是一个空 crate（见 Cargo.toml 说明），
//! 因此 `cargo check --workspace` 不会引入任何浏览器依赖。

#![cfg(target_arch = "wasm32")]

pub mod api;
pub mod components;
pub mod error_handler;
pub mod format;
pub mod icons;
pub mod ipc;
pub mod notify;
pub mod pages;
pub mod shell;
pub mod store;
pub mod time;

use leptos::mount::mount_to_body;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// WASM 入口：由 wasm-bindgen 生成的 JS 胶水在脚本加载时自动调用。
///
/// 挂载到 `index.html` 里的 `<div id="app">`（而不是 `mount_to_body` 默认的 `<body>`）：
/// `base.css` 把 `#app` 定为 `height: 100vh; overflow: hidden`，两者配合才能让
/// `.app-shell` 恰好铺满窗口；挂到 body 会在空 `#app` 之后多出一个 100vh 的偏移。
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    let root = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("app"))
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok());

    match root {
        Some(root) => leptos::mount::mount_to(root, shell::App).forget(),
        // 兜底：缺少 #app 容器时退回 body（正常构建不会走到这里）
        None => {
            leptos::logging::warn!("未找到 #app 容器，回退挂载到 <body>");
            mount_to_body(shell::App);
        }
    }
}
