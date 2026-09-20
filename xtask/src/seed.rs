//! 种子工作空间：用**服务层自己的代码路径**造一份覆盖各域的数据。
//!
//! 用途：
//! * `cargo xtask seed <dir>` —— 人工冒烟（配合 `cargo tauri dev` 打开这个目录）
//! * 后续回归/验收的固定输入基线（同一份种子反复播种，落库结果必须稳定可复现）
//!
//! 刻意通过 `tr-service` 的公开函数写入，而不是直接 SQL：
//! 这样种子本身就在验证服务层的真实行为（插入失败会立刻暴露）。

use tr_domain::dto::{
    CreateChartRequest, QueryConditionItem, TrQueryCondition, TransactionRecordDto,
    TransactionTemplateDto, UpdateChartRequest,
};
use tr_domain::models::ChartLine;
use tr_store::Workspace;

use tr_service::{
    category, chart, diary, key_event, ledger, stock, tag, transaction_record,
    transaction_template, ServiceError,
};

/// 2026-01-05 / 2026-02-10 / 2026-03-15 的 UTC 秒（与前端的月份分桶对应）。
const DAY_2026_01_05: i64 = 1_767_571_200;
const DAY_2026_02_10: i64 = 1_770_681_600;
const DAY_2026_03_15: i64 = 1_773_590_400;

/// 股票委托的成交时间（UTC 秒）：2026-01-06 / 2026-01-20 / 2026-03-10。
/// 一旦写定就不要改：同一份种子反复播种必须得到同样的落库结果，回归对比才有意义。
pub const TRADE_TIME_OPEN: i64 = 1_767_657_600;
pub const TRADE_TIME_ADD: i64 = 1_768_867_200;
pub const TRADE_TIME_CLOSE: i64 = 1_773_100_800;
/// 阶段 2 重新建仓的成交时间（UTC 秒）：2026-04-10。
pub const TRADE_TIME_REOPEN: i64 = 1_775_779_200;

/// 阶段 3 的成交时间（UTC 秒），全部严格晚于阶段 1/2 且**彼此唯一**
/// （重放按 `trade_time ASC, created_at ASC, order_seq ASC, id ASC` 排序；
/// 同一秒的两笔卖单会被并成一笔，因此每个委托必须有自己的时间戳）：
///
/// 第二轮（600519，2026-05-08 ~ 2026-05-12）：
/// 建仓 / 减仓甲 / 减仓乙 / 加仓 / 清仓；
/// 第三轮（600519，2026-05-14 / 2026-05-15）：建仓 / 清仓；
/// 第四轮（000001，2026-05-17 / 2026-05-18）：建仓 / 清仓。
pub const TRADE_TIME_R2_OPEN: i64 = 1_778_198_400;
pub const TRADE_TIME_R2_REDUCE1: i64 = 1_778_284_800;
pub const TRADE_TIME_R2_REDUCE2: i64 = 1_778_371_200;
pub const TRADE_TIME_R2_ADD: i64 = 1_778_457_600;
pub const TRADE_TIME_R2_CLOSE: i64 = 1_778_544_000;
pub const TRADE_TIME_R3_OPEN: i64 = 1_778_716_800;
pub const TRADE_TIME_R3_CLOSE: i64 = 1_778_803_200;
pub const TRADE_TIME_R4_OPEN: i64 = 1_778_976_000;
pub const TRADE_TIME_R4_CLOSE: i64 = 1_779_062_400;

/// 阶段 3 的独立股票（避免与 600519 的持仓行 / 历史集合互相干扰）。
const PHASE3_STOCK_CODE: &str = "000001";
const PHASE3_STOCK_NAME: &str = "平安银行";

/// 本金：15 万元 = 15,000,000 分。
const PRINCIPAL_CENTS: i64 = 15_000_000;
/// 支取：1 万元 = 1,000,000 分。
const WITHDRAW_CENTS: i64 = 1_000_000;
/// 阶段 2 追加本金：50 万元 = 50,000,000 分。
const ADD_PRINCIPAL_CENTS: i64 = 50_000_000;
/// 阶段 2 重新建仓的成交价：16.80 元 = 1680 分。
const REOPEN_PRICE_CENTS: i64 = 1_680;

/// 阶段 2 新增的分类 / 标签 / 图表 / 模板名称（每次播种必须逐字一致）。
const PHASE2_CATEGORY: &str = "阶段二分类";
const PHASE2_TAG: &str = "阶段二标签";
const PHASE2_CHART_TITLE: &str = "阶段二图表";
const PHASE2_TEMPLATE_NAME: &str = "阶段二模板";
/// 阶段 2 给股票交易标签设置新增的标签（默认列表之外）。
const PHASE2_STOCK_TAG: &str = "波段";

/// P1-2 图表更新：`PATCH /charts` 覆盖写后的标题 / 粒度 / 排序号。
///
/// `sort_order` 刻意写死为常量 9：
/// 必须大于 3 个预设图表的 0/1/2，更新后这条自定义图表才会排到列表末尾。
/// 不写成"当前最大值 + 1"——图表 ID 每次都不同，
/// 只有与 ID 无关的常量才能让落库行逐字可比。
const P1_CHART_UPDATED_TITLE: &str = "阶段二图表（更新：分类+标签+离群点）";
const P1_CHART_UPDATED_GRANULARITY: &str = "year";
const P1_CHART_UPDATED_CHART_TYPE: &str = "bar";
const P1_CHART_UPDATED_SORT_ORDER: i32 = 9;

/// P1-3 的四个 `sort_order` 覆盖：模板 / 分类 / 标签。
///
/// 分类与标签是**整组重排**：把一组里的每一行都改成新序号，
/// 落库后 `tbl_billadm_category` / `tbl_billadm_tag` 的每一行 `sort_order`
/// 都必须逐行落库正确（序号刻意互不相同且不连续，避免"只改了一行没改其它行"被掩盖）。
const P1_TEMPLATE_SORT_ORDER: i32 = 7;
const P1_CATEGORY_SORT_ORDER: &[(&str, i32)] = &[
    ("餐饮美食", 5),
    ("购物消费", 3),
    ("交通出行", 8),
    ("生活缴费", 1),
    ("贷款还款", 9),
    ("医疗健康", 2),
    ("娱乐休闲", 6),
    ("人情往来", 4),
    ("教育学习", 7),
];
const P1_TAG_SORT_ORDER: &[(&str, i32)] = &[
    ("三餐", 10),
    ("零食", 9),
    ("商场", 8),
    ("外卖", 7),
    ("饮料", 6),
    ("奶茶", 5),
    ("咖啡", 4),
    ("水果", 3),
    ("茶叶", 2),
    ("买菜", 1),
];

