//! xtask —— 仓库级验证工具。
//!
//! 提供数据兼容护栏与诊断能力：
//! * `schema-diff`：Rust 建库结果与参照物（基线 SQL 或 Go 0.27 建出的库）逐条比对
//! * `validate`   ：只读校验既有工作空间是否为最新格式（不建库、不改写）
//! * `dump`       ：把工作空间的业务表导成规范化 JSON（黄金对比与排障用）
//!
//! 用法：
//! ```text
//! cargo xtask schema-diff                        # 以 fixtures/schema/fresh_v0_27.sql 为参照
//! cargo xtask schema-diff --go-db <path/db>      # 直接与 Go 0.27 建出的库比对（更强）
//! cargo xtask validate <workspace-dir>
//! cargo xtask dump <workspace-dir> [--table <name>]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use rusqlite::Connection;

use tr_domain::consts;
use tr_store::{schema, Workspace};

mod parity;
mod seed;

/// 工作空间里的全部业务表（不含 sqlite 内部表与迁移登记表），按依赖顺序排列。
const BUSINESS_TABLES: &[&str] = &[
    "tbl_billadm_ledger",
    "tbl_billadm_transaction_record",
    "tbl_billadm_transaction_record_tag",
    "tbl_billadm_category",
    "tbl_billadm_tag",
    "tbl_billadm_transaction_tpl",
    "tbl_billadm_chart",
    "tbl_billadm_key_event",
    "tbl_billadm_key_event_image",
    "tbl_billadm_diary_entry",
    "tbl_billadm_stock_account",
    "tbl_billadm_stock_fee_setting",
    "tbl_billadm_stock_fund_record",
    "tbl_billadm_stock_position",
    "tbl_billadm_stock_trade",
    "tbl_billadm_stock_trade_history",
    "tbl_billadm_stock_trade_round",
    "tbl_billadm_stock_trade_tag_setting",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("schema-diff") => schema_diff(&args[1..]),
        Some("validate") => validate_workspace(&args[1..]),
        Some("dump") => dump_workspace(&args[1..]),
        Some("seed") => seed_workspace(&args[1..]),
        Some("parity") => parity_command(&args[1..]),
        Some(other) => {
            eprintln!("未知任务: {other}");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    println!(
        "用法:\n  \
         cargo xtask schema-diff [--go-db <path>]\n      \
         校验 Rust 建库结果与参照物逐条一致\n  \
         cargo xtask validate <workspace-dir>\n      \
         只读校验某个工作空间是否为最新格式（不会创建或修改任何文件）\n  \
         cargo xtask dump <workspace-dir> [--table <name>]\n      \
         把业务表导成规范化 JSON（只读）\n  \
         cargo xtask seed <workspace-dir>\n      \
         新建（若不存在）并用服务层写入一份可复现的示例数据\n  \
         cargo xtask parity normalize <in.json> [--out <out.json>]\n      \
         把易变字段（UUID/时间戳）替换为占位符\n  \
         cargo xtask parity diff <a.json> <b.json>\n      \
         归一化后逐路径比较两份结果（黄金对比）"
    );
}

/// `parity` 子命令：归一化与差异报告（黄金对比的底座）。
fn parity_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("normalize") => parity_normalize(&args[1..]),
        Some("diff") => parity_diff(&args[1..]),
        other => {
            eprintln!(
                "parity 子命令：normalize <in.json> [--out <out.json>] | diff <a.json> <b.json>（收到 {other:?}）"
            );
            ExitCode::FAILURE
        }
    }
}

/// 把易变字段替换为占位符（UUID / 时间戳），便于人工核对与提交基线。
fn parity_normalize(args: &[String]) -> ExitCode {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                match args.get(index) {
                    Some(path) => output = Some(PathBuf::from(path)),
                    None => {
                        eprintln!("--out 需要一个路径参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other => {
                if input.is_none() {
                    input = Some(PathBuf::from(other));
                } else {
                    eprintln!("多余的参数: {other}");
                    return ExitCode::FAILURE;
                }
            }
        }
        index += 1;
    }

    let Some(input) = input else {
        eprintln!("normalize 需要一个输入 JSON 文件");
        return ExitCode::FAILURE;
    };

    let mut value = match read_json(&input) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    parity::normalize(&mut value);

    let text = match serde_json::to_string_pretty(&value) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("序列化失败: {error}");
            return ExitCode::FAILURE;
        }
    };
    match output {
        Some(path) => match std::fs::write(&path, text) {
            Ok(()) => {
                println!("已写出归一化结果: {}", path.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("写入失败: {error}");
                ExitCode::FAILURE
            }
        },
        None => {
            println!("{text}");
            ExitCode::SUCCESS
        }
    }
}

