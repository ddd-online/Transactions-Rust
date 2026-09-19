//! Markdown 渲染（纯 Rust，无新依赖）。
//!
//! 解析与清洗合成一步——**先转义再拼标签**，
//! 因此输出天然不含任何来自输入的 HTML，`<script>` 之类不可能被执行。
//!
//! ## 支持范围
//!
//! | 语法 | 输出 |
//! |---|---|
//! | `# ~ ######` | `<h1>`~`<h6>`（`===` / `---` 下划线式标题也支持） |
//! | 段落 | `<p>`（空行分段，段内换行 → `<br>`） |
//! | `-` / `*` / `+` | `<ul><li>`（支持缩进嵌套与 GFM 任务列表 `- [x]`） |
//! | `1.` | `<ol><li>` |
//! | `>` | `<blockquote>`（可嵌套） |
//! | ``` 围栏 / 4 空格缩进 | `<pre><code>` |
//! | `---` / `***` / `___` | `<hr>` |
//! | GFM 表格 | `<table><thead><tbody><tr><th><td>`（含 `:---:` 对齐） |
//! | 行内 | `` `code` ``、`**粗**`、`*斜*`/`_斜_`、`~~删除~~`、`[文本](url)`、`![alt](url)` |
//!
//! ## 安全边界（有意为之）
//!
//! * **全文转义**：`&` `<` `>` `"` 先转义，任何原始 HTML 都只会当作纯文本显示
//!   （不是"过滤掉危险标签"，而是"根本不产生标签"）。
//! * **URL 白名单**：只放行 `http://` / `https://` / `mailto:` / 站内锚点 `#`；
//!   `javascript:`、`data:`、`file:` 等一律降级为纯文本（不给 `<a>`/`<img>`）。
//! * 不做语法高亮（本仓库界面层不引入 JS 库，
//!   代码块只按等宽字体 + `--transactions-color-markdown-code-block` 底色渲染）。
//! * 不做 HTML 实体解码：`&amp;` 这类输入会按字面显示成 `&amp;`。

use leptos::prelude::*;

