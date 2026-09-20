//! 图表服务：预设图表的补齐与图表的增 / 删 / 改 / 查。
//!
//! 本模块是自由函数 + `&Workspace`（与 `ledger.rs` 一致），不做接口抽象。
//! `chart_lines` 的 JSON 编解码用
//! [`tr_domain::dto::encode_chart_lines`] / [`tr_domain::dto::decode_chart_lines`]，
//! 输出为紧凑格式且字段顺序固定（预设图表的落库文本因此逐字节稳定）。

use tr_domain::dto::{
    decode_chart_lines, encode_chart_lines, ChartDto, CreateChartRequest, UpdateChartRequest,
};
use tr_domain::models::{Chart, ChartLine, QueryConditionItem};
use tr_store::dao::chart::ChartDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 一条曲线（`conditions` 为空数组）。
fn line(label: &str, transaction_type: &str, include_outlier: bool) -> ChartLine {
    ChartLine {
        label: label.to_string(),
        transaction_type: transaction_type.to_string(),
        include_outlier,
        conditions: Vec::new(),
    }
}

/// 一条带查询条件的曲线。
fn conditioned(
    label: &str,
    transaction_type: &str,
    include_outlier: bool,
    conditions: Vec<QueryConditionItem>,
) -> ChartLine {
    ChartLine {
        label: label.to_string(),
        transaction_type: transaction_type.to_string(),
        include_outlier,
        conditions,
    }
}

/// 单个查询条件项（`tagNot` 保持 false）。
fn condition(
    transaction_type: &str,
    category: &str,
    tags: &[&str],
    tag_policy: &str,
    description: &str,
) -> QueryConditionItem {
    QueryConditionItem {
        transaction_type: transaction_type.to_string(),
        category: category.to_string(),
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        tag_policy: tag_policy.to_string(),
        tag_not: false,
        description: description.to_string(),
    }
}

/// 预设的支出 / 收入 / 转账三条曲线；`include_outlier` 决定是否把离群点计入
/// （月度预设为 false，年度预设为 true）。
fn chart_lines(include_outlier: bool) -> Vec<ChartLine> {
    vec![
        line("支出", "expense", include_outlier),
        line("收入", "income", include_outlier),
        line("转账", "transfer", include_outlier),
    ]
}

/// 预设的年度收入曲线（含离群点与查询条件）。
fn income_chart_lines() -> Vec<ChartLine> {
    vec![
        line("年度总收入", "income", true),
        conditioned(
            "年度工资收入",
            "income",
            true,
            vec![condition("income", "工资奖金", &["工资"], "all", "")],
        ),
        conditioned(
            "年度奖金收入",
            "income",
            true,
            vec![condition("income", "工资奖金", &["奖金"], "all", "年奖金")],
        ),
        conditioned(
            "年度分红收入",
            "income",
            true,
            vec![condition("income", "投资理财", &[], "all", "年分红")],
        ),
    ]
}

/// 为账本写入 3 个预设图表（月度消费趋势 / 年度消费趋势 / 年度收入趋势）。
///
/// 已有任何图表时直接返回（幂等）。失败文案为 `marshal {title}` / `seed {title}`；
/// 调用方（[`list_by_ledger_id`]）只告警不中断。
fn seed_default_charts(workspace: &Workspace, ledger_id: &str) -> ServiceResult<()> {
    let conn = workspace.connection();

    let count = ChartDao::count_by_ledger_id(&conn, ledger_id).map_err(|error| {
        tracing::warn!("统计图表数量失败: {}", error);
        ServiceError::from(error)
    })?;
    if count > 0 {
        return Ok(());
    }
    tracing::info!("账本 {} 无图表，创建预设图表", ledger_id);

    let presets: [(&str, &str, Vec<ChartLine>, i32); 3] = [
        ("月度消费趋势", "month", chart_lines(false), 0),
        ("年度消费趋势", "year", chart_lines(true), 1),
        ("年度收入趋势", "year", income_chart_lines(), 2),
    ];

    for (title, granularity, lines, sort_order) in presets {
        let lines_json = encode_chart_lines(&lines)
            .map_err(|error| ServiceError::Internal(format!("marshal {title}: {error}")))?;

        ChartDao::create(
            &conn,
            &Chart {
                chart_id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                title: title.to_string(),
                granularity: granularity.to_string(),
                chart_lines: lines_json,
                chart_type: "line".to_string(),
                is_preset: true,
                sort_order,
                created_at: 0,
                updated_at: 0,
            },
        )
        .map_err(|error| ServiceError::Internal(format!("seed {title}: {error}")))?;
    }

    tracing::info!("已为账本 {} 创建 3 个预设图表", ledger_id);
    Ok(())
}

