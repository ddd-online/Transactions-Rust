//! 表格 —— 表头 / 空态 / 行 hover / 选中。
//!
//! 表格（列定义 + 行插槽），刻意**不做**
//! 数据驱动的单元格渲染：行内容差异太大（金额符号、类型标签、关联图标），
//! 交给页面写 `<tr>` 更直观，也让"行点击/行样式"这类行为留在页面里。
//!
//! ```
//! <Table columns=vec![
//!     TableColumn::new("日期").width(100).align(TableAlign::Center),
//!     TableColumn::new("描述"),
//! ]>
//!     <tr><td class="ui-table__cell">"…"</td></tr>
//! </Table>
//! ```
//!
//! 样式在 `static/css/ui.css` 的 `.ui-table*`；`.table-wrapper` 复用 `app.css` 里的滚动容器。

use leptos::prelude::*;

/// 单元格水平对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TableAlign {
    fn class(self) -> &'static str {
        match self {
            TableAlign::Left => "",
            TableAlign::Center => "ui-table__cell--center",
            TableAlign::Right => "ui-table__cell--right",
        }
    }
}

/// 一列的定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumn {
    pub label: String,
    /// 列宽（px）；`None` 表示自适应
    pub width: Option<u32>,
    pub align: TableAlign,
}

impl TableColumn {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            width: None,
            align: TableAlign::Center,
        }
    }

    pub fn width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = align;
        self
    }

    /// 表头单元格的 `style`（只有设了宽度才有内容）。
    fn style(&self) -> String {
        match self.width {
            Some(width) => format!("width: {width}px;"),
            None => String::new(),
        }
    }
}

impl From<&str> for TableColumn {
    fn from(label: &str) -> Self {
        TableColumn::new(label)
    }
}

/// 表格容器：`<div class="table-wrapper"><table class="ui-table">…`。
///
/// `children` 是 `<tbody>` 里的行（`<tr>`），必须用 [`ChildrenFn`]——页面通常把它包在
/// 条件渲染里，需要可重复求值。
#[component]
pub fn Table(
    /// 列定义（顺序即列顺序）
    columns: Vec<TableColumn>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
    /// 表体行
    children: ChildrenFn,
) -> impl IntoView {
    let mut classes = String::from("ui-table");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    let header = columns
        .iter()
        .map(|column| {
            let mut cell_class = String::from("ui-table__head-cell");
            let align = column.align.class();
            if !align.is_empty() {
                cell_class.push(' ');
                cell_class.push_str(align);
            }
            let style = column.style();
            let label = column.label.clone();
            view! {
                <th class=cell_class style=style>
                    {label}
                </th>
            }
        })
        .collect_view();

    view! {
        <div class="table-wrapper">
            <table class=classes>
                <thead>
                    <tr>{header}</tr>
                </thead>
                <tbody>{children()}</tbody>
            </table>
        </div>
    }
}