/// 渲染 Markdown 文本为 HTML 片段。
///
/// 调用方通过 Leptos 的 `inner_html` 注入；因为本函数保证了"输出里只有自己拼的标签"，
/// 注入是安全的（见模块级说明）。
pub fn render_markdown(source: &str) -> String {
    if source.trim().is_empty() {
        return String::new();
    }
    // 归一化换行，避免 \r\n 影响行级判断
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut out = String::with_capacity(source.len() + source.len() / 2);
    let mut index = 0_usize;

    while index < lines.len() {
        let line = lines[index];

        // 空行：跳过（段落由连续非空行组成）
        if line.trim().is_empty() {
            index += 1;
            continue;
        }

        // 围栏代码块 ``` 或 ~~~
        if let Some((fence_char, fence_len, info)) = parse_fence(line) {
            let mut body = String::new();
            index += 1;
            while index < lines.len() {
                if is_closing_fence(lines[index], fence_char, fence_len) {
                    index += 1;
                    break;
                }
                body.push_str(lines[index]);
                body.push('\n');
                index += 1;
            }
            out.push_str("<pre class=\"md-pre\"><code");
            if !info.is_empty() {
                // 语言名只作为 class 后缀，且已经过字母数字过滤
                out.push_str(" class=\"language-");
                out.push_str(&sanitize_class(&info));
                out.push('"');
            }
            out.push('>');
            out.push_str(&escape_html(&body));
            out.push_str("</code></pre>");
            continue;
        }

        // 水平分割线
        if is_thematic_break(line) {
            out.push_str("<hr>");
            index += 1;
            continue;
        }

        // ATX 标题
        if let Some((level, text)) = parse_atx_heading(line) {
            out.push_str(&format!("<h{level}>"));
            out.push_str(&render_inline(text));
            out.push_str(&format!("</h{level}>"));
            index += 1;
            continue;
        }

        // Setext 标题（下一行是 === 或 ---）
        if index + 1 < lines.len() && !line.trim().is_empty() {
            let underline = lines[index + 1].trim();
            let setext = underline
                .chars()
                .next()
                .filter(|_| underline.chars().all(|c| c == '=' || c == '-'));
            if let Some(marker) = setext {
                let level = if marker == '=' { 1 } else { 2 };
                out.push_str(&format!("<h{level}>"));
                out.push_str(&render_inline(line.trim()));
                out.push_str(&format!("</h{level}>"));
                index += 2;
                continue;
            }
        }

        // 引用块（连续以 > 开头的行，去掉一层 > 后递归渲染）
        if line.trim_start().starts_with('>') {
            let mut inner = String::new();
            while index < lines.len() {
                let current = lines[index];
                let trimmed = current.trim_start();
                if !trimmed.starts_with('>') {
                    break;
                }
                let rest = trimmed.strip_prefix('>').unwrap_or(trimmed);
                inner.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                inner.push('\n');
                index += 1;
            }
            out.push_str("<blockquote>");
            out.push_str(&render_markdown(&inner));
            out.push_str("</blockquote>");
            continue;
        }

        // GFM 表格：当前行含 | 且下一行是分隔行
        if line.contains('|') && index + 1 < lines.len() && is_table_delimiter(lines[index + 1]) {
            let aligns = parse_table_aligns(lines[index + 1]);
            let header = split_table_row(line);
            index += 2;
            let mut rows: Vec<Vec<String>> = Vec::new();
            while index < lines.len()
                && lines[index].contains('|')
                && !lines[index].trim().is_empty()
            {
                rows.push(split_table_row(lines[index]));
                index += 1;
            }
            out.push_str("<table class=\"md-table\"><thead><tr>");
            for (column, cell) in header.iter().enumerate() {
                out.push_str(&format!("<th{}>", align_attr(&aligns, column)));
                out.push_str(&render_inline(cell));
                out.push_str("</th>");
            }
            out.push_str("</tr></thead><tbody>");
            for row in &rows {
                out.push_str("<tr>");
                for (column, cell) in row.iter().enumerate() {
                    out.push_str(&format!("<td{}>", align_attr(&aligns, column)));
                    out.push_str(&render_inline(cell));
                    out.push_str("</td>");
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>");
            continue;
        }

        // 列表（有序 / 无序）
        if list_marker(line).is_some() {
            let ordered = list_marker(line).map(|(_, o)| o).unwrap_or(false);
            let (html, next) = render_list(&lines, index, ordered);
            out.push_str(&html);
            index = next;
            continue;
        }

        // 缩进代码块（4 空格 / 1 个 Tab）
        if is_indented_code(line) {
            let mut body = String::new();
            while index < lines.len()
                && (is_indented_code(lines[index]) || lines[index].trim().is_empty())
            {
                let current = lines[index];
                body.push_str(strip_indent(current));
                body.push('\n');
                index += 1;
            }
            out.push_str("<pre class=\"md-pre\"><code>");
            out.push_str(&escape_html(&body));
            out.push_str("</code></pre>");
            continue;
        }

        // 段落：连续非空、且不是其它块级起始的行
        let mut paragraph = String::new();
        while index < lines.len() {
            let current = lines[index];
            if current.trim().is_empty() || starts_a_block(&lines, index) {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(current.trim());
            index += 1;
        }
        if !paragraph.is_empty() {
            out.push_str("<p>");
            // 段内换行这里用 <br> 保留，而不是合并成一个空格；
            // 用户在文本域里敲下的换行会被原样呈现，更贴近预期
            let mut first = true;
            for part in paragraph.split('\n') {
                if !first {
                    out.push_str("<br>");
                }
                out.push_str(&render_inline(part));
                first = false;
            }
            out.push_str("</p>");
        }
    }

    out
}

/// Markdown 渲染组件：`<div class="md-body" inner_html=...>`。
///
/// `inner_html` 的内容由 [`render_markdown`] 生成（先转义再拼标签），
/// 因此这里不存在注入面。
#[component]
pub fn Markdown(
    /// Markdown 源文本
    #[prop(into)]
    source: Signal<String>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let class = class.unwrap_or_default();
    view! {
        <div
            class=format!("md-body {class}")
            inner_html=move || render_markdown(&source.get())
        ></div>
    }
}

// ---------------------------------------------------------------- 行级识别

/// 围栏起始行 → `(围栏字符, 围栏长度, 语言)`。
fn parse_fence(line: &str) -> Option<(char, usize, String)> {
    let trimmed = line.trim_start();
    let fence_char = trimmed.chars().next()?;
    if fence_char != '`' && fence_char != '~' {
        return None;
    }
    let fence_len = trimmed.chars().take_while(|c| *c == fence_char).count();
    if fence_len < 3 {
        return None;
    }
    let info = trimmed[fence_len..].trim().to_string();
    // 反引号围栏的 info 里不允许再出现反引号（CommonMark）
    if fence_char == '`' && info.contains('`') {
        return None;
    }
    Some((fence_char, fence_len, info))
}

/// 是否是结束围栏（同字符、长度不小于起始围栏、行内只有围栏）。
fn is_closing_fence(line: &str, fence_char: char, fence_len: usize) -> bool {
    let trimmed = line.trim();
    let count = trimmed.chars().take_while(|c| *c == fence_char).count();
    count >= fence_len && count == trimmed.chars().count()
}

/// 水平分割线：三个及以上的 `-` / `*` / `_`，可含空格。
fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    let mut marker = None;
    let mut count = 0_usize;
    for character in trimmed.chars() {
        if character == ' ' || character == '\t' {
            continue;
        }
        match marker {
            None => {
                if character != '-' && character != '*' && character != '_' {
                    return false;
                }
                marker = Some(character);
                count = 1;
            }
            Some(expected) => {
                if character != expected {
                    return false;
                }
                count += 1;
            }
        }
    }
    count >= 3
}

/// ATX 标题：`## 标题 ##`。
fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    // 去掉尾部成对的 #
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((level, text))
}

/// 列表标记 → `(缩进宽度, 是否有序)`。缩进按 2 空格一档（`- ` / `1. ` / `1) `）。
fn list_marker(line: &str) -> Option<(usize, bool)> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next().unwrap_or(' ');
        let second = trimmed.chars().nth(1).unwrap_or(' ');
        if (first == '-' || first == '*' || first == '+') && second == ' ' {
            return Some((indent, false));
        }
    }
    // 有序：数字 + . 或 ) + 空格
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 9 {
        let rest = &trimmed[digits..];
        if (rest.starts_with(". ") || rest.starts_with(") ")) && !rest.starts_with("..") {
            return Some((indent, true));
        }
    }
    None
}