/// 比较两个 JSON（先各自归一化，再逐路径比较）；有任何差异即返回失败。
fn parity_diff(args: &[String]) -> ExitCode {
    let (Some(left_path), Some(right_path)) = (args.first(), args.get(1)) else {
        eprintln!("diff 需要两个 JSON 文件参数");
        return ExitCode::FAILURE;
    };

    let mut left = match read_json(&PathBuf::from(left_path)) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let mut right = match read_json(&PathBuf::from(right_path)) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    parity::normalize(&mut left);
    parity::normalize(&mut right);

    let differences = parity::diff(&left, &right);
    if differences.is_empty() {
        println!("parity diff 通过：归一化后两侧完全一致 ✅");
        return ExitCode::SUCCESS;
    }

    println!("parity diff 失败：{} 处差异", differences.len());
    for difference in differences.iter().take(50) {
        println!(
            "  {}\n    A: {}\n    B: {}",
            difference.path, difference.left, difference.right
        );
    }
    if differences.len() > 50 {
        println!("  …（共 {} 处，仅显示前 50 处）", differences.len());
    }
    ExitCode::FAILURE
}

fn read_json(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 {} 失败: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("解析 {} 失败: {error}", path.display()))
}

/// 新建（必要时）并播种一个工作空间。
fn seed_workspace(args: &[String]) -> ExitCode {
    let Some(raw_dir) = args.first() else {
        eprintln!("seed 需要一个工作空间目录参数");
        return ExitCode::FAILURE;
    };

    let directory = PathBuf::from(raw_dir);
    let workspace = match Workspace::open(&directory) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("打开工作空间失败: {error}");
            return ExitCode::FAILURE;
        }
    };

    match seed::seed(&workspace) {
        Ok(summary) => {
            println!("已在 {} 播种示例数据：", directory.display());
            print!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("播种失败: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 只读导出业务表为规范化 JSON。
///
/// 「规范化」的含义（供黄金对比使用）：按 `rowid` 升序、列名升序输出为对象数组，
/// 因此不受列顺序或插入顺序以外的偶然因素影响；调用方再自行剔除时间戳这类易变字段。
fn dump_workspace(args: &[String]) -> ExitCode {
    let mut directory: Option<PathBuf> = None;
    let mut only_table: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--table" => {
                index += 1;
                match args.get(index) {
                    Some(table) => only_table = Some(table.clone()),
                    None => {
                        eprintln!("--table 需要一个表名");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other => {
                if directory.is_none() {
                    directory = Some(PathBuf::from(other));
                } else {
                    eprintln!("多余的参数: {other}");
                    return ExitCode::FAILURE;
                }
            }
        }
        index += 1;
    }

    let Some(directory) = directory else {
        eprintln!("dump 需要一个工作空间目录参数");
        return ExitCode::FAILURE;
    };
    let db_path = directory.join(consts::DB_NAME);
    if !db_path.exists() {
        eprintln!("目录中没有 {}: {}", consts::DB_NAME, directory.display());
        return ExitCode::FAILURE;
    }

    let conn =
        match Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(error) => {
                eprintln!("打开数据库失败: {error}");
                return ExitCode::FAILURE;
            }
        };

    let tables: Vec<&str> = match only_table.as_deref() {
        Some(table) => vec![table],
        None => BUSINESS_TABLES.to_vec(),
    };

    let mut output = serde_json::Map::new();
    for table in tables {
        match dump_table(&conn, table) {
            Ok(rows) => {
                output.insert(table.to_string(), serde_json::Value::Array(rows));
            }
            Err(error) => {
                eprintln!("导出 {table} 失败: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    match serde_json::to_string_pretty(&serde_json::Value::Object(output)) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("序列化失败: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 读一张表的全部行（列名按字典序排列，便于跨实现比对）。
fn dump_table(conn: &Connection, table: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .map_err(|error| error.to_string())?;

    let mut names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    names.sort();

    // 先按排好序的列名解析出列下标（`Statement::column_index`），
    // 再移进 query_map 的闭包——避免在闭包里再去查列名。
    let mut indices = Vec::with_capacity(names.len());
    for name in &names {
        indices.push(
            statement
                .column_index(name.as_str())
                .map_err(|error| error.to_string())?,
        );
    }

    let rows = statement
        .query_map([], move |row| {
            let mut object = serde_json::Map::new();
            for (position, index) in indices.iter().enumerate() {
                object.insert(names[position].clone(), value_to_json(row.get_ref(*index)?));
            }
            Ok(serde_json::Value::Object(object))
        })
        .map_err(|error| error.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|error| error.to_string())?);
    }
    Ok(result)
}

/// SQLite 值 → JSON。BLOB 用十六进制字符串表示（我们当前没有 BLOB 列，仅为完备性）。
fn value_to_json(value: rusqlite::types::ValueRef<'_>) -> serde_json::Value {
    match value {
        rusqlite::types::ValueRef::Null => serde_json::Value::Null,
        rusqlite::types::ValueRef::Integer(number) => serde_json::Value::from(number),
        rusqlite::types::ValueRef::Real(number) => serde_json::Value::from(number),
        rusqlite::types::ValueRef::Text(bytes) => {
            serde_json::Value::from(String::from_utf8_lossy(bytes).into_owned())
        }
        rusqlite::types::ValueRef::Blob(bytes) => {
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            serde_json::Value::from(hex)
        }
    }
}

/// 只读校验一个既有工作空间。
///
/// 与 `Workspace::open` 的区别：**不会创建目录或数据库**——
/// 目录里没有 `transactions.db` 时直接报错，避免"校验"动作意外建库。
fn validate_workspace(args: &[String]) -> ExitCode {
    let Some(raw_dir) = args.first() else {
        eprintln!("validate 需要一个工作空间目录参数");
        return ExitCode::FAILURE;
    };
    let directory = PathBuf::from(raw_dir);
    if !directory.is_dir() {
        eprintln!("目录不存在: {}", directory.display());
        return ExitCode::FAILURE;
    }
    let db_path = directory.join(consts::DB_NAME);
    if !db_path.exists() {
        eprintln!(
            "目录中没有 {}（本命令只校验既有工作空间，不会建库）: {}",
            consts::DB_NAME,
            directory.display()
        );
        return ExitCode::FAILURE;
    }

    // 直接以只读方式打开并跑校验：不经过 Workspace::open，确保零副作用
    let conn =
        match Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(error) => {
                eprintln!("打开数据库失败: {error}");
                return ExitCode::FAILURE;
            }
        };

    match schema::validate_current(&conn) {
        Ok(()) => {
            println!("✅ 工作空间格式校验通过: {}", directory.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("❌ {}", error);
            ExitCode::FAILURE
        }
    }
}

/// 建库 → 导出语句 → 与参照物比对。
fn schema_diff(args: &[String]) -> ExitCode {
    let mut go_db: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--go-db" => {
                index += 1;
                match args.get(index) {
                    Some(path) => go_db = Some(PathBuf::from(path)),
                    None => {
                        eprintln!("--go-db 需要一个路径参数");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other => {
                eprintln!("未知参数: {other}");
                return ExitCode::FAILURE;
            }
        }
        index += 1;
    }

    let temp_dir = fresh_temp_dir();
    if let Err(error) = std::fs::create_dir_all(&temp_dir) {
        eprintln!("创建临时目录失败: {error}");
        return ExitCode::FAILURE;
    }

    let result = (|| -> Result<(), String> {
        let workspace = Workspace::open(&temp_dir).map_err(|error| error.to_string())?;
        let conn = workspace.connection();
        schema::validate_current(&conn).map_err(|error| error.to_string())?;

        let actual = dump_schema(&conn)?;
        let (reference, reference_label) = match go_db.as_deref() {
            Some(path) if path.exists() => {
                let conn =
                    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(|error| error.to_string())?;
                (
                    dump_schema(&conn)?,
                    format!("Go 0.27 库 {}", path.display()),
                )
            }
            Some(path) => {
                eprintln!("指定的 --go-db 不存在: {}", path.display());
                return Err("参照库不存在".to_string());
            }
            None => (
                reference_from_sql_file(schema::FRESH_SCHEMA_SQL)?,
                "fixtures/schema/fresh_v0_27.sql".to_string(),
            ),
        };

        println!("参照物: {reference_label}");
        println!(
            "语句条数: Rust 建库 {} 条 / 参照 {} 条",
            actual.len(),
            reference.len()
        );

        let mut mismatches = 0_usize;
        let max_len = actual.len().max(reference.len());
        for index in 0..max_len {
            match (actual.get(index), reference.get(index)) {
                (Some(left), Some(right)) if left == right => {}
                (Some(left), Some(right)) => {
                    mismatches += 1;
                    println!(
                        "\n第 {} 条不一致:\n  Rust:   {left}\n  参照:   {right}",
                        index + 1
                    );
                }
                (Some(left), None) => {
                    mismatches += 1;
                    println!("\n第 {} 条仅存在于 Rust 建库结果: {left}", index + 1);
                }
                (None, Some(right)) => {
                    mismatches += 1;
                    println!("\n第 {} 条仅存在于参照物: {right}", index + 1);
                }
                (None, None) => break,
            }
        }

        if mismatches == 0 {
            println!("schema-diff 通过：结构逐条一致 ✅");
            Ok(())
        } else {
            Err(format!("schema-diff 失败：{mismatches} 条语句不一致"))
        }
    })();

    std::fs::remove_dir_all(&temp_dir).ok();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// 导出建表/建索引语句（顺序与 `sqlite_master` 中一致，即创建顺序。
/// `.schema` 输出同样按该顺序，因此可以直接与导出的 DDL 文本比对）。
fn dump_schema(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut statements = Vec::new();
    for row in rows {
        statements.push(row.map_err(|error| error.to_string())?);
    }
    Ok(statements)
}

/// 从建库 SQL 文件里提取 CREATE 语句（忽略注释与 INSERT）。
fn reference_from_sql_file(sql: &str) -> Result<Vec<String>, String> {
    let statements: Vec<String> = sql
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_uppercase().starts_with("CREATE "))
        .map(|line| line.trim_end_matches(';').to_string())
        .collect();
    if statements.is_empty() {
        return Err("参照 SQL 文件里没有解析到任何 CREATE 语句".to_string());
    }
    Ok(statements)
}

fn fresh_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "tr-xtask-schema-diff-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ))
}
