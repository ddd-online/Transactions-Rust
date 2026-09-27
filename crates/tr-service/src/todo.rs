//! 待办服务：卡片（主题） / 事项 / 进度记录。
//!
//! 三条纪律：
//! * **按账本隔离**：每个写操作都带 `ledger_id`，取到的行不属于本账本时一律当作"不存在"
//!   （不区分"没有这行"与"是别的账本的"）；
//! * **完成不搬数据**：把 `status` 写成 `done` 并记完成时刻即可 —— 卡片视图只取进行中的、
//!   历史视图只取已完成的，是同一张表的两个 WHERE；
//! * **删卡片 / 删事项连进度记录一起删**（同一个事务），不留孤儿行。

use std::collections::HashMap;

use rusqlite::Connection;

use tr_domain::consts;
use tr_domain::dto::{TodoCardDto, TodoHistoryDto, TodoItemDto, TodoProgressDto};
use tr_domain::error::AppError;
use tr_domain::models::{TodoCard, TodoItem, TodoProgress};
use tr_store::dao::is_not_found;
use tr_store::dao::todo::{TodoCardDao, TodoItemDao, TodoProgressDao};
use tr_store::Workspace;

use crate::error::db;
use crate::{ServiceError, ServiceResult};

// ---------------------------------------------------------------- 校验

/// 去掉首尾空白、校验非空，并按**字符**（不是字节）截断到上限。
fn require_text(value: &str, empty_message: &str, max_chars: usize) -> ServiceResult<String> {
    let text = value.trim();
    if text.is_empty() {
        return Err(AppError::bad_request(empty_message).into());
    }
    Ok(tr_domain::util::truncate_chars(text, max_chars))
}

/// 日期：**空串允许**（= 没填），否则必须是严格的 `YYYY-MM-DD`。
///
/// 解析复用股票域那个严格解析器（日记的校验也走它），三处口径一致：
/// `2026/01/05` 与 `2026-1-5` 都会被拒绝。
fn normalize_date(value: &str) -> ServiceResult<String> {
    let text = value.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    match crate::stock::parse_strict_date(text) {
        Some((year, month, day)) => Ok(format!("{year:04}-{month:02}-{day:02}")),
        None => Err(AppError::bad_request("日期格式应为 YYYY-MM-DD").into()),
    }
}

/// 紧急度 / 重要度必须落在 `-5..=5`。
fn check_level(value: i32, label: &str) -> ServiceResult<()> {
    if !(consts::TODO_LEVEL_MIN..=consts::TODO_LEVEL_MAX).contains(&value) {
        return Err(AppError::bad_request(format!(
            "{label}需在 {} 到 {} 之间",
            consts::TODO_LEVEL_MIN,
            consts::TODO_LEVEL_MAX
        ))
        .into());
    }
    Ok(())
}

/// 事项的四个可变字段一起校验，返回归一化后的 `(标题, 开始, 截止)`。
fn normalize_item_fields(
    title: &str,
    start_date: &str,
    due_date: &str,
    urgency: i32,
    importance: i32,
) -> ServiceResult<(String, String, String)> {
    let title = require_text(title, "请输入事项", consts::TODO_ITEM_TITLE_MAX)?;
    let start = normalize_date(start_date)?;
    let due = normalize_date(due_date)?;
    // 归一化后都是 `YYYY-MM-DD`，字典序即时间序
    if !start.is_empty() && !due.is_empty() && start > due {
        return Err(AppError::bad_request("截止日期不能早于开始日期").into());
    }
    check_level(urgency, "紧急度")?;
    check_level(importance, "重要度")?;
    Ok((title, start, due))
}

/// 取行：不存在时报用户可见的 404 文案，其它数据库错误照原样冒泡。
fn require_row<T>(result: rusqlite::Result<T>, missing: &str) -> ServiceResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) if is_not_found(&error) => Err(AppError::not_found(missing).into()),
        Err(error) => Err(error.into()),
    }
}

