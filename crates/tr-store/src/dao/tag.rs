//! 标签 DAO。
//!
//! 行为约定：
//! * `Tag` 模型没有时间戳列，`create` / `update_sort` 不写时间
//! * 列表排序固定为：`ORDER BY sort_order ASC, name DESC`
//! * 唯一键是 `(ledger_id, name, category_transaction_type)`，重复插入由 SQLite 报错
//! * `count_by_tag` 统计的是**交易记录标签关联表**（`tbl_billadm_transaction_record_tag`），
//!   不是标签表本身

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use tr_domain::models::Tag;

pub struct TagDao;

const COLUMNS: &str = "ledger_id, name, category_transaction_type, sort_order";

impl TagDao {
    /// 按账本查询标签；`category_transaction_type` 为空或 `all` 时不过滤。
    pub fn query_by_ledger(
        conn: &Connection,
        ledger_id: &str,
        category_transaction_type: &str,
    ) -> rusqlite::Result<Vec<Tag>> {
        let mut sql = format!("SELECT {COLUMNS} FROM tbl_billadm_tag WHERE ledger_id = ?1");
        let mut args = vec![rusqlite::types::Value::Text(ledger_id.to_string())];
        if !category_transaction_type.is_empty() && category_transaction_type != "all" {
            sql.push_str(" AND category_transaction_type = ?2");
            args.push(rusqlite::types::Value::Text(
                category_transaction_type.to_string(),
            ));
        }
        sql.push_str(" ORDER BY sort_order ASC, name DESC");

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(args), from_row)?;
        rows.collect()
    }

    /// 该账本 + `分类:交易类型` 下最大的 `sort_order`（无记录时为 0）。
    pub fn get_max_sort(
        conn: &Connection,
        ledger_id: &str,
        category_transaction_type: &str,
    ) -> rusqlite::Result<i32> {
        conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM tbl_billadm_tag \
             WHERE ledger_id = ?1 AND category_transaction_type = ?2",
            params![ledger_id, category_transaction_type],
            |row| row.get(0),
        )
    }

    /// 新建标签。
    pub fn create(conn: &Connection, tag: &Tag) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tbl_billadm_tag \
             (ledger_id, name, category_transaction_type, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![
                tag.ledger_id,
                tag.name,
                tag.category_transaction_type,
                tag.sort_order
            ],
        )?;
        Ok(())
    }

    /// 删除单个标签（不存在的记录视为成功）。
    pub fn delete(
        conn: &Connection,
        ledger_id: &str,
        name: &str,
        category_transaction_type: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_tag \
             WHERE ledger_id = ?1 AND name = ?2 AND category_transaction_type = ?3",
            params![ledger_id, name, category_transaction_type],
        )?;
        Ok(())
    }

    /// 删除某分类（`分类名:交易类型`）下的全部标签。
    pub fn delete_by_category(
        conn: &Connection,
        ledger_id: &str,
        category_transaction_type: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_tag \
             WHERE ledger_id = ?1 AND category_transaction_type = ?2",
            params![ledger_id, category_transaction_type],
        )?;
        Ok(())
    }

    /// 更新排序号。
    pub fn update_sort(
        conn: &Connection,
        ledger_id: &str,
        name: &str,
        category_transaction_type: &str,
        sort_order: i32,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_tag SET sort_order = ?4 \
             WHERE ledger_id = ?1 AND name = ?2 AND category_transaction_type = ?3",
            params![ledger_id, name, category_transaction_type, sort_order],
        )?;
        Ok(())
    }

    /// 该标签名下的交易记录数（查 `tbl_billadm_transaction_record_tag`）。
    pub fn count_by_tag(conn: &Connection, ledger_id: &str, tag: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_transaction_record_tag \
             WHERE ledger_id = ?1 AND tag = ?2",
            params![ledger_id, tag],
            |row| row.get(0),
        )
    }

    /// 批量统计每个标签名下的关联交易数（`SELECT tag, COUNT(*) … WHERE
    /// ledger_id = ? AND tag IN (…) GROUP BY tag`）。
    /// SQL 与 [`super::count_grouped_by`] 共用（与分类侧
    /// `count_records_by_categories` 逐字一致），表名/列名以常量传入。
    /// `names` 为空时直接返回空 map，不查库。
    pub fn count_records_by_tags(
        conn: &Connection,
        ledger_id: &str,
        names: &[String],
    ) -> rusqlite::Result<BTreeMap<String, i64>> {
        super::count_grouped_by(
            conn,
            ledger_id,
            names,
            "tag",
            "tbl_billadm_transaction_record_tag",
        )
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        ledger_id: row.get(0)?,
        name: row.get(1)?,
        category_transaction_type: row.get(2)?,
        sort_order: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(ledger_id: &str, name: &str, category_transaction_type: &str, sort_order: i32) -> Tag {
        Tag {
            ledger_id: ledger_id.to_string(),
            name: name.to_string(),
            category_transaction_type: category_transaction_type.to_string(),
            sort_order,
        }
    }

    fn record_tag(conn: &Connection, ledger_id: &str, transaction_id: &str, name: &str) {
        conn.execute(
            "INSERT INTO tbl_billadm_transaction_record_tag (ledger_id, transaction_id, tag) \
             VALUES (?1, ?2, ?3)",
            params![ledger_id, transaction_id, name],
        )
        .unwrap();
    }

    #[test]
    fn query_orders_by_sort_then_name_desc_and_filters_category() {
        let (workspace, dir) = crate::dao::test_workspace("tag-dao");
        let conn = workspace.connection();

        TagDao::create(&conn, &tag("l1", "三餐", "餐饮美食:expense", 1)).unwrap();
        TagDao::create(&conn, &tag("l1", "外卖", "餐饮美食:expense", 0)).unwrap();
        TagDao::create(&conn, &tag("l1", "工资", "工资奖金:income", 0)).unwrap();
        TagDao::create(&conn, &tag("l2", "三餐", "餐饮美食:expense", 0)).unwrap();

        let all = TagDao::query_by_ledger(&conn, "l1", "all").unwrap();
        let names: Vec<&str> = all.iter().map(|item| item.name.as_str()).collect();
        // sort_order 升序；同序号按 name DESC（工(5DE5) > 外(5916) > 三(4E09)）
        assert_eq!(names, vec!["工资", "外卖", "三餐"]);

        let dining = TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense").unwrap();
        let names: Vec<&str> = dining.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["外卖", "三餐"]);

        assert_eq!(TagDao::query_by_ledger(&conn, "l1", "").unwrap().len(), 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn max_sort_update_sort_and_unique_index() {
        let (workspace, dir) = crate::dao::test_workspace("tag-dao");
        let conn = workspace.connection();

        assert_eq!(
            TagDao::get_max_sort(&conn, "l1", "餐饮美食:expense").unwrap(),
            0
        );

        let mut item = tag("l1", "三餐", "餐饮美食:expense", 4);
        TagDao::create(&conn, &item).unwrap();
        assert_eq!(
            TagDao::get_max_sort(&conn, "l1", "餐饮美食:expense").unwrap(),
            4
        );

        let error = TagDao::create(&conn, &item).unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        item.sort_order = 8;
        TagDao::update_sort(&conn, "l1", "三餐", "餐饮美食:expense", 8).unwrap();
        let loaded = TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense").unwrap();
        assert_eq!(loaded[0].sort_order, 8);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_counts_use_transaction_tag_table() {
        let (workspace, dir) = crate::dao::test_workspace("tag-dao");
        let conn = workspace.connection();

        record_tag(&conn, "l1", "t1", "三餐");
        record_tag(&conn, "l1", "t1", "外卖");
        record_tag(&conn, "l1", "t2", "三餐");
        record_tag(&conn, "l2", "t3", "三餐");

        assert_eq!(TagDao::count_by_tag(&conn, "l1", "三餐").unwrap(), 2);
        assert_eq!(TagDao::count_by_tag(&conn, "l1", "外卖").unwrap(), 1);
        assert_eq!(TagDao::count_by_tag(&conn, "l1", "不存在").unwrap(), 0);

        let names = vec!["三餐".to_string(), "外卖".to_string(), "不存在".to_string()];
        let counts = TagDao::count_records_by_tags(&conn, "l1", &names).unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts["三餐"], 2);
        assert_eq!(counts["外卖"], 1);
        assert!(TagDao::count_records_by_tags(&conn, "l1", &[])
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_by_category_and_ledger_cleanup() {
        let (workspace, dir) = crate::dao::test_workspace("tag-dao");
        let conn = workspace.connection();

        TagDao::create(&conn, &tag("l1", "三餐", "餐饮美食:expense", 0)).unwrap();
        TagDao::create(&conn, &tag("l1", "外卖", "餐饮美食:expense", 0)).unwrap();
        TagDao::create(&conn, &tag("l1", "工资", "工资奖金:income", 0)).unwrap();
        TagDao::create(&conn, &tag("l2", "三餐", "餐饮美食:expense", 0)).unwrap();

        // 删分类连带删标签：只删同账本同 `分类:交易类型` 的行
        TagDao::delete_by_category(&conn, "l1", "餐饮美食:expense").unwrap();
        let remaining = TagDao::query_by_ledger(&conn, "l1", "all").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "工资");
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l2", "all").unwrap().len(),
            1
        );

        // 账本级联清理走活路径（语句与 tr-service 的 LEDGER_CASCADE 一致）
        conn.execute(
            "DELETE FROM tbl_billadm_tag WHERE ledger_id = ?1",
            params!["l1"],
        )
        .unwrap();
        assert!(TagDao::query_by_ledger(&conn, "l1", "all")
            .unwrap()
            .is_empty());
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l2", "all").unwrap().len(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
