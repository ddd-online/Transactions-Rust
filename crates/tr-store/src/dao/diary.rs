//! 日记 DAO（**按账本隔离**）。
//!
//! 三处必须照抄的细节：
//! 1. `list_dates` / `list_dates_by_keyword` 只 SELECT `date, word_count, mood`
//!    （只取这三列），其余字段保持零值——返回值随后被映射成 `DiaryDateItem`。
//! 2. `upsert` 的冲突键是 **`(ledger_id, date)`（账本内一天一篇）**，冲突时只更新
//!    `content / word_count / mood`，**不更新 `updated_at`**（更新列表里没有它）。
//!    同一日期在不同账本可以各存一篇，靠的是 schema 里的复合唯一索引。
//! 3. 关键词过滤用 `instr(content, ?) > 0`（区分大小写、不把 `%_` 当通配符）。
//!
//! 所有查询都必须带 `ledger_id` 条件：日记是账本私有数据，跨账本不可见。

use rusqlite::{params, Connection};

use tr_domain::models::DiaryEntry;

pub struct DiaryDao;

const COLUMNS: &str = "id, date, content, word_count, mood, created_at, updated_at, ledger_id";
const DATE_COLUMNS: &str = "date, word_count, mood";

impl DiaryDao {
    /// 某账本的日期列表（倒序），只带日期/字数/心情。
    pub fn list_dates(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {DATE_COLUMNS} FROM tbl_billadm_diary_entry \
             WHERE ledger_id = ?1 ORDER BY date DESC"
        ))?;
        let rows = statement.query_map([ledger_id], from_date_row)?;
        rows.collect()
    }

    /// 某账本下正文包含关键词的日期列表（倒序）。
    pub fn list_dates_by_keyword(
        conn: &Connection,
        ledger_id: &str,
        keyword: &str,
    ) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {DATE_COLUMNS} FROM tbl_billadm_diary_entry \
             WHERE ledger_id = ?1 AND instr(content, ?2) > 0 ORDER BY date DESC"
        ))?;
        let rows = statement.query_map(params![ledger_id, keyword], from_date_row)?;
        rows.collect()
    }

    /// 某账本的全部日记（正序，导出用）。
    pub fn list_by_ledger(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_diary_entry WHERE ledger_id = ?1 ORDER BY date ASC"
        ))?;
        let rows = statement.query_map([ledger_id], from_row)?;
        rows.collect()
    }

    /// 按账本 + 日期查询；不存在返回 `QueryReturnedNoRows`。
    pub fn query_by_date(
        conn: &Connection,
        ledger_id: &str,
        date: &str,
    ) -> rusqlite::Result<DiaryEntry> {
        conn.query_row(
            &format!(
                "SELECT {COLUMNS} FROM tbl_billadm_diary_entry WHERE ledger_id = ?1 AND date = ?2"
            ),
            params![ledger_id, date],
            from_row,
        )
    }

    /// 幂等写入（同账本同日期冲突则更新正文/字数/心情）。
    pub fn upsert(conn: &Connection, entry: &DiaryEntry) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_diary_entry \
             (id, date, content, word_count, mood, created_at, updated_at, ledger_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7) \
             ON CONFLICT(ledger_id, date) DO UPDATE SET \
                content = excluded.content, \
                word_count = excluded.word_count, \
                mood = excluded.mood",
            params![
                entry.id,
                entry.date,
                entry.content,
                entry.word_count,
                entry.mood,
                now,
                entry.ledger_id,
            ],
        )?;
        Ok(())
    }

    /// 按账本 + 日期删除（不存在不报错；别的账本的同日期不受影响）。
    pub fn delete_by_date(conn: &Connection, ledger_id: &str, date: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_diary_entry WHERE ledger_id = ?1 AND date = ?2",
            params![ledger_id, date],
        )?;
        Ok(())
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DiaryEntry> {
    Ok(DiaryEntry {
        id: row.get(0)?,
        date: row.get(1)?,
        content: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        word_count: row.get(3)?,
        mood: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        ledger_id: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
    })
}

