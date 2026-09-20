//! 日记 DAO。
//!
//! 三处必须照抄的细节：
//! 1. `list_dates` / `list_dates_by_keyword` 只 SELECT `date, word_count, mood`
//!    （只取这三列），其余字段保持零值——返回值随后被映射成 `DiaryDateItem`。
//! 2. `upsert` 的冲突键是 **`date`（全工作空间唯一）**，冲突时只更新
//!    `content / word_count / mood`，**不更新 `updated_at`**（更新列表里没有它）。
//! 3. 关键词过滤用 `instr(content, ?) > 0`（区分大小写、不把 `%_` 当通配符）。

use rusqlite::{params, Connection};

use tr_domain::models::DiaryEntry;

pub struct DiaryDao;

const COLUMNS: &str = "id, date, content, word_count, mood, created_at, updated_at";
const DATE_COLUMNS: &str = "date, word_count, mood";

impl DiaryDao {
    /// 日期列表（倒序），只带日期/字数/心情。
    pub fn list_dates(conn: &Connection) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {DATE_COLUMNS} FROM tbl_billadm_diary_entry ORDER BY date DESC"
        ))?;
        let rows = statement.query_map([], from_date_row)?;
        rows.collect()
    }

    /// 正文包含关键词的日期列表（倒序）。
    pub fn list_dates_by_keyword(
        conn: &Connection,
        keyword: &str,
    ) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {DATE_COLUMNS} FROM tbl_billadm_diary_entry \
             WHERE instr(content, ?1) > 0 ORDER BY date DESC"
        ))?;
        let rows = statement.query_map([keyword], from_date_row)?;
        rows.collect()
    }

    /// 全部日记（正序，导出用）。
    pub fn list_all(conn: &Connection) -> rusqlite::Result<Vec<DiaryEntry>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_diary_entry ORDER BY date ASC"
        ))?;
        let rows = statement.query_map([], from_row)?;
        rows.collect()
    }

    /// 按日期查询；不存在返回 `QueryReturnedNoRows`。
    pub fn query_by_date(conn: &Connection, date: &str) -> rusqlite::Result<DiaryEntry> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tbl_billadm_diary_entry WHERE date = ?1"),
            [date],
            from_row,
        )
    }

    /// 幂等写入（按日期冲突则更新正文/字数/心情）。
    pub fn upsert(conn: &Connection, entry: &DiaryEntry) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_diary_entry \
             (id, date, content, word_count, mood, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
             ON CONFLICT(date) DO UPDATE SET \
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
            ],
        )?;
        Ok(())
    }

    /// 按日期删除（不存在不报错）。
    pub fn delete_by_date(conn: &Connection, date: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_diary_entry WHERE date = ?1",
            [date],
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

    fn entry(id: &str, date: &str, content: &str, mood: &str) -> DiaryEntry {
        DiaryEntry {
            id: id.to_string(),
            date: date.to_string(),
            content: content.to_string(),
            word_count: content.chars().count() as i64,
            mood: mood.to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn upsert_by_date_keeps_id_and_updated_at() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-upsert");
        let conn = workspace.connection();

        DiaryDao::upsert(&conn, &entry("d1", "2026-01-02", "第一天", "开心")).unwrap();
        let first = DiaryDao::query_by_date(&conn, "2026-01-02").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        DiaryDao::upsert(&conn, &entry("d2", "2026-01-02", "第一天改", "平静")).unwrap();
        let second = DiaryDao::query_by_date(&conn, "2026-01-02").unwrap();

        assert_eq!(second.id, "d1", "按日期冲突时保留原 id");
        assert_eq!(second.content, "第一天改");
        assert_eq!(second.mood, "平静");
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
        DiaryDao::upsert(&conn, &entry("d1", "2026-01-01", "今天读书", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d2", "2025-12-31", "今天跑步 100%", "")).unwrap();
        DiaryDao::upsert(&conn, &entry("d3", "2026-02-01", "写代码", "充实")).unwrap();

        let dates = DiaryDao::list_dates(&conn).unwrap();
        let order: Vec<&str> = dates.iter().map(|item| item.date.as_str()).collect();
        assert_eq!(order, vec!["2026-02-01", "2026-01-01", "2025-12-31"]);
        // 只选三列：其余字段保持零值
        assert_eq!(dates[0].content, "");
        assert_eq!(dates[2].mood, "");
        assert_eq!(dates[0].mood, "充实");

        assert_eq!(
            DiaryDao::list_dates_by_keyword(&conn, "今天")
                .unwrap()
                .len(),
            2
        );
        // `%` 不当通配符（LIKE 会误命中全部）
        assert_eq!(
            DiaryDao::list_dates_by_keyword(&conn, "%").unwrap().len(),
            1
        );

        let all = DiaryDao::list_all(&conn).unwrap();
        let order: Vec<&str> = all.iter().map(|item| item.date.as_str()).collect();
        assert_eq!(order, vec!["2025-12-31", "2026-01-01", "2026-02-01"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_and_missing_lookup() {
        let (workspace, dir) = crate::dao::test_workspace("dao-diary-delete");
        let conn = workspace.connection();
        DiaryDao::upsert(&conn, &entry("d1", "2026-01-01", "内容", "")).unwrap();

        DiaryDao::delete_by_date(&conn, "2026-01-01").unwrap();
        DiaryDao::delete_by_date(&conn, "2026-01-01").unwrap(); // 幂等

        assert!(super::super::is_not_found(
            &DiaryDao::query_by_date(&conn, "2026-01-01").unwrap_err()
        ));

        std::fs::remove_dir_all(&dir).ok();
    }
}
