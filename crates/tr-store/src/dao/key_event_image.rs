//! 关键事件图片 DAO。对照 Go `kernel/dao/key_event_image_dao.go`。
//!
//! 图片按 `(ledger_id, event_date)` 归属；排序在插入时由服务层算成 `max(sort_order) + 1`。

use rusqlite::{params, Connection};

use tr_domain::models::KeyEventImage;

use super::now_unix;

pub struct KeyEventImageDao;

const COLUMNS: &str = "id, ledger_id, event_date, file_path, thumb_path, sort_order, created_at";

impl KeyEventImageDao {
    /// 插入一条图片记录（自动填充 `created_at`）。
    pub fn create(conn: &Connection, image: &KeyEventImage) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tbl_billadm_key_event_image \
             (id, ledger_id, event_date, file_path, thumb_path, sort_order, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                image.id,
                image.ledger_id,
                image.event_date,
                image.file_path,
                image.thumb_path,
                image.sort_order,
                now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 按 ID 查询；不存在返回 `QueryReturnedNoRows`。
    pub fn query_by_id(conn: &Connection, image_id: &str) -> rusqlite::Result<KeyEventImage> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tbl_billadm_key_event_image WHERE id = ?1"),
            [image_id],
            from_row,
        )
    }

    /// 某账本某天的全部图片（按 sort_order 升序，与原实现的查询顺序一致）。
    pub fn query_by_event_date(
        conn: &Connection,
        ledger_id: &str,
        date: &str,
    ) -> rusqlite::Result<Vec<KeyEventImage>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_key_event_image \
             WHERE ledger_id = ?1 AND event_date = ?2 ORDER BY sort_order ASC"
        ))?;
        let rows = statement.query_map(params![ledger_id, date], from_row)?;
        rows.collect()
    }

    /// 某账本下的全部图片（用于删除账本前收集待清理的磁盘文件）。
    pub fn query_by_ledger_id(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<KeyEventImage>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_key_event_image WHERE ledger_id = ?1"
        ))?;
        let rows = statement.query_map(params![ledger_id], from_row)?;
        rows.collect()
    }

    /// 按 ID 删除记录（不存在不报错）。
    pub fn delete_by_id(conn: &Connection, image_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_key_event_image WHERE id = ?1",
            [image_id],
        )?;
        Ok(())
    }

    /// 删除某账本某天的全部图片记录。
    pub fn delete_by_event_date(
        conn: &Connection,
        ledger_id: &str,
        date: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_key_event_image WHERE ledger_id = ?1 AND event_date = ?2",
            params![ledger_id, date],
        )?;
        Ok(())
    }

    /// 删除某账本的全部图片记录。
    pub fn delete_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_key_event_image WHERE ledger_id = ?1",
            [ledger_id],
        )?;
        Ok(())
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KeyEventImage> {
    Ok(KeyEventImage {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        event_date: row.get(2)?,
        file_path: row.get(3)?,
        thumb_path: row.get(4)?,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;

    #[test]
    fn lists_images_only_for_requested_ledger() {
        let dir = std::env::temp_dir().join(format!(
            "tr-key-event-image-dao-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = Workspace::open(&dir).unwrap();
        let conn = workspace.connection();
        for (id, ledger) in [("i1", "l1"), ("i2", "l1"), ("i3", "l2")] {
            conn.execute(
                "INSERT INTO tbl_billadm_key_event_image \
                 (id, ledger_id, event_date, file_path, thumb_path, sort_order, created_at) \
                 VALUES (?1, ?2, '2026-01-01', 'a.jpg', 'thumb_a.jpg', 0, 1)",
                params![id, ledger],
            )
            .unwrap();
        }

        let images = KeyEventImageDao::query_by_ledger_id(&conn, "l1").unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].file_path, "a.jpg");
        assert_eq!(images[0].thumb_path, "thumb_a.jpg");

        std::fs::remove_dir_all(&dir).ok();
    }
}