/// 新建图表，返回完整 DTO（`is_preset` 恒为 false）。
pub fn create(workspace: &Workspace, req: &CreateChartRequest) -> ServiceResult<ChartDto> {
    let conn = workspace.connection();
    let chart_id = tr_store::util::new_uuid();

    let max_sort_order =
        ChartDao::get_max_sort(&conn, &req.ledger_id).map_err(ServiceError::from)?;

    let lines_json = encode_chart_lines(&req.lines)
        .map_err(|error| ServiceError::Internal(format!("marshal chart lines failed: {error}")))?;

    let chart = Chart {
        chart_id,
        ledger_id: req.ledger_id.clone(),
        title: req.title.clone(),
        granularity: req.granularity.clone(),
        chart_lines: lines_json,
        chart_type: req.chart_type.clone(),
        is_preset: false,
        sort_order: max_sort_order + 1,
        created_at: 0,
        updated_at: 0,
    };

    ChartDao::create(&conn, &chart)
        .map_err(|error| ServiceError::Internal(format!("create chart failed: {error}")))?;

    to_dto(&chart)
}

/// 删除图表。
pub fn delete_by_id(workspace: &Workspace, chart_id: &str) -> ServiceResult<()> {
    ChartDao::delete_by_id(&workspace.connection(), chart_id)
        .map_err(|error| ServiceError::Internal(format!("delete chart failed: {error}")))
}

