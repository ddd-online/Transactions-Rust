//! 图表 DAO。
//!
//! 行为约定：
//! * `create` 自动填充 `created_at` / `updated_at`（均为秒级 Unix 秒）
//! * `save` 是"按主键写回全部字段"，并刷新 `updated_at`，
//!   因此这里整行回写（`ledger_id` / `created_at` 保持调用方传入的值）
//! * 列表排序固定为：`ORDER BY is_preset DESC, sort_order ASC, created_at DESC`
//! * `is_preset` 列在基线 schema 里是 `numeric`，按整数 0/1 存储/读取

use rusqlite::{params, Connection};

use tr_domain::models::Chart;

pub struct ChartDao;

const COLUMNS: &str = "chart_id, ledger_id, title, granularity, chart_lines, chart_type, \
                       is_preset, sort_order, created_at, updated_at";

impl ChartDao {
    /// 新建图表（自动填充时间戳）。
    pub fn create(conn: &Connection, chart: &Chart) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_chart \
             (chart_id, ledger_id, title, granularity, chart_lines, chart_type, is_preset, \
              sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                chart.chart_id,
                chart.ledger_id,
                chart.title,
                chart.granularity,
                chart.chart_lines,
                chart.chart_type,
                chart.is_preset,
                chart.sort_order,
                now
            ],
        )?;
        Ok(())
    }

    /// 按图表 ID 删除（不存在的记录视为成功）。
    pub fn delete_by_id(conn: &Connection, chart_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_chart WHERE chart_id = ?1",
            params![chart_id],
        )?;
        Ok(())
    }

    /// 该账本下最大的 `sort_order`（无记录时为 0）。
    pub fn get_max_sort(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i32> {
        conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM tbl_billadm_chart WHERE ledger_id = ?1",
            params![ledger_id],
            |row| row.get(0),
        )
    }

    /// 按图表 ID 查询；不存在时返回 `QueryReturnedNoRows`。
    pub fn query_by_id(conn: &Connection, chart_id: &str) -> rusqlite::Result<Chart> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM tbl_billadm_chart WHERE chart_id = ?1"),
            params![chart_id],
            from_row,
        )
    }

    /// 某账本的全部图表（预设图表优先，其次按排序号与创建时间）。
    pub fn query_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Vec<Chart>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_chart WHERE ledger_id = ?1 \
             ORDER BY is_preset DESC, sort_order ASC, created_at DESC"
        ))?;
        let rows = statement.query_map(params![ledger_id], from_row)?;
        rows.collect()
    }

    /// 某账本的图表数量（用于预设图表 seeding 的幂等判断）。
    pub fn count_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_chart WHERE ledger_id = ?1",
            params![ledger_id],
            |row| row.get(0),
        )
    }

    /// 整行回写（主键命中则更新全部字段并刷新 `updated_at`；否则不改动任何行）。
    pub fn save(conn: &Connection, chart: &Chart) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_chart SET ledger_id = ?2, title = ?3, granularity = ?4, \
             chart_lines = ?5, chart_type = ?6, is_preset = ?7, sort_order = ?8, \
             created_at = ?9, updated_at = ?10 WHERE chart_id = ?1",
            params![
                chart.chart_id,
                chart.ledger_id,
                chart.title,
                chart.granularity,
                chart.chart_lines,
                chart.chart_type,
                chart.is_preset,
                chart.sort_order,
                chart.created_at,
                crate::util::now_unix()
            ],
        )?;
        Ok(())
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chart> {
    Ok(Chart {
        chart_id: row.get(0)?,
        ledger_id: row.get(1)?,
        title: row.get(2)?,
        granularity: row.get(3)?,
        chart_lines: row.get(4)?,
        chart_type: row.get(5)?,
        is_preset: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 `tr-service` 预设曲线同构的一行 JSON（仅用于 DAO 往返断言）。
    const LINES: &str =
        r#"[{"label":"支出","transactionType":"expense","includeOutlier":false,"conditions":[]}]"#;

    fn chart(id: &str, ledger_id: &str, title: &str, is_preset: bool, sort_order: i32) -> Chart {
        Chart {
            chart_id: id.to_string(),
            ledger_id: ledger_id.to_string(),
            title: title.to_string(),
            granularity: "month".to_string(),
            chart_lines: LINES.to_string(),
            chart_type: "line".to_string(),
            is_preset,
            sort_order,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn create_fills_timestamps_and_reads_back_every_column() {
        let (workspace, dir) = crate::dao::test_workspace("chart-dao");
        let conn = workspace.connection();

        ChartDao::create(&conn, &chart("c1", "l1", "月度消费趋势", true, 0)).unwrap();
        let loaded = ChartDao::query_by_id(&conn, "c1").unwrap();
        assert_eq!(loaded.title, "月度消费趋势");
        assert_eq!(loaded.granularity, "month");
        assert_eq!(loaded.chart_type, "line");
        assert!(loaded.is_preset, "is_preset 应读回 true（numeric 1）");
        assert_eq!(loaded.sort_order, 0);
        assert!(loaded.created_at > 0);
        assert_eq!(loaded.created_at, loaded.updated_at);
        assert_eq!(loaded.chart_lines, LINES);

        // 主键冲突
        let error = ChartDao::create(&conn, &chart("c1", "l1", "重复", false, 1)).unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_chart_reports_not_found() {
        let (workspace, dir) = crate::dao::test_workspace("chart-dao");
        let error = ChartDao::query_by_id(&workspace.connection(), "nope").unwrap_err();
        assert!(super::super::is_not_found(&error), "error = {error:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn query_orders_preset_first_then_sort_then_created_desc() {
        let (workspace, dir) = crate::dao::test_workspace("chart-dao");
        let conn = workspace.connection();

        let insert = "INSERT INTO tbl_billadm_chart \
             (chart_id, ledger_id, title, granularity, chart_lines, chart_type, is_preset, \
              sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'month', '[]', 'line', ?4, ?5, ?6, ?6)";
        conn.execute(insert, params!["a", "l1", "自定义甲", 0, 0, 100])
            .unwrap();
        conn.execute(insert, params!["b", "l1", "预设甲", 1, 1, 200])
            .unwrap();
        conn.execute(insert, params!["c", "l1", "预设乙", 1, 0, 300])
            .unwrap();
        conn.execute(insert, params!["d", "l2", "其它账本", 1, 0, 400])
            .unwrap();

        let charts = ChartDao::query_by_ledger_id(&conn, "l1").unwrap();
        let ids: Vec<&str> = charts.iter().map(|item| item.chart_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b", "a"]);
        assert_eq!(ChartDao::count_by_ledger_id(&conn, "l1").unwrap(), 3);
        assert_eq!(ChartDao::get_max_sort(&conn, "l1").unwrap(), 1);
        assert_eq!(ChartDao::get_max_sort(&conn, "l9").unwrap(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_writes_back_all_fields_and_refreshes_updated_at() {
        let (workspace, dir) = crate::dao::test_workspace("chart-dao");
        let conn = workspace.connection();

        ChartDao::create(&conn, &chart("c1", "l1", "旧标题", true, 0)).unwrap();
        let created = ChartDao::query_by_id(&conn, "c1").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let mut edited = created.clone();
        edited.title = "新标题".to_string();
        edited.granularity = "year".to_string();
        edited.chart_lines = "[]".to_string();
        edited.chart_type = "bar".to_string();
        edited.sort_order = 7;
        ChartDao::save(&conn, &edited).unwrap();

        let saved = ChartDao::query_by_id(&conn, "c1").unwrap();
        assert_eq!(saved.title, "新标题");
        assert_eq!(saved.granularity, "year");
        assert_eq!(saved.chart_lines, "[]");
        assert_eq!(saved.chart_type, "bar");
        assert_eq!(saved.sort_order, 7);
        assert!(saved.is_preset, "Save 不改动 is_preset");
        assert_eq!(saved.created_at, created.created_at);
        assert!(saved.updated_at > created.updated_at, "updated_at 必须刷新");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_by_id_and_by_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("chart-dao");
        let conn = workspace.connection();

        ChartDao::create(&conn, &chart("c1", "l1", "甲", false, 0)).unwrap();
        ChartDao::create(&conn, &chart("c2", "l1", "乙", false, 1)).unwrap();
        ChartDao::create(&conn, &chart("c3", "l2", "丙", false, 0)).unwrap();

        ChartDao::delete_by_id(&conn, "c1").unwrap();
        assert_eq!(ChartDao::count_by_ledger_id(&conn, "l1").unwrap(), 1);

        // 账本级联清理走活路径（语句与 tr-service 的 LEDGER_CASCADE 一致）
        conn.execute(
            "DELETE FROM tbl_billadm_chart WHERE ledger_id = ?1",
            params!["l1"],
        )
        .unwrap();
        assert_eq!(ChartDao::count_by_ledger_id(&conn, "l1").unwrap(), 0);
        assert_eq!(ChartDao::count_by_ledger_id(&conn, "l2").unwrap(), 1);
        // 删除不存在的记录视为成功
        ChartDao::delete_by_id(&conn, "absent").unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }
}
