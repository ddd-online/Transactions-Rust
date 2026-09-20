//! 分类 DAO。
//!
//! 行为约定（改 SQL 前先读一遍）：
//! * `Category` 模型没有时间戳列，所以 `create` / `update_sort`
//!   不写 `created_at` / `updated_at`
//! * 列表排序固定为：`ORDER BY sort_order ASC, name DESC`
//! * 唯一键是 `(ledger_id, name, transaction_type)`（见基线 schema 的
//!   `idx_category_ledger_name_type`），重复插入由 SQLite 报错，删除则是幂等的
//! * `count_records_by_category` / `count_records_by_categories` 是分类维度的两处统计
//!   （单个分类的 `COUNT(*)` 与按 `GROUP BY category` 的批量版）。消费记录 DAO 由另一处
//!   负责，因此这两条只读 SQL 放在本文件，两处口径必须保持逐字一致。

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use tr_domain::models::Category;

pub struct CategoryDao;

const COLUMNS: &str = "ledger_id, name, transaction_type, sort_order";

impl CategoryDao {
    /// 按账本查询分类；`transaction_type` 为空或 `all` 时不过滤。
    pub fn query_by_ledger(
        conn: &Connection,
        ledger_id: &str,
        transaction_type: &str,
    ) -> rusqlite::Result<Vec<Category>> {
        let mut sql = format!("SELECT {COLUMNS} FROM tbl_billadm_category WHERE ledger_id = ?1");
        let mut args = vec![rusqlite::types::Value::Text(ledger_id.to_string())];
        if !transaction_type.is_empty() && transaction_type != "all" {
            sql.push_str(" AND transaction_type = ?2");
            args.push(rusqlite::types::Value::Text(transaction_type.to_string()));
        }
        sql.push_str(" ORDER BY sort_order ASC, name DESC");

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(args), from_row)?;
        rows.collect()
    }

    /// 该账本 + 交易类型下最大的 `sort_order`（无记录时为 0，SQL 里用
    /// `COALESCE(MAX(sort_order), 0)`）。
    pub fn get_max_sort(
        conn: &Connection,
        ledger_id: &str,
        transaction_type: &str,
    ) -> rusqlite::Result<i32> {
        conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM tbl_billadm_category \
             WHERE ledger_id = ?1 AND transaction_type = ?2",
            params![ledger_id, transaction_type],
            |row| row.get(0),
        )
    }

    /// 新建分类。
    pub fn create(conn: &Connection, category: &Category) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tbl_billadm_category (ledger_id, name, transaction_type, sort_order) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                category.ledger_id,
                category.name,
                category.transaction_type,
                category.sort_order
            ],
        )?;
        Ok(())
    }

    /// 删除单个分类（不存在的记录视为成功，删除是幂等的）。
    pub fn delete(
        conn: &Connection,
        ledger_id: &str,
        name: &str,
        transaction_type: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_category \
             WHERE ledger_id = ?1 AND name = ?2 AND transaction_type = ?3",
            params![ledger_id, name, transaction_type],
        )?;
        Ok(())
    }

    /// 更新排序号。
    pub fn update_sort(
        conn: &Connection,
        ledger_id: &str,
        name: &str,
        transaction_type: &str,
        sort_order: i32,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_category SET sort_order = ?4 \
             WHERE ledger_id = ?1 AND name = ?2 AND transaction_type = ?3",
            params![ledger_id, name, transaction_type, sort_order],
        )?;
        Ok(())
    }

    /// 某账本的分类总数（用于判断是否已初始化）。
    pub fn count_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_category WHERE ledger_id = ?1",
            params![ledger_id],
            |row| row.get(0),
        )
    }

    /// 单个分类名下的交易记录数。
    pub fn count_records_by_category(
        conn: &Connection,
        ledger_id: &str,
        category: &str,
    ) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_transaction_record \
             WHERE ledger_id = ?1 AND category = ?2",
            params![ledger_id, category],
            |row| row.get(0),
        )
    }

    /// 批量统计每个分类名下的交易记录数（`SELECT category, COUNT(*) … WHERE
    /// ledger_id = ? AND category IN (…) GROUP BY category`）。
    ///
    /// SQL 与 [`super::count_grouped_by`] 共用（与标签侧 `count_records_by_tags`
    /// 逐字一致），表名/列名以常量传入。返回的 map 只包含**确实有记录**的分类名
    /// （调用方用 `counts[name]` 取不到时得 0）；`names` 为空时直接返回空 map，不查库。
    pub fn count_records_by_categories(
        conn: &Connection,
        ledger_id: &str,
        names: &[String],
    ) -> rusqlite::Result<BTreeMap<String, i64>> {
        super::count_grouped_by(
            conn,
            ledger_id,
            names,
            "category",
            "tbl_billadm_transaction_record",
        )
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        ledger_id: row.get(0)?,
        name: row.get(1)?,
        transaction_type: row.get(2)?,
        sort_order: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(ledger_id: &str, name: &str, transaction_type: &str, sort_order: i32) -> Category {
        Category {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            transaction_type: transaction_type.to_string(),
            sort_order,
        }
    }

    fn record(conn: &Connection, ledger_id: &str, id: &str, category_name: &str) {
        conn.execute(
            "INSERT INTO tbl_billadm_transaction_record \
             (transaction_id, ledger_id, price, transaction_type, category, transaction_at, created_at, updated_at) \
             VALUES (?1, ?2, 100, 'expense', ?3, 1, 1, 1)",
            params![id, ledger_id, category_name],
        )
        .unwrap();
    }

    #[test]
    fn query_orders_by_sort_then_name_desc_and_filters_type() {
        let (workspace, dir) = crate::dao::test_workspace("category-dao");
        let conn = workspace.connection();

        CategoryDao::create(&conn, &category("l1", "甲", "expense", 1)).unwrap();
        CategoryDao::create(&conn, &category("l1", "乙", "expense", 0)).unwrap();
        CategoryDao::create(&conn, &category("l1", "丙", "income", 0)).unwrap();
        CategoryDao::create(&conn, &category("l2", "其它账本", "expense", 0)).unwrap();

        let all = CategoryDao::query_by_ledger(&conn, "l1", "all").unwrap();
        let names: Vec<&str> = all.iter().map(|item| item.name.as_str()).collect();
        // sort_order 升序；同序号按 name DESC（SQLite 的二进制比较：乙(4E59) > 丙(4E19)）
        assert_eq!(names, vec!["乙", "丙", "甲"]);

        let expense = CategoryDao::query_by_ledger(&conn, "l1", "expense").unwrap();
        let names: Vec<&str> = expense.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["乙", "甲"]);

        // 空字符串与 "all" 等价：不过滤
        assert_eq!(
            CategoryDao::query_by_ledger(&conn, "l1", "").unwrap().len(),
            3
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn max_sort_and_update_sort() {
        let (workspace, dir) = crate::dao::test_workspace("category-dao");
        let conn = workspace.connection();

        assert_eq!(
            CategoryDao::get_max_sort(&conn, "l1", "expense").unwrap(),
            0
        );

        CategoryDao::create(&conn, &category("l1", "甲", "expense", 3)).unwrap();
        CategoryDao::create(&conn, &category("l1", "乙", "income", 7)).unwrap();
        assert_eq!(
            CategoryDao::get_max_sort(&conn, "l1", "expense").unwrap(),
            3
        );
        assert_eq!(CategoryDao::get_max_sort(&conn, "l1", "income").unwrap(), 7);
        assert_eq!(
            CategoryDao::get_max_sort(&conn, "l1", "transfer").unwrap(),
            0
        );

        CategoryDao::update_sort(&conn, "l1", "甲", "expense", 9).unwrap();
        let loaded = CategoryDao::query_by_ledger(&conn, "l1", "expense").unwrap();
        assert_eq!(loaded[0].sort_order, 9);

        // 不存在的记录更新视为成功（命中 0 行不报错）
        CategoryDao::update_sort(&conn, "l1", "不存在", "expense", 1).unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_category_violates_unique_index() {
        let (workspace, dir) = crate::dao::test_workspace("category-dao");
        let conn = workspace.connection();

        CategoryDao::create(&conn, &category("l1", "餐饮美食", "expense", 0)).unwrap();
        let error =
            CategoryDao::create(&conn, &category("l1", "餐饮美食", "expense", 1)).unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        // 交易类型不同则是另一条记录（唯一键含 transaction_type）
        CategoryDao::create(&conn, &category("l1", "餐饮美食", "income", 0)).unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_counts_are_grouped_by_name() {
        let (workspace, dir) = crate::dao::test_workspace("category-dao");
        let conn = workspace.connection();

        record(&conn, "l1", "t1", "餐饮美食");
        record(&conn, "l1", "t2", "餐饮美食");
        record(&conn, "l1", "t3", "购物消费");
        record(&conn, "l2", "t4", "餐饮美食");

        assert_eq!(
            CategoryDao::count_records_by_category(&conn, "l1", "餐饮美食").unwrap(),
            2
        );

        let names = vec![
            "餐饮美食".to_string(),
            "购物消费".to_string(),
            "无记录".to_string(),
        ];
        let counts = CategoryDao::count_records_by_categories(&conn, "l1", &names).unwrap();
        assert_eq!(counts.len(), 2, "只返回有记录的分类名");
        assert_eq!(counts["餐饮美食"], 2);
        assert_eq!(counts["购物消费"], 1);
        assert_eq!(counts.get("无记录"), None);

        // 空名单不查库（提前返回）
        assert!(CategoryDao::count_records_by_categories(&conn, "l1", &[])
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_and_count_by_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("category-dao");
        let conn = workspace.connection();

        CategoryDao::create(&conn, &category("l1", "甲", "expense", 0)).unwrap();
        CategoryDao::create(&conn, &category("l1", "乙", "income", 0)).unwrap();
        CategoryDao::create(&conn, &category("l2", "丙", "expense", 0)).unwrap();

        assert_eq!(CategoryDao::count_by_ledger_id(&conn, "l1").unwrap(), 2);

        CategoryDao::delete(&conn, "l1", "甲", "expense").unwrap();
        assert_eq!(CategoryDao::count_by_ledger_id(&conn, "l1").unwrap(), 1);
        // 交易类型不匹配时不会误删
        CategoryDao::delete(&conn, "l1", "乙", "expense").unwrap();
        assert_eq!(CategoryDao::count_by_ledger_id(&conn, "l1").unwrap(), 1);

        // 账本级联清理走活路径（语句与 tr-service 的 LEDGER_CASCADE 一致）
        conn.execute(
            "DELETE FROM tbl_billadm_category WHERE ledger_id = ?1",
            params!["l1"],
        )
        .unwrap();
        assert_eq!(CategoryDao::count_by_ledger_id(&conn, "l1").unwrap(), 0);
        assert_eq!(CategoryDao::count_by_ledger_id(&conn, "l2").unwrap(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
