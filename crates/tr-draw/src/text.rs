//! 用户输入文本 → 数值：金额（分）与费率。
//!
//! 从页面里搬来（ADR-0001 的第二波）：它们是**输入校验规则**，而规则算错的代价是
//! "保存被拦下"或"存进去一个错的数"。从前两份实现分散在消费记录页（`parse_price`，
//! 自带中文文案）与股票页（`parse_number`，费率）—— 现在各有名字、都在
//! `cargo test -p tr-draw` 的覆盖里。

/// 金额文本 → **分**（消费记录页的输入口径）。
///
/// 规则：`^(0|[1-9]\d*)(\.\d{1,2})?$`（不许前导零、最多两位小数、不许负数），
/// 通过后再交给 [`tr_domain::money::yuan_to_cents`]。
///
/// 文案是用户可见的（调用方直接把它弹进提示）：
/// * 空串 → 「请输入金额」
/// * 形状不对 → 「请输入不小于 0 的金额，最多两位小数」
/// * 形状对但换算不过（越界等）→ `MoneyError` 自己那句
pub fn parse_amount_cents(input: &str) -> Result<i64, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("请输入金额".to_string());
    }
    let mut parts = text.splitn(2, '.');
    let integer = parts.next().unwrap_or_default();
    let decimals = parts.next();

    let integer_ok = integer == "0"
        || (!integer.is_empty()
            && !integer.starts_with('0')
            && integer.bytes().all(|byte| byte.is_ascii_digit()));
    let decimals_ok = match decimals {
        None => true,
        Some(part) => {
            !part.is_empty() && part.len() <= 2 && part.bytes().all(|byte| byte.is_ascii_digit())
        }
    };
    if !integer_ok || !decimals_ok {
        return Err("请输入不小于 0 的金额，最多两位小数".to_string());
    }
    tr_domain::money::yuan_to_cents(text).map_err(|error| error.to_string())
}

/// 费率文本 → `f64`（股票费用设置：佣金率 / 印花税率 / 过户费率）。
///
/// 非数字、非有限值（`NaN` / `inf`）与空串一律 `None` —— 调用方据此提示"费率不合法"。
pub fn parse_rate(input: &str) -> Option<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

/// `YYYY` → 年；空串或非法（含 `<= 0`）一律 `None`（表示"不限"）。
///
/// 应用设置里的"自定义年份"筛选用它 —— 空 = 不限是这条规则的一半。
pub fn parse_year_bound(input: &str) -> Option<i64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i64>().ok().filter(|value| *value > 0)
}

/// `YYYY-MM` → `(年, 月)`；空串或非法一律 `(None, None)`（表示"不限"）。
///
/// 只填年份时按"按年"处理（月份留空），与界面上的分段选择一致。
pub fn parse_year_month_bound(input: &str) -> (Option<i64>, Option<i64>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    match trimmed.split_once('-') {
        Some((year, month)) => (parse_year_bound(year), parse_year_bound(month)),
        None => (parse_year_bound(trimmed), None),
    }
}

