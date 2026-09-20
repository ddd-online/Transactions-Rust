//! 共用辅助："查无记录"错误判定、按名分组的批量计数、测试工作空间。
//!
//! 各业务 Dao 随 P2–P4 阶段逐个补齐（账本、交易、分类、标签、图表、模板、
//! 关键事件、日记、股票）；本模块只放跨 Dao 的公共工具。

use std::collections::BTreeMap;

use rusqlite::Connection;

pub mod category;
pub mod chart;
pub mod diary;
pub mod key_event;
pub mod key_event_image;
pub mod ledger;
pub mod stock;
pub mod tag;
pub mod transaction_record;
pub mod transaction_record_tag;
pub mod transaction_template;

/// 判定"查无记录"（rusqlite 的 `QueryReturnedNoRows`）。服务层用它在
/// "不存在则创建"这类逻辑里区分"查无记录"与真实错误。
pub fn is_not_found(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::QueryReturnedNoRows)
}

/// 按 `column`（分类名 / 标签名）分组统计交易记录数：`SELECT column, COUNT(*) …
/// WHERE ledger_id = ? AND column IN (…) GROUP BY column`。
///
/// `column` / `table` **必须由调用方以常量传入**（分类与标签两处口径逐字一致，
/// 所以这里做一份共用实现）；用户输入一律走 `?` 占位符。
///
/// 返回的 map 只包含**确实有记录**的名字（调用方取不到时得 0）；
/// `names` 为空时直接返回空 map，不查库。
pub(crate) fn count_grouped_by(
    conn: &Connection,
    ledger_id: &str,
    names: &[String],
    column: &str,
    table: &str,
) -> rusqlite::Result<BTreeMap<String, i64>> {
    let mut counts = BTreeMap::new();
    if names.is_empty() {
        return Ok(counts);
    }

    let placeholders = vec!["?"; names.len()].join(", ");
    let sql = format!(
        "SELECT {column}, COUNT(*) FROM {table} \
         WHERE ledger_id = ? AND {column} IN ({placeholders}) GROUP BY {column}"
    );

    let mut args = vec![rusqlite::types::Value::Text(ledger_id.to_string())];
    args.extend(
        names
            .iter()
            .map(|name| rusqlite::types::Value::Text(name.clone())),
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(args), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (name, count) = row?;
        counts.insert(name, count);
    }
    Ok(counts)
}

/// 测试用临时工作空间：临时目录 + `Workspace::open`，`tag` 只用于区分目录前缀。
#[cfg(test)]
pub(crate) fn test_workspace(tag: &str) -> (crate::Workspace, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "tr-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    (crate::Workspace::open(&dir).unwrap(), dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_not_found_only_matches_missing_rows() {
        assert!(is_not_found(&rusqlite::Error::QueryReturnedNoRows));
        assert!(!is_not_found(&rusqlite::Error::InvalidQuery));
    }
}