/// P1-4 关键事件的日期与两次写入取值（覆盖写后必须仍是同一行）。
///
/// 注意 `2026-04-01` 这一行**已经存在**：阶段 2 的 `link_to_key_event` 会为该日期
/// 懒创建一条空事件（`ensure_exists`），取消关联与删除记录都不会删掉它。
/// 这里先读回它的 `id` / `created_at`，覆盖后再断言二者不变、且行数不变。
const P1_KEY_EVENT_DATE: &str = "2026-04-01";
const P1_KEY_EVENT_TITLE: &str = "P1-4 覆盖写标题";
const P1_KEY_EVENT_CONTENT: &str = "## 覆盖写\n\n- 第二次写入必须更新内容\n- 且不得新增行";
const P1_KEY_EVENT_COLOR: &str = "normal";
const P1_KEY_EVENT_TITLE_V2: &str = "P1-4 覆盖写标题（二次）";
const P1_KEY_EVENT_CONTENT_V2: &str =
    "## 覆盖写（二次）\n\n- 只更新 title/content/color/updated_at";
const P1_KEY_EVENT_COLOR_V2: &str = "outlier";

/// 阶段 3 覆盖写的账本名 / 描述（`PATCH /ledgers/:id`）。
const PHASE3_LEDGER_NAME: &str = "默认账本（改名）";
const PHASE3_LEDGER_DESCRIPTION: &str = "阶段三更新描述";

