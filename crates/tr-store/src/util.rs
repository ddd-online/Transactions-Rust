//! 通用工具：UUID 生成等。

/// 生成 UUID v4 字符串（标准带连字符的小写十六进制格式，长度 36）。
/// 用于所有主键：账本、交易、模板、关键事件、图片、股票交易与轮次。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 当前 Unix 秒（各表 `created_at` / `updated_at` 都写这个秒级时间戳）。
/// 服务层构造返回值时也需要它（例如日记保存后返回带时间戳的条目）。
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Unicode 字符数（不是字节数）。
/// 用于日记字数与关键事件标题截断——两者都必须按字符计。
pub fn char_count(text: &str) -> i64 {
    text.chars().count() as i64
}

/// 按**字符**截断到最多 `max` 个字符（按 Unicode 标量值截断，不会切开多字节字符）。
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

    #[test]
    fn now_unix_is_plausible_seconds() {
        // 2020-01-01 之后、2100 年之前
        let now = now_unix();
        assert!(now > 1_577_836_800, "now = {now}");
        assert!(now < 4_102_444_800, "now = {now}");
    }
}
