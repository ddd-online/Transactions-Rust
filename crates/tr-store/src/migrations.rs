//! 工作空间迁移引擎：把更早格式的数据库升级到当前格式。
//!
//! ## 什么时候跑
//!
//! [`crate::Workspace::open`] 打开**既有**数据库时调用 [`apply_all`]：先按登记表算出待应用的迁移，
//! 有待应用的就**先备份**再逐条应用，最后交给 [`crate::schema::validate_current`] 做只读校验。
//! 新库（`create_fresh`）不经过迁移 —— 它直接建出当前格式，`fresh.sql` 末尾已把全部迁移登记为已应用。
//! `cargo xtask validate` / `dump` 仍然只读：它们只**报告**待应用的迁移，不应用。
//!
//! ## 迁移编写规范（新增迁移必须逐条满足）
//!
//! 1. **id 唯一且稳定**：`YYYYMMDD_描述`，写进 `tbl_billadm_schema_migration`；
//! 2. **一个事务**：引擎用 `BEGIN` 包住「执行 + 登记」，任一步失败整体回滚，库保持原样；
//! 3. **幂等 / 防御式**：先查 `PRAGMA table_info` / `sqlite_master` 判断结构是否已在，
//!    已在就只补登记行（这样"手工升级过但忘了登记"的库也能自愈，重复打开也不会重复改）；
//! 4. **只碰本次升级涉及的表**：不许顺手"修复"别的结构；
//! 5. **留单测**：旧格式 → 迁移后校验通过、数据一字不差、重复应用无副作用。
//!
//! ## 备份
//!
//! 只要有待应用的迁移，就先 `VACUUM INTO` 出一份**单文件一致快照**
//! （`<workspace>/transactions.db.pre-migration-<unix秒>.bak`；含 WAL 里已提交的内容）。
//! 备份失败 → **不升级**并拒绝打开工作空间：宁可不升级，也不能在没有退路的情况下改用户数据。
//!
//! **同一工作空间只保留最近一份**：新备份**成功之后**才清掉同目录下按同一命名规则的旧 `.bak`
//! （刻意不放在备份之前 —— 那样一旦新备份失败，旧的那份也已经被删，退路就没了）。
//! 只认自己的命名规则（`<DB_NAME>.pre-migration-*.bak`），绝不动工作空间里别的文件。
//!
//! ## 历史迁移为什么只做"结构前置检查"
//!
//! 前三条迁移在引入本模块之前就已经在你的库上执行并登记过（`fresh.sql` 里也有它们的登记行）。
//! 对它们的**数据回填**语义（例如按父事件的账本回填图片的 `ledger_id`）没有原始脚本可依，
//! 猜测着重放风险远大于收益 —— 所以它们只检查"结构是否已经具备"，具备就补登记行，
//! 不具备就明确拒绝并报出迁移 id，交给人工判断。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use tr_domain::consts;

use crate::workspace::WorkspaceError;

/// 单条迁移的执行结果；错误文本会出现在用户可见的「工作空间升级失败: …」里。
pub type MigrateResult = Result<(), String>;

/// 一条迁移。
pub struct Migration {
    /// 登记 id（`YYYYMMDD_描述`），与 `tbl_billadm_schema_migration.id` 一致。
    pub id: &'static str,
    /// 执行体。必须幂等（见模块头第 3 条）。
    pub apply: fn(&Connection) -> MigrateResult,
}

/// 全部迁移，**顺序即应用顺序**。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "20260101_key_event_ledger_date_composite_unique",
        apply: key_event_ledger_date_composite_unique,
    },
    Migration {
        id: "20260101_key_event_image_backfill_ledger_id",
        apply: key_event_image_backfill_ledger_id,
    },
    Migration {
        id: "20260918_stock_trade_backfill_order_id",
        apply: stock_trade_backfill_order_id,
    },
    Migration {
        id: "20260920_diary_ledger_scope",
        apply: diary_ledger_scope,
    },
];

/// 已是当前格式（无需迁移）。
pub fn is_current(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(pending(conn)?.is_empty())
}