/// 某账本的全部图表；查询前先尝试补齐预设图表（失败只告警）。
pub fn list_by_ledger_id(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Vec<ChartDto>> {
    if let Err(error) = seed_default_charts(workspace, ledger_id) {
        tracing::warn!("为账本 {} 创建预设图表失败: {}", ledger_id, error);
    }

    let charts = ChartDao::query_by_ledger_id(&workspace.connection(), ledger_id)
        .map_err(ServiceError::from)?;

    charts.iter().map(to_dto).collect()
}

/// 更新图表（标题 / 粒度 / 曲线 / 类型 / 排序号）。
pub fn update(workspace: &Workspace, req: &UpdateChartRequest) -> ServiceResult<ChartDto> {
    let conn = workspace.connection();

    let mut chart = ChartDao::query_by_id(&conn, &req.chart_id).map_err(|error| {
        ServiceError::Internal(format!("get chart failed: {}", ServiceError::from(error)))
    })?;

    let lines_json = encode_chart_lines(&req.lines)
        .map_err(|error| ServiceError::Internal(format!("marshal chart lines failed: {error}")))?;

    chart.title = req.title.clone();
    chart.granularity = req.granularity.clone();
    chart.chart_lines = lines_json;
    chart.chart_type = req.chart_type.clone();
    chart.sort_order = req.sort_order;

    ChartDao::save(&conn, &chart)
        .map_err(|error| ServiceError::Internal(format!("update chart failed: {error}")))?;

    to_dto(&chart)
}

/// 模型 → DTO；`chart_lines` 解析失败时报错（文案固定为
/// `unmarshal chart lines failed: ...`）。
fn to_dto(chart: &Chart) -> ServiceResult<ChartDto> {
    let lines = decode_chart_lines(&chart.chart_lines).map_err(|error| {
        ServiceError::Internal(format!("unmarshal chart lines failed: {error}"))
    })?;

    Ok(ChartDto {
        chart_id: chart.chart_id.clone(),
        ledger_id: chart.ledger_id.clone(),
        title: chart.title.clone(),
        granularity: chart.granularity.clone(),
        lines,
        chart_type: chart.chart_type.clone(),
        is_preset: chart.is_preset,
        sort_order: chart.sort_order,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;

    /// 以下期望值来自一次真实运行，作为回归基线，不要手改：
    /// 月度图表的默认曲线（`includeOutlier:false`、空的 conditions）。
    const MONTHLY_LINES_JSON: &str = concat!(
        r#"[{"label":"支出","transactionType":"expense","includeOutlier":false,"conditions":[]},"#,
        r#"{"label":"收入","transactionType":"income","includeOutlier":false,"conditions":[]},"#,
        r#"{"label":"转账","transactionType":"transfer","includeOutlier":false,"conditions":[]}]"#
    );

    /// 以下期望值来自一次真实运行，作为回归基线，不要手改：
    /// 预设年度曲线（与月度同构，但 `includeOutlier:true`）。
    const YEARLY_LINES_JSON: &str = concat!(
        r#"[{"label":"支出","transactionType":"expense","includeOutlier":true,"conditions":[]},"#,
        r#"{"label":"收入","transactionType":"income","includeOutlier":true,"conditions":[]},"#,
        r#"{"label":"转账","transactionType":"transfer","includeOutlier":true,"conditions":[]}]"#
    );

    /// 以下期望值来自一次真实运行，作为回归基线，不要手改：
    /// 预设年度收入曲线（含 category / tags / tagPolicy / description）。
    const INCOME_LINES_JSON: &str = concat!(
        r#"[{"label":"年度总收入","transactionType":"income","includeOutlier":true,"conditions":[]},"#,
        r#"{"label":"年度工资收入","transactionType":"income","includeOutlier":true,"conditions":["#,
        r#"{"transactionType":"income","category":"工资奖金","tags":["工资"],"tagPolicy":"all","tagNot":false,"description":""}]},"#,
        r#"{"label":"年度奖金收入","transactionType":"income","includeOutlier":true,"conditions":["#,
        r#"{"transactionType":"income","category":"工资奖金","tags":["奖金"],"tagPolicy":"all","tagNot":false,"description":"年奖金"}]},"#,
        r#"{"label":"年度分红收入","transactionType":"income","includeOutlier":true,"conditions":["#,
        r#"{"transactionType":"income","category":"投资理财","tags":[],"tagPolicy":"all","tagNot":false,"description":"年分红"}]}]"#
    );

    fn line_of(label: &str, transaction_type: &str) -> ChartLine {
        line(label, transaction_type, false)
    }

    #[test]
    fn list_seeds_three_presets_with_exact_json() {
        let (workspace, dir) = workspace("seed");
        let charts = list_by_ledger_id(&workspace, "l1").unwrap();
        assert_eq!(charts.len(), 3, "预设图表数量");

        let titles: Vec<&str> = charts.iter().map(|chart| chart.title.as_str()).collect();
        // is_preset DESC, sort_order ASC
        assert_eq!(titles, vec!["月度消费趋势", "年度消费趋势", "年度收入趋势"]);
        assert!(charts.iter().all(|chart| chart.is_preset));
        assert_eq!(
            charts
                .iter()
                .map(|chart| chart.sort_order)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            charts
                .iter()
                .map(|chart| chart.granularity.as_str())
                .collect::<Vec<_>>(),
            vec!["month", "year", "year"]
        );
        assert!(charts.iter().all(|chart| chart.chart_type == "line"));

        // chart_lines 的落库文本必须逐字节一致
        let conn = workspace.connection();
        let stored = ChartDao::query_by_ledger_id(&conn, "l1").unwrap();
        let json: Vec<&str> = stored
            .iter()
            .map(|chart| chart.chart_lines.as_str())
            .collect();
        assert_eq!(
            json,
            vec![MONTHLY_LINES_JSON, YEARLY_LINES_JSON, INCOME_LINES_JSON]
        );

        // 解析往返：DTO 的 lines 与逐字 JSON 一一对应；再编码回去完全相同
        assert_eq!(charts[0].lines, chart_lines(false));
        assert_eq!(charts[1].lines, chart_lines(true));
        assert_eq!(charts[2].lines, income_chart_lines());
        for chart in &charts {
            let reencoded = encode_chart_lines(&chart.lines).unwrap();
            assert_eq!(
                decode_chart_lines(&reencoded).unwrap(),
                chart.lines,
                "encode/decode 必须往返一致"
            );
        }
        assert_eq!(
            encode_chart_lines(&income_chart_lines()).unwrap(),
            INCOME_LINES_JSON
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_is_idempotent_and_skips_seeding_when_charts_exist() {
        let (first_ws, dir) = workspace("idempotent");
        let first = list_by_ledger_id(&first_ws, "l1").unwrap();
        let second = list_by_ledger_id(&first_ws, "l1").unwrap();
        assert_eq!(second.len(), 3, "第二次调用不得重复插入");
        assert_eq!(
            first
                .iter()
                .map(|chart| chart.chart_id.clone())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|chart| chart.chart_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            ChartDao::count_by_ledger_id(&first_ws.connection(), "l1").unwrap(),
            3
        );

        // 已有图表（哪怕是自定义的）时不再补预设
        let (other_ws, other_dir) = workspace("idempotent-other");
        create(
            &other_ws,
            &CreateChartRequest {
                ledger_id: "l1".to_string(),
                title: "自定义".to_string(),
                granularity: "month".to_string(),
                lines: vec![line_of("支出", "expense")],
                chart_type: "bar".to_string(),
            },
        )
        .unwrap();
        let charts = list_by_ledger_id(&other_ws, "l1").unwrap();
        assert_eq!(charts.len(), 1, "已有图表时不插入预设");
        assert_eq!(charts[0].title, "自定义");

        // 不同账本各自 seed
        assert_eq!(list_by_ledger_id(&first_ws, "l2").unwrap().len(), 3);

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other_dir).ok();
    }

    #[test]
    fn create_assigns_uuid_and_sort_order_after_presets() {
        let (workspace, dir) = workspace("create");
        list_by_ledger_id(&workspace, "l1").unwrap();

        let created = create(
            &workspace,
            &CreateChartRequest {
                ledger_id: "l1".to_string(),
                title: "自定义图表".to_string(),
                granularity: "month".to_string(),
                lines: vec![line_of("支出", "expense"), line_of("收入", "income")],
                chart_type: "bar".to_string(),
            },
        )
        .unwrap();

        assert!(!created.chart_id.is_empty());
        assert!(!created.is_preset);
        assert_eq!(created.sort_order, 3, "预设占 0..2，新建取最大值 + 1");
        assert_eq!(created.chart_type, "bar");
        assert_eq!(
            created.lines,
            vec![line_of("支出", "expense"), line_of("收入", "income")]
        );

        let stored = ChartDao::query_by_id(&workspace.connection(), &created.chart_id).unwrap();
        assert_eq!(
            stored.chart_lines,
            r#"[{"label":"支出","transactionType":"expense","includeOutlier":false,"conditions":[]},{"label":"收入","transactionType":"income","includeOutlier":false,"conditions":[]}]"#
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_keeps_is_preset_and_reports_missing_chart() {
        let (workspace, dir) = workspace("update");
        list_by_ledger_id(&workspace, "l1").unwrap();
        let preset_id = ChartDao::query_by_ledger_id(&workspace.connection(), "l1")
            .unwrap()
            .remove(0)
            .chart_id;

        let updated = update(
            &workspace,
            &UpdateChartRequest {
                chart_id: preset_id,
                title: "改名后的月度".to_string(),
                granularity: "year".to_string(),
                lines: vec![line_of("支出", "expense")],
                chart_type: "bar".to_string(),
                sort_order: 9,
            },
        )
        .unwrap();

        assert_eq!(updated.title, "改名后的月度");
        assert_eq!(updated.granularity, "year");
        assert_eq!(updated.chart_type, "bar");
        assert_eq!(updated.sort_order, 9);
        assert!(updated.is_preset, "预设标记不变");
        assert_eq!(updated.lines, vec![line_of("支出", "expense")]);

        let error = update(
            &workspace,
            &UpdateChartRequest {
                chart_id: "absent".to_string(),
                ..UpdateChartRequest::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "get chart failed: record not found");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_only_that_chart() {
        let (workspace, dir) = workspace("delete");
        let charts = list_by_ledger_id(&workspace, "l1").unwrap();
        let target = charts[0].chart_id.clone();

        delete_by_id(&workspace, &target).unwrap();
        let remaining = list_by_ledger_id(&workspace, "l1").unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|chart| chart.chart_id != target));
        // 删到 0 之后，下一次查询会重新补齐预设（count > 0 则跳过）
        delete_by_id(&workspace, &remaining[0].chart_id).unwrap();
        delete_by_id(&workspace, &remaining[1].chart_id).unwrap();
        assert_eq!(list_by_ledger_id(&workspace, "l1").unwrap().len(), 3);

        // 不存在的 ID 视为成功
        delete_by_id(&workspace, "absent").unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }
}
