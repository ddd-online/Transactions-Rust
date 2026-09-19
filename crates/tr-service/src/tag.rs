//! 标签服务：标签的查询 / 新建 / 删除 / 排序与关联交易数统计。
//!
//! 本模块是自由函数 + `&Workspace`（与 `ledger.rs` 一致），不做接口抽象。
//! 删除标签时"先删交易记录上的关联、再删标签本身"的顺序与事务边界是硬约定。

use std::collections::BTreeMap;

use tr_domain::models::Tag;
use tr_store::dao::tag::TagDao;
use tr_store::dao::transaction_record_tag::TrTagDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 查询标签（`category_transaction_type` 为空或 `all` 时不过滤）。
pub fn query_tags(
    workspace: &Workspace,
    ledger_id: &str,
    category_transaction_type: &str,
) -> ServiceResult<Vec<Tag>> {
    TagDao::query_by_ledger(
        &workspace.connection(),
        ledger_id,
        category_transaction_type,
    )
    .map_err(ServiceError::from)
}

/// 新建标签：排序号取当前最大值 + 1。
pub fn create_tag(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    category_transaction_type: &str,
) -> ServiceResult<()> {
    let conn = workspace.connection();

    let max_sort_order = TagDao::get_max_sort(&conn, ledger_id, category_transaction_type)
        .map_err(|error| {
            tracing::error!("获取最大排序号失败: {}", error);
            ServiceError::from(error)
        })?;

    let tag = Tag {
        ledger_id: ledger_id.to_string(),
        name: name.to_string(),
        category_transaction_type: category_transaction_type.to_string(),
        sort_order: max_sort_order + 1,
    };

    TagDao::create(&conn, &tag).map_err(|error| {
        tracing::error!("创建标签失败: {}", error);
        ServiceError::from(error)
    })
}

/// 删除某分类（`分类名:交易类型`）下的全部标签
/// （分类服务删除分类时一并调用）。
pub fn delete_tags_by_category(
    workspace: &Workspace,
    ledger_id: &str,
    category_transaction_type: &str,
) -> ServiceResult<()> {
    TagDao::delete_by_category(
        &workspace.connection(),
        ledger_id,
        category_transaction_type,
    )
    .map_err(ServiceError::from)
}

/// 删除标签：先删交易记录上的该标签关联，再删标签本身（单事务）。
pub fn delete_tag(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    category_transaction_type: &str,
) -> ServiceResult<()> {
    workspace
        .transaction(|conn| {
            TrTagDao::delete_by_tag(conn, ledger_id, name)?;
            TagDao::delete(conn, ledger_id, name, category_transaction_type)?;
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("删除标签失败: {}", error);
            error
        })
}

/// 更新标签排序号。
pub fn update_tag_sort(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    category_transaction_type: &str,
    sort_order: i32,
) -> ServiceResult<()> {
    TagDao::update_sort(
        &workspace.connection(),
        ledger_id,
        name,
        category_transaction_type,
        sort_order,
    )
    .map_err(|error| {
        tracing::error!("更新标签排序失败: {}", error);
        ServiceError::from(error)
    })
}

/// 单个标签名下的关联交易数。
pub fn count_records_by_tag(
    workspace: &Workspace,
    ledger_id: &str,
    tag: &str,
) -> ServiceResult<i64> {
    TagDao::count_by_tag(&workspace.connection(), ledger_id, tag).map_err(ServiceError::from)
}