/// 待应用的迁移 id（按 [`MIGRATIONS`] 顺序）。**只读**，不建表、不写入。
///
/// 登记表本身不存在（极老库）时视为"全部待应用"，让各迁移自己的结构前置检查去判定。
pub fn pending(conn: &Connection) -> rusqlite::Result<Vec<&'static str>> {
    if !table_exists(conn, "tbl_billadm_schema_migration")? {
        return Ok(MIGRATIONS.iter().map(|item| item.id).collect());
    }

    let mut statement = conn.prepare("SELECT id FROM tbl_billadm_schema_migration")?;
    let applied: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(MIGRATIONS
        .iter()
        .filter(|migration| !applied.iter().any(|id| id == migration.id))
        .map(|migration| migration.id)
        .collect())
}

/// 应用全部待应用的迁移，返回本次**实际应用**的 id（已是当前格式时返回空 Vec，且不备份、不写入）。
pub fn apply_all(conn: &mut Connection, directory: &Path) -> Result<Vec<String>, WorkspaceError> {
    let todo: Vec<&'static str> = pending(conn)?;
    if todo.is_empty() {
        return Ok(Vec::new());
    }

    let backup = backup_path(directory);
    backup_database(conn, &backup)?;
    // 备份**成功之后**才清掉上一份：新备份失败时旧的那份还在，退路不会一起丢。
    prune_older_backups(directory, &backup);
    tracing::info!(
        "工作空间需要升级（{} 条待应用），已备份到 {}（同一工作空间只保留这一份）",
        todo.len(),
        backup.display()
    );

    // 极老库可能连登记表都没有：迁移要往里写登记行，先按基线文本补出来（IF NOT EXISTS，已存在则无副作用）。
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tbl_billadm_schema_migration \
         (`id` text,`applied_at` integer,PRIMARY KEY (`id`))",
    )?;

    let mut applied = Vec::new();
    for migration in MIGRATIONS {
        if !todo.contains(&migration.id) {
            continue;
        }
        let transaction = conn.transaction()?;
        (migration.apply)(&transaction).map_err(|message| {
            WorkspaceError::Migration(format!("{} 失败: {message}", migration.id))
        })?;
        transaction.execute(
            "INSERT INTO tbl_billadm_schema_migration (id, applied_at) VALUES (?1, ?2)",
            params![migration.id, crate::util::now_unix()],
        )?;
        transaction.commit()?;
        tracing::info!("已应用迁移 {}", migration.id);
        applied.push(migration.id.to_string());
    }

    Ok(applied)
}

/// 备份文件路径（`transactions.db.pre-migration-<unix秒>.bak`，与库同目录）。
pub fn backup_path(directory: &Path) -> PathBuf {
    directory.join(format!(
        "{}.pre-migration-{}.bak",
        consts::DB_NAME,
        crate::util::now_unix()
    ))
}

/// 是否是本模块的备份文件名（`<DB_NAME>.pre-migration-*.bak`）。
///
/// 清理旧备份时**只认这个规则**：工作空间目录里可能有用户自己放的别的东西，一律不动。
fn is_backup_file_name(name: &str) -> bool {
    name.starts_with(&format!("{}.pre-migration-", consts::DB_NAME)) && name.ends_with(".bak")
}

/// 只保留 `keep` 这一份迁移备份，删掉同目录下同一命名规则的其它备份。
///
/// 刻意由 [`apply_all`] 在**新备份成功之后**调用：备份失败时旧备份仍然在。
/// 删除失败只记警告、不中断升级（旧文件多留一份不影响正确性）。
fn prune_older_backups(directory: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_backup_file_name(name) {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!("已清理上一份迁移备份 {}", path.display()),
            Err(error) => tracing::warn!("清理旧迁移备份失败 {}: {error}", path.display()),
        }
    }
}

