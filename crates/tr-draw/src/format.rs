//! 展示层格式化（金额、交易类型文案、股票卡片上的盈亏文字与标尺）。
//!
//! 从 `tr-ui/src/format.rs` 搬来（ADR-0001 的第二波）：这里全是**纯函数** ——
//! 只要有值就能算，与宿主、DOM、信号都无关，所以它们的测试面应该是
//! `cargo test -p tr-draw`，而不是"真开一次窗口看像素"。
//!
//! **金额换算必须走 [`tr_domain::money`]**：那是"金额恒为整数分"纪律的唯一守门人，
//! 任何地方都不得自行实现 `/100`。本模块只负责加上符号、类型文案与紧凑写法。

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
/// `compact_yuan(12_345_678_900)` → `"¥1.2亿"`；0 → `"¥0"`。
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

/// 价格（**厘**/股）文字：能整除分时两位小数、带厘位时三位（`100000 -> "100.00"`、`4389 -> "4.389"`）。
///
/// **价格与金额是两套单位**（价格走厘、金额走分）：成交价、现价、当日涨跌都用它，
/// 金额、市值、费用、盈亏仍然用 [`amount`]。
pub fn price(milli: i64) -> String {
    tr_domain::money::milli_to_yuan(milli)
}

/// 带符号价格（当日涨跌这类差值）：`>0` 加 `+`、`<0` 自带 `-`、`0` 不加。
pub fn signed_price(milli: i64) -> String {
    if milli > 0 {
        format!("+{}", price(milli))
    } else if milli < 0 {
        format!("-{}", price(milli.saturating_abs()))
    } else {
        price(0)
    }
}

/// 现货价格文字：行情缺失（`None` 或 `<= 0`）→ `-`。
pub fn quote_text(latest_price: Option<i64>) -> String {
    match latest_price {
        Some(price_milli) if price_milli > 0 => format!("¥{}", price(price_milli)),
        _ => "-".to_string(),
    }
}

/// 是否有有效行情（存在且大于 0）。
pub fn has_quote(latest_price: Option<i64>) -> bool {
    matches!(latest_price, Some(price) if price > 0)
}

/// 股数 → 手数（1 手 = 100 股，**向下取整**）。
pub fn lots_of(shares: i64) -> i64 {
    shares.div_euclid(tr_domain::stock::SHARES_PER_LOT)
}

