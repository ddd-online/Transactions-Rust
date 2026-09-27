//! 待办 DAO：卡片（主题） / 事项 / 进度记录。
//!
//! 约定与其它 DAO 一致：入参是模型结构体，时间戳由 DAO 在 INSERT/UPDATE 时填，
//! 列名恒为 snake_case（列映射在这里显式书写，不依赖 serde）。
//!
//! 「进度记录」在读取时**按账本一次捞全**再在服务层按 `item_id` 分桶：
//! 单个账本的待办量级很小，一条 `SELECT` 比"每个事项查一次"省得多。

use rusqlite::{params, Connection};

use tr_domain::models::{TodoCard, TodoItem, TodoProgress};

pub struct TodoCardDao;
pub struct TodoItemDao;
pub struct TodoProgressDao;

const CARD_COLUMNS: &str = "id, ledger_id, title, created_at, updated_at";
const ITEM_COLUMNS: &str = "id, ledger_id, card_id, title, start_date, due_date, urgency, \
     importance, status, completed_at, created_at, updated_at";
const PROGRESS_COLUMNS: &str = "id, ledger_id, item_id, content, created_at";

impl TodoCardDao {
    /// 新建卡片（时间戳取当前时刻）。
    pub fn create(conn: &Connection, card: &TodoCard) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_todo_card (id, ledger_id, title, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![card.id, card.ledger_id, card.title, now],
        )?;
        Ok(())
    }

    /// 某账本的全部卡片，按创建顺序（同秒时按 rowid，保证稳定）。
    pub fn list_by_ledger(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Vec<TodoCard>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {CARD_COLUMNS} FROM tbl_billadm_todo_card WHERE ledger_id = ?1 \
             ORDER BY created_at, rowid"
        ))?;
        let rows = statement.query_map([ledger_id], card_from_row)?;
        rows.collect()
    }

    /// 取一张卡片；不存在返回 `QueryReturnedNoRows`。
    pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<TodoCard> {
        conn.query_row(
            &format!("SELECT {CARD_COLUMNS} FROM tbl_billadm_todo_card WHERE id = ?1"),
            [id],
            card_from_row,
        )
    }

    /// 删一张卡片（事项与进度记录由服务层在同一个事务里一起删）。
    pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
        conn.execute("DELETE FROM tbl_billadm_todo_card WHERE id = ?1", [id])?;
        Ok(())
    }
}