/// 第 `line` 行（0 起）**行末**在 UTF-16 里的下标 —— `setSelectionRange` 要的单位。
///
/// 复盘弹窗打开时模板刚预填进文本域，光标该落在**第一行末尾**（「买入理由：」之后）：
/// 文首会写在标签前面，文末会写在「交易心得：」之后，两者都要用户先挪一次光标。
/// `line` 越界（或文本没有那么多行）时返回文末 —— 退化成"跟在文末"，不 panic。
pub fn line_end_caret(text: &str, line: usize) -> u32 {
    let mut units = 0_usize;
    for (index, segment) in text.split('\n').enumerate() {
        if index == line {
            return (units + segment.encode_utf16().count()) as u32;
        }
        // 换行符本身也占一个 UTF-16 码元
        units += segment.encode_utf16().count() + 1;
    }
    text.encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 光标位：按**行末**算，且单位是 UTF-16 码元（代理对占 2），越界退到文末。
    #[test]
    fn line_end_caret_counts_utf16_units_and_falls_back_to_the_end() {
        assert_eq!(line_end_caret("", 0), 0);
        assert_eq!(line_end_caret("买入理由：\n卖出理由：\n交易心得：\n", 0), 5);
        assert_eq!(
            line_end_caret("买入理由：\n卖出理由：\n交易心得：\n", 1),
            11
        );
        assert_eq!(line_end_caret("abc\ndef", 1), 7, "行末含中间那个换行符");
        assert_eq!(line_end_caret("abc", 0), 3);
        assert_eq!(line_end_caret("abc\ndef", 9), 7, "行号越界 = 文末");
        assert_eq!(line_end_caret("😀\nab", 0), 2, "代理对占两个码元");
        assert_eq!(line_end_caret("😀\nab", 5), 5, "文末同样是 UTF-16 长度");
    }

    /// 筛选用的年份 / 年月输入：空 = 不限，非法也当"不限"（不会把筛选拦下来）。
    #[test]
    fn year_and_month_bounds_treat_blank_and_invalid_as_unbounded() {
        assert_eq!(parse_year_bound("2026"), Some(2026));
        assert_eq!(parse_year_bound(" 2026 "), Some(2026));
        assert_eq!(parse_year_bound(""), None);
        assert_eq!(parse_year_bound("0"), None, "0 年不是有效边界");
        assert_eq!(parse_year_bound("-3"), None);
        assert_eq!(parse_year_bound("abc"), None);

        assert_eq!(parse_year_month_bound("2026-06"), (Some(2026), Some(6)));
        assert_eq!(parse_year_month_bound(" 2026-6 "), (Some(2026), Some(6)));
        assert_eq!(
            parse_year_month_bound("2026"),
            (Some(2026), None),
            "只填年份 = 按年"
        );
        assert_eq!(parse_year_month_bound(""), (None, None));
        assert_eq!(
            parse_year_month_bound("2026-坏"),
            (Some(2026), None),
            "月份非法时只丢月份这一半"
        );
        assert_eq!(parse_year_month_bound("坏-06"), (None, Some(6)));
    }

    #[test]
    fn amount_text_becomes_cents() {
        assert_eq!(parse_amount_cents("0").unwrap(), 0);
        assert_eq!(parse_amount_cents("12").unwrap(), 1200);
        assert_eq!(parse_amount_cents("12.3").unwrap(), 1230);
        assert_eq!(parse_amount_cents("12.34").unwrap(), 1234);
        assert_eq!(parse_amount_cents(" 12.34 ").unwrap(), 1234, "两侧空白忽略");
        assert_eq!(parse_amount_cents("0.05").unwrap(), 5);
    }

    /// 空串与形状不对给的是两句**不同**的文案（调用方直接展示）。
    #[test]
    fn amount_text_rejects_with_the_two_documented_messages() {
        assert_eq!(parse_amount_cents("").unwrap_err(), "请输入金额");
        assert_eq!(parse_amount_cents("   ").unwrap_err(), "请输入金额");
        for bad in [
            "-1", "1.", ".5", "1.234", "01", "1a", "1..2", "abc", "1,000",
        ] {
            assert_eq!(
                parse_amount_cents(bad).unwrap_err(),
                "请输入不小于 0 的金额，最多两位小数",
                "输入：{bad}"
            );
        }
    }

    #[test]
    fn rate_text_needs_a_finite_number() {
        assert_eq!(parse_rate("0.0003"), Some(0.0003));
        assert_eq!(parse_rate(" 5 "), Some(5.0));
        assert_eq!(parse_rate("0"), Some(0.0));
        assert_eq!(parse_rate("-1.5"), Some(-1.5), "符号本身不在这一层拦");
        assert_eq!(parse_rate(""), None);
        assert_eq!(parse_rate("   "), None);
        assert_eq!(parse_rate("abc"), None);
        assert_eq!(parse_rate("NaN"), None);
        assert_eq!(parse_rate("inf"), None);
        assert_eq!(parse_rate("-inf"), None);
        assert_eq!(parse_rate("1e999"), None, "溢出成 inf 也算不合法");
    }
}
