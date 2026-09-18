//! 通用工具：UUID 生成等。对照 Go `kernel/util/{uuid,string}.go`。

/// 生成 UUID v4 字符串（与原实现 `google/uuid` 的 `NewString()` 同格式）。
/// 用于所有主键：账本、交易、模板、关键事件、图片、股票交易与轮次。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 当前 Unix 秒。等价 GORM 的 `autoCreateTime:unix` / `autoUpdateTime:unix`，
/// 服务层构造返回值时也需要它（例如日记保存后返回带时间戳的条目）。
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Unicode 字符数（不是字节数）。对照 Go `utf8.RuneCountInString`，
/// 用于日记字数与关键事件标题截断——两者都必须按字符计。
pub fn char_count(text: &str) -> i64 {
    text.chars().count() as i64
}

/// 按**字符**截断到最多 `max` 个字符（对照 Go `util.TruncateString` 的按 rune 截断语义）。
pub fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_has_canonical_shape_and_is_unique() {
        let first = new_uuid();
        let second = new_uuid();
        assert_ne!(first, second);
        assert_eq!(first.len(), 36);
        let parts: Vec<&str> = first.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[4].len(), 12);
    }

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
