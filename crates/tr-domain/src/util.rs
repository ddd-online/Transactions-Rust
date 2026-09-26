//! 纯文本小工具。
//!
//! 放这里的原因：**同一条计数规则**被两侧共用 —— 后端写库时按字符数落
//! `word_count` / 截断标题，界面按字符数显示「N 字」。两处各写一份迟早会长歪
//! （用户看到 500 字没超、落库却被截断）。

/// Unicode 字符数（不是字节数）。
pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// 按**字符**截断到最多 `max` 个字符（按 Unicode 标量值截断，不会切开多字节字符）。
pub fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_count_counts_characters_not_bytes() {
        assert_eq!(char_count("中文abc"), 5);
        assert_eq!(char_count(""), 0);
    }

    #[test]
    fn truncate_chars_never_splits_a_character() {
        assert_eq!(truncate_chars("关键事件标题", 4), "关键事件");
        assert_eq!(truncate_chars("ab", 5), "ab");
    }
}
