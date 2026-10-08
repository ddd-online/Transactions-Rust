//! 金额换算：**金额一律以整数分存储**，界面层负责分/元换算。
//!
//! 这几个函数是"金额恒为整数分"纪律的守门人，行为是硬契约（含负号、`.5` 这类输入）。
//!
//! **价格是另一套单位：整数厘（1/1000 元）**。ETF / LOF / REITs 这些场内基金的交易所报价单位就是
//! 0.001 元（实测 `510300` = 4.389），按"分"存会把 `4.389` 变成 `4.39`，成交额、成本、浮盈跟着偏。
//! 所以：**金额走分、价格走厘**，两者只在算成交额时相遇（[`crate::stock::amount_of`] 折那一次）。
//!
//! 两条"元 → 分"的分工要看清，**别互相替换、也别在别处再手写一遍**：
//! * [`yuan_to_cents`]：走**字符串**（界面输入框里的金额文本）；
//! * [`price_yuan_to_milli`]：走**浮点**（wire 上的价格是 `f64` 元，落库为厘）。
//!
//! 两者在"第三位小数恰好进位"的边界上可能给出不同结果，各自都有测试钉着（而且单位本就不同：
//! 一个是分、一个是厘，不存在互相替换的余地）。

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// 金额格式非法（非数字、多个小数点等）
    InvalidFormat,
    /// 数值超出可表示范围
    Overflow,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyError::InvalidFormat => write!(f, "无效的金额格式"),
            MoneyError::Overflow => write!(f, "金额超出范围"),
        }
    }
}

impl std::error::Error for MoneyError {}

/// 分 → 元字符串，固定两位小数、**不做千分位分组**。
///
/// 用整数运算实现，避免浮点误差：
/// `-5 -> "-0.05"`、`12345 -> "123.45"`、`0 -> "0.00"`。
pub fn cents_to_yuan(cents: i64) -> String {
    let abs = cents.unsigned_abs();
    let body = format!("{}.{:02}", abs / 100, abs % 100);
    if cents < 0 {
        format!("-{body}")
    } else {
        body
    }
}

/// 元字符串 → 分：
/// 允许前导/尾随空格与负号；整数部分可省略（`.5`）；小数最多取 3 位，第 3 位四舍五入到分；
/// 非数字输入返回 [`MoneyError::InvalidFormat`]。
pub fn yuan_to_cents(input: &str) -> Result<i64, MoneyError> {
    let trimmed = input.trim();
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };

    let (int_raw, dec_raw) = match body.split_once('.') {
        Some((int_part, dec_part)) => (int_part, dec_part),
        None => (body, ""),
    };
    // 空整数部分视为 0（支持 ".5" 输入）
    let int_part = if int_raw.is_empty() { "0" } else { int_raw };

    if !is_ascii_digits(int_part) || (!dec_raw.is_empty() && !is_ascii_digits(dec_raw)) {
        return Err(MoneyError::InvalidFormat);
    }

    // 小数取前三位：前两位是分，第三位用于四舍五入
    let mut digits = [b'0'; 3];
    for (slot, byte) in digits.iter_mut().zip(dec_raw.bytes().chain([b'0', b'0'])) {
        *slot = byte;
    }
    let mut cents = i64::from(digits[0] - b'0') * 10 + i64::from(digits[1] - b'0');
    if digits[2] >= b'5' {
        cents += 1;
    }

    let integer: i64 = int_part.parse().map_err(|_| MoneyError::Overflow)?;
    let total = integer
        .checked_mul(100)
        .and_then(|v| v.checked_add(cents))
        .ok_or(MoneyError::Overflow)?;

    Ok(if negative { -total } else { total })
}

fn is_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// 厘 → 元字符串：能整除分时固定两位小数（`100000 -> "100.00"`），
/// 带厘位时三位小数（`4389 -> "4.389"`）。价格展示**照实显示存下来的精度**，不按品种分叉。
pub fn milli_to_yuan(milli: i64) -> String {
    if milli % 10 == 0 {
        return cents_to_yuan(milli / 10);
    }
    let abs = milli.unsigned_abs();
    let body = format!("{}.{:03}", abs / 1000, abs % 1000);
    if milli < 0 {
        format!("-{body}")
    } else {
        body
    }
}

/// 价格（元，浮点）→ 厘（1/1000 元），四舍五入到整数厘。
///
/// wire 上的价格是 `f64` 元（见 `tr-ipc` 的股票命令），落库要折成整数厘 ——
/// 这是**价格**的唯一入口（金额走 [`yuan_to_cents`]，别混）。
/// 场内基金的报价单位是 0.001 元，厘刚好装得下；第四位小数按四舍五入进到厘。
pub fn price_yuan_to_milli(price_yuan: f64) -> i64 {
    (price_yuan * 1000.0).round() as i64
}

