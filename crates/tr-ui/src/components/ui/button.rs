//! 按钮 —— 统一按钮系统。
//!
//! ## 变体与尺寸
//!
//! 变体 `primary`（实心主色）/ `secondary`（描边次要）/ `text`（纯文字）/
//! `text-danger`（文字危险）/ `primary-danger`（实心危险）/ `dashed`（虚线）/ `link`（链接）；
//! 尺寸 `sm` 28px / `md` 36px / `lg` 44px。
//!
//! **默认变体是 `secondary`**（不是 `primary`）：一个界面里"主操作"永远只有一个，
//! 让默认值落到次要按钮上，主按钮就必须显式声明 `variant = ButtonVariant::Primary`，
//! 层次因此是"写出来的"而不是"漏出来的"。
//!
//! ## 什么时候用哪个（全仓统一口径）
//!
//! | 位置 | 变体 | 尺寸 |
//! |---|---|---|
//! | 页面主操作（记一笔 / 建仓 / 新建模板 / 初始化 / 弹窗确认） | `Primary` | `Middle` |
//! | 工具条里的其它动作（刷新 / 上下周期 / 今天 / 全部收起） | `Secondary` | `Middle` |
//! | 面板、卡片、栏头内部的动作（保存 / 新增分类 / 重命名 / 编辑） | `Secondary` | `Small` |
//! | 表格行内与非模态小动作 | `Secondary` | `Small` |
//! | 删除确认的执行键 | `PrimaryDanger` | `Small` |
//! | 行内文字动作（编辑 / 删除 / 刷新行情） | `Link` | `Small` |
//! | 图标按钮 | 见 [`IconButton`] | 28px |
//!
//! 尺寸口径：**28px 在 36px 高的工具条里是错的**。工具条（含日期字段、Select、
//! 账本胶囊，都是 36px）里的按钮一律 `Middle`；`Small` 只出现在面板/卡片/表格行内部。
//!
//! `icon_only` 让按钮退化为正方形图标按钮（`min-width: auto` + 与高度等宽）。

use leptos::prelude::*;

use crate::icons::{self, Icon};

/// 按钮变体。
///
/// 默认是 [`ButtonVariant::Secondary`]：主按钮必须显式写出来（见模块头）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Text,
    TextDanger,
    PrimaryDanger,
    Dashed,
    Link,
}

impl ButtonVariant {
    pub fn class(self) -> &'static str {
        match self {
            ButtonVariant::Primary => "ui-btn--primary",
            ButtonVariant::Secondary => "ui-btn--secondary",
            ButtonVariant::Text => "ui-btn--text",
            ButtonVariant::TextDanger => "ui-btn--text-danger",
            ButtonVariant::PrimaryDanger => "ui-btn--primary-danger",
            ButtonVariant::Dashed => "ui-btn--dashed",
            ButtonVariant::Link => "ui-btn--link",
        }
    }
}

/// 按钮尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Small,
    #[default]
    Middle,
    Large,
}

impl ButtonSize {
    pub fn class(self) -> &'static str {
        match self {
            ButtonSize::Small => "ui-btn--sm",
            ButtonSize::Middle => "",
            ButtonSize::Large => "ui-btn--lg",
        }
    }
}

