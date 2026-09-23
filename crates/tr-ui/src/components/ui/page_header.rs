//! 页面页头 —— 外壳的**自绘标题栏**（无边框窗口，48px，整条是拖动区）。
//!
//! 标题是**居中的小字**，文案 `Transactions-<页面名>`（如 `Transactions-记账`）：
//! 它是"窗口标题"性质的处所标签，不是页面大标题 —— 页面身份已经由侧栏当前项与
//! 子功能图标条给出，标题栏只留一行安静的文字，不与内容抢注意力。
//!
//! 右上角叠着外壳的窗口三键（`.app-top-bar`，绝对定位浮层），所以标题栏**两侧**
//! 都留出三键的宽度：既保证标题不会被三键压住，又因为左右等宽而让标题落在
//! 标题栏正中（居中不依赖标题自身的宽度）。
//!
//! ⚠ 但标题栏是**内容区**的一行（左边还有一列固定宽度的侧栏），所以"容器内居中"
//! 看着会整体偏右半个侧栏宽。标题自己再往左挪半个侧栏（base.css 的 `.page-title`
//! 用 `--transactions-size-sidebar` 做这件事），才是**软件宽度**的正中。
//!
//! 5 个顶级功能（记账 / 股票 / 事件 / 日记 / 应用设置）共用这一个标题栏。

use leptos::prelude::*;

/// 标题栏文案：`Transactions-<页面名>`。
///
/// 抽成纯函数是为了能直接断言格式 —— 标题栏文案是用户一眼就看到的产品约定。
fn header_text(title: &str) -> String {
    format!("Transactions-{title}")
}

/// 页头（标题由调用方传入——各页的 `PAGE_TITLE` 常量）。
#[component]
pub fn PageHeader(
    /// 页面标题
    title: &'static str,
) -> impl IntoView {
    // 前缀是常量、标题是 `&'static str`，所以在视图外一次算好
    let text = header_text(title);
    view! {
        <header class="page-header">
            <h1 class="page-title">{text}</h1>
        </header>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标题栏文案格式：`Transactions-xx`（连字符，无空格）。
    #[test]
    fn header_text_uses_transactions_prefix() {
        assert_eq!(header_text("记账"), "Transactions-记账");
        assert_eq!(header_text("股票"), "Transactions-股票");
        assert_eq!(header_text("应用设置"), "Transactions-应用设置");
        assert!(!header_text("日记").contains(' '));
        assert!(header_text("日记").starts_with("Transactions-"));
    }

    /// 标题栏的排版约定：**居中小字**（居中于**软件宽度**，不是内容区那一列）。
    ///
    /// 反向断言几条容易退化的点：① 不能退回"左标题 + 右留位"（`space-between`）；
    /// ② 不能又变回大号 display（那会让标题栏变回页面大标题）；
    /// ③ 不能丢掉"半个侧栏宽"的补偿 —— 丢了标题就整体偏右，看着不居中。
    #[test]
    fn title_is_centered_small_text_in_the_title_bar() {
        let app = include_str!("../../../static/css/app.css");
        let base = include_str!("../../../static/css/base.css");
        let tokens = include_str!("../../../static/css/tokens.css");

        // 居中：flex 主轴居中 + 左右内边距**等宽**（容器内居中靠这条等宽）
        let start = app
            .find(".page-header {")
            .expect("app.css 里应有 .page-header");
        let tail = &app[start..];
        let header = &tail[..tail.find('}').expect("规则应闭合")];
        assert!(header.contains("justify-content: center"), "{header}");
        assert!(
            !header.contains("space-between"),
            "不能退回左标题布局：{header}"
        );
        assert!(
            header.contains("--page-header-side"),
            "三键留位要有一个具名宽度，左右复用：{header}"
        );
        assert!(
            header.contains(
                "padding: 0 calc(var(--page-header-side) + var(--transactions-space-region))"
            ),
            "左右内边距必须等宽（容器内居中靠它）：{header}"
        );

        // 小字：caption 档，且是弱化色（不与内容抢注意力）
        let title_start = base
            .find(".page-title {")
            .expect("base.css 里应有 .page-title");
        let title_tail = &base[title_start..];
        let title = &title_tail[..title_tail.find('}').expect("规则应闭合")];
        assert!(
            title.contains("--transactions-size-text-caption"),
            "标题栏标题用 caption 小字档：{title}"
        );
        assert!(
            title.contains("color: var(--transactions-color-text-secondary)"),
            "弱化色：{title}"
        );
        assert!(
            !title.contains("--transactions-size-text-display"),
            "不能再是大号 display：{title}"
        );
        // 长标题在窄窗口里省略号收尾，而不是把三键顶开
        assert!(title.contains("text-overflow: ellipsis"), "{title}");
        assert!(title.contains("white-space: nowrap"), "{title}");

        // 窗口正中：标题栏在内容区里，容器内居中会偏右半个侧栏 —— 标题必须挪回来。
        // 这一条同时锁住"侧栏宽度只有一个来源"（别再手写 200px 而与侧栏脱钩）。
        assert!(
            title.contains("transform: translateX(calc(-0.5 * var(--transactions-size-sidebar)))"),
            "标题要补偿半个侧栏宽才是窗口正中：{title}"
        );
        assert!(
            tokens.contains("--transactions-size-sidebar:"),
            "侧栏宽度要以令牌定义：{tokens}"
        );
        assert!(
            app.contains("width: var(--transactions-size-sidebar)"),
            "侧栏宽度必须用同一个令牌，不能与补偿脱钩"
        );
    }
}
