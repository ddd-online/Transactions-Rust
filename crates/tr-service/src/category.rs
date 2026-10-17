//! 分类服务：分类的查询 / 新建 / 删除 / 排序，分类下的记录数统计，
//! 以及默认分类与标签的种子数据。
//!
//! ## 设计说明
//!
//! * 本 crate 不用接口抽象与依赖注入：全部是**无状态自由函数 + `&Workspace` 入参**
//!   （与 `ledger.rs` 一致）。
//! * 种子数据放在唯一调用方（分类服务）里，经 `CategoryDao` / `TagDao` 写入；
//!   按字面量的书写顺序固定遍历（不依赖 map 的随机顺序，落库结果不受影响：
//!   所有 `sort_order` 都是 0，列表统一按 `sort_order ASC, name DESC` 排序）。
//! * 分类下的记录数统计（两条只读 SQL）放在 `CategoryDao`
//!   （消费记录 DAO 由另一处负责）。

use std::collections::BTreeMap;

use tr_domain::models::{Category, Tag};
use tr_store::dao::category::CategoryDao;
use tr_store::dao::tag::TagDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 一个默认分类：`(分类名, 标签名列表)`。
pub type DefaultCategory = (&'static str, &'static [&'static str]);

/// 一个交易类型下的默认分类集合：`(交易类型, 分类列表)`。
pub type DefaultCategoryGroup = (&'static str, &'static [DefaultCategory]);

/// 默认分类与标签（逐字固定，改动即破坏既有工作空间的数据兼容）：
/// 交易类型 → 分类名 → 标签名列表。共 19 个分类 / 57 个标签。
pub const DEFAULT_DATA: &[DefaultCategoryGroup] = &[
    (
        "expense",
        &[
            (
                "餐饮美食",
                &[
                    "三餐", "零食", "商场", "外卖", "饮料", "奶茶", "咖啡", "水果", "茶叶", "买菜",
                ],
            ),
            (
                "购物消费",
                &[
                    "衣物", "数码", "家居", "书籍", "礼物", "玩具", "宠物", "游戏", "快递", "彩票",
                    "电影", "运动", "酒店", "烟酒", "充值", "汽车", "还款",
                ],
            ),
            (
                "交通出行",
                &[
                    "打车", "地铁", "公交", "高铁", "油费", "停车", "ETC", "车险",
                ],
            ),
            (
                "生活缴费",
                &[
                    "房租", "物业", "燃气", "水费", "电费", "通讯", "还款", "网费", "理发",
                ],
            ),
            ("贷款还款", &[]),
            ("医疗健康", &["医药", "医险"]),
            ("娱乐休闲", &[]),
            ("人情往来", &["红包", "请客", "礼金"]),
            ("教育学习", &[]),
        ],
    ),
    (
        "income",
        &[
            ("工资奖金", &["工资", "奖金"]),
            ("补贴补助", &[]),
            ("退税退款", &[]),
            ("二手转卖", &[]),
            ("彩票收入", &[]),
            ("投资理财", &[]),
            ("借贷借款", &[]),
            ("红包转账", &[]),
        ],
    ),
    (
        "transfer",
        &[
            ("五险一金", &["养老", "医疗", "失业", "住房"]),
            ("税费党费", &["团费", "交税"]),
        ],
    ),
];

/// 写入默认分类与标签，返回 `(分类数, 标签数)`。
///
/// **不使用事务**：逐行插入，任一行失败即整体报错（已插入的行保留）。
pub fn seed_default_data(workspace: &Workspace, ledger_id: &str) -> ServiceResult<(i32, i32)> {
    let conn = workspace.connection();
    let mut category_count = 0;
    let mut tag_count = 0;

    for (transaction_type, categories) in DEFAULT_DATA {
        for (category_name, tags) in *categories {
            let category = Category {
                ledger_id: ledger_id.to_string(),
                name: (*category_name).to_string(),
                transaction_type: (*transaction_type).to_string(),
                sort_order: 0,
            };
            CategoryDao::create(&conn, &category).map_err(|error| {
                tracing::error!("创建分类失败: {}", error);
                ServiceError::from(error)
            })?;
            category_count += 1;

            let category_transaction_type = format!("{category_name}:{transaction_type}");
            for tag_name in *tags {
                let tag = Tag {
                    ledger_id: ledger_id.to_string(),
                    name: (*tag_name).to_string(),
                    category_transaction_type: category_transaction_type.clone(),
                    sort_order: 0,
                };
                TagDao::create(&conn, &tag).map_err(|error| {
                    tracing::error!("创建标签失败: {}", error);
                    ServiceError::from(error)
                })?;
                tag_count += 1;
            }
        }
    }

    Ok((category_count, tag_count))
}

/// 查询分类（`transaction_type` 为空或 `all` 时不过滤）。
pub fn query_category(
    workspace: &Workspace,
    ledger_id: &str,
    transaction_type: &str,
) -> ServiceResult<Vec<Category>> {
    CategoryDao::query_by_ledger(&workspace.connection(), ledger_id, transaction_type)
        .map_err(ServiceError::from)
}

/// 新建分类：排序号取当前最大值 + 1。
pub fn create_category(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    transaction_type: &str,
) -> ServiceResult<()> {
    let conn = workspace.connection();

    let max_sort_order =
        CategoryDao::get_max_sort(&conn, ledger_id, transaction_type).map_err(|error| {
            tracing::error!("获取最大排序号失败: {}", error);
            ServiceError::from(error)
        })?;

    let category = Category {
        ledger_id: ledger_id.to_string(),
        name: name.to_string(),
        transaction_type: transaction_type.to_string(),
        sort_order: max_sort_order + 1,
    };

    CategoryDao::create(&conn, &category).map_err(|error| {
        tracing::error!("创建分类失败: {}", error);
        ServiceError::from(error)
    })
}

/// 删除分类，并在**同一事务**内连带删除该分类下的标签（`分类名:交易类型`）。
pub fn delete_category(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    transaction_type: &str,
) -> ServiceResult<()> {
    let category_transaction_type = format!("{name}:{transaction_type}");

    workspace
        .transaction(|conn| {
            TagDao::delete_by_category(conn, ledger_id, &category_transaction_type)?;
            CategoryDao::delete(conn, ledger_id, name, transaction_type)?;
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("删除分类失败: {}", error);
            error
        })
}

/// 更新分类排序号。
pub fn update_category_sort(
    workspace: &Workspace,
    ledger_id: &str,
    name: &str,
    transaction_type: &str,
    sort_order: i32,
) -> ServiceResult<()> {
    CategoryDao::update_sort(
        &workspace.connection(),
        ledger_id,
        name,
        transaction_type,
        sort_order,
    )
    .map_err(|error| {
        tracing::error!("更新分类排序失败: {}", error);
        ServiceError::from(error)
    })
}

/// 单个分类名下的交易记录数。
pub fn count_records_by_category(
    workspace: &Workspace,
    ledger_id: &str,
    category: &str,
) -> ServiceResult<i64> {
    CategoryDao::count_records_by_category(&workspace.connection(), ledger_id, category)
        .map_err(ServiceError::from)
}

/// 批量统计每个分类名下的交易记录数（空名单直接返回空 map）。
pub fn count_records_by_categories(
    workspace: &Workspace,
    ledger_id: &str,
    names: &[String],
) -> ServiceResult<BTreeMap<String, i64>> {
    CategoryDao::count_records_by_categories(&workspace.connection(), ledger_id, names)
        .map_err(ServiceError::from)
}

/// 为账本初始化默认分类与标签，返回 `(分类数, 标签数)`。
///
/// 已有分类时报错（文案："该账本已有分类，无需初始化"）。
/// 注意该错误经内部错误通道兜底为 500，不是 400，
/// 因此这里用 [`ServiceError::Internal`]。
pub fn initialize_categories(workspace: &Workspace, ledger_id: &str) -> ServiceResult<(i32, i32)> {
    tracing::info!("开始初始化账本 {} 的分类", ledger_id);

    let count =
        CategoryDao::count_by_ledger_id(&workspace.connection(), ledger_id).map_err(|error| {
            tracing::error!("检查分类是否存在失败: {}", error);
            ServiceError::from(error)
        })?;
    if count > 0 {
        return Err(ServiceError::Internal(
            "该账本已有分类，无需初始化".to_string(),
        ));
    }

    let (category_count, tag_count) = seed_default_data(workspace, ledger_id).map_err(|error| {
        tracing::error!("初始化分类失败: {}", error);
        error
    })?;

    tracing::info!(
        "初始化分类成功, 账本: {}, 分类: {}, 标签: {}",
        ledger_id,
        category_count,
        tag_count
    );
    Ok((category_count, tag_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;

    #[test]
    fn default_data_has_expected_shape() {
        let categories: usize = DEFAULT_DATA.iter().map(|(_, items)| items.len()).sum();
        let tags: usize = DEFAULT_DATA
            .iter()
            .flat_map(|(_, items)| items.iter())
            .map(|(_, names)| names.len())
            .sum();
        assert_eq!(categories, 19, "默认分类数必须是 19");
        assert_eq!(tags, 57, "默认标签数必须是 57");

        // 关键条目：分类归属的交易类型、`分类名:交易类型` 的标签关联格式
        let expense = DEFAULT_DATA
            .iter()
            .find(|(transaction_type, _)| *transaction_type == "expense")
            .unwrap()
            .1;
        let dining = expense
            .iter()
            .find(|(name, _)| *name == "餐饮美食")
            .unwrap()
            .1;
        assert_eq!(
            *dining,
            [
                "三餐", "零食", "商场", "外卖", "饮料", "奶茶", "咖啡", "水果", "茶叶", "买菜"
            ]
        );
        // 「还款」同时属于两个分类（各自一条标签记录，靠 category_transaction_type 区分）
        assert!(expense
            .iter()
            .any(|(name, tags)| *name == "购物消费" && tags.contains(&"还款")));
        assert!(expense
            .iter()
            .any(|(name, tags)| *name == "生活缴费" && tags.contains(&"还款")));

        let transfer = DEFAULT_DATA
            .iter()
            .find(|(transaction_type, _)| *transaction_type == "transfer")
            .unwrap()
            .1;
        assert!(transfer
            .iter()
            .any(|(name, tags)| *name == "五险一金" && *tags == ["养老", "医疗", "失业", "住房"]));
    }

    /// 默认种子数据的规范化导出
    /// （`交易类型|分类名|标签,标签`，交易类型与分类名按 UTF-8 字节序排序，标签保持字面量顺序）。
    ///
    /// 以下期望值来自一次真实运行，作为回归基线，不要手改：
    /// 任何名字、归属或顺序上的偏差都会让下面的测试失败。
    const DEFAULT_DATA_DUMP: &str = "\
expense|交通出行|打车,地铁,公交,高铁,油费,停车,ETC,车险
expense|人情往来|红包,请客,礼金
expense|医疗健康|医药,医险
expense|娱乐休闲|
expense|教育学习|
expense|生活缴费|房租,物业,燃气,水费,电费,通讯,还款,网费,理发
expense|购物消费|衣物,数码,家居,书籍,礼物,玩具,宠物,游戏,快递,彩票,电影,运动,酒店,烟酒,充值,汽车,还款
expense|贷款还款|
expense|餐饮美食|三餐,零食,商场,外卖,饮料,奶茶,咖啡,水果,茶叶,买菜
income|二手转卖|
income|借贷借款|
income|工资奖金|工资,奖金
income|彩票收入|
income|投资理财|
income|红包转账|
income|补贴补助|
income|退税退款|
transfer|五险一金|养老,医疗,失业,住房
transfer|税费党费|团费,交税";

    #[test]
    fn default_data_matches_seed_dump_verbatim() {
        let mut rows: Vec<(String, String, String)> = Vec::new();
        for (transaction_type, categories) in DEFAULT_DATA {
            for (name, tags) in *categories {
                rows.push((
                    (*transaction_type).to_string(),
                    (*name).to_string(),
                    tags.join(","),
                ));
            }
        }
        rows.sort();

        let dump = rows
            .iter()
            .map(|(transaction_type, name, tags)| format!("{transaction_type}|{name}|{tags}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(dump, DEFAULT_DATA_DUMP);
    }

    #[test]
    fn initialize_seeds_categories_and_tags() {
        let (workspace, dir) = workspace("initialize");
        let (category_count, tag_count) = initialize_categories(&workspace, "l1").unwrap();
        assert_eq!(category_count, 19);
        assert_eq!(tag_count, 57);

        let conn = workspace.connection();
        let categories = CategoryDao::query_by_ledger(&conn, "l1", "all").unwrap();
        assert_eq!(categories.len(), 19);
        assert!(categories.iter().all(|item| item.sort_order == 0));
        assert!(categories
            .iter()
            .any(|item| item.name == "餐饮美食" && item.transaction_type == "expense"));
        assert!(categories
            .iter()
            .any(|item| item.name == "五险一金" && item.transaction_type == "transfer"));

        let dining = TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense").unwrap();
        assert_eq!(dining.len(), 10);
        assert!(dining.iter().any(|item| item.name == "三餐"));
        assert!(TagDao::query_by_ledger(&conn, "l1", "工资奖金:income")
            .unwrap()
            .iter()
            .any(|item| item.name == "工资"));
        // 「还款」在两个分类下各自存在
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "购物消费:expense")
                .unwrap()
                .iter()
                .filter(|item| item.name == "还款")
                .count(),
            1
        );
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "生活缴费:expense")
                .unwrap()
                .iter()
                .filter(|item| item.name == "还款")
                .count(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn initialize_twice_returns_same_message_with_500() {
        let (workspace, dir) = workspace("initialize-twice");
        initialize_categories(&workspace, "l1").unwrap();

        let error = initialize_categories(&workspace, "l1").unwrap_err();
        assert_eq!(error.to_string(), "该账本已有分类，无需初始化");
        assert_eq!(error.into_app_error().status, 500, "普通 error 兜底为 500");
        // 第二次不得重复插入
        assert_eq!(
            CategoryDao::count_by_ledger_id(&workspace.connection(), "l1").unwrap(),
            19
        );

        // 其它账本不受影响
        let (other_count, _) = initialize_categories(&workspace, "l2").unwrap();
        assert_eq!(other_count, 19);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_category_appends_to_max_sort_and_rejects_duplicate() {
        let (workspace, dir) = workspace("create");
        create_category(&workspace, "l1", "餐饮美食", "expense").unwrap();
        create_category(&workspace, "l1", "购物消费", "expense").unwrap();
        create_category(&workspace, "l1", "工资奖金", "income").unwrap();

        let conn = workspace.connection();
        let expense = CategoryDao::query_by_ledger(&conn, "l1", "expense").unwrap();
        assert_eq!(expense.len(), 2);
        assert_eq!(expense[0].sort_order, 1, "先建的排在前面");
        assert_eq!(expense[1].sort_order, 2, "排序号 = 最大值 + 1");
        // 交易类型各自独立计数
        assert_eq!(
            CategoryDao::query_by_ledger(&conn, "l1", "income").unwrap()[0].sort_order,
            1
        );

        let error = create_category(&workspace, "l1", "餐饮美食", "expense").unwrap_err();
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "error = {error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_category_cascades_tags_in_one_transaction() {
        let (workspace, dir) = workspace("delete");
        initialize_categories(&workspace, "l1").unwrap();
        let conn = workspace.connection();
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense")
                .unwrap()
                .len(),
            10
        );

        delete_category(&workspace, "l1", "餐饮美食", "expense").unwrap();

        assert!(!CategoryDao::query_by_ledger(&conn, "l1", "all")
            .unwrap()
            .iter()
            .any(|item| item.name == "餐饮美食" && item.transaction_type == "expense"));
        assert!(TagDao::query_by_ledger(&conn, "l1", "餐饮美食:expense")
            .unwrap()
            .is_empty());
        // 其它分类的标签不受影响
        assert_eq!(
            TagDao::query_by_ledger(&conn, "l1", "工资奖金:income")
                .unwrap()
                .len(),
            2
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_sort_is_silent_for_missing_row() {
        let (workspace, dir) = workspace("sort");
        create_category(&workspace, "l1", "餐饮美食", "expense").unwrap();
        update_category_sort(&workspace, "l1", "餐饮美食", "expense", 9).unwrap();
        assert_eq!(
            CategoryDao::query_by_ledger(&workspace.connection(), "l1", "expense").unwrap()[0]
                .sort_order,
            9
        );
        // 不存在的记录：不报错
        update_category_sort(&workspace, "l1", "不存在", "expense", 1).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_counts_are_batched_by_name() {
        let (workspace, dir) = workspace("count");
        create_category(&workspace, "l1", "餐饮美食", "expense").unwrap();
        create_category(&workspace, "l1", "购物消费", "expense").unwrap();
        {
            let conn = workspace.connection();
            for (id, category) in [("t1", "餐饮美食"), ("t2", "餐饮美食"), ("t3", "购物消费")]
            {
                conn.execute(
                    "INSERT INTO tbl_billadm_transaction_record \
                     (transaction_id, ledger_id, price, transaction_type, category, transaction_at, created_at, updated_at) \
                     VALUES (?1, 'l1', 100, 'expense', ?2, 1, 1, 1)",
                    rusqlite::params![id, category],
                )
                .unwrap();
            }
        }

        assert_eq!(
            count_records_by_category(&workspace, "l1", "餐饮美食").unwrap(),
            2
        );

        let names = vec!["餐饮美食".to_string(), "不存在".to_string()];
        let counts = count_records_by_categories(&workspace, "l1", &names).unwrap();
        assert_eq!(counts["餐饮美食"], 2);
        assert_eq!(counts.get("不存在"), None);
        assert!(count_records_by_categories(&workspace, "l1", &[])
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