/// 通用按钮。
///
/// * `loading` / `disabled` 接受 `bool`、信号或闭包（`#[prop(into)]`）；
/// * `on_click` 直接接受 `move || { ... }` 闭包；
/// * `children` 是按钮文案或图标。
#[component]
pub fn Button(
    /// 外观变体（默认 `Secondary`；主操作用 `Primary`）
    #[prop(optional)]
    variant: ButtonVariant,
    /// 占满父容器宽度
    #[prop(optional)]
    block: bool,
    /// 尺寸（默认 `Middle` 36px；面板/表格行内用 `Small` 28px，见模块头）
    #[prop(optional)]
    size: ButtonSize,
    /// 图标按钮（正方形，无最小宽度）
    #[prop(optional)]
    icon_only: bool,
    /// 原生 `title`（悬浮提示）
    #[prop(optional, into)]
    title: Option<Signal<String>>,
    /// 可访问名（`aria-label`）。**视觉文案会被角标/图标改写时必填**：
    /// 例如「筛选 3」这种按钮，读屏听到的应该是动作本身，而不是计数。
    #[prop(optional, into)]
    aria_label: Option<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 加载中：显示旋转指示并禁用点击
    #[prop(optional, into)]
    loading: Option<Signal<bool>>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 点击回调
    #[prop(optional, into)]
    on_click: Option<UnsyncCallback<()>>,
    children: Children,
) -> impl IntoView {
    let loading = loading.unwrap_or_else(|| Signal::derive(|| false));
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from("ui-btn ");
    classes.push_str(variant.class());
    classes.push(' ');
    classes.push_str(size.class());
    if icon_only {
        classes.push_str(" ui-btn--icon-only");
    }
    if block {
        classes.push_str(" ui-btn--block");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <button
            type="button"
            class=classes
            title=title
            aria-label=aria_label
            disabled=move || disabled.get() || loading.get()
            on:click=move |_| {
                if let Some(callback) = on_click {
                    callback.run(());
                }
            }
        >
            <Show when=move || loading.get()>
                <span class="ui-btn__icon ui-spin__indicator">
                    {icons::icon(Icon::Loading)}
                </span>
            </Show>
            {children()}
        </button>
    }
}

/// 图标按钮的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonVariant {
    /// 通用图标动作（编辑 / 关联 / 同步 / 下载）
    #[default]
    Plain,
    /// 删除类（悬停转为危险色）
    Danger,
    /// 叠在图片上的图标动作：自带毛玻璃底，保证在任意图片上都看得见
    OnMedia,
}

impl IconButtonVariant {
    fn class(self) -> &'static str {
        match self {
            IconButtonVariant::Plain => "ui-icon-btn",
            IconButtonVariant::Danger => "ui-icon-btn ui-icon-btn--danger",
            IconButtonVariant::OnMedia => "ui-icon-btn ui-icon-btn--on-media",
        }
    }
}

/// 图标按钮。
///
/// 全仓只有一个尺寸口径（28px 正方形、6px 圆角、透明底、悬停浅底 + 主色 focus 环）；
/// `compact` 是唯一的例外（20px），只给缩略图角标这类"控件本身就极小"的位置用。
/// 按钮必须给 `label`：它同时充当 `title` 与 `aria-label`，保证图标按钮有可访问名。
#[component]
pub fn IconButton(
    /// 语义（默认通用）
    #[prop(optional)]
    variant: IconButtonVariant,
    /// 紧凑尺寸（20px），只给缩略图角标一类极小控件
    #[prop(optional)]
    compact: bool,
    /// 可访问名 / 悬浮提示
    #[prop(into)]
    label: String,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 阻止冒泡：图标按钮嵌在"整行可点"的容器（事件卡片、关联交易卡片）里时必开，
    /// 否则点删除会顺带把整行选中
    #[prop(optional)]
    stop_propagation: bool,
    /// 点击回调
    #[prop(optional, into)]
    on_click: Option<UnsyncCallback<()>>,
    children: Children,
) -> impl IntoView {
    let disabled = disabled.unwrap_or_else(|| Signal::derive(|| false));

    let mut classes = String::from(variant.class());
    if compact {
        classes.push_str(" ui-icon-btn--compact");
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <button
            type="button"
            class=classes
            title=label.clone()
            aria-label=label
            disabled=move || disabled.get()
            on:click=move |event| {
                if stop_propagation {
                    event.stop_propagation();
                }
                if let Some(callback) = on_click {
                    callback.run(());
                }
            }
        >
            {children()}
        </button>
    }
}