/// 取卡片；不存在或不属于本账本时统一报「卡片不存在」。
fn card_of(conn: &Connection, ledger_id: &str, card_id: &str) -> ServiceResult<TodoCard> {
    let card = require_row(TodoCardDao::get(conn, card_id), "卡片不存在")?;
    if card.ledger_id != ledger_id {
        return Err(AppError::not_found("卡片不存在").into());
    }
    Ok(card)
}

/// 取事项；不存在或不属于本账本时统一报「事项不存在」。
fn item_of(conn: &Connection, ledger_id: &str, item_id: &str) -> ServiceResult<TodoItem> {
    let item = require_row(TodoItemDao::get(conn, item_id), "事项不存在")?;
    if item.ledger_id != ledger_id {
        return Err(AppError::not_found("事项不存在").into());
    }
    Ok(item)
}

// ---------------------------------------------------------------- 读

/// 某账本的进度记录按 `item_id` 分桶（一次查询而不是每事项一次）。
fn progress_by_item(
    conn: &Connection,
    ledger_id: &str,
) -> ServiceResult<HashMap<String, Vec<TodoProgressDto>>> {
    let mut buckets: HashMap<String, Vec<TodoProgressDto>> = HashMap::new();
    for progress in db(TodoProgressDao::list_by_ledger(conn, ledger_id))? {
        buckets
            .entry(progress.item_id.clone())
            .or_default()
            .push(TodoProgressDto::from(&progress));
    }
    Ok(buckets)
}