/// 4 空格或一个 Tab 缩进的代码行。
fn is_indented_code(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

/// 去掉一层缩进（4 空格或一个 Tab）。
fn strip_indent(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('\t') {
        return rest;
    }
    line.strip_prefix("    ").unwrap_or(line)
}

/// 当前行是否会开启一个新的块（用于终止段落收集）。
fn starts_a_block(lines: &[&str], index: usize) -> bool {
    let line = lines[index];
    if parse_fence(line).is_some()
        || is_thematic_break(line)
        || parse_atx_heading(line).is_some()
        || list_marker(line).is_some()
        || is_indented_code(line)
    {
        return true;
    }
    if line.trim_start().starts_with('>') {
        return true;
    }
    if line.contains('|') && index + 1 < lines.len() && is_table_delimiter(lines[index + 1]) {
        return true;
    }
    false
}

// ---------------------------------------------------------------- 列表

/// 渲染一个列表（含缩进嵌套），返回 `(html, 下一行下标)`。
fn render_list(lines: &[&str], start: usize, ordered: bool) -> (String, usize) {
    let base_indent = list_marker(lines[start])
        .map(|(indent, _)| indent)
        .unwrap_or(0);
    let tag = if ordered { "ol" } else { "ul" };
    let mut html = format!("<{tag}>");
    let mut index = start;

    while index < lines.len() {
        let line = lines[index];
        // 空行：看下一行是否仍是同一列表（松散列表），否则结束
        if line.trim().is_empty() {
            let next = index + 1;
            match list_marker(lines.get(next).copied().unwrap_or("")) {
                Some((indent, same_ordered))
                    if indent >= base_indent && same_ordered == ordered =>
                {
                    index = next;
                    continue;
                }
                _ => break,
            }
        }

        let Some((indent, item_ordered)) = list_marker(line) else {
            // 列表项的续行（缩进对齐到内容列）
            break;
        };
        if item_ordered != ordered || indent < base_indent {
            break;
        }
        if indent > base_indent {
            // 交给下一轮：把嵌套列表当作当前项的子内容（这里直接换层渲染）
            break;
        }

        let content = list_item_content(line).unwrap_or_default();
        // 任务列表（GFM：`- [x] 内容`）
        let (checkbox, text) = split_task_marker(content);
        html.push_str("<li>");
        if let Some(checked) = checkbox {
            html.push_str("<input type=\"checkbox\" disabled");
            if checked {
                html.push_str(" checked");
            }
            html.push('>');
            html.push(' ');
        }
        html.push_str(&render_inline(text));
        index += 1;

        // 子列表：缩进更深的紧随行
        let mut nested = String::new();
        while index < lines.len() {
            let current = lines[index];
            if current.trim().is_empty() {
                break;
            }
            match list_marker(current) {
                Some((child_indent, child_ordered)) if child_indent > base_indent => {
                    let (child_html, next) = render_list(lines, index, child_ordered);
                    nested.push_str(&child_html);
                    index = next;
                }
                _ => break,
            }
        }
        html.push_str(&nested);
        html.push_str("</li>");
    }

    html.push_str(&format!("</{tag}>"));
    (html, index)
}

/// 列表项的内容（去掉标记与一个前导空格）。
fn list_item_content(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = &line[indent..];
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some(rest);
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(value) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(value);
        }
    }
    None
}