/// `VACUUM INTO` 出一份单文件一致快照（含 WAL 中已提交的内容）。
fn backup_database(conn: &Connection, path: &Path) -> Result<(), WorkspaceError> {
    // 同一秒内重复升级会算出一模一样的文件名，而 `VACUUM INTO` 拒绝写到已存在的文件：
    // 那份旧文件本来就是"同一位置的上一份备份"，先让位。
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| {
            WorkspaceError::Migration(format!("清理同名旧备份 {} 失败: {error}", path.display()))
        })?;
    }
    let target = path.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{target}'"))
        .map_err(|error| {
            WorkspaceError::Migration(format!("升级前备份到 {} 失败: {error}", path.display()))
        })?;
    Ok(())
}

// ==================================================================== 迁移实现

/// 关键事件：全局唯一 `date` → `(ledger_id, date)` 复合唯一。
///
/// 只在旧的全局唯一索引还在时才动（幂等）。
fn key_event_ledger_date_composite_unique(conn: &Connection) -> MigrateResult {
    if !index_exists(conn, "idx_tbl_billadm_key_event_date").map_err(stringify)? {
        return Ok(());
    }
    conn.execute_batch(
        "DROP INDEX idx_tbl_billadm_key_event_date;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_key_event_ledger_date \
             ON tbl_billadm_key_event(ledger_id, date);",
    )
    .map_err(stringify)
}

/// 关键事件图片的 `ledger_id` 回填：**只做结构前置检查**（见模块头最后一节）。
fn key_event_image_backfill_ledger_id(conn: &Connection) -> MigrateResult {
    require_column(conn, "tbl_billadm_key_event_image", "ledger_id")
}

/// 股票成交的 `order_id` / `order_seq` 回填：**只做结构前置检查**（见模块头最后一节）。
fn stock_trade_backfill_order_id(conn: &Connection) -> MigrateResult {
    require_column(conn, "tbl_billadm_stock_trade", "order_id")?;
    require_column(conn, "tbl_billadm_stock_trade", "order_seq")
}

/// 日记：加上 `ledger_id`（按账本隔离），旧的全工作空间唯一改成 `(ledger_id, date)` 复合唯一。
///
/// 旧日记一律回填到**创建时间最早**的账本；工作空间一个账本都没有时保持空串
/// （界面上不可见，但不会让升级失败）。整条迁移在一个事务里执行（由 [`apply_all`] 包）。
fn diary_ledger_scope(conn: &Connection) -> MigrateResult {
    if !table_exists(conn, "tbl_billadm_diary_entry").map_err(stringify)? {
        return Err("缺少数据表 tbl_billadm_diary_entry".to_string());
    }

    if !column_exists(conn, "tbl_billadm_diary_entry", "ledger_id").map_err(stringify)? {
        conn.execute_batch(
            "ALTER TABLE tbl_billadm_diary_entry ADD COLUMN ledger_id varchar(36) DEFAULT ''",
        )
        .map_err(stringify)?;
    }

    // 回填：老库没有"账本"概念，整体归到最早创建的账本（多个账本时也只归一个 —— 无从拆分）。
    if table_exists(conn, "tbl_billadm_ledger").map_err(stringify)? {
        conn.execute_batch(
            "UPDATE tbl_billadm_diary_entry \
                SET ledger_id = (SELECT id FROM tbl_billadm_ledger ORDER BY created_at, rowid LIMIT 1) \
              WHERE ledger_id IS NULL OR ledger_id = '';",
        )
        .map_err(stringify)?;
    }

    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_tbl_billadm_diary_entry_date;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_tbl_billadm_diary_entry_ledger_date \
             ON tbl_billadm_diary_entry(ledger_id, date);",
    )
    .map_err(stringify)
}

// ==================================================================== 小工具

fn stringify(error: rusqlite::Error) -> String {
    error.to_string()
}

fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn index_exists(conn: &Connection, index: &str) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [index],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    // table 只来自本文件的常量，不是外部输入
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|name| name == column))
}

