//! 账本 DAO。
//!
//! 行为约定：
//! * `create` 自动填充 `created_at` / `updated_at`（均为秒级 Unix 秒）
//! * `update` 只写 `name` / `description`，但同样刷新 `updated_at`
//! * 查询单条记录找不到时返回 [`rusqlite::Error::QueryReturnedNoRows`]，
//!   与服务层 [`super::is_not_found`] 配合，区分"查无记录"与真实错误

use rusqlite::{params, Connection};

use tr_domain::models::Ledger;

pub struct LedgerDao;

const COLUMNS: &str = "id, name, description, created_at, updated_at";

impl LedgerDao {
    /// 新建账本（自动填充时间戳）。
    pub fn create(conn: &Connection, ledger: &Ledger) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_ledger (id, name, description, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![ledger.id, ledger.name, ledger.description, now],
        )?;
        Ok(())
    }

    /// 修改账本名称与描述（刷新 `updated_at`）。不存在的 id 视为成功（命中 0 行不报错）。
    pub fn update(conn: &Connection, ledger: &Ledger) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_ledger SET name = ?2, description = ?3, updated_at = ?4 \
             WHERE id = ?1",
            params![
                ledger.id,
                ledger.name,
                ledger.description,
                crate::util::now_unix()
            ],
        )?;
        Ok(())
    }

    /// 全部账本（顺序由数据库决定：调用方负责按创建时间排序）。
    pub fn list_all(conn: &Connection) -> rusqlite::Result<Vec<Ledger>> {
        let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM tbl_billadm_ledger"))?;
        let rows = statement.query_map([], from_row)?;
        rows.collect()
    }

    /// 按 id 查询；不存在时返回 `QueryReturnedNoRows`。
    pub fn query_by_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Ledger> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tbl_billadm_ledger WHERE id = ?1"),
            params![ledger_id],
            from_row,
        )
    }

    /// 按名称查询；不存在时返回 `QueryReturnedNoRows`。
    pub fn query_by_name(conn: &Connection, ledger_name: &str) -> rusqlite::Result<Ledger> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tbl_billadm_ledger WHERE name = ?1"),
            params![ledger_name],
            from_row,
        )
    }

    /// 删除账本本身（级联清理由服务层在事务中完成）。
    pub fn delete_by_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_ledger WHERE id = ?1",
            params![ledger_id],
        )?;
        Ok(())
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ledger> {
    Ok(Ledger {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(id: &str, name: &str) -> Ledger {
        Ledger {
            id: id.to_string(),
            name: name.to_string(),
            description: "备注".to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn create_fills_timestamps_and_roundtrips() {
        let (workspace, dir) = crate::dao::test_workspace("ledger-dao");
        let conn = workspace.connection();

        LedgerDao::create(&conn, &ledger("l1", "默认账本")).unwrap();
        let loaded = LedgerDao::query_by_id(&conn, "l1").unwrap();
        assert_eq!(loaded.name, "默认账本");
        assert_eq!(loaded.description, "备注");
        assert!(loaded.created_at > 0, "created_at 必须被自动填充");
        assert_eq!(loaded.created_at, loaded.updated_at);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_changes_name_and_description_only() {
        let (workspace, dir) = crate::dao::test_workspace("ledger-dao");
        let conn = workspace.connection();
        LedgerDao::create(&conn, &ledger("l1", "旧名")).unwrap();
        let created = LedgerDao::query_by_id(&conn, "l1").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        LedgerDao::update(&conn, &ledger("l1", "新名")).unwrap();

        let updated = LedgerDao::query_by_id(&conn, "l1").unwrap();
        assert_eq!(updated.name, "新名");
        assert_eq!(updated.created_at, created.created_at);
        assert!(updated.updated_at >= created.updated_at);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_ledger_reports_not_found() {
        let (workspace, dir) = crate::dao::test_workspace("ledger-dao");
        let error = LedgerDao::query_by_id(&workspace.connection(), "nope").unwrap_err();
        assert!(super::super::is_not_found(&error), "error = {error:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_all_and_query_by_name() {
        let (workspace, dir) = crate::dao::test_workspace("ledger-dao");
        let conn = workspace.connection();
        LedgerDao::create(&conn, &ledger("l1", "甲")).unwrap();
        LedgerDao::create(&conn, &ledger("l2", "乙")).unwrap();

        let all = LedgerDao::list_all(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(LedgerDao::query_by_name(&conn, "乙").unwrap().id, "l2");

        LedgerDao::delete_by_id(&conn, "l1").unwrap();
        assert_eq!(LedgerDao::list_all(&conn).unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_on_missing_id_is_silent() {
        let (workspace, dir) = crate::dao::test_workspace("ledger-dao");
        LedgerDao::update(&workspace.connection(), &ledger("absent", "x")).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