/// 任务列表前缀 → `(Some(是否勾选), 剩余文本)`。
fn split_task_marker(content: &str) -> (Option<bool>, &str) {
    let lower = content.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("[x] ") {
        let offset = content.len() - rest.len();
        return (Some(true), &content[offset..]);
    }
    if let Some(rest) = lower.strip_prefix("[ ] ") {
        let offset = content.len() - rest.len();
        return (Some(false), &content[offset..]);
    }
    (None, content)
}

// ---------------------------------------------------------------- 表格

/// 表格分隔行：`| --- | :--: |`。
fn is_table_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('-') {
        return false;
    }
    let cells = split_table_row(trimmed);
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|cell| {
        let text = cell.trim();
        !text.is_empty()
            && text.starts_with([':', '-'])
            && text.ends_with([':', '-'])
            && text.chars().all(|c| c == ':' || c == '-' || c == ' ')
            && text.chars().filter(|c| *c == '-').count() >= 1
    })
}

/// 分隔行 → 每列对齐方式（`""` / `left` / `center` / `right`）。
fn parse_table_aligns(line: &str) -> Vec<&'static str> {
    split_table_row(line)
        .iter()
        .map(|cell| {
            let text = cell.trim();
            let left = text.starts_with(':');
            let right = text.ends_with(':');
            match (left, right) {
                (true, true) => "center",
                (false, true) => "right",
                (true, false) => "left",
                _ => "",
            }
        })
        .collect()
}

