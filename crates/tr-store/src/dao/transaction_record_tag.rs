//! 消费记录 ↔ 标签关联 DAO。对照 Go `kernel/dao/transaction_record_tag_dao.go`。
//!
//! 该表**没有主键**（与原 schema 一致），重复插入同一组 (ledger_id, transaction_id, tag)
//! 不会报错也不会去重——因此写入前必须先删后插，这一语义与原实现相同。

use std::collections::HashMap;

use rusqlite::{params, Connection};

use tr_domain::models::TrTag;

pub struct TrTagDao;

const COLUMNS: &str = "ledger_id, transaction_id, tag";

impl TrTagDao {
    /// 批量写入标签关联。
    pub fn create_batch(conn: &Connection, tags: &[TrTag]) -> rusqlite::Result<()> {
        if tags.is_empty() {
            return Ok(());
        }
        for tag in tags {
            conn.execute(
                "INSERT INTO tbl_billadm_transaction_record_tag (ledger_id, transaction_id, tag) \
                 VALUES (?1, ?2, ?3)",
                params![tag.ledger_id, tag.transaction_id, tag.tag],
            )?;
        }
        Ok(())
    }

    /// 删除某条记录的全部标签关联。
    pub fn delete_by_tr_id(conn: &Connection, transaction_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_record_tag WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        Ok(())
    }

    /// 删除某账本的全部标签关联。
    pub fn delete_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_record_tag WHERE ledger_id = ?1",
            [ledger_id],
        )?;
        Ok(())
    }

    /// 删除某账本下某个标签的全部关联（删除标签时调用）。
    pub fn delete_by_tag(conn: &Connection, ledger_id: &str, tag: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_record_tag WHERE ledger_id = ?1 AND tag = ?2",
            params![ledger_id, tag],
        )?;
        Ok(())
    }

    /// 批量查询若干记录的全部标签，按 transaction_id 归组。
    pub fn query_by_tr_ids(
        conn: &Connection,
        transaction_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, Vec<TrTag>>> {
        let mut result: HashMap<String, Vec<TrTag>> = HashMap::new();
        if transaction_ids.is_empty() {
            return Ok(result);
        }

        // 与 GORM 的 `IN ?` 等价：按 id 数量拼占位符（id 为内部生成，不存在注入面）
        let placeholders = vec!["?"; transaction_ids.len()].join(",");
        let sql = format!(
            "SELECT {COLUMNS} FROM tbl_billadm_transaction_record_tag \
             WHERE transaction_id IN ({placeholders})"
        );
        let mut statement = conn.prepare(&sql)?;
        let rows =
            statement.query_map(rusqlite::params_from_iter(transaction_ids.iter()), |row| {
                Ok(TrTag {
                    ledger_id: row.get(0)?,
                    transaction_id: row.get(1)?,
                    tag: row.get(2)?,
                })
            })?;
        for row in rows {
            let tag = row?;
            result
                .entry(tag.transaction_id.clone())
                .or_default()
                .push(tag);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;

    fn workspace() -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-dao-trtag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

    fn tag(tr_id: &str, name: &str) -> TrTag {
        TrTag {
            ledger_id: "l1".to_string(),
            transaction_id: tr_id.to_string(),
            tag: name.to_string(),
        }
    }

    #[test]
    fn batch_insert_query_and_delete() {
        let (workspace, dir) = workspace();
        let conn = workspace.connection();

        TrTagDao::create_batch(
            &conn,
            &[tag("t1", "三餐"), tag("t1", "外卖"), tag("t2", "三餐")],
        )
        .unwrap();

        let grouped =
            TrTagDao::query_by_tr_ids(&conn, &["t1".to_string(), "t2".to_string()]).unwrap();
        assert_eq!(grouped["t1"].len(), 2);
        assert_eq!(grouped["t2"].len(), 1);
        // 空输入直接返回空 map（原实现同样短路）
        assert!(TrTagDao::query_by_tr_ids(&conn, &[]).unwrap().is_empty());

        TrTagDao::delete_by_tr_id(&conn, "t1").unwrap();
        let grouped = TrTagDao::query_by_tr_ids(&conn, &["t1".to_string()]).unwrap();
        assert!(grouped.is_empty());

        TrTagDao::delete_by_tag(&conn, "l1", "三餐").unwrap();
        let grouped = TrTagDao::query_by_tr_ids(&conn, &["t2".to_string()]).unwrap();
        assert!(grouped.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_rows_are_allowed_like_the_original_table() {
        // 该表无主键：重复写入会产生重复行（原实现同样如此），服务层靠"先删后插"避免
        let (workspace, dir) = workspace();
        let conn = workspace.connection();
        TrTagDao::create_batch(&conn, &[tag("t1", "三餐")]).unwrap();
        TrTagDao::create_batch(&conn, &[tag("t1", "三餐")]).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tbl_billadm_transaction_record_tag WHERE transaction_id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