/// 结构前置检查：列不存在就报出可读的原因（而不是让引擎抛一句 SQL 错误）。
fn require_column(conn: &Connection, table: &str, column: &str) -> MigrateResult {
    if column_exists(conn, table, column).map_err(stringify)? {
        return Ok(());
    }
    Err(format!("结构不符合当前格式：{table} 缺少列 {column}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::create_fresh(&conn).unwrap();
        conn
    }

    /// 把当前格式的库**降级**成"日记还没有账本"的旧格式：老索引回来、新列去掉、登记行删掉。
    fn downgrade_diary(conn: &Connection) {
        conn.execute_batch(
            "DROP INDEX idx_tbl_billadm_diary_entry_ledger_date;
             CREATE UNIQUE INDEX idx_tbl_billadm_diary_entry_date ON tbl_billadm_diary_entry(date);
             ALTER TABLE tbl_billadm_diary_entry DROP COLUMN ledger_id;
             DELETE FROM tbl_billadm_schema_migration WHERE id = '20260920_diary_ledger_scope';",
        )
        .unwrap();
    }

    fn insert_ledger(conn: &Connection, id: &str, name: &str, created_at: i64) {
        conn.execute(
            "INSERT INTO tbl_billadm_ledger (id, name, description, created_at, updated_at) \
             VALUES (?1, ?2, '', ?3, ?3)",
            params![id, name, created_at],
        )
        .unwrap();
    }

    fn insert_diary(conn: &Connection, id: &str, date: &str, content: &str) {
        conn.execute(
            "INSERT INTO tbl_billadm_diary_entry \
             (id, date, content, word_count, mood, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 0, '开心', 1, 1)",
            params![id, date, content],
        )
        .unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tr-migrate-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fresh_database_has_nothing_pending() {
        let conn = fresh_conn();
        assert!(pending(&conn).unwrap().is_empty());
        assert!(is_current(&conn).unwrap());
    }

    #[test]
    fn missing_registry_table_means_everything_is_pending() {
        let conn = fresh_conn();
        conn.execute_batch("DROP TABLE tbl_billadm_schema_migration")
            .unwrap();
        let todo = pending(&conn).unwrap();
        assert_eq!(todo.len(), MIGRATIONS.len());
        // 只读：不建表
        assert!(!table_exists(&conn, "tbl_billadm_schema_migration").unwrap());
    }

    #[test]
    fn diary_migration_upgrades_legacy_database_and_keeps_content() {
        let dir = temp_dir("diary-upgrade");
        let mut conn = fresh_conn();
        insert_ledger(&conn, "l-new", "新账本", 200);
        insert_ledger(&conn, "l-old", "老账本", 100);
        insert_diary(&conn, "d1", "2026-01-01", "第一天的正文");
        insert_diary(&conn, "d2", "2026-02-01", "第二天的正文");
        downgrade_diary(&conn);

        let applied = apply_all(&mut conn, &dir).unwrap();
        assert_eq!(applied, vec!["20260920_diary_ledger_scope".to_string()]);

        // 结构：新列 + 复合唯一索引，旧索引消失
        assert!(column_exists(&conn, "tbl_billadm_diary_entry", "ledger_id").unwrap());
        assert!(index_exists(&conn, "idx_tbl_billadm_diary_entry_ledger_date").unwrap());
        assert!(!index_exists(&conn, "idx_tbl_billadm_diary_entry_date").unwrap());
        // 登记行
        assert!(pending(&conn).unwrap().is_empty());

        // 数据：正文一字不差，且都归到最早创建的账本
        let mut statement = conn
            .prepare("SELECT date, content, ledger_id FROM tbl_billadm_diary_entry ORDER BY date")
            .unwrap();
        let rows: Vec<(String, String, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, "第一天的正文");
        assert_eq!(rows[1].1, "第二天的正文");
        assert!(rows.iter().all(|row| row.2 == "l-old"));

        // 备份文件已落盘
        let backups: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".pre-migration-"))
            })
            .collect();
        assert_eq!(backups.len(), 1, "升级前必须备份一次");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_all_is_idempotent() {
        let dir = temp_dir("idempotent");
        let mut conn = fresh_conn();
        insert_ledger(&conn, "l1", "账本", 1);
        insert_diary(&conn, "d1", "2026-01-01", "正文");
        downgrade_diary(&conn);

        let first = apply_all(&mut conn, &dir).unwrap();
        assert_eq!(first.len(), 1);
        // 再打开一次：没有任何待应用迁移，不写库、不再备份
        let second = apply_all(&mut conn, &dir).unwrap();
        assert!(second.is_empty());
        let backups = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(backups, 1, "第二次打开不得再产生备份");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_diary_entry", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 同一工作空间**只保留最近一份**备份：升级前先放一份旧的，升级后只剩新的那份。
    #[test]
    fn older_backups_are_replaced_by_the_newest_one() {
        let dir = temp_dir("backup-retention");
        let stale = dir.join(format!(
            "{}.pre-migration-1.bak",
            tr_domain::consts::DB_NAME
        ));
        // 冒充"上一次升级留下的备份"（内容无所谓，只要文件名符合规则）
        std::fs::write(&stale, b"stale-backup").unwrap();
        // 顺带放一个**不符合命名规则**的文件：清理时绝不能碰它
        let bystander = dir.join("用户自己放的东西.bak");
        std::fs::write(&bystander, b"keep-me").unwrap();

        let mut conn = fresh_conn();
        insert_ledger(&conn, "l1", "账本", 1);
        insert_diary(&conn, "d1", "2026-01-01", "正文");
        downgrade_diary(&conn);

        apply_all(&mut conn, &dir).unwrap();

        assert!(!stale.exists(), "上一份迁移备份必须被清掉");
        assert!(bystander.exists(), "不符合命名规则的文件一律不动");

        let mut backups: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| is_backup_file_name(name))
            .collect();
        backups.sort();
        assert_eq!(backups.len(), 1, "只留最近一份，实际 {backups:?}");
        assert_ne!(backups[0], "transactions.db.pre-migration-1.bak");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn failed_migration_leaves_database_untouched() {
        let dir = temp_dir("rollback");
        let mut conn = fresh_conn();
        // 把日记表整张删掉：迁移必然失败（缺表），事务回滚，不留登记行
        conn.execute_batch("DROP TABLE tbl_billadm_diary_entry")
            .unwrap();
        conn.execute_batch(
            "DELETE FROM tbl_billadm_schema_migration WHERE id = '20260920_diary_ledger_scope';",
        )
        .unwrap();

        let error = apply_all(&mut conn, &dir).unwrap_err();
        assert!(
            matches!(error, WorkspaceError::Migration(_)),
            "应是升级失败: {error}"
        );
        assert!(
            !pending(&conn).unwrap().is_empty(),
            "失败的迁移不得被登记为已应用"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_key_event_unique_index_is_replaced() {
        let dir = temp_dir("key-event");
        let mut conn = fresh_conn();
        conn.execute_batch(
            "DROP INDEX idx_key_event_ledger_date;
             CREATE UNIQUE INDEX idx_tbl_billadm_key_event_date ON tbl_billadm_key_event(date);
             DELETE FROM tbl_billadm_schema_migration \
              WHERE id = '20260101_key_event_ledger_date_composite_unique';",
        )
        .unwrap();

        let applied = apply_all(&mut conn, &dir).unwrap();
        assert_eq!(
            applied,
            vec!["20260101_key_event_ledger_date_composite_unique".to_string()]
        );
        assert!(!index_exists(&conn, "idx_tbl_billadm_key_event_date").unwrap());
        assert!(index_exists(&conn, "idx_key_event_ledger_date").unwrap());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_older_structure_is_reported_not_guessed() {
        let dir = temp_dir("unknown");
        let mut conn = fresh_conn();
        // 模拟"列都没有"的更早格式：先去掉引用该列的索引，再删列，并抹掉登记行
        conn.execute_batch(
            "DROP INDEX idx_stock_trade_ledger_order;
             ALTER TABLE tbl_billadm_stock_trade DROP COLUMN order_id;
             DELETE FROM tbl_billadm_schema_migration \
              WHERE id = '20260918_stock_trade_backfill_order_id';",
        )
        .unwrap();

        let error = apply_all(&mut conn, &dir).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("20260918_stock_trade_backfill_order_id"),
            "{message}"
        );
        assert!(message.contains("缺少列 order_id"), "{message}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