/// 按 `scale` 换算并格式化：先按 `scale` 换算，再四舍五入到 `digits` 位小数，最后去掉多余的 0。
///
/// 统计页的标尺（张数 / 金额缩放）用它 —— 从 `pages/stock.rs` 搬来。
pub fn scaled_text(value: f64, scale: f64, digits: i32) -> String {
    let factor = 10_f64.powi(digits);
    let rounded = (value * scale * factor).round() / factor;
    format!("{rounded}")
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
    // 判据只有一份：`tr_domain::stock::is_buy`（用 `consts::STOCK_TRADE_*`，不是裸字面量）。
    // 从前这里自己写了一遍 `"open" | "add"` —— 两处漂移的开始（/code-review 的 Standards 轴点名）。
    tr_domain::stock::is_buy(trade_type)
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

/// 截断文本（超出时补 `…`，按**字符**计数，规则见 [`tr_domain::util`]）。
pub fn truncate(text: &str, max: usize) -> String {
    if tr_domain::util::char_count(text) <= max {
        return text.to_string();
    }
    format!("{}…", tr_domain::util::truncate_chars(text, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_two_decimals_from_cents() {
        assert_eq!(amount(0), "0.00");
        assert_eq!(amount(12_345), "123.45");
        assert_eq!(amount(-12_345), "-123.45");
        assert_eq!(amount(5), "0.05");
    }

    /// 符号由**交易类型**决定（不是金额正负）：支出 `-`、收入 `+`、转账与未知不加。
    #[test]
    fn signed_amount_follows_the_transaction_type() {
        assert_eq!(signed_amount("expense", 12_345), "-123.45");
        assert_eq!(signed_amount("income", 12_345), "+123.45");
        assert_eq!(signed_amount("transfer", 12_345), "123.45");
        assert_eq!(signed_amount("unknown", 12_345), "123.45");
    }

    /// 另一个符号口径：由**正负**决定，且负数取绝对值再加 `-`（不能出现 `--`）。
    #[test]
    fn signed_yuan_follows_the_sign() {
        assert_eq!(signed_yuan(12_345), "+123.45");
        assert_eq!(signed_yuan(-12_345), "-123.45");
        assert_eq!(signed_yuan(0), "0.00");
        assert_eq!(
            signed_yuan(i64::MIN),
            "-92233720368547758.07",
            "saturating_abs：不许溢出成 --"
        );
    }

    #[test]
    fn transaction_type_copy_and_classes() {
        assert_eq!(transaction_type_label("income"), "收入");
        assert_eq!(transaction_type_label("expense"), "支出");
        assert_eq!(transaction_type_label("transfer"), "转账");
        assert_eq!(transaction_type_label("weird"), "");
        assert_eq!(transaction_type_text("weird"), "weird", "未知回落到原值");
        assert_eq!(amount_class("income"), "price-income");
        assert_eq!(amount_class("weird"), "");
        assert_eq!(row_class("expense"), "row-type-expense");
        assert_eq!(type_class("transfer"), "type-transfer");
    }

    #[test]
    fn pnl_colour_and_label_split_at_zero() {
        assert_eq!(pnl_class(1), "amount-income");
        assert_eq!(pnl_class(-1), "amount-expense");
        assert_eq!(pnl_class(0), "");
        assert_eq!(result_label(1), "盈利");
        assert_eq!(result_label(-1), "亏损");
        assert_eq!(result_label(0), "平");
        assert_eq!(result_class(1), "result-win");
        assert_eq!(result_class(-1), "result-loss");
        assert_eq!(result_class(0), "result-even");
    }

    /// 百分比：两位小数；`NaN` / `∞` 一律回落 `0.00%`（不能让页面出现 `NaN%`）。
    #[test]
    fn percents_are_two_decimals_and_never_nan() {
        assert_eq!(rate_text(12.345), "12.35%");
        assert_eq!(rate_text(0.0), "0.00%");
        assert_eq!(rate_text(-3.0), "-3.00%");
        assert_eq!(rate_text(f64::NAN), "0.00%");
        assert_eq!(rate_text(f64::INFINITY), "0.00%");
        assert_eq!(signed_percent(12.345), "+12.35%");
        assert_eq!(signed_percent(0.0), "+0.00%", "0 也算正号（既有行为）");
        assert_eq!(signed_percent(-12.345), "-12.35%");
        assert_eq!(signed_percent(f64::NAN), "0.00%");
        assert_eq!(optional_signed_percent(None), "—");
        assert_eq!(optional_signed_percent(Some(1.5)), "+1.50%");
    }

    /// 盈亏比：没有亏损样本（`None`）与"亏得没法算"（非有限）都是 `∞`。
    #[test]
    fn ratio_text_falls_back_to_infinity() {
        assert_eq!(ratio_text(Some(2.5)), "2.50");
        assert_eq!(ratio_text(Some(0.0)), "0.00");
        assert_eq!(ratio_text(None), "∞");
        assert_eq!(ratio_text(Some(f64::INFINITY)), "∞");
        assert_eq!(ratio_text(Some(f64::NAN)), "∞");
    }

    /// 紧凑金额的三档阈值（亿 / 万 / 元）与负数取绝对值。
    #[test]
    fn compact_yuan_switches_at_yi_and_wan() {
        assert_eq!(compact_yuan(0), "¥0");
        assert_eq!(compact_yuan(99_999), "¥1000", "不到 1 万：整数元");
        assert_eq!(compact_yuan(1_000_000), "¥1.0万");
        assert_eq!(compact_yuan(12_345_600), "¥12.3万");
        assert_eq!(compact_yuan(10_000_000_000), "¥1.0亿");
        assert_eq!(compact_yuan(-10_000_000_000), "¥1.0亿", "负号由调用方给");
        assert_eq!(pnl_text(10_000_000_000), "盈 ¥1.0亿");
        assert_eq!(pnl_text(-1_000_000), "亏 ¥1.0万");
        assert_eq!(pnl_text(0), "平 ¥0");
    }

    #[test]
    fn quotes_need_a_positive_price() {
        assert_eq!(
            quote_text(Some(123_450)),
            "¥123.45",
            "整分的价格仍是两位小数"
        );
        assert_eq!(
            quote_text(Some(4_389)),
            "¥4.389",
            "场内基金的 0.001 元要显示出来"
        );
        assert_eq!(quote_text(Some(0)), "-");
        assert_eq!(quote_text(Some(-1)), "-");
        assert_eq!(quote_text(None), "-");
        assert!(has_quote(Some(1)));
        assert!(!has_quote(Some(0)));
        assert!(!has_quote(None));
    }

    /// 价格（厘）与金额（分）是两套单位：价格能带第三位小数，金额永远是两位。
    #[test]
    fn prices_keep_the_milli_digit_only_when_it_is_there() {
        assert_eq!(price(0), "0.00");
        assert_eq!(price(10_000), "10.00");
        assert_eq!(price(100_000), "100.00");
        assert_eq!(price(4_389), "4.389");
        assert_eq!(price(4_390), "4.39", "厘位是 0 就退回两位");
        // 与金额函数同名不同单位：`amount(4_389)` 是 43.89 元（分）—— 别互换
        assert_eq!(amount(4_389), "43.89");
    }

    #[test]
    fn signed_price_marks_direction() {
        assert_eq!(signed_price(43), "+0.043");
        assert_eq!(signed_price(-43), "-0.043");
        assert_eq!(signed_price(0), "0.00");
        assert_eq!(signed_price(100_000), "+100.00");
    }

    #[test]
    fn lots_floor_towards_negative_infinity() {
        assert_eq!(lots_of(100), 1);
        assert_eq!(lots_of(250), 2);
        assert_eq!(lots_of(99), 0);
        assert_eq!(lots_of(0), 0);
        assert_eq!(lots_of(-50), -1, "div_euclid：负数也是向下取整");
    }

    #[test]
    fn trade_type_copy_and_direction() {
        assert_eq!(trade_type_label("open"), "建仓");
        assert_eq!(trade_type_label("add"), "加仓");
        assert_eq!(trade_type_label("reduce"), "减仓");
        assert_eq!(trade_type_label("close"), "清仓");
        assert_eq!(trade_type_label("weird"), "weird");
        assert!(is_buy("open") && is_buy("add"));
        assert!(!is_buy("reduce") && !is_buy("close"));
    }

    /// 缩放格式化：先乘 `scale`、四舍五入到 `digits` 位、去掉多余的 0。
    #[test]
    fn scaled_text_rounds_then_trims() {
        assert_eq!(scaled_text(1234.0, 1.0, 2), "1234");
        assert_eq!(scaled_text(1234.567, 1.0, 2), "1234.57");
        assert_eq!(scaled_text(300.0, 0.01, 2), "3");
        assert_eq!(scaled_text(1.0 / 3.0, 100.0, 2), "33.33");
        assert_eq!(scaled_text(0.0, 100.0, 2), "0");
    }

    #[test]
    fn short_date_drops_the_year_and_leading_zeros() {
        assert_eq!(short_date("2026-06-19"), "6-19");
        assert_eq!(short_date("2026-12-01"), "12-1");
        assert_eq!(short_date("2026-06"), "2026-06", "形状不对原样返回");
        assert_eq!(short_date(""), "");
    }

    /// 截断按**字符**计数：中文一个字算一个（不是字节），末尾补省略号。
    #[test]
    fn truncate_counts_characters_not_bytes() {
        assert_eq!(truncate("abcdef", 6), "abcdef", "正好一样长不动它");
        assert_eq!(truncate("abcdefg", 3), "abc…");
        assert_eq!(truncate("中文标题很长", 3), "中文标…");
        assert_eq!(truncate("", 0), "");
    }
}
