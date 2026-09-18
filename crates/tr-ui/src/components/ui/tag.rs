//! 语义标签 —— 10% 语义色底 + 语义色文字（原 `_components.scss` 的 `.tag-*`）。
//!
//! 用于交易类型（收入/支出/转账）、离群值标记，以及中性/主色标签。
//! 样式：`--transactions-color-<kind>` + `--transactions-color-<kind>-tint`。

use leptos::prelude::*;

/// 标签语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagKind {
    /// 中性（灰底灰字，等价表格里的普通标签）
    #[default]
    Neutral,
    Income,
    Expense,
    Transfer,
    Outlier,
    /// 主色标签（信息性标记）
    Primary,
}

impl TagKind {
    pub fn class(self) -> Option<&'static str> {
        match self {
            TagKind::Neutral => None,
            TagKind::Income => Some("ui-tag--income"),
            TagKind::Expense => Some("ui-tag--expense"),
            TagKind::Transfer => Some("ui-tag--transfer"),
            TagKind::Outlier => Some("ui-tag--outlier"),
            TagKind::Primary => Some("ui-tag--primary"),
        }
    }

    /// 由交易类型字符串（`income` / `expense` / `transfer`）映射到语义标签。
    ///
    /// 未知类型回落到中性，避免"后端多了一种类型就渲染成空标签"。
    pub fn from_transaction_type(transaction_type: &str) -> Self {
        match transaction_type {
            "income" => TagKind::Income,
            "expense" => TagKind::Expense,
            "transfer" => TagKind::Transfer,
            _ => TagKind::Neutral,
        }
    }
}

#[component]
pub fn Tag(
    /// 语义
    #[prop(optional)]
    kind: TagKind,
    /// 附加类名（例如 `tag-item`）
    #[prop(optional, into)]
    class: Option<String>,
    /// 原生 `title`
    #[prop(optional, into)]
    title: Option<String>,
    children: Children,
) -> impl IntoView {
    let mut classes = String::from("ui-tag");
    if let Some(kind_class) = kind.class() {
        classes.push(' ');
        classes.push_str(kind_class);
    }
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <span class=classes title=title>
            {children()}
        </span>
    }
}
