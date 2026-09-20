//! 消费模板 DAO。
//!
//! 行为约定：
//! * `create` 自动填充 `created_at` / `updated_at`（均为秒级 Unix 秒）
//! * `update_sort` 只改 `sort_order`，但同样刷新 `updated_at`，
//!   因此这里一并写 `updated_at`
//! * 列表排序固定为：`ORDER BY sort_order ASC, created_at DESC`

use rusqlite::{params, Connection};

use tr_domain::models::TransactionTemplate;

pub struct TransactionTemplateDao;

const COLUMNS: &str = "template_id, ledger_id, template_name, transaction_type, category, \
                       tags, flags, description, sort_order, created_at, updated_at";

impl TransactionTemplateDao {
    /// 新建模板（自动填充时间戳）。
    pub fn create(conn: &Connection, template: &TransactionTemplate) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_transaction_tpl \
             (template_id, ledger_id, template_name, transaction_type, category, tags, flags, \
              description, sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                template.template_id,
                template.ledger_id,
                template.template_name,
                template.transaction_type,
                template.category,
                template.tags,
                template.flags,
                template.description,
                template.sort_order,
                now
            ],
        )?;
        Ok(())
    }

    /// 按模板 ID 删除（不存在的记录视为成功）。
    pub fn delete_by_id(conn: &Connection, template_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_tpl WHERE template_id = ?1",
            params![template_id],
        )?;
        Ok(())
    }

    /// 该账本下最大的 `sort_order`（无记录时为 0）。
    pub fn get_max_sort(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i32> {
        conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM tbl_billadm_transaction_tpl \
             WHERE ledger_id = ?1",
            params![ledger_id],
            |row| row.get(0),
        )
    }

    /// 某账本的全部模板（按 `sort_order ASC, created_at DESC`）。
    pub fn query_by_ledger_id(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<TransactionTemplate>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_transaction_tpl \
             WHERE ledger_id = ?1 ORDER BY sort_order ASC, created_at DESC"
        ))?;
        let rows = statement.query_map(params![ledger_id], from_row)?;
        rows.collect()
    }

    /// 更新排序号（并刷新 `updated_at`）。
    pub fn update_sort(
        conn: &Connection,
        template_id: &str,
        sort_order: i32,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_transaction_tpl SET sort_order = ?2, updated_at = ?3 \
             WHERE template_id = ?1",
            params![template_id, sort_order, crate::util::now_unix()],
        )?;
        Ok(())
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionTemplate> {
    Ok(TransactionTemplate {
        template_id: row.get(0)?,
        ledger_id: row.get(1)?,
        template_name: row.get(2)?,
        transaction_type: row.get(3)?,
        category: row.get(4)?,
        tags: row.get(5)?,
        flags: row.get(6)?,
        description: row.get(7)?,
        sort_order: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, ledger_id: &str, name: &str, sort_order: i32) -> TransactionTemplate {
        TransactionTemplate {
            template_id: id.to_string(),
            ledger_id: ledger_id.to_string(),
            template_name: name.to_string(),
            transaction_type: "expense".to_string(),
            category: "餐饮美食".to_string(),
            tags: r#"["三餐"]"#.to_string(),
            flags: String::new(),
            description: "午餐".to_string(),
            sort_order,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn create_fills_timestamps_and_roundtrips() {
        let (workspace, dir) = crate::dao::test_workspace("template-dao");
        let conn = workspace.connection();

        TransactionTemplateDao::create(&conn, &template("t1", "l1", "模板", 1)).unwrap();
        let loaded = TransactionTemplateDao::query_by_ledger_id(&conn, "l1")
            .unwrap()
            .remove(0);
        assert_eq!(loaded.template_name, "模板");
        assert_eq!(loaded.tags, r#"["三餐"]"#);
        assert_eq!(loaded.description, "午餐");
        assert_eq!(loaded.sort_order, 1);
        assert!(loaded.created_at > 0, "created_at 必须被自动填充");
        assert_eq!(loaded.created_at, loaded.updated_at);

        // 主键冲突
        let error =
            TransactionTemplateDao::create(&conn, &template("t1", "l1", "另一个", 2)).unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn query_orders_by_sort_then_created_at_desc() {
        let (workspace, dir) = crate::dao::test_workspace("template-dao");
        let conn = workspace.connection();

        // 直接插入以控制 created_at（DAO 的 create 只写当前秒）
        let insert = "INSERT INTO tbl_billadm_transaction_tpl \
             (template_id, ledger_id, template_name, transaction_type, category, tags, flags, \
              description, sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'expense', '餐饮美食', '[]', '', '', ?4, ?5, ?5)";
        conn.execute(insert, params!["a", "l1", "甲", 1, 100])
            .unwrap();
        conn.execute(insert, params!["b", "l1", "乙", 0, 200])
            .unwrap();
        conn.execute(insert, params!["c", "l1", "丙", 0, 300])
            .unwrap();
        conn.execute(insert, params!["d", "l2", "丁", 0, 400])
            .unwrap();

        let templates = TransactionTemplateDao::query_by_ledger_id(&conn, "l1").unwrap();
        let ids: Vec<&str> = templates
            .iter()
            .map(|item| item.template_id.as_str())
            .collect();
        assert_eq!(ids, vec!["c", "b", "a"]);
        assert_eq!(
            TransactionTemplateDao::get_max_sort(&conn, "l1").unwrap(),
            1
        );
        assert_eq!(
            TransactionTemplateDao::get_max_sort(&conn, "l3").unwrap(),
            0
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_sort_refreshes_updated_at_only() {
        let (workspace, dir) = crate::dao::test_workspace("template-dao");
        let conn = workspace.connection();

        TransactionTemplateDao::create(&conn, &template("t1", "l1", "模板", 1)).unwrap();
        let created = TransactionTemplateDao::query_by_ledger_id(&conn, "l1")
            .unwrap()
            .remove(0);

        std::thread::sleep(std::time::Duration::from_millis(1100));
        TransactionTemplateDao::update_sort(&conn, "t1", 5).unwrap();

        let updated = TransactionTemplateDao::query_by_ledger_id(&conn, "l1")
            .unwrap()
            .remove(0);
        assert_eq!(updated.sort_order, 5);
        assert_eq!(updated.created_at, created.created_at);
        assert!(
            updated.updated_at > created.updated_at,
            "updated_at 必须刷新"
        );
        assert_eq!(updated.template_name, "模板");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_by_id_and_by_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("template-dao");
        let conn = workspace.connection();

        TransactionTemplateDao::create(&conn, &template("t1", "l1", "甲", 0)).unwrap();
        TransactionTemplateDao::create(&conn, &template("t2", "l1", "乙", 0)).unwrap();
        TransactionTemplateDao::create(&conn, &template("t3", "l2", "丙", 0)).unwrap();

        TransactionTemplateDao::delete_by_id(&conn, "t1").unwrap();
        assert_eq!(
            TransactionTemplateDao::query_by_ledger_id(&conn, "l1")
                .unwrap()
                .len(),
            1
        );

        // 账本级联清理走活路径（语句与 tr-service 的 LEDGER_CASCADE 一致）
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_tpl WHERE ledger_id = ?1",
            params!["l1"],
        )
        .unwrap();
        assert!(TransactionTemplateDao::query_by_ledger_id(&conn, "l1")
            .unwrap()
            .is_empty());
        assert_eq!(
            TransactionTemplateDao::query_by_ledger_id(&conn, "l2")
                .unwrap()
                .len(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
