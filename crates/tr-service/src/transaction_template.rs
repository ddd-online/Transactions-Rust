//! 消费模板服务：新建 / 删除 / 列出模板与排序号更新。
//!
//! 设计说明：
//! * 没有单独的绑定层，校验放在 `create` 入口（这样 headless 测试能覆盖到文案）。
//! * 校验失败经内部错误通道兜底为 **500**（不是 400），
//!   因此这里把校验失败收敛为 [`ServiceError::Internal`]，文案逐字保留。

use tr_domain::dto::TransactionTemplateDto;
use tr_store::dao::transaction_template::TransactionTemplateDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 新建模板，返回新模板 ID（模板 ID 由服务端生成，忽略请求体里的 `template_id`）。
pub fn create(workspace: &Workspace, dto: &TransactionTemplateDto) -> ServiceResult<String> {
    validate(dto)?;

    let template_id = tr_store::util::new_uuid();
    let conn = workspace.connection();

    let max_sort_order =
        TransactionTemplateDao::get_max_sort(&conn, &dto.ledger_id).map_err(|error| {
            tracing::error!("获取最大排序号失败: {}", error);
            ServiceError::from(error)
        })?;

    let mut record = dto.to_template();
    record.template_id = template_id.clone();
    record.sort_order = max_sort_order + 1;

    TransactionTemplateDao::create(&conn, &record).map_err(|error| {
        tracing::error!("创建交易模板失败: {}", error);
        ServiceError::from(error)
    })?;

    Ok(template_id)
}

/// 删除模板。
pub fn delete_by_id(workspace: &Workspace, template_id: &str) -> ServiceResult<()> {
    TransactionTemplateDao::delete_by_id(&workspace.connection(), template_id).map_err(|error| {
        tracing::error!("删除交易模板失败: {}", error);
        ServiceError::from(error)
    })
}

/// 某账本的全部模板（`sort_order ASC, created_at DESC`）。
pub fn list_by_ledger_id(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<Vec<TransactionTemplateDto>> {
    let templates = TransactionTemplateDao::query_by_ledger_id(&workspace.connection(), ledger_id)
        .map_err(ServiceError::from)?;

    Ok(templates
        .iter()
        .map(TransactionTemplateDto::from_template)
        .collect())
}

/// 更新模板排序号（`ledger_id` 不参与 SQL，仅保留在接口上）。
pub fn update_sort_order(
    workspace: &Workspace,
    template_id: &str,
    _ledger_id: &str,
    sort_order: i32,
) -> ServiceResult<()> {
    TransactionTemplateDao::update_sort(&workspace.connection(), template_id, sort_order).map_err(
        |error| {
            tracing::error!("更新模板排序失败: {}", error);
            ServiceError::from(error)
        },
    )
}

/// 模板校验。
fn validate(dto: &TransactionTemplateDto) -> ServiceResult<()> {
    dto.validate()
        .map_err(|error| ServiceError::Internal(error.msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;
    use tr_store::dao::transaction_template::TransactionTemplateDao as Dao;

    fn dto(name: &str) -> TransactionTemplateDto {
        TransactionTemplateDto {
            ledger_id: "l1".to_string(),
            template_name: name.to_string(),
            transaction_type: "expense".to_string(),
            category: "餐饮美食".to_string(),
            tags: vec!["三餐".to_string(), "外卖".to_string()],
            flags: "flag".to_string(),
            description: "午餐".to_string(),
            ..TransactionTemplateDto::default()
        }
    }

    #[test]
    fn create_assigns_uuid_and_sort_order_and_lists_back() {
        let (workspace, dir) = workspace("create");
        let first = create(&workspace, &dto("模板甲")).unwrap();
        let second = create(&workspace, &dto("模板乙")).unwrap();
        assert_ne!(first, second, "模板 ID 由服务端生成");

        let templates = list_by_ledger_id(&workspace, "l1").unwrap();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].template_id, first);
        assert_eq!(templates[0].sort_order, 1);
        assert_eq!(templates[1].sort_order, 2);
        assert_eq!(
            templates[0].tags,
            vec!["三餐".to_string(), "外卖".to_string()]
        );
        assert_eq!(templates[0].flags, "flag");
        assert_eq!(templates[0].description, "午餐");
        assert_eq!(templates[0].category, "餐饮美食");

        // 落库的 tags 是 JSON 数组字符串
        let stored = Dao::query_by_ledger_id(&workspace.connection(), "l1").unwrap();
        assert_eq!(stored[0].tags, r#"["三餐","外卖"]"#);
        assert!(stored[0].created_at > 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_validates_with_stable_messages_and_500() {
        let (workspace, dir) = workspace("validate");

        let cases = [
            (TransactionTemplateDto::default(), "模板名称不能为空"),
            (
                TransactionTemplateDto {
                    template_name: "模板".to_string(),
                    transaction_type: "bad".to_string(),
                    ..TransactionTemplateDto::default()
                },
                "invalid transaction type: bad",
            ),
            (
                TransactionTemplateDto {
                    template_name: "模板".to_string(),
                    transaction_type: "expense".to_string(),
                    ..TransactionTemplateDto::default()
                },
                "分类不能为空",
            ),
        ];

        for (request, expected) in cases {
            let error = create(&workspace, &request).unwrap_err();
            assert_eq!(error.to_string(), expected);
            // 校验失败经内部错误通道 → 兜底 500
            assert_eq!(error.into_app_error().status, 500, "文案: {expected}");
        }

        // 校验失败不得落库
        assert!(Dao::query_by_ledger_id(&workspace.connection(), "l1")
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_sort_order_refreshes_updated_at() {
        let (workspace, dir) = workspace("sort");
        let id = create(&workspace, &dto("模板")).unwrap();
        let created = Dao::query_by_ledger_id(&workspace.connection(), "l1")
            .unwrap()
            .remove(0);

        std::thread::sleep(std::time::Duration::from_millis(1100));
        update_sort_order(&workspace, &id, "l1", 5).unwrap();

        let updated = Dao::query_by_ledger_id(&workspace.connection(), "l1")
            .unwrap()
            .remove(0);
        assert_eq!(updated.sort_order, 5);
        assert_eq!(updated.created_at, created.created_at);
        assert!(updated.updated_at > created.updated_at);
        // 不存在的模板：不报错
        update_sort_order(&workspace, "absent", "l1", 1).unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_by_id_only_removes_that_template() {
        let (workspace, dir) = workspace("delete");
        let first = create(&workspace, &dto("甲")).unwrap();
        let _second = create(&workspace, &dto("乙")).unwrap();

        delete_by_id(&workspace, &first).unwrap();
        let templates = list_by_ledger_id(&workspace, "l1").unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].template_name, "乙");

        // 不存在的 ID 视为成功（删除 0 行不报错）
        delete_by_id(&workspace, "absent").unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }
}