fn align_attr(aligns: &[&'static str], column: usize) -> String {
    match aligns.get(column).copied().unwrap_or("") {
        "" => String::new(),
        other => format!(" style=\"text-align: {other}\""),
    }
}

/// 拆分表格行（去掉首尾的 `|`，按未转义的 `|` 切分）。
fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for character in trimmed.chars() {
        if escaped {
            if character != '|' {
                current.push('\\');
            }
            current.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '|' => {
                cells.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(character),
        }
    }
    if escaped {
        current.push('\\');
    }
    cells.push(current.trim().to_string());
    cells
}

// ---------------------------------------------------------------- 行内

/// 行内渲染：代码 → 图片 → 链接 → 粗体 → 删除线 → 斜体，其余文本转义。
fn render_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0_usize;

    while index < chars.len() {
        let character = chars[index];

        // 行内代码 `code`（反引号个数可多于 1）
        if character == '`' {
            let ticks = chars[index..].iter().take_while(|c| **c == '`').count();
            let fence = "`".repeat(ticks);
            let rest: String = chars[index + ticks..].iter().collect();
            if let Some((code, _after)) = rest.split_once(&fence) {
                out.push_str("<code>");
                out.push_str(&escape_html(code));
                out.push_str("</code>");
                index += ticks + code.chars().count() + ticks;
                continue;
            }
        }

        // 行内数学/路径里常见的反斜杠转义
        if character == '\\' && index + 1 < chars.len() && is_escapable(chars[index + 1]) {
            out.push_str(&escape_html(&chars[index + 1].to_string()));
            index += 2;
            continue;
        }

        // 图片 ![alt](url) 或链接 [text](url)
        if character == '!' || character == '[' {
            let is_image = character == '!';
            let bracket_index = if is_image { index + 1 } else { index };
            if let Some(consumed) = try_render_link(&chars, bracket_index, is_image, &mut out) {
                index += consumed + if is_image { 1 } else { 0 };
                continue;
            }
        }

        // 粗体 / 斜体 / 删除线
        if character == '*' || character == '_' || character == '~' {
            let run = chars[index..]
                .iter()
                .take_while(|c| **c == character)
                .count();
            // 依次尝试 3 连、2 连、1 连；**右侧分隔符必须与左侧等长**，
            // 否则 `**bold*` 这类不闭合输入会算出错误的下标。
            let mut matched = false;
            for width in [3_usize, 2, 1] {
                if run < width {
                    continue;
                }
                let delimiter = character.to_string().repeat(width);
                let rest: String = chars[index + width..].iter().collect();
                let Some((inner, _after)) = rest.split_once(&delimiter) else {
                    continue;
                };
                if inner.trim().is_empty() || inner.starts_with(char::is_whitespace) {
                    continue;
                }
                let tag = match (width, character) {
                    (3, _) | (2, '*') | (2, '_') => "strong",
                    (1, '~') => "del", // 单个 ~ 不构成删除线，按普通文本处理
                    _ if character == '~' => "del",
                    _ => "em",
                };
                out.push_str(&format!("<{tag}>"));
                out.push_str(&render_inline(inner));
                out.push_str(&format!("</{tag}>"));
                index += width + inner.chars().count() + width;
                matched = true;
                break;
            }
            if matched {
                continue;
            }
        }

        // 普通字符（含 `<` `>` `&` `"` 等一律转义）
        out.push_str(&escape_html(&character.to_string()));
        index += 1;
    }

    out
}