/// 卡片视图：卡片（含没有事项的）+ 每张卡片下**进行中**的事项（进度记录内嵌）。
pub fn list_cards(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Vec<TodoCardDto>> {
    let conn = workspace.connection();
    let mut buckets = progress_by_item(&conn, ledger_id)?;
    let cards = db(TodoCardDao::list_by_ledger(&conn, ledger_id))?;
    let mut result = Vec::with_capacity(cards.len());
    for card in cards {
        let items = db(TodoItemDao::list_by_card(
            &conn,
            &card.id,
            consts::TODO_STATUS_DOING,
        ))?;
        let items = items
            .iter()
            .map(|item| {
                let progress = buckets.remove(&item.id).unwrap_or_default();
                TodoItemDto::from_item(item, progress)
            })
            .collect();
        result.push(TodoCardDto::from_card(&card, items));
    }
    Ok(result)
}

/// 历史视图：**已完成**的事项（按完成时刻倒序）+ 主题名 + 进度记录。
pub fn list_history(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Vec<TodoHistoryDto>> {
    let conn = workspace.connection();
    let mut buckets = progress_by_item(&conn, ledger_id)?;
    let titles: HashMap<String, String> = db(TodoCardDao::list_by_ledger(&conn, ledger_id))?
        .into_iter()
        .map(|card| (card.id, card.title))
        .collect();
    let items = db(TodoItemDao::list_by_status(
        &conn,
        ledger_id,
        consts::TODO_STATUS_DONE,
    ))?;
    Ok(items
        .iter()
        .map(|item| {
            let card_title = titles.get(&item.card_id).cloned().unwrap_or_default();
            let progress = buckets.remove(&item.id).unwrap_or_default();
            TodoHistoryDto::from_item(item, &card_title, progress)
        })
        .collect())
}

// ---------------------------------------------------------------- 卡片

/// 新建卡片（主题）。同一账本允许重名 —— 主题是给人看的分类，不是主键。
pub fn create_card(
    workspace: &Workspace,
    ledger_id: &str,
    title: &str,
) -> ServiceResult<TodoCardDto> {
    let title = require_text(title, "请输入卡片主题", consts::TODO_CARD_TITLE_MAX)?;
    let card = TodoCard {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        title,
        created_at: 0,
        updated_at: 0,
    };
    db(TodoCardDao::create(&workspace.connection(), &card))?;
    // 回读一次拿 DAO 填的时间戳
    let saved = db(TodoCardDao::get(&workspace.connection(), &card.id))?;
    Ok(TodoCardDto::from_card(&saved, Vec::new()))
}

/// 删卡片：同一个事务里连它的事项与进度记录一起删。
///
/// 删除只按 id（与图表/模板/事件的删除同一约定）：界面上的行本来就来自当前账本，
/// 而 `IdRequest` 只带 id —— 不为此多定义一个请求结构。
pub fn delete_card(workspace: &Workspace, card_id: &str) -> ServiceResult<()> {
    if let Err(error) = workspace.transaction(|conn| {
        require_row(TodoCardDao::get(conn, card_id), "卡片不存在")?;
        db(TodoProgressDao::delete_by_card(conn, card_id))?;
        db(TodoItemDao::delete_by_card(conn, card_id))?;
        db(TodoCardDao::delete(conn, card_id))?;
        Ok(())
    }) {
        tracing::error!("删除待办卡片失败, card: {}, err: {}", card_id, error);
        return Err(error);
    }
    Ok(())
}

// ---------------------------------------------------------------- 事项

/// 在卡片下新建事项（新事项一律是「进行中」）。
#[allow(clippy::too_many_arguments)]
pub fn create_item(
    workspace: &Workspace,
    ledger_id: &str,
    card_id: &str,
    title: &str,
    start_date: &str,
    due_date: &str,
    urgency: i32,
    importance: i32,
) -> ServiceResult<TodoItemDto> {
    let (title, start, due) =
        normalize_item_fields(title, start_date, due_date, urgency, importance)?;
    let conn = workspace.connection();
    card_of(&conn, ledger_id, card_id)?;
    let item = TodoItem {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        card_id: card_id.to_string(),
        title,
        start_date: start,
        due_date: due,
        urgency,
        importance,
        status: consts::TODO_STATUS_DOING.to_string(),
        completed_at: 0,
        created_at: 0,
        updated_at: 0,
    };
    db(TodoItemDao::create(&conn, &item))?;
    let saved = db(TodoItemDao::get(&conn, &item.id))?;
    Ok(TodoItemDto::from_item(&saved, Vec::new()))
}

/// 编辑事项的正文与属性（**不动**状态；完成/取消完成走 [`set_item_status`]）。
#[allow(clippy::too_many_arguments)]
pub fn update_item(
    workspace: &Workspace,
    ledger_id: &str,
    item_id: &str,
    title: &str,
    start_date: &str,
    due_date: &str,
    urgency: i32,
    importance: i32,
) -> ServiceResult<TodoItemDto> {
    let (title, start, due) =
        normalize_item_fields(title, start_date, due_date, urgency, importance)?;
    let conn = workspace.connection();
    let mut item = item_of(&conn, ledger_id, item_id)?;
    item.title = title;
    item.start_date = start;
    item.due_date = due;
    item.urgency = urgency;
    item.importance = importance;
    db(TodoItemDao::update(&conn, &item))?;
    let saved = db(TodoItemDao::get(&conn, item_id))?;
    Ok(TodoItemDto::from_item(
        &saved,
        db(TodoProgressDao::list_by_ledger(&conn, ledger_id))?
            .iter()
            .filter(|progress| progress.item_id == item_id)
            .map(TodoProgressDto::from)
            .collect(),
    ))
}

/// 改状态：`done` 记下完成时刻（进历史），`doing` 清掉完成时刻（退回卡片）。
pub fn set_item_status(
    workspace: &Workspace,
    ledger_id: &str,
    item_id: &str,
    status: &str,
) -> ServiceResult<TodoItemDto> {
    if !consts::TODO_STATUSES.contains(&status) {
        return Err(AppError::bad_request("待办状态只能是进行中或已完成").into());
    }
    let conn = workspace.connection();
    item_of(&conn, ledger_id, item_id)?;
    let completed_at = if status == consts::TODO_STATUS_DONE {
        tr_store::util::now_unix()
    } else {
        0
    };
    db(TodoItemDao::update_status(
        &conn,
        item_id,
        status,
        completed_at,
    ))?;
    let saved = db(TodoItemDao::get(&conn, item_id))?;
    Ok(TodoItemDto::from_item(&saved, Vec::new()))
}

/// 删事项：同一个事务里连它的进度记录一起删。
/// 删事项（只按 id，见 [`delete_card`] 的说明）：同一个事务里连它的进度记录一起删。
pub fn delete_item(workspace: &Workspace, item_id: &str) -> ServiceResult<()> {
    if let Err(error) = workspace.transaction(|conn| {
        require_row(TodoItemDao::get(conn, item_id), "事项不存在")?;
        db(TodoProgressDao::delete_by_item(conn, item_id))?;
        db(TodoItemDao::delete(conn, item_id))?;
        Ok(())
    }) {
        tracing::error!("删除待办事项失败, item: {}, err: {}", item_id, error);
        return Err(error);
    }
    Ok(())
}

// ---------------------------------------------------------------- 进度记录

/// 追加一条进度记录（只增不写：要改就删了重记）。
pub fn add_progress(
    workspace: &Workspace,
    ledger_id: &str,
    item_id: &str,
    content: &str,
) -> ServiceResult<TodoProgressDto> {
    let content = require_text(content, "请输入进度", consts::TODO_PROGRESS_MAX)?;
    let conn = workspace.connection();
    item_of(&conn, ledger_id, item_id)?;
    let progress = TodoProgress {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        item_id: item_id.to_string(),
        content,
        created_at: 0,
    };
    db(TodoProgressDao::create(&conn, &progress))?;
    let saved = db(TodoProgressDao::list_by_ledger(&conn, ledger_id))?
        .into_iter()
        .find(|row| row.id == progress.id)
        .ok_or_else(|| ServiceError::Internal("写入进度记录后未取到该行".into()))?;
    Ok(TodoProgressDto::from(&saved))
}

/// 删一条进度记录（只按 id，见 [`delete_card`] 的说明）。
pub fn delete_progress(workspace: &Workspace, progress_id: &str) -> ServiceResult<()> {
    let conn = workspace.connection();
    let affected = db(TodoProgressDao::delete(&conn, progress_id))?;
    if affected == 0 {
        return Err(AppError::not_found("进度记录不存在").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;

    const LEDGER: &str = "ledger-todo";

    fn card_id(workspace: &Workspace, title: &str) -> String {
        create_card(workspace, LEDGER, title).unwrap().id
    }

    /// 建卡片 → 加事项 → 加两条进度 → 完成：事项离开卡片视图、带着主题名进历史、进度跟着走。
    #[test]
    fn item_moves_from_card_to_history_with_progress() {
        let (workspace, dir) = workspace("todo-flow");
        let card = card_id(&workspace, "季度目标");

        let item = create_item(
            &workspace,
            LEDGER,
            &card,
            "写完季度复盘",
            "2026-09-01",
            "2026-09-30",
            3,
            -2,
        )
        .unwrap();
        add_progress(&workspace, LEDGER, &item.id, "列了大纲").unwrap();
        add_progress(&workspace, LEDGER, &item.id, "写完前两节").unwrap();

        // 卡片视图：这张卡片下有一条进行中的事项，进度记录两条
        let cards = list_cards(&workspace, LEDGER).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "季度目标");
        assert_eq!(cards[0].items.len(), 1);
        assert_eq!(cards[0].items[0].title, "写完季度复盘");
        assert_eq!(cards[0].items[0].urgency, 3);
        assert_eq!(cards[0].items[0].importance, -2);
        assert_eq!(cards[0].items[0].progress.len(), 2);
        assert!(list_history(&workspace, LEDGER).unwrap().is_empty());

        // 完成 → 卡片视图空、历史里带上主题名与进度
        set_item_status(&workspace, LEDGER, &item.id, consts::TODO_STATUS_DONE).unwrap();
        let cards = list_cards(&workspace, LEDGER).unwrap();
        assert_eq!(cards.len(), 1, "卡片本身还在");
        assert!(cards[0].items.is_empty(), "完成的事项不再出现在卡片里");

        let history = list_history(&workspace, LEDGER).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].card_title, "季度目标");
        assert_eq!(history[0].title, "写完季度复盘");
        assert_eq!(history[0].progress.len(), 2);
        assert_eq!(history[0].progress[0].content, "列了大纲");
        assert!(history[0].completed_at > 0);

        // 退回进行中：又回到卡片，完成时刻清掉
        set_item_status(&workspace, LEDGER, &item.id, consts::TODO_STATUS_DOING).unwrap();
        assert!(list_history(&workspace, LEDGER).unwrap().is_empty());
        let cards = list_cards(&workspace, LEDGER).unwrap();
        assert_eq!(cards[0].items.len(), 1);
        assert_eq!(cards[0].items[0].completed_at, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 校验与隔离：空文本、越界的紧急度/重要度、倒挂的日期、跨账本的行。
    #[test]
    fn validation_and_ledger_isolation() {
        let (workspace, dir) = workspace("todo-validate");
        let card = card_id(&workspace, "主题");

        assert!(create_card(&workspace, LEDGER, "   ").is_err());
        assert!(create_item(&workspace, LEDGER, &card, "", "", "", 0, 0).is_err());
        assert!(create_item(&workspace, LEDGER, &card, "事项", "", "", 6, 0).is_err());
        assert!(create_item(&workspace, LEDGER, &card, "事项", "", "", 0, -6).is_err());
        assert!(create_item(&workspace, LEDGER, &card, "事项", "2026/09/01", "", 0, 0).is_err());
        assert!(
            create_item(
                &workspace,
                LEDGER,
                &card,
                "事项",
                "2026-09-30",
                "2026-09-01",
                0,
                0
            )
            .is_err(),
            "截止不能早于开始"
        );
        assert!(set_item_status(&workspace, LEDGER, "nope", "finished").is_err());

        // 别的账本看不见、也删不掉
        let item = create_item(&workspace, LEDGER, &card, "事项", "", "", 0, 0).unwrap();
        assert!(list_cards(&workspace, "other-ledger").unwrap().is_empty());
        // 删除只按 id（界面上的行本来就来自当前账本），这里删掉之后再确认卡片视图空了
        delete_item(&workspace, &item.id).unwrap();
        assert!(create_item(&workspace, "other-ledger", &card, "事项", "", "", 0, 0).is_err());
        assert!(list_cards(&workspace, LEDGER).unwrap()[0].items.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 删卡片：事项与进度记录一起走（不留孤儿行）。
    #[test]
    fn deleting_a_card_removes_its_items_and_progress() {
        let (workspace, dir) = workspace("todo-delete-card");
        let card = card_id(&workspace, "主题");
        let item = create_item(&workspace, LEDGER, &card, "事项", "", "", 0, 0).unwrap();
        let progress = add_progress(&workspace, LEDGER, &item.id, "进度一").unwrap();

        delete_card(&workspace, &card).unwrap();

        let conn = workspace.connection();
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_todo_item", [], |row| {
                row.get(0)
            })
            .unwrap();
        let progress_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tbl_billadm_todo_progress",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(items, 0);
        assert_eq!(progress_rows, 0, "进度记录不能留下孤儿行");
        drop(conn);

        // 已经删掉的卡片再删一次 → 报「卡片不存在」
        assert!(delete_card(&workspace, &card).is_err());
        // 已删除的事项的进度也删不掉
        assert!(delete_progress(&workspace, &progress.id).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