/// 厘 → 价格（元，浮点）：[`price_yuan_to_milli`] 的逆，给"要把价格再送回 wire"的地方用。
pub fn milli_to_price_yuan(milli: i64) -> f64 {
    milli as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_to_yuan_formats_two_decimals_without_grouping() {
        assert_eq!(cents_to_yuan(0), "0.00");
        assert_eq!(cents_to_yuan(5), "0.05");
        assert_eq!(cents_to_yuan(12345), "123.45");
        assert_eq!(cents_to_yuan(-5), "-0.05");
        assert_eq!(cents_to_yuan(-12345), "-123.45");
        assert_eq!(cents_to_yuan(100_000_000), "1000000.00");
    }

    #[test]
    fn yuan_to_cents_handles_signs_and_abbreviated_inputs() {
        assert_eq!(yuan_to_cents("123.45").unwrap(), 12345);
        assert_eq!(yuan_to_cents(" 123 ").unwrap(), 12300);
        assert_eq!(yuan_to_cents("-0.05").unwrap(), -5);
        assert_eq!(yuan_to_cents(".5").unwrap(), 50);
        assert_eq!(yuan_to_cents("0.5").unwrap(), 50);
        assert_eq!(yuan_to_cents("1.").unwrap(), 100);
        assert_eq!(yuan_to_cents("1").unwrap(), 100);
        // 空串的整数部分视为 0，结果为 0 分而不是报错
        assert_eq!(yuan_to_cents("").unwrap(), 0);
        assert_eq!(yuan_to_cents("   ").unwrap(), 0);
    }

    #[test]
    fn yuan_to_cents_rounds_third_decimal_to_cent() {
        // 第三位 >= 5 进位；< 5 舍去
        assert_eq!(yuan_to_cents("0.004").unwrap(), 0);
        assert_eq!(yuan_to_cents("0.005").unwrap(), 1);
        assert_eq!(yuan_to_cents("0.014").unwrap(), 1);
        assert_eq!(yuan_to_cents("0.015").unwrap(), 2);
        assert_eq!(yuan_to_cents("0.999").unwrap(), 100);
        // 超过三位的小数只取前三位
        assert_eq!(yuan_to_cents("0.0049").unwrap(), 0);
        assert_eq!(yuan_to_cents("0.0051").unwrap(), 1);
    }

    #[test]
    fn yuan_to_cents_rejects_non_numeric() {
        assert_eq!(yuan_to_cents("abc"), Err(MoneyError::InvalidFormat));
        assert_eq!(yuan_to_cents("1.2.3"), Err(MoneyError::InvalidFormat));
        assert_eq!(yuan_to_cents("1a"), Err(MoneyError::InvalidFormat));
        assert_eq!(yuan_to_cents("--1"), Err(MoneyError::InvalidFormat));
    }

    #[test]
    fn price_yuan_to_milli_rounds_floats_to_milli() {
        // 股票（整分）：10.00 元 / 38.06 元
        assert_eq!(price_yuan_to_milli(10.0), 10_000);
        assert_eq!(price_yuan_to_milli(38.06), 38_060);
        assert_eq!(price_yuan_to_milli(0.0), 0);
        // 场内基金的报价单位就是 0.001 元：4.389 / 1.537 / 0.528 一位不丢
        assert_eq!(price_yuan_to_milli(4.389), 4_389);
        assert_eq!(price_yuan_to_milli(1.537), 1_537);
        assert_eq!(price_yuan_to_milli(0.528), 528);
        // 第四位小数四舍五入进到厘（价格里不该出现，口径仍要写全）
        assert_eq!(price_yuan_to_milli(4.3894), 4_389);
        assert_eq!(price_yuan_to_milli(4.3896), 4_390);
        // 负数（价格不应出现，但口径要写全）：四舍五入远离 0
        assert_eq!(price_yuan_to_milli(-12.345), -12_345);
    }

    /// 厘 → 元：能整除分时两位小数，带厘位时三位 —— 展示**照实反映存下来的精度**。
    #[test]
    fn milli_to_yuan_keeps_the_sub_cent_digit_only_when_present() {
        assert_eq!(milli_to_yuan(0), "0.00");
        assert_eq!(milli_to_yuan(10_000), "10.00");
        assert_eq!(milli_to_yuan(10_050), "10.05");
        assert_eq!(milli_to_yuan(100_000), "100.00");
        assert_eq!(milli_to_yuan(4_389), "4.389");
        assert_eq!(milli_to_yuan(1_537), "1.537");
        assert_eq!(milli_to_yuan(528), "0.528");
        assert_eq!(milli_to_yuan(-4_389), "-4.389");
        assert_eq!(milli_to_yuan(-10_000), "-10.00");
    }

    #[test]
    fn roundtrip_is_lossless_for_two_decimal_values() {
        for cents in [-99_999_i64, -1, 0, 1, 7, 100, 12_345, 987_654] {
            assert_eq!(yuan_to_cents(&cents_to_yuan(cents)).unwrap(), cents);
        }
    }
}
