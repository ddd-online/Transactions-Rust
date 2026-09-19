//! 关键事件 DAO。
//!
//! **幂等 upsert 的语义细节**（容易写错）：
//! 冲突键是 `(ledger_id, date)`；冲突时只更新 `title / content / color / updated_at`，
//! **保留原有的 `id` 与 `created_at`**。因此"同一天反复保存"不会产生新 id，
//! 也不会刷新创建时间。

use rusqlite::{params, Connection};

use tr_domain::models::KeyEvent;

use super::now_unix;

pub struct KeyEventDao;

const COLUMNS: &str = "id, date, title, content, color, created_at, updated_at, ledger_id";

/// 标题按**字符**（不是字节）截断的上限：200。
pub const TITLE_MAX_CHARS: usize = 200;

impl KeyEventDao {
    /// 幂等写入：`(ledger_id, date)` 冲突时更新正文相关字段。
    pub fn upsert(conn: &Connection, event: &KeyEvent) -> rusqlite::Result<()> {
        let now = now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_key_event \
             (id, date, title, content, color, created_at, updated_at, ledger_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7) \
             ON CONFLICT(ledger_id, date) DO UPDATE SET \
                title = excluded.title, \
                content = excluded.content, \
                color = excluded.color, \
                updated_at = excluded.updated_at",
            params![
                event.id,
                event.date,
                event.title,
                event.content,
                event.color,
                now,
                event.ledger_id,
            ],
        )?;
        Ok(())
    }

    /// 按账本 + 日期查询；不存在返回 `QueryReturnedNoRows`。
    pub fn query_by_date(
        conn: &Connection,
        ledger_id: &str,
        date: &str,
    ) -> rusqlite::Result<KeyEvent> {
        conn.query_row(
            &format!(
                "SELECT {COLUMNS} FROM tbl_billadm_key_event WHERE ledger_id = ?1 AND date = ?2"
            ),
            params![ledger_id, date],
            from_row,
        )
    }

    /// 某账本某年的全部关键事件（用 `date LIKE 'YYYY-%'`）。
    pub fn query_by_year(
        conn: &Connection,
        ledger_id: &str,
        year: &str,
    ) -> rusqlite::Result<Vec<KeyEvent>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_key_event WHERE ledger_id = ?1 AND date LIKE ?2"
        ))?;
        let pattern = format!("{year}-%");
        let rows = statement.query_map(params![ledger_id, pattern], from_row)?;
        rows.collect()
    }

    /// 删除某天的关键事件（不存在不报错）。
    pub fn delete_by_date(conn: &Connection, ledger_id: &str, date: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_key_event WHERE ledger_id = ?1 AND date = ?2",
            params![ledger_id, date],
        )?;
        Ok(())
    }

    /// 删除某账本的全部关键事件。
    ///
    /// 注意：删除账本走的是服务层的单事务级联 SQL，本方法供关键事件域之外的小范围清理使用。
    pub fn delete_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_key_event WHERE ledger_id = ?1",
            [ledger_id],
        )?;
        Ok(())
    }

    /// 确保某天存在关键事件：不存在则自动创建一条空事件
    /// （标题/正文/颜色均为空串），返回是否发生了创建。
    pub fn ensure_exists(conn: &Connection, ledger_id: &str, date: &str) -> rusqlite::Result<bool> {
        match Self::query_by_date(conn, ledger_id, date) {
            Ok(_) => Ok(false),
            Err(error) if super::is_not_found(&error) => {
                Self::upsert(
                    conn,
                    &KeyEvent {
                        id: crate::util::new_uuid(),
                        date: date.to_string(),
                        ledger_id: ledger_id.to_string(),
                        ..KeyEvent::default()
                    },
                )?;
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KeyEvent> {
    Ok(KeyEvent {
        id: row.get(0)?,
        date: row.get(1)?,
        // title/content/color 在基线 schema 里可空，取不到时按空串处理
        title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        content: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        color: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        ledger_id: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;

    fn workspace() -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-dao-keyevent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

    fn event(id: &str, date: &str, title: &str) -> KeyEvent {
        KeyEvent {
            id: id.to_string(),
            date: date.to_string(),
            title: title.to_string(),
            content: "正文".to_string(),
            color: "red".to_string(),
            ledger_id: "l1".to_string(),
            ..KeyEvent::default()
        }
    }

    #[test]
    fn upsert_keeps_original_id_and_created_at() {
        let (workspace, dir) = workspace();
        let conn = workspace.connection();

        KeyEventDao::upsert(&conn, &event("e1", "2026-01-01", "标题一")).unwrap();
        let first = KeyEventDao::query_by_date(&conn, "l1", "2026-01-01").unwrap();

        // 同一天再写一次（新 id）：应更新内容但保留 id 与 created_at
        KeyEventDao::upsert(&conn, &event("e2", "2026-01-01", "标题二")).unwrap();
        let second = KeyEventDao::query_by_date(&conn, "l1", "2026-01-01").unwrap();

        assert_eq!(second.id, "e1", "冲突时应保留原 id");
        assert_eq!(second.title, "标题二");
        assert_eq!(second.created_at, first.created_at);
        assert!(second.updated_at >= first.updated_at);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_key_event", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_date_in_different_ledgers_coexists() {
        let (workspace, dir) = workspace();
        let conn = workspace.connection();

        KeyEventDao::upsert(&conn, &event("e1", "2026-01-01", "甲的标题")).unwrap();
        let mut other = event("e2", "2026-01-01", "乙的标题");
        other.ledger_id = "l2".to_string();
        KeyEventDao::upsert(&conn, &other).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_key_event", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2, "复合唯一键 (ledger_id, date) 允许不同账本同日");
        assert_eq!(
            KeyEventDao::query_by_date(&conn, "l2", "2026-01-01")
                .unwrap()
                .title,
            "乙的标题"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn query_by_year_filters_ledger_and_year() {
        let (workspace, dir) = workspace();
        let conn = workspace.connection();

        KeyEventDao::upsert(&conn, &event("e1", "2025-12-31", "去年")).unwrap();
        KeyEventDao::upsert(&conn, &event("e2", "2026-01-01", "今年一月")).unwrap();
        KeyEventDao::upsert(&conn, &event("e3", "2026-06-01", "今年六月")).unwrap();

        let events = KeyEventDao::query_by_year(&conn, "l1", "2026").unwrap();
        assert_eq!(events.len(), 2);
        // 账本隔离：另一个账本同年查不到
        assert!(KeyEventDao::query_by_year(&conn, "l2", "2026")
            .unwrap()
            .is_empty());

        KeyEventDao::delete_by_date(&conn, "l1", "2026-01-01").unwrap();
        KeyEventDao::delete_by_date(&conn, "l1", "2026-01-01").unwrap(); // 幂等
        assert_eq!(
            KeyEventDao::query_by_year(&conn, "l1", "2026")
                .unwrap()
                .len(),
            1
        );

        KeyEventDao::delete_by_ledger_id(&conn, "l1").unwrap();
        assert!(KeyEventDao::query_by_year(&conn, "l1", "2026")
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_exists_creates_empty_event_only_once() {
        let (workspace, dir) = workspace();
        let conn = workspace.connection();

        assert!(KeyEventDao::ensure_exists(&conn, "l1", "2026-02-02").unwrap());
        let created = KeyEventDao::query_by_date(&conn, "l1", "2026-02-02").unwrap();
        assert_eq!(created.title, "");
        assert_eq!(created.content, "");
        assert_eq!(created.color, "");

        assert!(!KeyEventDao::ensure_exists(&conn, "l1", "2026-02-02").unwrap());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_key_event", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