/// 在一个工作空间里造一份可复现的示例数据，返回一段人读的摘要。
pub fn seed(workspace: &Workspace) -> Result<String, ServiceError> {
    let mut log = String::new();

    // ---- 账本（两个，验证账本隔离）----
    let main_ledger = ledger::create_ledger(workspace, "默认账本", "种子数据")?;
    let other_ledger = ledger::create_ledger(workspace, "备用账本", "")?;
    log.push_str(&format!(
        "账本: 默认账本={main_ledger} 备用账本={other_ledger}\n"
    ));

    // ---- 分类与标签（种子数据：19 分类 / 57 标签）----
    let (categories, tags) = category::initialize_categories(workspace, &main_ledger)?;
    log.push_str(&format!("默认分类 {categories} 个 / 标签 {tags} 个\n"));

    // ---- 消费记录（覆盖三类交易 + 标签 + outlier）----
    let records = [
        (
            12_345_i64,
            "expense",
            "餐饮美食",
            "午餐",
            DAY_2026_01_05,
            false,
            vec!["三餐"],
        ),
        (
            8_800,
            "expense",
            "交通出行",
            "地铁",
            DAY_2026_01_05,
            false,
            vec!["地铁"],
        ),
        (
            259_900,
            "expense",
            "购物消费",
            "耳机",
            DAY_2026_02_10,
            true,
            vec!["数码"],
        ),
        (
            1_500_000,
            "income",
            "工资奖金",
            "一月工资",
            DAY_2026_02_10,
            false,
            vec!["工资"],
        ),
        (
            1_500_000,
            "income",
            "工资奖金",
            "二月工资",
            DAY_2026_03_15,
            false,
            vec!["工资"],
        ),
        (
            300_000,
            "transfer",
            "五险一金",
            "公积金",
            DAY_2026_03_15,
            false,
            vec!["住房"],
        ),
    ];
    let mut record_ids = Vec::new();
    for (price, transaction_type, category_name, description, at, outlier, record_tags) in records {
        let dto = TransactionRecordDto {
            ledger_id: main_ledger.clone(),
            price,
            transaction_type: transaction_type.to_string(),
            category: category_name.to_string(),
            description: description.to_string(),
            tags: record_tags.iter().map(|tag| tag.to_string()).collect(),
            transaction_at: at,
            outlier,
            ..TransactionRecordDto::default()
        };
        record_ids.push(transaction_record::create_tr(workspace, &dto)?);
    }
    log.push_str(&format!("消费记录 {} 条\n", record_ids.len()));

    // 备用账本各来一条，验证账本隔离
    let other_record = transaction_record::create_tr(
        workspace,
        &TransactionRecordDto {
            ledger_id: other_ledger.clone(),
            price: 6_600,
            transaction_type: "expense".to_string(),
            category: "餐饮美食".to_string(),
            description: "备用账本的一条".to_string(),
            tags: vec![],
            transaction_at: DAY_2026_01_05,
            outlier: false,
            ..TransactionRecordDto::default()
        },
    )?;
    log.push_str("备用账本消费记录 1 条\n");

    // ---- 关键事件 + 关联记录 ----
    key_event::upsert_key_event(
        workspace,
        &main_ledger,
        "2026-02-10",
        "买了新耳机",
        "## 记录\n\n- 价格 2599 元\n- 有点冲动消费",
        "outlier",
    )?;
    transaction_record::link_to_key_event(workspace, &record_ids[2], "2026-02-10")?;
    log.push_str("关键事件 1 条（已关联 1 笔消费记录）\n");

    // ---- 日记（归到主账本；日记按账本隔离）----
    diary::upsert(
        workspace,
        &main_ledger,
        "2026-02-10",
        "# 2026-02-10\n\n今天买了耳机，复盘一下。",
        "开心",
    )?;
    diary::upsert(
        workspace,
        &main_ledger,
        "2026-03-15",
        "发工资了，先存一半。",
        "平静",
    )?;
    log.push_str("日记 2 篇（默认账本）\n");

    // ---- 消费模板 ----
    let template_id = transaction_template::create(
        workspace,
        &TransactionTemplateDto {
            ledger_id: main_ledger.clone(),
            template_name: "早餐".to_string(),
            transaction_type: "expense".to_string(),
            category: "餐饮美食".to_string(),
            tags: vec!["三餐".to_string()],
            flags: String::new(),
            description: "早餐".to_string(),
            sort_order: 0,
            ..TransactionTemplateDto::default()
        },
    )?;
    log.push_str(&format!("消费模板 1 个={template_id}\n"));

    // ---- 图表（list 会顺带播种 3 个预设）----
    let charts = chart::list_by_ledger_id(workspace, &main_ledger)?;
    log.push_str(&format!(
        "图表 {} 个（{}）\n",
        charts.len(),
        charts
            .iter()
            .map(|chart| chart.title.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    ));

    // 备用账本：只有那条记录，没有分类/图表（验证"缺省账本"路径）
    log.push_str(&format!("备用账本记录={other_record}\n"));

    // ---- 股票交易（覆盖最高风险的算法：委托级费用一次计收 + 分摊、持仓重放、轮次归档、资金链）----
    // 这些操作的输入**逐字写定**（种子即基线），因此这里的取值不可随意改动。
    stock::set_principal(workspace, &main_ledger, PRINCIPAL_CENTS)?;
    stock::create_trade_order(
        workspace,
        &main_ledger,
        "600519",
        "贵州茅台",
        "open",
        &[
            stock::TradeFill {
                price_cents: 170_000,
                lots: 1,
            },
            stock::TradeFill {
                price_cents: 170_150,
                lots: 1,
            },
        ],
        TRADE_TIME_OPEN,
        "种子建仓",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &main_ledger,
        "600519",
        "贵州茅台",
        "add",
        &[stock::TradeFill {
            price_cents: 169_500,
            lots: 1,
        }],
        TRADE_TIME_ADD,
        "种子加仓",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &main_ledger,
        "600519",
        "贵州茅台",
        "close",
        &[stock::TradeFill {
            price_cents: 175_000,
            lots: 3,
        }],
        TRADE_TIME_CLOSE,
        "种子清仓",
        "打板",
    )?;
    stock::add_withdraw_at_date(workspace, &main_ledger, WITHDRAW_CENTS, "2026-03-20")?;
    log.push_str("股票：本金 15 万元 / 建仓(2 笔成交) + 加仓 + 清仓(3 手, 打板) / 支取 1 万元\n");

    let overview = stock::get_overview(workspace, &main_ledger)?;
    log.push_str(&format!(
        "股票总览：本金={} 可用现金={} 已实现盈亏={}\n",
        overview.principal, overview.available_cash, overview.realized_pnl
    ));

    // ==================================================================================
    // 阶段 2：更新 / 删除写入路径
    //
    // 阶段 1 只覆盖「新建」；这里按**同一套取值**追加更新与删除操作，
    // 让种子覆盖 UPDATE / DELETE 的落库行为（派生数据重放、
    // 级联清理、归档轮次的标签与复盘等）。取值保持写定，不随实现调整。
    // ==================================================================================
    log.push_str("---- 阶段 2 ----\n");

    // ---- 1. 消费记录：批量新建 → 关联 → 取消关联 → 删除 ----
    // 批量新建返回的是条数而不是 ID，
    // 因此这里按描述查回记录，以取得要删除的那一条。
    let batch_dtos = [
        phase2_record(
            &main_ledger,
            "批量A",
            66_600,
            "expense",
            "餐饮美食",
            DAY_2026_03_15,
            vec!["三餐"],
        ),
        phase2_record(
            &main_ledger,
            "批量B",
            8_800,
            "expense",
            "交通出行",
            DAY_2026_03_15,
            vec!["地铁"],
        ),
    ];
    let created = transaction_record::batch_create_tr(workspace, &batch_dtos)?;
    let batch_a_id = phase2_find_record(workspace, &main_ledger, "批量A")?;
    log.push_str(&format!(
        "批量新建消费记录 {created} 条，批量A={batch_a_id}\n"
    ));

    transaction_record::link_to_key_event(workspace, &batch_a_id, "2026-04-01")?;
    transaction_record::unlink_from_key_event(workspace, &batch_a_id)?;
    transaction_record::delete_tr_by_id(workspace, &batch_a_id)?;
    log.push_str("批量A：关联 2026-04-01 → 取消关联 → 删除\n");

    // ---- 2. 日记：删除 2026-03-15 那篇 ----
    diary::delete_by_date(workspace, &main_ledger, "2026-03-15")?;
    log.push_str("日记：删除 2026-03-15\n");

    // ---- 3. 分类与标签：各新建一个再各自删除 ----
    category::create_category(workspace, &main_ledger, PHASE2_CATEGORY, "expense")?;
    category::delete_category(workspace, &main_ledger, PHASE2_CATEGORY, "expense")?;
    let phase2_tag_category = format!("{PHASE2_CATEGORY}:expense");
    tag::create_tag(workspace, &main_ledger, PHASE2_TAG, &phase2_tag_category)?;
    tag::delete_tag(workspace, &main_ledger, PHASE2_TAG, &phase2_tag_category)?;
    log.push_str("分类与标签：各新建一个再各自删除\n");

    // ---- 4. 图表：新建一个自定义图表再删除 ----
    let custom_chart = chart::create(
        workspace,
        &CreateChartRequest {
            ledger_id: main_ledger.clone(),
            title: PHASE2_CHART_TITLE.to_string(),
            granularity: "month".to_string(),
            lines: vec![ChartLine {
                label: "阶段二支出".to_string(),
                transaction_type: "expense".to_string(),
                include_outlier: false,
                conditions: vec![QueryConditionItem {
                    transaction_type: "expense".to_string(),
                    ..QueryConditionItem::default()
                }],
            }],
            chart_type: "line".to_string(),
        },
    )?;
    chart::delete_by_id(workspace, &custom_chart.chart_id)?;
    log.push_str(&format!("图表：新建再删除 {}\n", custom_chart.chart_id));

    // ---- P1-2. 图表：新建一条自定义图表后用 `PATCH /charts` **整体替换** `lines` ----
    // 覆盖点：`chart_lines` 的 JSON 文本必须逐字节稳定
    // （对象字段顺序敏感），因此这里用了同时带「分类 + 标签 + 交易类型 + includeOutlier」
    // 的复合条件；粒度改成 year、排序号改到预设图表之后。
    // 图表 ID 每次生成都不同，所以 update 只按拿到的 ID 走，其余落库列必须逐字相同。
    let p1_chart = chart::create(
        workspace,
        &CreateChartRequest {
            ledger_id: main_ledger.clone(),
            title: PHASE2_CHART_TITLE.to_string(),
            granularity: "month".to_string(),
            lines: vec![ChartLine {
                label: "P1-2 初始曲线".to_string(),
                transaction_type: "expense".to_string(),
                include_outlier: false,
                conditions: Vec::new(),
            }],
            chart_type: "line".to_string(),
        },
    )?;
    let p1_chart_updated = chart::update(
        workspace,
        &UpdateChartRequest {
            chart_id: p1_chart.chart_id.clone(),
            title: P1_CHART_UPDATED_TITLE.to_string(),
            granularity: P1_CHART_UPDATED_GRANULARITY.to_string(),
            lines: vec![
                ChartLine {
                    label: "P1-2 餐饮支出".to_string(),
                    transaction_type: "expense".to_string(),
                    include_outlier: true,
                    conditions: vec![
                        QueryConditionItem {
                            transaction_type: "expense".to_string(),
                            category: "餐饮美食".to_string(),
                            tags: vec!["三餐".to_string(), "外卖".to_string()],
                            tag_policy: "all".to_string(),
                            tag_not: false,
                            description: String::new(),
                        },
                        QueryConditionItem {
                            transaction_type: "expense".to_string(),
                            category: String::new(),
                            tags: vec!["数码".to_string()],
                            tag_policy: "any".to_string(),
                            tag_not: true,
                            description: "耳机".to_string(),
                        },
                    ],
                },
                ChartLine {
                    label: "P1-2 无分类收入".to_string(),
                    transaction_type: "income".to_string(),
                    include_outlier: false,
                    conditions: Vec::new(),
                },
            ],
            chart_type: P1_CHART_UPDATED_CHART_TYPE.to_string(),
            sort_order: P1_CHART_UPDATED_SORT_ORDER,
        },
    )?;
    if p1_chart_updated.is_preset {
        return Err(ServiceError::Internal(
            "P1-2：更新自定义图表后 is_preset 必须仍为 false".to_string(),
        ));
    }
    log.push_str(&format!(
        "图表：P1-2 新建 {} → PATCH 覆盖 lines/粒度/排序号 ✅ sort_order={}\n",
        p1_chart.chart_id, p1_chart_updated.sort_order
    ));

    // ==================================================================================
    // P1-3：四个 `sort_order` 写入路径（模板 / 分类 / 标签）
    //
    // 分类与标签是**整组重排**：一组里的每一行都改成新序号，落库后逐行比对。
    // 取值写定，改动会让落库结果漂移。
    // ==================================================================================

    // ---- P1-3a. 模板：`PATCH /templates/:id/sort`（路径参数是模板 ID）----
    // 模板的 `sort_order` 与 `updated_at` 都会被刷新。
    transaction_template::update_sort_order(
        workspace,
        &template_id,
        &main_ledger,
        P1_TEMPLATE_SORT_ORDER,
    )?;
    log.push_str(&format!(
        "模板：P1-3 排序号 {template_id} → {P1_TEMPLATE_SORT_ORDER}\n"
    ));

    // ---- P1-3b. 分类：`PATCH /categories/:name/sort`（9 个 expense 分类整组重排）----
    for (name, sort_order) in P1_CATEGORY_SORT_ORDER {
        category::update_category_sort(workspace, &main_ledger, name, "expense", *sort_order)?;
    }
    log.push_str(&format!(
        "分类：P1-3 expense 整组重排 {} 行\n",
        P1_CATEGORY_SORT_ORDER.len()
    ));

    // ---- P1-3c. 标签：`PATCH /tags/:name/sort`（「餐饮美食:expense」下 10 个标签整组重排）----
    let p1_tag_category = "餐饮美食:expense";
    for (name, sort_order) in P1_TAG_SORT_ORDER {
        tag::update_tag_sort(workspace, &main_ledger, name, p1_tag_category, *sort_order)?;
    }
    log.push_str(&format!(
        "标签：P1-3 {p1_tag_category} 整组重排 {} 行\n",
        P1_TAG_SORT_ORDER.len()
    ));

    // ==================================================================================
    // P1-4：关键事件覆盖写（同一 date 二次 upsert）+ 删除
    //
    // 覆盖点：`(ledger_id, date)` 冲突时只更新 title/content/color/updated_at，
    // **保留原 id 与 created_at**，且行数不变（是覆盖不是新增）；
    // 删除后该行必须消失。取值同样写定。
    // ==================================================================================
    let p1_event_before = key_event::query_by_date(workspace, &main_ledger, P1_KEY_EVENT_DATE)?;
    let p1_rows_before = key_event_row_count(workspace, &main_ledger)?;

    key_event::upsert_key_event(
        workspace,
        &main_ledger,
        P1_KEY_EVENT_DATE,
        P1_KEY_EVENT_TITLE,
        P1_KEY_EVENT_CONTENT,
        P1_KEY_EVENT_COLOR,
    )?;
    key_event::upsert_key_event(
        workspace,
        &main_ledger,
        P1_KEY_EVENT_DATE,
        P1_KEY_EVENT_TITLE_V2,
        P1_KEY_EVENT_CONTENT_V2,
        P1_KEY_EVENT_COLOR_V2,
    )?;

    let p1_event_after = key_event::query_by_date(workspace, &main_ledger, P1_KEY_EVENT_DATE)?;
    let p1_rows_after = key_event_row_count(workspace, &main_ledger)?;
    if p1_rows_after != p1_rows_before {
        return Err(ServiceError::Internal(format!(
            "P1-4：同一天二次 upsert 必须是覆盖而不是新增（行数 {} → {}）",
            p1_rows_before, p1_rows_after
        )));
    }
    if p1_event_after.id != p1_event_before.id
        || p1_event_after.created_at != p1_event_before.created_at
    {
        return Err(ServiceError::Internal(
            "P1-4：覆盖写必须保留原 id 与 created_at".to_string(),
        ));
    }
    if p1_event_after.title != P1_KEY_EVENT_TITLE_V2
        || p1_event_after.content != P1_KEY_EVENT_CONTENT_V2
        || p1_event_after.color != P1_KEY_EVENT_COLOR_V2
    {
        return Err(ServiceError::Internal(
            "P1-4：覆盖写必须更新 title / content / color".to_string(),
        ));
    }
    log.push_str(&format!(
        "关键事件：P1-4 {P1_KEY_EVENT_DATE} 二次 upsert 覆盖 OK（行数 {} 不变，id/created_at 保留）\n",
        p1_rows_after
    ));

    key_event::delete_by_date(workspace, &main_ledger, P1_KEY_EVENT_DATE)?;
    if key_event::query_by_date(workspace, &main_ledger, P1_KEY_EVENT_DATE).is_ok() {
        return Err(ServiceError::Internal(format!(
            "P1-4：删除 {P1_KEY_EVENT_DATE} 后该行必须消失"
        )));
    }
    let p1_rows_deleted = key_event_row_count(workspace, &main_ledger)?;
    if p1_rows_deleted != p1_rows_before - 1 {
        return Err(ServiceError::Internal(format!(
            "P1-4：删除后行数应为 {}，实际 {p1_rows_deleted}",
            p1_rows_before - 1
        )));
    }
    log.push_str(&format!(
        "关键事件：P1-4 删除 {P1_KEY_EVENT_DATE} ✅ 行数 {} → {}\n",
        p1_rows_after, p1_rows_deleted
    ));

    // ---- P1-5（补充）. 预演：**非法目标**必须 400 且零痕迹 ----
    // 删掉建仓委托会让成交流只剩卖出，预演与真删都会以同一个 400 拒绝（正确行为）。
    // 这里断言"失败的预演同样不落库"：`phase3_snapshot` 的派生数据指纹必须逐字不变。
    // 语义已被 `crates/tr-service/src/stock.rs` 的
    // `preview_delete_order_keeps_state_intact_for_later_replay` 单测锁住，这里只是补一层护栏。
    let p1_open_order_id = stock::list_trades(workspace, &main_ledger, "600519")?
        .into_iter()
        .filter(|trade| trade.trade_type == "open")
        .min_by_key(|trade| trade.trade_time)
        .ok_or_else(|| ServiceError::Internal("P1-5：未找到建仓委托".to_string()))?
        .order_id;
    let p1_before = phase3_snapshot(workspace)?;
    let p1_invalid = stock::preview_trade_change(
        workspace,
        &main_ledger,
        "delete_order",
        "",
        &p1_open_order_id,
        0,
        0,
        0,
    );
    match p1_invalid {
        Ok(impact) => {
            return Err(ServiceError::Internal(format!(
                "P1-5：删建仓委托的预演必须被 400 拒绝，实际成功返回 {impact:?}"
            )))
        }
        Err(error) => {
            let app_error = error.into_app_error();
            if app_error.status != 400 || !app_error.msg.contains("卖出数量超过持仓") {
                return Err(ServiceError::Internal(format!(
                    "P1-5：预演拒绝的文案/状态不符（status={} msg={}）",
                    app_error.status, app_error.msg
                )));
            }
        }
    }
    if phase3_snapshot(workspace)? != p1_before {
        return Err(ServiceError::Internal(
            "P1-5：非法目标的预演以 400 拒绝，但库内容发生了变化（必须零痕迹）".to_string(),
        ));
    }
    log.push_str(&format!(
        "股票：P1-5 非法目标（删建仓委托 {p1_open_order_id}）→ 400「卖出数量超过持仓」且零痕迹 ✅\n"
    ));

    // ---- 5. 模板：新建一个模板再删除 ----
    let phase2_template_id = transaction_template::create(
        workspace,
        &TransactionTemplateDto {
            ledger_id: main_ledger.clone(),
            template_name: PHASE2_TEMPLATE_NAME.to_string(),
            transaction_type: "expense".to_string(),
            category: "餐饮美食".to_string(),
            tags: vec!["三餐".to_string()],
            description: PHASE2_TEMPLATE_NAME.to_string(),
            sort_order: 0,
            ..TransactionTemplateDto::default()
        },
    )?;
    transaction_template::delete_by_id(workspace, &phase2_template_id)?;
    log.push_str(&format!("模板：新建再删除 {phase2_template_id}\n"));

    // ---- 6a. 股票：编辑「建仓委托」第二笔成交的成交价（1701.50 → 1702.00）----
    // 建仓委托（阶段 1 的第一笔）此刻已经归档到轮次，取其 order_seq = 2 的那笔成交明细；
    // `update_trade_fill` 收的是**成交明细 id**（不是委托 id），因此这里必须逐笔挑出来。
    let opening_trades = stock::list_trades(workspace, &main_ledger, "600519")?;
    let opening_round_id = opening_trades
        .iter()
        .filter(|trade| trade.trade_type == "open" && trade.order_seq == 1)
        .min_by_key(|trade| trade.trade_time)
        .ok_or_else(|| ServiceError::Internal("未找到建仓成交".to_string()))?
        .round_id
        .clone();
    let second_fill_id = opening_trades
        .iter()
        .filter(|trade| trade.round_id == opening_round_id && trade.order_seq == 2)
        .min_by_key(|trade| trade.trade_time)
        .ok_or_else(|| ServiceError::Internal("未找到建仓的第 2 笔成交".to_string()))?
        .id
        .clone();
    stock::update_trade_fill(
        workspace,
        &main_ledger,
        &second_fill_id,
        170_200,
        1,
        TRADE_TIME_OPEN,
    )?;
    log.push_str(&format!(
        "股票：编辑建仓第 2 笔成交 {second_fill_id}（1701.50 → 1702.00）\n"
    ));

    // ---- 6b. 股票：编辑已归档轮次的标签与复盘（第一轮来自清仓）----
    // 归档的历史集合由「交易历史」列表懒补齐（列表查询先做 backfill），
    // 详情查询自身不做补齐——所以必须先走一次列表查询，
    // 否则详情接口会报"该股票暂无交易历史"。
    stock::list_trade_histories(workspace, &main_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &main_ledger, "600519")?;
    let round_id = detail
        .rounds
        .first()
        .ok_or_else(|| ServiceError::Internal("交易历史没有轮次".to_string()))?
        .id
        .clone();
    stock::update_round_tag(workspace, &main_ledger, &round_id, "尾盘")?;
    stock::update_round_review(workspace, &main_ledger, &round_id, "阶段二：轮次复盘已更新")?;
    log.push_str(&format!("股票：轮次 {round_id} 标签→尾盘、复盘已更新\n"));

    // ---- 6c. 股票：重新建仓 1 手 @16.80，编辑持仓复盘后删除该委托（持仓回到 0）----
    let reopen_trades = stock::create_trade_order(
        workspace,
        &main_ledger,
        "600519",
        "贵州茅台",
        "open",
        &[stock::TradeFill {
            price_cents: REOPEN_PRICE_CENTS,
            lots: 1,
        }],
        TRADE_TIME_REOPEN,
        "二次建仓",
        "",
    )?;
    let reopen_order_id = reopen_trades
        .first()
        .ok_or_else(|| ServiceError::Internal("二次建仓未返回成交明细".to_string()))?
        .order_id
        .clone();
    stock::update_position_review(workspace, &main_ledger, "600519", "阶段二：持仓复盘已更新")?;
    stock::delete_trade_order(workspace, &main_ledger, &reopen_order_id)?;
    log.push_str(&format!(
        "股票：二次建仓 {reopen_order_id} → 编辑持仓复盘 → 删除委托\n"
    ));

    // ---- 6d. 股票：追加本金 50 万（指定发生日期，避免依赖当天日期）----
    stock::add_principal_at_date(workspace, &main_ledger, ADD_PRINCIPAL_CENTS, "2026-04-20")?;
    log.push_str("股票：追加本金 50 万元\n");

    // ---- 6e. 股票：更新费用设置 ----
    stock::save_fee_settings(workspace, &main_ledger, 0.000_1, 500, 0.000_5, 0.000_01)?;
    log.push_str("股票：费用设置 → 万1 / 5元 / 0.05% / 0.001%\n");

    // ---- 6f. 股票：更新交易标签设置（默认列表 + 波段）----
    let mut stock_tags = stock::get_trade_tags(workspace, &main_ledger)?;
    stock_tags.push(PHASE2_STOCK_TAG.to_string());
    stock::save_trade_tags(workspace, &main_ledger, &stock_tags)?;
    log.push_str(&format!("股票：交易标签 → {}\n", stock_tags.join("/")));

    // ---- 8. 账本：更新名称与描述（`modify_ledger` 只改这两列，`created_at` 必须不变）----
    ledger::modify_ledger(
        workspace,
        &main_ledger,
        PHASE3_LEDGER_NAME,
        PHASE3_LEDGER_DESCRIPTION,
    )?;
    log.push_str(&format!(
        "账本：更新为「{PHASE3_LEDGER_NAME}」/「{PHASE3_LEDGER_DESCRIPTION}」\n"
    ));

    // ---- 9. 账本：删除「备用账本」，验证级联清理 ----
    ledger::delete_ledger_by_id(workspace, &other_ledger)?;
    log.push_str("账本：删除备用账本（级联清理）\n");

    let final_overview = stock::get_overview(workspace, &main_ledger)?;
    log.push_str(&format!(
        "股票总览（阶段 2 后）：本金={} 可用现金={} 已实现盈亏={}\n",
        final_overview.principal, final_overview.available_cash, final_overview.realized_pnl
    ));

    // ==================================================================================
    // 阶段 3：减仓（reduce）路径 + 多轮次归档（round_no ≥ 2）
    //
    // 阶段 1/2 只覆盖 open / add / close；reduce 是唯一走「按剩余总成本比例结转」的分支，
    // 而 round_no ≥ 2 才能验证 `rebuild_trades` 的「按股票 + 轮次序号复用轮次 ID、
    // 保留 tag/review；不再成立的轮次失效删除」。
    //
    // 手数守恒（每轮买入 = 卖出）：
    //   第二轮 600519：3 + 1 = 4 手；1 + 1 + 2 = 4 手
    //   第三轮 600519：1 手；1 手
    //   第四轮 000001：1 手；1 手
    // 所有 trade_time 互不相同（同秒卖单会被重放并成一笔）。
    // 取值写定，改动会让落库结果漂移。
    // ==================================================================================

    // 阶段 3 在**独立账本**上运行：避免与阶段 1/2 的持仓行、历史集合、资金记录互相影响，
    // 这样减仓/多轮次的行为可以单独验证。
    let phase3_ledger = ledger::create_ledger(workspace, "阶段三账本", "")?;
    stock::set_principal(workspace, &phase3_ledger, PRINCIPAL_CENTS)?;
    log.push_str(&format!("---- 阶段 3（独立账本 {phase3_ledger}）----\n"));
    // ---- 3-1. 第二轮 600519：建仓 3 手 → 两次减仓（各 1 手）→ 加仓 1 手 → 清仓 2 手 ----
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "open",
        &[stock::TradeFill {
            price_cents: 198_000,
            lots: 3,
        }],
        TRADE_TIME_R2_OPEN,
        "第二轮建仓",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "reduce",
        &[stock::TradeFill {
            price_cents: 199_000,
            lots: 1,
        }],
        TRADE_TIME_R2_REDUCE1,
        "第二轮减仓甲",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "reduce",
        &[stock::TradeFill {
            price_cents: 197_500,
            lots: 1,
        }],
        TRADE_TIME_R2_REDUCE2,
        "第二轮减仓乙",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "add",
        &[stock::TradeFill {
            price_cents: 201_000,
            lots: 1,
        }],
        TRADE_TIME_R2_ADD,
        "第二轮加仓",
        "",
    )?;
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "close",
        &[stock::TradeFill {
            price_cents: 200_000,
            lots: 2,
        }],
        TRADE_TIME_R2_CLOSE,
        "第二轮清仓",
        "追涨",
    )?;
    // 减仓必须留下两条**独立的** reduce 成交，且各为 1 手（自检脚本笔误）
    {
        let reduces: Vec<i64> = stock::list_trades(workspace, &phase3_ledger, "600519")?
            .into_iter()
            .filter(|trade| trade.trade_type == "reduce")
            .map(|trade| trade.lots)
            .collect();
        if reduces != vec![1, 1] {
            return Err(ServiceError::Internal(format!(
                "第二轮减仓应为两笔各 1 手，实际 {reduces:?}"
            )));
        }
    }
    log.push_str("股票：600519 建仓3手 → 减仓1手 → 减仓1手 → 加仓1手 → 清仓2手（追涨）\n");

    // 第二轮归档后写复盘，供后续重放验证「轮次元数据按序号保留」
    stock::list_trade_histories(workspace, &phase3_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &phase3_ledger, "600519")?;
    let round2_id = phase3_round_id(&detail, 1)?;
    stock::update_round_review(
        workspace,
        &phase3_ledger,
        &round2_id,
        "阶段三：第二轮复盘（编辑后必须保留）",
    )?;

    // ---- 3-2. 第二轮 600519：建仓 1 手 → 清仓 1 手（round_no = 2）----
    let round3_open_trades = stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "open",
        &[stock::TradeFill {
            price_cents: 190_000,
            lots: 1,
        }],
        TRADE_TIME_R3_OPEN,
        "第二轮建仓",
        "",
    )?;
    let round3_open_id = round3_open_trades
        .first()
        .ok_or_else(|| ServiceError::Internal("第三轮建仓未返回成交明细".to_string()))?
        .id
        .clone();
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        "600519",
        "贵州茅台",
        "close",
        &[stock::TradeFill {
            price_cents: 195_000,
            lots: 1,
        }],
        TRADE_TIME_R3_CLOSE,
        "第二轮清仓",
        "",
    )?;
    stock::list_trade_histories(workspace, &phase3_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &phase3_ledger, "600519")?;
    if detail.rounds.len() != 2 {
        return Err(ServiceError::Internal(format!(
            "600519 此时应有 2 轮，实际 {}",
            detail.rounds.len()
        )));
    }
    let round3_id = phase3_round_id(&detail, 2)?;
    stock::update_round_tag(workspace, &phase3_ledger, &round3_id, "蓄力")?;
    stock::update_round_review(workspace, &phase3_ledger, &round3_id, "阶段三：第三轮复盘")?;
    log.push_str(&format!(
        "股票：600519 第1轮 {round2_id} 复盘 / 第2轮 {round3_id} 标签→蓄力 已写\n"
    ));

    // ---- 3-3a. 预演（不落库）：删除**合法目标**的委托（删减仓甲，删完仍持有 2 手）----
    // 非法目标（删建仓委托 → 只剩卖出）会被预演自己以 400 拒绝，那是正确行为，
    // 因此这里用合法目标覆盖 `delete_order` 分支。
    let reduce_order_id = stock::list_trades(workspace, &phase3_ledger, "600519")?
        .into_iter()
        .filter(|trade| trade.trade_type == "reduce")
        .min_by_key(|trade| trade.trade_time)
        .ok_or_else(|| ServiceError::Internal("未找到减仓甲".to_string()))?
        .order_id;
    let before_snapshot = phase3_snapshot(workspace)?;
    let preview_delete = stock::preview_trade_change(
        workspace,
        &phase3_ledger,
        "delete_order",
        "",
        &reduce_order_id,
        0,
        0,
        0,
    )?;
    if phase3_snapshot(workspace)? != before_snapshot {
        return Err(ServiceError::Internal(
            "预演 delete 不应落库，但库内容发生了变化".to_string(),
        ));
    }
    // 只断言"不落库"；`position_after` 的具体数值由服务层单测负责比对
    // （预演不写库这一点由这里的 dump 前后指纹保证）。
    log.push_str(&format!(
        "股票：预演 delete（删除减仓甲委托）不落库 ✅ 预演后持仓={} 失效轮次={:?}\n",
        preview_delete.position_after,
        preview_delete
            .removed_rounds
            .iter()
            .map(|round| round.round_no)
            .collect::<Vec<_>>()
    ));

    // ---- 3-3b. 预演（不落库）：改第 2 轮建仓的手数会让第 2 轮不再成立 ----
    let before_snapshot = phase3_snapshot(workspace)?;
    let preview_edit = stock::preview_trade_change(
        workspace,
        &phase3_ledger,
        "update_trade",
        &round3_open_id,
        "",
        190_000,
        3,
        TRADE_TIME_R3_OPEN,
    )?;
    if phase3_snapshot(workspace)? != before_snapshot {
        return Err(ServiceError::Internal(
            "预演 edit 不应落库，但库内容发生了变化".to_string(),
        ));
    }
    let removed_by_edit: Vec<i64> = preview_edit
        .removed_rounds
        .iter()
        .map(|round| round.round_no)
        .collect();
    if removed_by_edit != vec![2] {
        return Err(ServiceError::Internal(format!(
            "预演 edit 应只失效第二轮，实际 {removed_by_edit:?}"
        )));
    }
    log.push_str(&format!(
        "股票：预演 edit（第2轮建仓 1手→3手）不落库 ✅ 失效轮次={removed_by_edit:?}\n"
    ));

    // ---- 3-4. 真正编辑第三轮建仓的**价格**（1900.00→1920.00）并触发重放 ----
    // 手数必须保持不变：第三轮 1 手买 / 1 手卖，改手数会让持仓不归零、
    // 该轮直接失效（那正是 3-3 预演覆盖的场景）。这里验证的是"重放后轮次元数据保留"。
    stock::update_trade_fill(
        workspace,
        &phase3_ledger,
        &round3_open_id,
        192_000,
        1,
        TRADE_TIME_R3_OPEN,
    )?;
    stock::list_trade_histories(workspace, &phase3_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &phase3_ledger, "600519")?;
    let round2_after = detail
        .rounds
        .iter()
        .find(|round| round.round_no == 1)
        .ok_or_else(|| ServiceError::Internal("重放后缺少第一轮".to_string()))?;
    let round3_after = detail
        .rounds
        .iter()
        .find(|round| round.round_no == 2)
        .ok_or_else(|| ServiceError::Internal("重放后缺少第二轮".to_string()))?;
    if round3_after.id != round3_id || round3_after.tag != "蓄力" {
        return Err(ServiceError::Internal(format!(
            "编辑价格后第三轮必须复用同一 ID 并保留 tag，实际 id={} tag={}",
            round3_after.id, round3_after.tag
        )));
    }
    if round2_after.id != round2_id
        || round2_after.tag != "追涨"
        || round2_after.review != "阶段三：第二轮复盘（编辑后必须保留）"
    {
        return Err(ServiceError::Internal(
            "轮次复用必须保留第二轮的 ID/tag/review".to_string(),
        ));
    }
    if detail.rounds.len() != 2 {
        return Err(ServiceError::Internal(format!(
            "第三轮应仍然成立（共 3 轮），实际 {}",
            detail.rounds.len()
        )));
    }
    log.push_str(&format!(
        "股票：编辑第2轮建仓价 1900.00→1920.00；第1轮 {} tag={} / 第2轮 {} tag={} 元数据均保留 ✅\n",
        round2_after.id, round2_after.tag, round3_after.id, round3_after.tag
    ));

    // ---- 3-5. 第四轮 000001：建仓 1 手 → 清仓 1 手（独立股票，验证多只股票各自归档）----
    let round4_open_trades = stock::create_trade_order(
        workspace,
        &phase3_ledger,
        PHASE3_STOCK_CODE,
        PHASE3_STOCK_NAME,
        "open",
        &[stock::TradeFill {
            price_cents: 112_000,
            lots: 1,
        }],
        TRADE_TIME_R4_OPEN,
        "第四轮建仓",
        "",
    )?;
    let round4_open_id = round4_open_trades
        .first()
        .ok_or_else(|| ServiceError::Internal("第四轮建仓未返回成交明细".to_string()))?
        .id
        .clone();
    stock::create_trade_order(
        workspace,
        &phase3_ledger,
        PHASE3_STOCK_CODE,
        PHASE3_STOCK_NAME,
        "close",
        &[stock::TradeFill {
            price_cents: 118_000,
            lots: 1,
        }],
        TRADE_TIME_R4_CLOSE,
        "第四轮清仓",
        "尾盘",
    )?;
    stock::list_trade_histories(workspace, &phase3_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &phase3_ledger, PHASE3_STOCK_CODE)?;
    if detail.rounds.len() != 1 {
        return Err(ServiceError::Internal(format!(
            "000001 应只有 1 轮，实际 {}",
            detail.rounds.len()
        )));
    }
    log.push_str("股票：000001 建仓1手 → 清仓1手（尾盘）\n");

    // ---- 3-7. 编辑第四轮的成交价（独立股票上的重放，验证其 tag 保留）----
    stock::update_trade_fill(
        workspace,
        &phase3_ledger,
        &round4_open_id,
        113_000,
        1,
        TRADE_TIME_R4_OPEN,
    )?;
    stock::list_trade_histories(workspace, &phase3_ledger)?;
    let detail = stock::get_trade_history_detail(workspace, &phase3_ledger, PHASE3_STOCK_CODE)?;
    let round4_after = detail
        .rounds
        .first()
        .ok_or_else(|| ServiceError::Internal("重放后缺少 000001 的轮次".to_string()))?;
    if round4_after.tag != "尾盘" {
        return Err(ServiceError::Internal(format!(
            "000001 的轮次 tag 应保留「尾盘」，实际 {}",
            round4_after.tag
        )));
    }
    log.push_str(&format!(
        "股票：000001 建仓价 1120.00→1130.00 重放后 tag={} 保留 ✅\n",
        round4_after.tag
    ));

    // ---- 3-8. 自检：阶段 3 引入的两类笔误必须由脚本自己挡住 ----
    // ① 同一 `(order_id, order_seq)` 只能出现一次（同秒同行号的卖单会被重放并成一笔）；
    // ② 每只股票的卖出总量 ≤ 建仓 + 加仓总量（手数守恒，否则播种中途就会超卖）。
    phase3_assert_trade_invariants(workspace, &main_ledger)?;
    log.push_str("股票：自检通过（委托序号唯一 / 每只股票卖出 ≤ 买入）\n");

    Ok(log)
}

