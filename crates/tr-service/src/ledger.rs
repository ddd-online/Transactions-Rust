//! 账本服务。对照 Go `kernel/service/ledger_service.go`。
//!
//! ## 与原实现的结构差异（有意为之）
//!
//! Go 版为每个服务定义接口并用 `server/wire.go` 做依赖注入，目的是便于 mock。
//! Rust 版改为**无状态函数 + `&Workspace` 入参**：数据访问层是自由函数，
//! 测试直接开一个真实临时工作空间（Go 测试也是这么做的），因此不需要 DI 容器，
//! 也就没有 `wire.go` 的对应物。

use tr_domain::models::Ledger;
use tr_store::dao::key_event_image::KeyEventImageDao;
use tr_store::dao::ledger::LedgerDao;
use tr_store::Workspace;

use crate::{assets, ServiceError, ServiceResult};

/// 删除账本时的级联清理顺序，与 Go `DeleteLedgerById` 事务内的顺序**逐条一致**：
/// 交易标签 → 交易 → 分类 → 标签 → 图表 → 模板 → 关键事件图片 → 关键事件
/// → 股票（资金记录/费用设置/标签设置/交易/轮次/历史/持仓/账户）→ 账本本身。
const LEDGER_CASCADE: &[&str] = &[
    "DELETE FROM tbl_billadm_transaction_record_tag WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_transaction_record WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_category WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_tag WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_chart WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_transaction_tpl WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_key_event_image WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_key_event WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_fee_setting WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_tag_setting WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_round WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_history WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_position WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_account WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_ledger WHERE id = ?1",
];

/// 新建账本，返回新账本 ID。
pub fn create_ledger(
    workspace: &Workspace,
    ledger_name: &str,
    description: &str,
) -> ServiceResult<String> {
    let ledger = Ledger {
        id: tr_store::util::new_uuid(),
        name: ledger_name.to_string(),
        description: description.to_string(),
        created_at: 0,
        updated_at: 0,
    };

    LedgerDao::create(&workspace.connection(), &ledger).map_err(|error| {
        tracing::error!("创建账本失败, name: {}, err: {}", ledger_name, error);
        ServiceError::from(error)
    })?;

    Ok(ledger.id)
}

/// 修改账本名称与描述。
pub fn modify_ledger(
    workspace: &Workspace,
    ledger_id: &str,
    ledger_name: &str,
    description: &str,
) -> ServiceResult<()> {
    let ledger = Ledger {
        id: ledger_id.to_string(),
        name: ledger_name.to_string(),
        description: description.to_string(),
        created_at: 0,
        updated_at: 0,
    };

    LedgerDao::update(&workspace.connection(), &ledger).map_err(|error| {
        tracing::error!("修改账本失败, id: {}, err: {}", ledger_id, error);
        ServiceError::from(error)
    })
}

/// 全部账本。
pub fn list_all_ledger(workspace: &Workspace) -> ServiceResult<Vec<Ledger>> {
    LedgerDao::list_all(&workspace.connection()).map_err(|error| {
        tracing::error!("列出账本失败, err: {}", error);
        ServiceError::from(error)
    })
}

