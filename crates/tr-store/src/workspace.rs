//! 工作空间：一个目录 = 一个独立 SQLite 数据库，加上 `data/assets` 资产目录。
//!
//! 连接参数（WAL / busy_timeout / synchronous=NORMAL / foreign_keys=ON，连接池 4）
//! 是本模块的硬约定：这组参数决定了对同一数据库的并发行为与锁等待语义，不要随意改动。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use tr_domain::consts;

use crate::{migrations, schema};

/// 打开工作空间时可能出现的失败。
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("访问工作空间目录失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("数据库连接池错误: {0}")]
    Pool(#[from] r2d2::Error),
    /// 既有数据库比当前格式更早，且**没有**可用的升级路径（见 [`crate::schema::validate_current`]）。
    #[error("{0}")]
    Incompatible(String),
    /// 升级既有数据库时失败（已回滚，库保持升级前的样子；备份路径见日志与错误文本）。
    #[error("工作空间升级失败: {0}")]
    Migration(String),
}

/// 打开中的工作空间。
pub struct Workspace {
    directory: PathBuf,
    pool: Pool<SqliteConnectionManager>,
}

/// 只暴露目录（连接池没有有意义的调试表示），但足以让测试里的 `unwrap_err()`
/// 与日志输出可用。
impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl Workspace {
    /// 打开（必要时创建）指定目录下的工作空间。
    ///
    /// * 目录不存在 → 创建
    /// * `transactions.db` 不存在 → 执行最新 schema DDL 建库
    /// * 已存在 → 先跑**迁移引擎**把更早格式升级到当前格式（升级前自动备份），
    ///   再按当前格式**只读校验**；没有可用升级路径时返回 [`WorkspaceError::Incompatible`]
    pub fn open(directory: &Path) -> Result<Self, WorkspaceError> {
        if !directory.is_dir() {
            std::fs::create_dir_all(directory)?;
        }

        let db_path = directory.join(consts::DB_NAME);
        let is_new = !db_path.exists();

        let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
            // 这四个 PRAGMA 与上方的连接参数约定一一对应。
            // journal_mode 会返回一行结果，因此用 query_row 而不是 execute_batch。
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            )?;
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(4)
            .min_idle(Some(1))
            .idle_timeout(Some(Duration::from_secs(30 * 60)))
            .build(manager)?;
        let workspace = Self {
            directory: directory.to_path_buf(),
            pool,
        };

        if is_new {
            let conn = workspace.connection();
            schema::create_fresh(&conn)?;
        } else {
            // 既有库：先按登记表升级（无待应用迁移时是纯只读的 no-op），再校验。
            let mut conn = workspace.connection();
            migrations::apply_all(&mut conn, directory)?;
        }

        // 建库/升级后立刻自检一次，避免 DDL、迁移与校验规则三者漂移
        let conn = workspace.connection();
        schema::validate_current(&conn)?;

        Ok(workspace)
    }

    /// 取一个连接（池化）。
    pub fn connection(&self) -> PooledConnection<SqliteConnectionManager> {
        self.pool
            .get()
            .expect("工作空间连接池不可用（池在 Workspace 生命周期内始终有效）")
    }

    /// 工作空间目录。
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// 资产目录（`<workspace>/data/assets`）。图片与缩略图都存在这里。
    pub fn assets_directory(&self) -> PathBuf {
        self.directory.join("data").join("assets")
    }

    /// 在一个数据库事务中执行 `f`；`f` 返回 `Err` 时回滚。
    pub fn transaction<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Connection) -> Result<T, E>,
        E: From<rusqlite::Error>,
    {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

/// 已打开工作空间的持有者（进程内单例语义）。
///
/// 进程内单例语义：再次打开会先关闭上一个。
#[derive(Default)]
pub struct WsManager {
    workspace: Mutex<Option<Arc<Workspace>>>,
}

impl WsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开工作空间；若已有打开的工作空间则替换之。
    pub fn open_workspace(&self, directory: &Path) -> Result<Arc<Workspace>, WorkspaceError> {
        let workspace = Arc::new(Workspace::open(directory)?);
        let mut guard = self.workspace.lock().expect("工作空间管理器锁中毒");
        *guard = Some(Arc::clone(&workspace));
        Ok(workspace)
    }

    /// 当前打开的工作空间；未打开时为 `None`。
    pub fn opened_workspace(&self) -> Option<Arc<Workspace>> {
        self.workspace.lock().expect("工作空间管理器锁中毒").clone()
    }

    /// 关闭工作空间（连接池随 Arc 释放）。
    pub fn close(&self) {
        let mut guard = self.workspace.lock().expect("工作空间管理器锁中毒");
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tr-store-test-{tag}-{}-{}",
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
    fn open_creates_database_with_wal_mode() {
        let dir = temp_dir("create");
        let workspace = Workspace::open(&dir).unwrap();

        let db = dir.join(consts::DB_NAME);
        assert!(db.exists(), "应创建 {}", db.display());

        let conn = workspace.connection();
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reopen_does_not_touch_schema_when_already_current() {
        let dir = temp_dir("reopen");
        {
            let workspace = Workspace::open(&dir).unwrap();
            // 去掉一个索引，模拟"库被动过"：已是最新格式时重开应原样保留，不被自动补回
            // （迁移只按登记表逐条应用，不会"顺手修复"无关结构）
            workspace
                .connection()
                .execute_batch("DROP INDEX idx_chart_ledger")
                .unwrap();
        }
        {
            let _workspace = Workspace::open(&dir).unwrap();
            let conn = _workspace.connection();
            let chart_index: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_chart_ledger'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(chart_index, 0, "已是最新格式时重开不得执行任何 DDL");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manager_replaces_previous_workspace() {
        let first = temp_dir("ws-first");
        let second = temp_dir("ws-second");

        let manager = WsManager::new();
        assert!(manager.opened_workspace().is_none());

        manager.open_workspace(&first).unwrap();
        assert_eq!(
            manager.opened_workspace().unwrap().directory(),
            first.as_path()
        );

        manager.open_workspace(&second).unwrap();
        assert_eq!(
            manager.opened_workspace().unwrap().directory(),
            second.as_path()
        );

        manager.close();
        assert!(manager.opened_workspace().is_none());

        std::fs::remove_dir_all(&first).ok();
        std::fs::remove_dir_all(&second).ok();
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let dir = temp_dir("tx");
        let workspace = Workspace::open(&dir).unwrap();

        let result: Result<(), WorkspaceError> = workspace.transaction(|conn| {
            conn.execute(
                "INSERT INTO tbl_billadm_ledger (id, name, description, created_at, updated_at) \
                 VALUES ('l1', '账本', '', 1, 1)",
                [],
            )?;
            Err(WorkspaceError::Incompatible("故意失败".into()))
        });
        assert!(result.is_err());

        let count: i64 = workspace
            .connection()
            .query_row("SELECT COUNT(*) FROM tbl_billadm_ledger", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "事务失败后必须回滚");

        std::fs::remove_dir_all(&dir).ok();
    }
}