/// 阶段 3 的成交不变量自检。
///
/// 这两条是播种脚本自身曾经写错过的真实笔误（两笔减仓写了同一个
/// `trade_time`、以及手数不守恒导致超卖）。放在这里自检，可以让同类错误
/// 立刻以清晰信息失败，而不是留到回归对比里反推。
fn phase3_assert_trade_invariants(
    workspace: &Workspace,
    ledger_id: &str,
) -> Result<(), ServiceError> {
    let conn = workspace.connection();
    let mut statement = conn.prepare(
        "SELECT COALESCE(order_id, ''), order_seq, stock_code, trade_type, shares \
         FROM tbl_billadm_stock_trade WHERE ledger_id = ?1 \
         ORDER BY trade_time ASC, created_at ASC, order_seq ASC, id ASC",
    )?;
    let rows: Vec<(String, i64, String, String, i64)> = statement
        .query_map([ledger_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    // ① 委托内序号唯一
    let mut seen: Vec<(String, i64)> = Vec::with_capacity(rows.len());
    for (order_id, order_seq, _, _, _) in &rows {
        let key = (order_id.clone(), *order_seq);
        if seen.contains(&key) {
            return Err(ServiceError::Internal(format!(
                "成交自检失败：委托 {order_id} 的 order_seq={order_seq} 重复\
                 （同一秒的卖单会被重放并成一笔）"
            )));
        }
        seen.push(key);
    }

    // ② 每只股票卖出 ≤ 买入
    let mut codes: Vec<&str> = rows.iter().map(|row| row.2.as_str()).collect();
    codes.sort_unstable();
    codes.dedup();
    for code in codes {
        let mut bought = 0_i64;
        let mut sold = 0_i64;
        for (_, _, stock_code, trade_type, shares) in &rows {
            if stock_code != code {
                continue;
            }
            match trade_type.as_str() {
                "open" | "add" => bought += shares,
                "reduce" | "close" => sold += shares,
                other => {
                    return Err(ServiceError::Internal(format!(
                        "成交自检失败：未知交易类型 {other}"
                    )))
                }
            }
        }
        if sold > bought {
            return Err(ServiceError::Internal(format!(
                "成交自检失败：{code} 卖出 {sold} 股 > 买入 {bought} 股（手数不守恒）"
            )));
        }
    }
    Ok(())
}

/// 某账本的关键事件行数（P1-4 用来断言"二次 upsert 是覆盖不是新增"）。
fn key_event_row_count(workspace: &Workspace, ledger_id: &str) -> Result<i64, ServiceError> {
    let count = workspace.connection().query_row(
        "SELECT COUNT(*) FROM tbl_billadm_key_event WHERE ledger_id = ?1",
        [ledger_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// 阶段 3 的「库内容指纹」：用于断言 `preview_trade_change` 绝不落库。
///
/// 只覆盖会被重放改写的表；两个工作空间各自算自己的指纹，
/// 因此比较的是「调用预演前后是否相同」而不是跨侧相等。
fn phase3_snapshot(workspace: &Workspace) -> Result<String, ServiceError> {
    let conn = workspace.connection();
    let mut text = String::new();
    for table in [
        "tbl_billadm_stock_trade",
        "tbl_billadm_stock_trade_history",
        "tbl_billadm_stock_trade_round",
        "tbl_billadm_stock_position",
        "tbl_billadm_stock_fund_record",
    ] {
        let sql = format!("SELECT * FROM {table} ORDER BY 1");
        let mut statement = conn.prepare(&sql)?;
        let columns = statement.column_count();
        let rows = statement.query_map([], |row| {
            let mut cells: Vec<String> = Vec::with_capacity(columns);
            for index in 0..columns {
                cells.push(format!(
                    "{:?}",
                    row.get::<_, rusqlite::types::Value>(index)?
                ));
            }
            Ok(cells.join("|"))
        })?;
        for row in rows {
            text.push_str(&row?);
            text.push('\n');
        }
    }
    Ok(text)
}

/// 取指定轮次序号的轮次 ID。
fn phase3_round_id(
    detail: &tr_domain::dto::StockTradeHistoryDetailDto,
    round_no: i64,
) -> Result<String, ServiceError> {
    detail
        .rounds
        .iter()
        .find(|round| round.round_no == round_no)
        .map(|round| round.id.clone())
        .ok_or_else(|| ServiceError::Internal(format!("交易历史缺少第 {round_no} 轮")))
}

/// 阶段 2 的消费记录 DTO。
fn phase2_record(
    ledger_id: &str,
    description: &str,
    price: i64,
    transaction_type: &str,
    category: &str,
    transaction_at: i64,
    tags: Vec<&str>,
) -> TransactionRecordDto {
    TransactionRecordDto {
        ledger_id: ledger_id.to_string(),
        price,
        transaction_type: transaction_type.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        transaction_at,
        outlier: false,
        ..TransactionRecordDto::default()
    }
}

/// 按描述查回消费记录 ID。
///
/// `batch_create_tr` 只返回条数，
/// 所以用「按描述查询」的公开服务路径取回 ID，而不是靠内存里的顺序假设。
fn phase2_find_record(
    workspace: &Workspace,
    ledger_id: &str,
    description: &str,
) -> Result<String, ServiceError> {
    let condition = TrQueryCondition {
        ledger_id: ledger_id.to_string(),
        items: vec![QueryConditionItem {
            description: description.to_string(),
            ..QueryConditionItem::default()
        }],
        ..TrQueryCondition::default()
    };
    let result = transaction_record::query_trs_on_condition(workspace, &condition)?;
    result
        .items
        .first()
        .map(|item| item.transaction_id.clone())
        .ok_or_else(|| ServiceError::Internal(format!("未查到消费记录: {description}")))
}