/// 按 ID 查询账本。
pub fn query_ledger_by_id(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Ledger> {
    LedgerDao::query_by_id(&workspace.connection(), ledger_id).map_err(|error| {
        tracing::error!("按 ID 查询账本失败, id: {}, err: {}", ledger_id, error);
        ServiceError::from(error)
    })
}

/// 按名称查询账本。
pub fn query_ledger_by_name(workspace: &Workspace, ledger_name: &str) -> ServiceResult<Ledger> {
    LedgerDao::query_by_name(&workspace.connection(), ledger_name).map_err(|error| {
        tracing::error!("按名称查询账本失败, name: {}, err: {}", ledger_name, error);
        ServiceError::from(error)
    })
}

/// 删除账本及其全部业务数据（单事务级联），提交后再清理磁盘上的图片文件。
pub fn delete_ledger_by_id(workspace: &Workspace, ledger_id: &str) -> ServiceResult<()> {
    // 事务前收集该账本的图片路径：删库成功后无法再查到它们
    let images = KeyEventImageDao::query_by_ledger_id(&workspace.connection(), ledger_id)
        .map_err(ServiceError::from)?;
    let image_files: Vec<(String, String)> = images
        .into_iter()
        .map(|image| (image.file_path, image.thumb_path))
        .collect();

    workspace
        .transaction(|conn| {
            for sql in LEDGER_CASCADE {
                conn.execute(sql, [ledger_id])?;
            }
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("删除账本失败, id: {}, err: {}", ledger_id, error);
            error
        })?;

    // 事务提交成功后再删除磁盘文件（与原实现一致，避免删库成功而删文件失败导致记录缺失）
    for (file_path, thumb_path) in image_files {
        assets::remove_image_files(workspace, &file_path, &thumb_path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_store::dao::is_not_found;

    fn workspace(tag: &str) -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-ledger-service-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

    #[test]
    fn create_then_query_roundtrip() {
        let (workspace, dir) = workspace("create");
        let id = create_ledger(&workspace, "默认账本", "说明").unwrap();

        let loaded = query_ledger_by_id(&workspace, &id).unwrap();
        assert_eq!(loaded.name, "默认账本");
        assert_eq!(loaded.description, "说明");
        assert!(loaded.created_at > 0);

        modify_ledger(&workspace, &id, "改名", "新说明").unwrap();
        let updated = query_ledger_by_id(&workspace, &id).unwrap();
        assert_eq!(updated.name, "改名");
        assert_eq!(updated.description, "新说明");

        assert_eq!(list_all_ledger(&workspace).unwrap().len(), 1);
        assert_eq!(query_ledger_by_name(&workspace, "改名").unwrap().id, id);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_ledger_surfaces_not_found_error() {
        let (workspace, dir) = workspace("missing");
        let error = query_ledger_by_id(&workspace, "absent").unwrap_err();
        match error {
            ServiceError::Database(db_error) => assert!(is_not_found(&db_error)),
            other => panic!("期望数据库未命中错误，实际 {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_ledger_cascades_all_business_tables() {
        let (workspace, dir) = workspace("cascade");
        let ledger_id = create_ledger(&workspace, "待删", "").unwrap();
        let other_id = create_ledger(&workspace, "保留", "").unwrap();

        // 每个业务表各插入两行（分别属于两个账本），列取值全部用字面量，避免参数顺序出错。
        // 表清单必须与 LEDGER_CASCADE 覆盖的表一致——新增业务表时这里会提示补测。
        let inserts: &[&str] = &[
            "INSERT INTO tbl_billadm_transaction_record_tag (ledger_id, transaction_id, tag) VALUES (?1, 'tr-a', '三餐')",
            "INSERT INTO tbl_billadm_transaction_record \
             (transaction_id, ledger_id, price, transaction_type, category, description, flags, key_event_date, transaction_at, created_at, updated_at) \
             VALUES (?2, ?1, 100, 'expense', '餐饮美食', '', '{}', '', 1, 1, 1)",
            "INSERT INTO tbl_billadm_category (ledger_id, name, transaction_type, sort_order) VALUES (?1, '餐饮美食', 'expense', 0)",
            "INSERT INTO tbl_billadm_tag (ledger_id, name, category_transaction_type, sort_order) VALUES (?1, '三餐', '餐饮美食:expense', 0)",
            "INSERT INTO tbl_billadm_chart \
             (chart_id, ledger_id, title, granularity, chart_lines, chart_type, is_preset, sort_order, created_at, updated_at) \
             VALUES (?2, ?1, '图表', 'month', '[]', 'line', 0, 0, 1, 1)",
            "INSERT INTO tbl_billadm_transaction_tpl \
             (template_id, ledger_id, template_name, transaction_type, category, tags, flags, description, sort_order, created_at, updated_at) \
             VALUES (?2, ?1, '模板', 'expense', '餐饮美食', '[]', '', '', 0, 1, 1)",
            "INSERT INTO tbl_billadm_key_event_image \
             (id, ledger_id, event_date, file_path, thumb_path, sort_order, created_at) \
             VALUES (?2, ?1, '2026-01-01', 'a.jpg', 'thumb_a.jpg', 0, 1)",
            "INSERT INTO tbl_billadm_key_event (id, ledger_id, date, title, content, color, created_at, updated_at) \
             VALUES (?2, ?1, '2026-01-01', '标题', '内容', '', 1, 1)",
            "INSERT INTO tbl_billadm_stock_fund_record \
             (id, ledger_id, record_date, event_type, event_text, amount_change, cash_balance, net_pnl, remark, created_at) \
             VALUES (?2, ?1, '2026-01-01', 'add_principal', '', 100, 100, NULL, '', 1)",
            "INSERT INTO tbl_billadm_stock_fee_setting \
             (id, ledger_id, commission_rate, min_commission, stamp_duty_rate, transfer_fee_rate, created_at, updated_at) \
             VALUES (?2, ?1, 0.0002354, 500, 0.0005, 0.00001, 1, 1)",
            "INSERT INTO tbl_billadm_stock_trade_tag_setting (id, ledger_id, tags, created_at, updated_at) \
             VALUES (?2, ?1, '[]', 1, 1)",
            "INSERT INTO tbl_billadm_stock_trade \
             (id, ledger_id, stock_code, stock_name, trade_type, round_id, order_id, order_seq, price, lots, shares, amount, fee, commission, stamp_duty, transfer_fee, realized_pnl, trade_time, remark, created_at) \
             VALUES (?2, ?1, '600519', '贵州茅台', 'open', '', '', 1, 100, 1, 100, 10000, 5, 5, 0, 0, NULL, 1, '', 1)",
            "INSERT INTO tbl_billadm_stock_trade_round \
             (id, ledger_id, stock_code, history_id, round_no, opened_at, closed_at, tag, review, created_at) \
             VALUES (?2, ?1, '600519', ?2, 1, 1, 2, '分析', '', 1)",
            "INSERT INTO tbl_billadm_stock_trade_history \
             (id, ledger_id, stock_code, stock_name, created_at, updated_at) VALUES (?2, ?1, '600519', '贵州茅台', 1, 1)",
            "INSERT INTO tbl_billadm_stock_position \
             (id, ledger_id, stock_code, stock_name, quantity, total_cost, realized_pnl, review, created_at, updated_at) \
             VALUES (?2, ?1, '600519', '贵州茅台', 100, 10000, 0, '', 1, 1)",
            "INSERT INTO tbl_billadm_stock_account (id, ledger_id, principal, created_at, updated_at) VALUES (?2, ?1, 0, 1, 1)",
        ];

        {
            let conn = workspace.connection();
            for sql in inserts {
                // 每条语句只声明它真正用到的占位符（`?1` = 账本 id，`?2` = 行唯一 id）。
                // 注意同一编号可能在一条语句里出现多次（例如 id 与 history_id），
                // 因此按"最大编号"而不是"出现次数"传参。
                let placeholder_max = sql
                    .match_indices('?')
                    .filter_map(|(index, _)| sql[index + 1..].chars().next()?.to_digit(10))
                    .max()
                    .unwrap_or(1) as usize;
                let unique_id = tr_store::util::new_uuid();
                for (ledger, prefix) in [(&ledger_id, "a"), (&other_id, "b")] {
                    let args: Vec<String> = (1..=placeholder_max)
                        .map(|position| {
                            if position == 1 {
                                ledger.clone()
                            } else {
                                format!("{prefix}-{unique_id}")
                            }
                        })
                        .collect();
                    conn.execute(sql, rusqlite::params_from_iter(args))
                        .unwrap_or_else(|error| panic!("插入失败: {sql}\n{error}"));
                }
            }
        }

        delete_ledger_by_id(&workspace, &ledger_id).unwrap();

        // 从 SQL 文本里抽出表名，逐表断言"待删账本清零、保留账本仍在"
        let conn = workspace.connection();
        for sql in inserts {
            let table = sql.split_whitespace().nth(2).expect("INSERT INTO <table>");
            let remaining: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE ledger_id = ?1"),
                    [&ledger_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(remaining, 0, "{table} 未级联清理");
            let kept: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE ledger_id = ?1"),
                    [&other_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(kept, 1, "{table} 误删了其它账本的数据");
        }

        let ledgers = list_all_ledger(&workspace).unwrap();
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].id, other_id);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cascade_covers_every_ledger_scoped_table_in_the_workspace() {
        // 防回归：除账本表本身外，所有带 ledger_id 列的表都必须出现在级联里，
        // 否则删账本会留下孤儿数据。
        let (workspace, dir) = workspace("cascade-coverage");
        let conn = workspace.connection();

        let mut statement = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        let mut ledger_scoped: Vec<String> = Vec::new();
        for table in tables {
            let mut columns = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            let names: Vec<String> = columns
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            if names.iter().any(|name| name == "ledger_id") {
                ledger_scoped.push(table);
            }
        }

        for table in ledger_scoped {
            let covered = LEDGER_CASCADE.iter().any(|sql| sql.contains(&table));
            assert!(covered, "{table} 带 ledger_id 但未纳入账本级联删除");
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
