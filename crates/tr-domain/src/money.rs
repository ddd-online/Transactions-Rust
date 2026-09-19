//! 金额换算：**金额一律以整数分存储**，界面层负责分/元换算。
//!
//! 这两个函数是"金额恒为整数分"纪律的守门人，行为是硬契约（含负号、`.5` 这类输入）。

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
    fn roundtrip_is_lossless_for_two_decimal_values() {
        for cents in [-99_999_i64, -1, 0, 1, 7, 100, 12_345, 987_654] {
            assert_eq!(yuan_to_cents(&cents_to_yuan(cents)).unwrap(), cents);
        }
    }
}