impl TodoItemDao {
    /// 新建事项（时间戳取当前时刻）。
    pub fn create(conn: &Connection, item: &TodoItem) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_todo_item \
             (id, ledger_id, card_id, title, start_date, due_date, urgency, importance, status, \
              completed_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                item.id,
                item.ledger_id,
                item.card_id,
                item.title,
                item.start_date,
                item.due_date,
                item.urgency,
                item.importance,
                item.status,
                item.completed_at,
                now,
            ],
        )?;
        Ok(())
    }

    /// 编辑事项的正文与属性（**不动** status / completed_at，那两个走 [`Self::update_status`]）。
    pub fn update(conn: &Connection, item: &TodoItem) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_todo_item SET title = ?2, start_date = ?3, due_date = ?4, \
             urgency = ?5, importance = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                item.id,
                item.title,
                item.start_date,
                item.due_date,
                item.urgency,
                item.importance,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 改状态（顺带写/清完成时刻）。
    pub fn update_status(
        conn: &Connection,
        id: &str,
        status: &str,
        completed_at: i64,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_todo_item SET status = ?2, completed_at = ?3, updated_at = ?4 \
             WHERE id = ?1",
            params![id, status, completed_at, crate::util::now_unix()],
        )?;
        Ok(())
    }

    /// 取一个事项；不存在返回 `QueryReturnedNoRows`。
    pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<TodoItem> {
        conn.query_row(
            &format!("SELECT {ITEM_COLUMNS} FROM tbl_billadm_todo_item WHERE id = ?1"),
            [id],
            item_from_row,
        )
    }

    /// 某账本按状态取事项。`status` 为空串时取全部（历史/卡片视图都只用具体状态）。
    ///
    /// 排序：进行中的按创建时间（新的在前，与"刚加的在上"一致），
    /// 已完成的按**完成时刻倒序**（最近完成的在最上面）。
    pub fn list_by_status(
        conn: &Connection,
        ledger_id: &str,
        status: &str,
    ) -> rusqlite::Result<Vec<TodoItem>> {
        let sql = if status.is_empty() {
            format!(
                "SELECT {ITEM_COLUMNS} FROM tbl_billadm_todo_item WHERE ledger_id = ?1 \
                 ORDER BY created_at DESC, rowid DESC"
            )
        } else if status == tr_domain::consts::TODO_STATUS_DONE {
            format!(
                "SELECT {ITEM_COLUMNS} FROM tbl_billadm_todo_item WHERE ledger_id = ?1 \
                 AND status = ?2 ORDER BY completed_at DESC, rowid DESC"
            )
        } else {
            format!(
                "SELECT {ITEM_COLUMNS} FROM tbl_billadm_todo_item WHERE ledger_id = ?1 \
                 AND status = ?2 ORDER BY created_at DESC, rowid DESC"
            )
        };
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params![ledger_id, status], item_from_row)?;
        rows.collect()
    }

    /// 某卡片下某状态的事项（卡片视图按卡片取；排序与 [`Self::list_by_status`] 一致）。
    pub fn list_by_card(
        conn: &Connection,
        card_id: &str,
        status: &str,
    ) -> rusqlite::Result<Vec<TodoItem>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {ITEM_COLUMNS} FROM tbl_billadm_todo_item WHERE card_id = ?1 AND status = ?2 \
             ORDER BY created_at DESC, rowid DESC"
        ))?;
        let rows = statement.query_map(params![card_id, status], item_from_row)?;
        rows.collect()
    }

    /// 某卡片下**全部**事项（删卡片时用来找要一起删的进度记录）。
    pub fn list_ids_by_card(conn: &Connection, card_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut statement =
            conn.prepare("SELECT id FROM tbl_billadm_todo_item WHERE card_id = ?1")?;
        let rows = statement.query_map([card_id], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// 删一个事项（进度记录由服务层在同一个事务里一起删）。
    pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
        conn.execute("DELETE FROM tbl_billadm_todo_item WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 删一张卡片下的全部事项。
    pub fn delete_by_card(conn: &Connection, card_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_todo_item WHERE card_id = ?1",
            [card_id],
        )?;
        Ok(())
    }
}

impl TodoProgressDao {
    /// 追加一条进度记录。
    pub fn create(conn: &Connection, progress: &TodoProgress) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tbl_billadm_todo_progress (id, ledger_id, item_id, content, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                progress.id,
                progress.ledger_id,
                progress.item_id,
                progress.content,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 某账本的全部进度记录（服务层按 `item_id` 分桶；按记录顺序）。
    pub fn list_by_ledger(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<TodoProgress>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {PROGRESS_COLUMNS} FROM tbl_billadm_todo_progress WHERE ledger_id = ?1 \
             ORDER BY created_at, rowid"
        ))?;
        let rows = statement.query_map([ledger_id], progress_from_row)?;
        rows.collect()
    }

    /// 删一条进度记录，返回删掉的行数（0 = 本来就没有这一行）。
    pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<usize> {
        conn.execute("DELETE FROM tbl_billadm_todo_progress WHERE id = ?1", [id])
    }

    /// 删一个事项下的全部进度记录。
    pub fn delete_by_item(conn: &Connection, item_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_todo_progress WHERE item_id = ?1",
            [item_id],
        )?;
        Ok(())
    }

    /// 删一张卡片下全部事项的进度记录（删卡片时用）。
    pub fn delete_by_card(conn: &Connection, card_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_todo_progress WHERE item_id IN \
             (SELECT id FROM tbl_billadm_todo_item WHERE card_id = ?1)",
            [card_id],
        )?;
        Ok(())
    }
}

fn card_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TodoCard> {
    Ok(TodoCard {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        title: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TodoItem> {
    Ok(TodoItem {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        card_id: row.get(2)?,
        title: row.get(3)?,
        start_date: row.get(4)?,
        due_date: row.get(5)?,
        urgency: row.get(6)?,
        importance: row.get(7)?,
        status: row.get(8)?,
        completed_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn progress_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TodoProgress> {
    Ok(TodoProgress {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        item_id: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
    })
}
