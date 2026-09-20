//! 展示层格式化（金额、交易类型文案）。
//!
//! **金额换算必须走 [`tr_domain::money`]**：这是"金额恒为整数分"纪律的唯一守门人，
//! 界面层不得自行实现 `/100`。本模块只负责加上符号与类型文案。

use tr_domain::money::cents_to_yuan;

/// 交易类型 → 中文标签：income→收入、expense→支出、transfer→转账。
pub fn transaction_type_label(transaction_type: &str) -> &'static str {
    match transaction_type {
        "income" => "收入",
        "expense" => "支出",
        "transfer" => "转账",
        // 未知类型回落到原始字符串，这里交给调用方处理（返回空串表示"未知"）
        _ => "",
    }
}

/// 交易类型标签，未知类型回落到原始值。
pub fn transaction_type_text(transaction_type: &str) -> String {
    let label = transaction_type_label(transaction_type);
    if label.is_empty() {
        transaction_type.to_string()
    } else {
        label.to_string()
    }
}

/// 分 → 元（两位小数，**不做千分位分组**）。
pub fn amount(cents: i64) -> String {
    cents_to_yuan(cents)
}

/// 带符号金额：支出前缀 `-`、收入前缀 `+`、转账不加符号。
///
/// 符号由**交易类型**决定，而不是由金额正负决定（与 [`signed_yuan`] 相反）。
pub fn signed_amount(transaction_type: &str, cents: i64) -> String {
    match transaction_type {
        "expense" => format!("-{}", cents_to_yuan(cents)),
        "income" => format!("+{}", cents_to_yuan(cents)),
        _ => cents_to_yuan(cents),
    }
}

/// 金额语义色类名（`app.css` 的 `.tr-cell-price.price-*`）。
pub fn amount_class(transaction_type: &str) -> &'static str {
    match transaction_type {
        "income" => "price-income",
        "expense" => "price-expense",
        "transfer" => "price-transfer",
        _ => "",
    }
}

/// 交易类型行底色类名（`row-type-*`）。
pub fn row_class(transaction_type: &str) -> String {
    format!("row-type-{transaction_type}")
}

/// 交易类型文字色类名（`app.css` 的 `.tr-cell-type.type-*`）。
pub fn type_class(transaction_type: &str) -> String {
    format!("type-{transaction_type}")
}

// ---------------------------------------------------------------- 股票域 / 通用

/// 带符号金额（元）：`>0` 加 `+`、`<0` 由 [`amount`] 自带 `-`、`0` 不加。
///
/// 符号由**正负**决定（与 [`signed_amount`] 按交易类型决定符号不同），值为绝对值。
pub fn signed_yuan(cents: i64) -> String {
    if cents > 0 {
        format!("+{}", amount(cents))
    } else if cents < 0 {
        format!("-{}", amount(cents.saturating_abs()))
    } else {
        amount(0)
    }
}

/// 盈亏着色类名（**A 股红涨绿跌**，股票页 CSS 会把两个类反向映射）。
///
/// `>0` → `amount-income`、`<0` → `amount-expense`、`0` → 空串（继承默认色）。
pub fn pnl_class(cents: i64) -> &'static str {
    if cents > 0 {
        "amount-income"
    } else if cents < 0 {
        "amount-expense"
    } else {
        ""
    }
}

/// 百分比（两位小数）：`rate_text(12.345)` → `"12.35%"`（**不加正号**，统计页用）。
pub fn rate_text(percent: f64) -> String {
    if percent.is_finite() {
        format!("{percent:.2}%")
    } else {
        "0.00%".to_string()
    }
}

/// 百分比（两位小数，正数带 `+`）：`signed_percent(12.345)` → `"+12.35%"`（股票页用）。
pub fn signed_percent(percent: f64) -> String {
    if !percent.is_finite() {
        return "0.00%".to_string();
    }
    if percent >= 0.0 {
        format!("+{percent:.2}%")
    } else {
        format!("{percent:.2}%")
    }
}

/// 可选百分比：`None` → `"—"`（行情缺失时的占位）。
pub fn optional_signed_percent(percent: Option<f64>) -> String {
    match percent {
        Some(value) => signed_percent(value),
        None => "—".to_string(),
    }
}

/// 盈亏比：`None`（尚无亏损样本）→ `∞`，否则两位小数。
pub fn ratio_text(ratio: Option<f64>) -> String {
    match ratio {
        Some(value) if value.is_finite() => format!("{value:.2}"),
        Some(_) => "∞".to_string(),
        None => "∞".to_string(),
    }
}

/// 紧凑金额（股票卡片用）：亿 / 万 / 元，
/// `compact_yuan(123_456_789_00)` → `"¥1.2亿"`；0 → `"¥0"`。
pub fn compact_yuan(cents: i64) -> String {
    let yuan = cents.saturating_abs() as f64 / 100.0;
    if yuan >= 1e8 {
        format!("¥{:.1}亿", yuan / 1e8)
    } else if yuan >= 1e4 {
        format!("¥{:.1}万", yuan / 1e4)
    } else {
        format!("¥{yuan:.0}")
    }
}

/// 盈亏文字（股票卡片）：`盈/亏/平` + 紧凑金额。
pub fn pnl_text(cents: i64) -> String {
    let label = if cents > 0 {
        "盈"
    } else if cents < 0 {
        "亏"
    } else {
        "平"
    };
    format!("{label} {}", compact_yuan(cents))
}

/// 现货价格文字：行情缺失（`None` 或 `<= 0`）→ `-`。
pub fn quote_text(latest_price: Option<i64>) -> String {
    match latest_price {
        Some(price) if price > 0 => format!("¥{}", amount(price)),
        _ => "-".to_string(),
    }
}

/// 是否有有效行情（存在且大于 0）。
pub fn has_quote(latest_price: Option<i64>) -> bool {
    matches!(latest_price, Some(price) if price > 0)
}

/// 股数 → 手数（1 手 = 100 股，**向下取整**）。
pub fn lots_of(shares: i64) -> i64 {
    shares.div_euclid(100)
}

/// 股票类型 → 中文标签（未知值回落到原始字符串）。
pub fn trade_type_label(trade_type: &str) -> String {
    match trade_type {
        "open" => "建仓".to_string(),
        "add" => "加仓".to_string(),
        "reduce" => "减仓".to_string(),
        "close" => "清仓".to_string(),
        other => other.to_string(),
    }
}

/// 是否为买入方向（`open` / `add`）。
pub fn is_buy(trade_type: &str) -> bool {
    matches!(trade_type, "open" | "add")
}

/// 轮次盈亏结果标签：`盈利` / `亏损` / `平`。
pub fn result_label(pnl: i64) -> &'static str {
    if pnl > 0 {
        "盈利"
    } else if pnl < 0 {
        "亏损"
    } else {
        "平"
    }
}

/// 轮次结果徽标的类名后缀（`result-win` / `result-loss` / `result-even`）。
pub fn result_class(pnl: i64) -> &'static str {
    if pnl > 0 {
        "result-win"
    } else if pnl < 0 {
        "result-loss"
    } else {
        "result-even"
    }
}

/// `YYYY-MM-DD` → `M-D`（事件列表的短日期；解析失败原样返回）。
pub fn short_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return date.to_string();
    }
    let month = parts[1].parse::<u32>().unwrap_or(1);
    let day = parts[2].parse::<u32>().unwrap_or(1);
    format!("{month}-{day}")
}

/// 截断文本（超出时补 `…`，按**字符**计数）。
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…")
}

/// 字符数（按 Unicode 码点计）。
pub fn char_count(text: &str) -> usize {
    text.chars().count()
}
