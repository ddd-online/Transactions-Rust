//! 日记服务：日期树（列表/查询）、导入（扫描目录 + 逐文件导入）、导出（按月/年筛选后写 `<date>.md`）。
//!
//! **日记按账本隔离**：所有读写都必须带 `ledger_id`（唯一键是 `(ledger_id, date)`），
//! 导入落到指定账本，导出只导指定账本。
//!
//! 编码回退链：合法 UTF-8 直接用；带 BOM 的 UTF-16 解码；
//! 否则按 GBK 解码；仍不合法则把非法序列替换为 `?`。
//! 这条链子是历史日记文件（Windows 记事本保存的 ANSI/UTF-16）能正确导入的关键。

use std::path::Path;

use tr_domain::dto::{DiaryExportFileError, DiaryExportResult, DiaryFileItem, DiaryScanResponse};
use tr_domain::models::{DiaryDateItem, DiaryEntry};
use tr_store::dao::diary::DiaryDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 某账本的日期列表（倒序）。
pub fn list_dates(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Vec<DiaryDateItem>> {
    let entries = DiaryDao::list_dates(&workspace.connection(), ledger_id)?;
    Ok(entries.into_iter().map(DiaryDateItem::from).collect())
}

/// 按账本 + 日期取日记；不存在时报错（而不是返回空条目）。
pub fn get_by_date(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
) -> ServiceResult<DiaryEntry> {
    Ok(DiaryDao::query_by_date(
        &workspace.connection(),
        ledger_id,
        date,
    )?)
}

/// 保存日记，返回写入后的条目（字数按 Unicode 字符数计算）。
pub fn upsert(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
    content: &str,
    mood: &str,
) -> ServiceResult<DiaryEntry> {
    // 写入时一并填充创建/更新时间，返回的结构体带着它们
    let now = tr_store::util::now_unix();
    let entry = DiaryEntry {
        id: tr_store::util::new_uuid(),
        date: date.to_string(),
        content: content.to_string(),
        word_count: tr_store::util::char_count(content),
        mood: mood.to_string(),
        created_at: now,
        updated_at: now,
        ledger_id: ledger_id.to_string(),
    };
    DiaryDao::upsert(&workspace.connection(), &entry)?;
    Ok(entry)
}

/// 删除某账本某天的日记（别的账本的同一天不受影响）。
pub fn delete_by_date(workspace: &Workspace, ledger_id: &str, date: &str) -> ServiceResult<()> {
    tracing::info!("删除日记, 账本: {}, 日期: {}", ledger_id, date);
    DiaryDao::delete_by_date(&workspace.connection(), ledger_id, date)?;
    Ok(())
}

/// 递归扫描目录，找出 `YYYY-MM-DD.txt` / `YYYY-MM-DD.md` 并按日期升序返回。
///
/// 只读文件系统、不碰数据库，因此**不需要账本**。
pub fn scan_directory(directory: &str) -> ServiceResult<DiaryScanResponse> {
    let mut files: Vec<DiaryFileItem> = Vec::new();
    collect_diary_files(Path::new(directory), &mut files)
        .map_err(|error| ServiceError::Internal(format!("扫描目录失败: {error}")))?;
    files.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(DiaryScanResponse { files })
}

/// 导入单个文件（编码自动识别）到指定账本，导入后返回条目。
pub fn import_file(
    workspace: &Workspace,
    ledger_id: &str,
    path: &str,
    date: &str,
) -> ServiceResult<DiaryEntry> {
    let raw = std::fs::read(path)
        .map_err(|error| ServiceError::Internal(format!("读取文件失败 {path}: {error}")))?;
    let content = decode_text(&raw);
    upsert(workspace, ledger_id, date, &content, "")
}

/// 把某账本的日记导出到目录：`year`/`month` 为 0 表示不限（month 需配合 year）。
pub fn export_to_directory(
    workspace: &Workspace,
    ledger_id: &str,
    directory: &str,
    year: i64,
    month: i64,
) -> ServiceResult<DiaryExportResult> {
    let mut entries = DiaryDao::list_by_ledger(&workspace.connection(), ledger_id)?;

    if year > 0 || month > 0 {
        entries.retain(|entry| {
            let Some((entry_year, entry_month)) = parse_year_month(&entry.date) else {
                return false; // 日期非法：直接跳过
            };
            if year > 0 && i64::from(entry_year) != year {
                return false;
            }
            if month > 0 && i64::from(entry_month) != month {
                return false;
            }
            true
        });
    }

    std::fs::create_dir_all(directory)
        .map_err(|error| ServiceError::Internal(format!("创建导出目录失败: {error}")))?;

    let mut result = DiaryExportResult {
        total: entries.len() as i32,
        success: 0,
        failed: Vec::new(),
    };

    for entry in &entries {
        let file_path = Path::new(directory).join(format!("{}.md", entry.date));
        match std::fs::write(&file_path, entry.content.as_bytes()) {
            Ok(()) => result.success += 1,
            Err(error) => result.failed.push(DiaryExportFileError {
                date: entry.date.clone(),
                error: error.to_string(),
            }),
        }
    }

    Ok(result)
}

/// 递归收集日记文件（`filepath.WalkDir` 的等价实现，遇到错误即中止）。
fn collect_diary_files(directory: &Path, out: &mut Vec<DiaryFileItem>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_diary_files(&path, out)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(date) = parse_diary_file_name(name) {
            out.push(DiaryFileItem {
                date: date.to_string(),
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    Ok(())
}

/// 匹配 `^(\d{4}-\d{2}-\d{2})\.(txt|md)$`，并校验日期真实存在。
fn parse_diary_file_name(name: &str) -> Option<&str> {
    let (stem, extension) = name.rsplit_once('.')?;
    if extension != "txt" && extension != "md" {
        return None;
    }
    if !is_valid_date(stem) {
        return None;
    }
    Some(stem)
}

/// `YYYY-MM-DD` 严格校验（含闰年）：格式不合法或日期不存在都算非法。
///
/// 复用股票服务里同一口径的严格解析（chrono 的公历规则），
/// 不再自己手写 `days_in_month` / `is_leap_year`。
fn is_valid_date(value: &str) -> bool {
    crate::stock::parse_strict_date(value).is_some()
}

/// `YYYY-MM-DD` → (年, 月)。
fn parse_year_month(value: &str) -> Option<(i32, u32)> {
    if !is_valid_date(value) {
        return None;
    }
    Some((value[0..4].parse().ok()?, value[5..7].parse().ok()?))
}

/// 文本解码回退链（UTF-8 → UTF-16(BOM) → GBK → 非法序列替换为 `?`）。
fn decode_text(raw: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(raw) {
        return text.to_string();
    }
    if let Some(decoded) = decode_utf16(raw) {
        return decoded;
    }
    let (decoded, _, had_errors) = encoding_rs::GBK.decode(raw);
    if !had_errors {
        return decoded.into_owned();
    }
    replace_invalid_utf8(raw)
}

/// 带 BOM 的 UTF-16 解码；无 BOM 返回 `None`。
fn decode_utf16(raw: &[u8]) -> Option<String> {
    if raw.len() < 2 {
        return None;
    }
    let little_endian = match (raw[0], raw[1]) {
        (0xFF, 0xFE) => true,
        (0xFE, 0xFF) => false,
        _ => return None,
    };
    let units: Vec<u16> = raw[2..]
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// 把非法 UTF-8 序列替换为 `?`。
fn replace_invalid_utf8(raw: &[u8]) -> String {
    let mut output = String::new();
    let mut rest = raw;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                output.push_str(text);
                return output;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                output.push_str(std::str::from_utf8(&rest[..valid]).expect("合法前缀"));
                output.push('?');
                let skip = error.error_len().unwrap_or(1);
                rest = &rest[(valid + skip).min(rest.len())..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;

    /// 测试统一用的账本 id（服务层不校验账本是否存在，DAO 也不做外键约束）。
    const LEDGER: &str = "l1";

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tr-diary-files-{tag}-{}-{}",
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
    fn upsert_counts_characters_and_returns_entry() {
        let (workspace, dir) = workspace("upsert");
        let entry = upsert(&workspace, LEDGER, "2026-01-01", "中文abc", "开心").unwrap();
        assert_eq!(entry.word_count, 5, "字数按 Unicode 字符数");
        assert_eq!(entry.mood, "开心");
        assert!(!entry.id.is_empty());

        let loaded = get_by_date(&workspace, LEDGER, "2026-01-01").unwrap();
        assert_eq!(loaded.content, "中文abc");
        assert_eq!(loaded.word_count, 5);

        // 覆盖保存：内容更新、id 保留
        upsert(&workspace, LEDGER, "2026-01-01", "改了", "").unwrap();
        let loaded = get_by_date(&workspace, LEDGER, "2026-01-01").unwrap();
        assert_eq!(loaded.content, "改了");
        assert_eq!(loaded.word_count, 2);

        let dates = list_dates(&workspace, LEDGER).unwrap();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].word_count, 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_entry_reports_error() {
        let (workspace, dir) = workspace("missing");
        let error = get_by_date(&workspace, LEDGER, "2000-01-01").unwrap_err();
        assert_eq!(error.to_string(), "record not found");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_finds_only_valid_names_recursively_and_sorts() {
        let dir = temp_dir("scan");
        let nested = dir.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("2026-01-02.md"), "b").unwrap();
        std::fs::write(nested.join("2026-01-01.txt"), "a").unwrap();
        std::fs::write(dir.join("2026-01-03.markdown"), "c").unwrap(); // 扩展名不支持
        std::fs::write(dir.join("2026-02-30.md"), "d").unwrap(); // 不存在的日期
        std::fs::write(dir.join("note.md"), "e").unwrap(); // 无日期
        std::fs::write(dir.join("x2026-01-04.md"), "f").unwrap(); // 前缀多余

        let response = scan_directory(dir.to_str().unwrap()).unwrap();
        let dates: Vec<&str> = response
            .files
            .iter()
            .map(|file| file.date.as_str())
            .collect();
        assert_eq!(dates, vec!["2026-01-01", "2026-01-02"]);
        assert!(response.files[1].path.ends_with("2026-01-02.md"));

        // 目录不存在时报错
        assert!(scan_directory(dir.join("nope").to_str().unwrap()).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn import_decodes_utf8_utf16_and_gbk() {
        let (workspace, dir) = workspace("import");
        let files = temp_dir("import");
        let utf8_path = files.join("utf8.txt");
        let utf16_path = files.join("utf16.txt");
        let gbk_path = files.join("gbk.txt");
        let broken_path = files.join("broken.txt");

        std::fs::write(&utf8_path, "中文内容".as_bytes()).unwrap();
        // UTF-16LE + BOM
        let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
        for unit in "中文内容".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&utf16_path, &utf16).unwrap();
        // GBK：用 encoding_rs 反向编码得到字节，保证是真的 GBK
        let (gbk_bytes, _, _) = encoding_rs::GBK.encode("中文内容");
        std::fs::write(&gbk_path, &gbk_bytes).unwrap();
        // 非法 UTF-8（0xFF 0xFE 之外的孤立字节）→ 替换为 ?
        std::fs::write(&broken_path, [b'a', 0xFF, b'b']).unwrap();

        for (path, date) in [
            (&utf8_path, "2026-01-01"),
            (&utf16_path, "2026-01-02"),
            (&gbk_path, "2026-01-03"),
        ] {
            let entry = import_file(&workspace, LEDGER, path.to_str().unwrap(), date).unwrap();
            assert_eq!(entry.content, "中文内容", "{path:?} 解码失败");
            assert_eq!(entry.word_count, 4);
            assert_eq!(entry.mood, "");
        }

        let broken = import_file(
            &workspace,
            LEDGER,
            broken_path.to_str().unwrap(),
            "2026-01-04",
        )
        .unwrap();
        assert!(
            broken.content.contains('?'),
            "非法字节应被替换: {:?}",
            broken.content
        );

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&files).ok();
    }

    #[test]
    fn export_filters_by_year_and_month() {
        let (workspace, dir) = workspace("export");
        upsert(&workspace, LEDGER, "2025-12-31", "去年", "").unwrap();
        upsert(&workspace, LEDGER, "2026-01-01", "一月", "").unwrap();
        upsert(&workspace, LEDGER, "2026-02-01", "二月", "").unwrap();

        let out = temp_dir("export-out");
        // 全部导出
        let result = export_to_directory(&workspace, LEDGER, out.to_str().unwrap(), 0, 0).unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.success, 3);
        assert!(result.failed.is_empty());
        assert!(out.join("2026-01-01.md").exists());
        assert_eq!(
            std::fs::read_to_string(out.join("2026-02-01.md")).unwrap(),
            "二月"
        );

        // 按年
        let year_out = temp_dir("export-year");
        let result =
            export_to_directory(&workspace, LEDGER, year_out.to_str().unwrap(), 2026, 0).unwrap();
        assert_eq!(result.total, 2);
        assert!(!year_out.join("2025-12-31.md").exists());

        // 按年月
        let month_out = temp_dir("export-month");
        let result =
            export_to_directory(&workspace, LEDGER, month_out.to_str().unwrap(), 2026, 2).unwrap();
        assert_eq!(result.total, 1);
        assert!(month_out.join("2026-02-01.md").exists());
        assert!(!month_out.join("2026-01-01.md").exists());

        // 目录不存在时自动创建
        let nested = out.join("a").join("b");
        assert!(export_to_directory(&workspace, LEDGER, nested.to_str().unwrap(), 2026, 1).is_ok());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
        std::fs::remove_dir_all(&year_out).ok();
        std::fs::remove_dir_all(&month_out).ok();
    }

    #[test]
    fn date_validation_is_strict() {
        assert!(is_valid_date("2026-01-01"));
        assert!(is_valid_date("2024-02-29"), "闰年 2 月 29 日合法");
        assert!(!is_valid_date("2026-02-29"), "平年 2 月 29 日非法");
        assert!(!is_valid_date("2026-13-01"));
        assert!(!is_valid_date("2026-00-10"));
        assert!(!is_valid_date("2026-01-00"));
        assert!(!is_valid_date("2026-1-01"), "必须零填充");
        assert!(!is_valid_date("26-01-01"));
        assert!(!is_valid_date("2026/01/01"));
        assert_eq!(parse_year_month("2026-03-09"), Some((2026, 3)));
        assert_eq!(parse_year_month("2026-03"), None);
    }

    /// 用户真实流程：A 工作空间导出 → 扫描目录 → 导入 B 工作空间，正文必须逐字节还原。
    #[test]
    fn export_then_import_round_trips_content() {
        let (source, source_dir) = workspace("roundtrip-src");
        let (target, target_dir) = workspace("roundtrip-dst");

        let contents = [
            ("2026-01-01", "第一行\n第二行"),
            ("2026-01-02", "带 emoji 🙂 与制表符\t结束"),
            ("2026-01-03", "无结尾换行"),
        ];
        for (date, content) in contents {
            upsert(&source, LEDGER, date, content, "开心").unwrap();
        }

        let out = temp_dir("roundtrip-out");
        let result = export_to_directory(&source, LEDGER, out.to_str().unwrap(), 0, 0).unwrap();
        assert_eq!((result.total, result.success), (3, 3));

        // 扫描出的文件按日期升序，且能被 import_file 逐个吃下
        let scan = scan_directory(out.to_str().unwrap()).unwrap();
        assert_eq!(scan.files.len(), 3);
        let dates: Vec<&str> = scan.files.iter().map(|file| file.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-01-01", "2026-01-02", "2026-01-03"]);

        for file in &scan.files {
            let entry = import_file(&target, LEDGER, &file.path, &file.date).unwrap();
            assert_eq!(entry.date, file.date);
            // 导出格式是纯 Markdown，只有正文；mood 不随文件走
            assert_eq!(entry.mood, "");
        }

        for (date, content) in contents {
            let loaded = get_by_date(&target, LEDGER, date).unwrap();
            assert_eq!(loaded.content, content, "{date} 正文未逐字节还原");
            assert_eq!(loaded.word_count, tr_store::util::char_count(content));
        }

        for dir in [source_dir, target_dir, out] {
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn delete_is_idempotent() {
        let (workspace, dir) = workspace("delete");
        upsert(&workspace, LEDGER, "2026-01-01", "内容", "").unwrap();
        delete_by_date(&workspace, LEDGER, "2026-01-01").unwrap();
        delete_by_date(&workspace, LEDGER, "2026-01-01").unwrap();
        assert!(list_dates(&workspace, LEDGER).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 同一天在两个账本里各存一篇：互不覆盖、互不可见（复合唯一键的行为）。
    #[test]
    fn same_date_in_two_ledgers_is_independent() {
        let (workspace, dir) = workspace("two-ledgers");
        let other = "l2";

        upsert(&workspace, LEDGER, "2026-01-01", "账本一的正文", "开心").unwrap();
        upsert(&workspace, other, "2026-01-01", "账本二的正文", "平静").unwrap();

        assert_eq!(
            get_by_date(&workspace, LEDGER, "2026-01-01")
                .unwrap()
                .content,
            "账本一的正文"
        );
        assert_eq!(
            get_by_date(&workspace, other, "2026-01-01")
                .unwrap()
                .content,
            "账本二的正文"
        );
        assert_eq!(list_dates(&workspace, LEDGER).unwrap().len(), 1);
        assert_eq!(list_dates(&workspace, other).unwrap().len(), 1);

        // 删掉账本一那篇，账本二不受影响
        delete_by_date(&workspace, LEDGER, "2026-01-01").unwrap();
        assert!(list_dates(&workspace, LEDGER).unwrap().is_empty());
        assert_eq!(list_dates(&workspace, other).unwrap().len(), 1);

        // 保存返回的条目带着账本 id
        let saved = upsert(&workspace, other, "2026-01-02", "第二天", "").unwrap();
        assert_eq!(saved.ledger_id, other);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 导出只导指定账本（与导入对称）：另一个账本的日记绝不落盘。
    #[test]
    fn export_only_covers_the_given_ledger() {
        let (workspace, dir) = workspace("export-ledger");
        let other = "l2";
        upsert(&workspace, LEDGER, "2026-01-01", "我的日记", "").unwrap();
        upsert(&workspace, other, "2026-01-02", "别人的日记", "").unwrap();

        let out = temp_dir("export-ledger-out");
        let result = export_to_directory(&workspace, LEDGER, out.to_str().unwrap(), 0, 0).unwrap();
        assert_eq!((result.total, result.success), (1, 1));
        assert!(out.join("2026-01-01.md").exists());
        assert!(
            !out.join("2026-01-02.md").exists(),
            "别的账本的日记不得被导出"
        );

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }
}