/// 尝试渲染 `[文本](地址)` / `![替代文本](地址)`；成功时写入 `out` 并返回消耗的字符数。
fn try_render_link(
    chars: &[char],
    bracket_index: usize,
    is_image: bool,
    out: &mut String,
) -> Option<usize> {
    if chars.get(bracket_index) != Some(&'[') {
        return None;
    }
    let mut depth = 0_i32;
    let mut close = None;
    for (offset, character) in chars[bracket_index..].iter().enumerate() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(bracket_index + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut paren_depth = 0_i32;
    let mut end = None;
    for (offset, character) in chars[close + 1..].iter().enumerate() {
        match character {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    end = Some(close + 1 + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;

    let label: String = chars[bracket_index + 1..close].iter().collect();
    let target: String = chars[close + 2..end].iter().collect();
    // `[文本](url "标题")`：标题部分丢弃（这里只保留 href/src/alt，不输出 title）
    let url = target
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();

    if !is_safe_url(&url) {
        // 不安全协议：降级为纯文本
        out.push_str(&escape_html(&format!(
            "{}[{}]({})",
            if is_image { "!" } else { "" },
            label,
            url
        )));
        return Some(end - bracket_index);
    }

    let url = escape_attr(&url);
    if is_image {
        out.push_str(&format!(
            "<img src=\"{}\" alt=\"{}\">",
            url,
            escape_attr(&label)
        ));
    } else {
        out.push_str(&format!(
            "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
            url,
            render_inline(&label)
        ));
    }
    Some(end - bracket_index)
}

/// 可被反斜杠转义的字符（CommonMark 的 ASCII 标点）。
fn is_escapable(character: char) -> bool {
    matches!(
        character,
        '\\' | '`'
            | '*'
            | '_'
            | '{'
            | '}'
            | '['
            | ']'
            | '('
            | ')'
            | '#'
            | '+'
            | '-'
            | '.'
            | '!'
            | '~'
            | '|'
            | '>'
    )
}

/// URL 白名单：只允许 `http(s)` / `mailto` / 站内锚点 / 相对路径。
fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with('#')
        || lower.starts_with('/')
        || lower.starts_with("./")
        || lower.starts_with("../")
    {
        return true;
    }
    // 没有 `:` 的形态（例如 `foo/bar.png`）视为相对路径
    !trimmed.contains(':')
}

// ---------------------------------------------------------------- 转义

/// HTML 文本转义（`&` `<` `>` `"` `'`）。
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

/// HTML 属性值转义（比文本多转义换行，避免属性被截断）。
fn escape_attr(input: &str) -> String {
    escape_html(input).replace(['\n', '\r'], " ")
}

/// 语言名 → 合法 class 片段（只保留字母、数字、`-`、`_`、`+`、`#`）。
fn sanitize_class(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '#'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_tags_are_escaped_not_executed() {
        let html = render_markdown("<script>alert(1)</script>");
        assert!(!html.contains("<script"), "不应产生可执行脚本标签: {html}");
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn dangerous_urls_are_downgraded_to_text() {
        let html = render_markdown("[点我](javascript:alert(1))");
        assert!(!html.contains("href"), "javascript: 不应产生链接: {html}");
        let html = render_markdown("![图](data:text/html;base64,AAA)");
        assert!(!html.contains("<img"), "data: 不应产生图片: {html}");
    }

    #[test]
    fn safety_attribute_quotes_are_escaped() {
        // 链接地址里塞入 `"` + 事件处理器：引号必须被转义成 `&quot;`，
        // 否则浏览器会把 `onmouseover=alert(1)` 当成一个真的属性。
        let html = render_markdown("[x](https://a.com/?q=\"onmouseover=alert(1))");
        assert!(
            !html.contains("\"onmouseover=alert"),
            "属性引号必须被转义，不能形成新的属性: {html}"
        );
        assert!(
            html.contains("&quot;onmouseover=alert(1)"),
            "应转义为 HTML 实体: {html}"
        );
    }

    #[test]
    fn headings_lists_and_code_blocks_render() {
        let html = render_markdown("# 标题\n\n- 一\n- 二\n\n```rust\nlet a = 1;\n```\n");
        assert!(html.contains("<h1>标题</h1>"));
        assert!(html.contains("<ul><li>一</li><li>二</li></ul>"));
        assert!(html.contains("<pre class=\"md-pre\"><code class=\"language-rust\">"));
        assert!(html.contains("let a = 1;"));
    }

    #[test]
    fn tables_render_with_alignment() {
        let html = render_markdown("| 名称 | 金额 |\n| :--- | ---: |\n| 早餐 | 12 |\n");
        assert!(html.contains("<table class=\"md-table\">"));
        assert!(html.contains("<th style=\"text-align: left\">名称</th>"));
        assert!(html.contains("<td style=\"text-align: right\">12</td>"));
    }

    #[test]
    fn empty_input_returns_empty_string() {
        assert_eq!(render_markdown(""), "");
        assert_eq!(render_markdown("   \n  "), "");
    }
}