/// 日期列表专用映射：只填 date / word_count / mood（与 `DATE_COLUMNS` 的 SELECT 列表一致）。
fn from_date_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DiaryEntry> {
    Ok(DiaryEntry {
        date: row.get(0)?,
        word_count: row.get(1)?,
        mood: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        ..DiaryEntry::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, ledger_id: &str, date: &str, content: &str, mood: &str) -> DiaryEntry {
        DiaryEntry {
            id: id.to_string(),
            date: date.to_string(),
            content: content.to_string(),
            word_count: content.chars().count() as i64,
            mood: mood.to_string(),
            created_at: 0,
            updated_at: 0,
            ledger_id: ledger_id.to_string(),
        }
    }

    #[test]
    fn upsert_by_date_keeps_id_and_updated_at() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-upsert");
        let conn = workspace.connection();

        DiaryDao::upsert(&conn, &entry("d1", "l1", "2026-01-02", "第一天", "开心")).unwrap();
        let first = DiaryDao::query_by_date(&conn, "l1", "2026-01-02").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        DiaryDao::upsert(&conn, &entry("d2", "l1", "2026-01-02", "第一天改", "平静")).unwrap();
        let second = DiaryDao::query_by_date(&conn, "l1", "2026-01-02").unwrap();

        assert_eq!(second.id, "d1", "按日期冲突时保留原 id");
        assert_eq!(second.content, "第一天改");
        assert_eq!(second.mood, "平静");
        assert_eq!(second.ledger_id, "l1");
        assert_eq!(second.created_at, first.created_at);
        // 冲突时更新的列不含 updated_at：更新时间保持首次写入的值
        assert_eq!(second.updated_at, first.updated_at);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_diary_entry", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_dates_is_desc_and_keyword_uses_instr() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-list");
        let conn = workspace.connection();
        DiaryDao::upsert(&conn, &entry("d1", "l1", "2026-01-01", "今天读书", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d2", "l1", "2025-12-31", "今天跑步 100%", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d3", "l1", "2026-02-01", "写代码", "充实")).unwrap();
        // 别的账本：完全不可见
        DiaryDao::upsert(&conn, &entry("d4", "l2", "2026-03-01", "今天读书", "开心")).unwrap();

        let dates = DiaryDao::list_dates(&conn, "l1").unwrap();
        let order: Vec<&str> = dates.iter().map(|item| item.date.as_str()).collect();
        assert_eq!(order, vec!["2026-02-01", "2026-01-01", "2025-12-31"]);
        // 只选三列：其余字段保持零值
        assert_eq!(dates[0].content, "");
        assert_eq!(dates[2].mood, "");
        assert_eq!(dates[0].mood, "充实");

        assert_eq!(
            DiaryDao::list_dates_by_keyword(&conn, "l1", "今天")
                .unwrap()
                .len(),
            2
        );
        // `%` 不当通配符（LIKE 会误命中全部）
        assert_eq!(
            DiaryDao::list_dates_by_keyword(&conn, "l1", "%")
                .unwrap()
                .len(),
            1
        );

        let all = DiaryDao::list_by_ledger(&conn, "l1").unwrap();
        let order: Vec<&str> = all.iter().map(|item| item.date.as_str()).collect();
        assert_eq!(order, vec!["2025-12-31", "2026-01-01", "2026-02-01"]);
        assert!(all.iter().all(|item| item.ledger_id == "l1"));

        // 另一个账本只看得见自己那篇
        let other = DiaryDao::list_by_ledger(&conn, "l2").unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].date, "2026-03-01");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 复合唯一键 (ledger_id, date)：不同账本同日互不覆盖。
    #[test]
    fn same_date_in_two_ledgers_creates_two_rows() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-ledger");
        let conn = workspace.connection();

        DiaryDao::upsert(&conn, &entry("d1", "l1", "2026-01-01", "账本一的正文", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d2", "l2", "2026-01-01", "账本二的正文", "")).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_diary_entry", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2, "不同账本的同一天必须各存一篇");

        assert_eq!(
            DiaryDao::query_by_date(&conn, "l1", "2026-01-01")
                .unwrap()
                .content,
            "账本一的正文"
        );
        assert_eq!(
            DiaryDao::query_by_date(&conn, "l2", "2026-01-01")
                .unwrap()
                .content,
            "账本二的正文"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_and_missing_lookup_are_scoped_by_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-delete");
        let conn = workspace.connection();
        DiaryDao::upsert(&conn, &entry("d1", "l1", "2026-01-01", "内容", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d2", "l2", "2026-01-01", "别人的内容", "")).unwrap();

        // 删别的账本的同日期：no-op
        DiaryDao::delete_by_date(&conn, "l3", "2026-01-01").unwrap();
        assert_eq!(DiaryDao::list_by_ledger(&conn, "l2").unwrap().len(), 1);

        DiaryDao::delete_by_date(&conn, "l1", "2026-01-01").unwrap();
        DiaryDao::delete_by_date(&conn, "l1", "2026-01-01").unwrap(); // 幂等

        assert!(super::super::is_not_found(
            &DiaryDao::query_by_date(&conn, "l1", "2026-01-01").unwrap_err()
        ));
        assert_eq!(DiaryDao::list_by_ledger(&conn, "l2").unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