/// 批量统计每个标签名下的关联交易数（空名单直接返回空 map）。
pub fn count_records_by_tags(
    workspace: &Workspace,
    ledger_id: &str,
    names: &[String],
) -> ServiceResult<BTreeMap<String, i64>> {
    TagDao::count_records_by_tags(&workspace.connection(), ledger_id, names)
        .map_err(ServiceError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::models::TrTag;

    fn workspace(tag: &str) -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-tag-service-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

    fn tr_tag(ledger_id: &str, transaction_id: &str, tag: &str) -> TrTag {
        TrTag {
            ledger_id: ledger_id.to_string(),
            transaction_id: transaction_id.to_string(),
            tag: tag.to_string(),
        }
    }

    #[test]
    fn create_tag_appends_to_max_sort_per_category_and_rejects_duplicate() {
        let (workspace, dir) = workspace("create");
        create_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap();
        create_tag(&workspace, "l1", "外卖", "餐饮美食:expense").unwrap();
        create_tag(&workspace, "l1", "工资", "工资奖金:income").unwrap();

        let conn = workspace.connection();
        let dining = TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense").unwrap();
        assert_eq!(dining.len(), 2);
        assert_eq!(dining[0].sort_order, 1);
        assert_eq!(dining[1].sort_order, 2);
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "工资奖金:income").unwrap()[0].sort_order,
            1
        );

        let error = create_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_tag_removes_record_associations_first() {
        let (workspace, dir) = workspace("delete");
        create_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap();
        create_tag(&workspace, "l1", "外卖", "餐饮美食:expense").unwrap();
        {
            let conn = workspace.connection();
            TrTagDao::create_batch(
                &conn,
                &[
                    tr_tag("l1", "t1", "三餐"),
                    tr_tag("l1", "t1", "外卖"),
                    tr_tag("l1", "t2", "三餐"),
                    tr_tag("l2", "t3", "三餐"),
                ],
            )
            .unwrap();
        }

        delete_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap();

        let conn = workspace.connection();
        assert_eq!(TagDao::count_by_tag(&conn, "l1", "三餐").unwrap(), 0);
        assert_eq!(TagDao::count_by_tag(&conn, "l1", "外卖").unwrap(), 1);
        // 其它账本的关联不受影响（删除同样带 ledger_id 条件）
        assert_eq!(TagDao::count_by_tag(&conn, "l2", "三餐").unwrap(), 1);
        let remaining = TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "外卖");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_tags_by_category_only_touches_that_category() {
        let (workspace, dir) = workspace("delete-category");
        create_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap();
        create_tag(&workspace, "l1", "外卖", "餐饮美食:expense").unwrap();
        create_tag(&workspace, "l1", "工资", "工资奖金:income").unwrap();
        create_tag(&workspace, "l2", "三餐", "餐饮美食:expense").unwrap();

        delete_tags_by_category(&workspace, "l1", "餐饮美食:expense").unwrap();

        let conn = workspace.connection();
        assert!(TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense")
            .unwrap()
            .is_empty());
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "工资奖金:income")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l2", "餐饮美食:expense")
                .unwrap()
                .len(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_sort_is_silent_for_missing_row() {
        let (workspace, dir) = workspace("sort");
        create_tag(&workspace, "l1", "三餐", "餐饮美食:expense").unwrap();
        update_tag_sort(&workspace, "l1", "三餐", "餐饮美食:expense", 6).unwrap();
        assert_eq!(
            TagDao::query_by_ledger(&workspace.connection(), "l1", "餐饮美食:expense").unwrap()[0]
                .sort_order,
            6
        );
        update_tag_sort(&workspace, "l1", "不存在", "餐饮美食:expense", 1).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_counts_are_batched_by_name() {
        let (workspace, dir) = workspace("count");
        {
            let conn = workspace.connection();
            TrTagDao::create_batch(
                &conn,
                &[
                    tr_tag("l1", "t1", "三餐"),
                    tr_tag("l1", "t1", "外卖"),
                    tr_tag("l1", "t2", "三餐"),
                ],
            )
            .unwrap();
        }

        assert_eq!(count_records_by_tag(&workspace, "l1", "三餐").unwrap(), 2);

        let names = vec!["三餐".to_string(), "外卖".to_string(), "不存在".to_string()];
        let counts = count_records_by_tags(&workspace, "l1", &names).unwrap();
        assert_eq!(counts["三餐"], 2);
        assert_eq!(counts["外卖"], 1);
        assert_eq!(counts.get("不存在"), None);
        assert!(count_records_by_tags(&workspace, "l1", &[])
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
