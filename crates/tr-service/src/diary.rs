//! 日记服务。对照 Go `kernel/service/diary_service.go`。
//!
//! 三块能力：日期树（列表/查询）、导入（扫描目录 + 逐文件导入）、导出（按月/年筛选后写 `<date>.md`）。
//!
//! 编码回退链与原实现一致：合法 UTF-8 直接用；带 BOM 的 UTF-16 解码；
//! 否则按 GBK 解码；仍不合法则把非法序列替换为 `?`（对齐 Go 的 `bytes.ToValidUTF8(raw, "?")`）。
//! 这条链子是历史日记文件（Windows 记事本保存的 ANSI/UTF-16）能正确导入的关键。

use std::path::Path;

use tr_domain::dto::{DiaryExportFileError, DiaryExportResult, DiaryFileItem, DiaryScanResponse};
use tr_domain::models::{DiaryDateItem, DiaryEntry};
use tr_store::dao::diary::DiaryDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 日期列表（倒序）。
pub fn list_dates(workspace: &Workspace) -> ServiceResult<Vec<DiaryDateItem>> {
    let entries = DiaryDao::list_dates(&workspace.connection())?;
    Ok(entries.into_iter().map(DiaryDateItem::from).collect())
}

/// 按日期取日记；不存在时报错（与原实现一致，而不是返回空条目）。
pub fn get_by_date(workspace: &Workspace, date: &str) -> ServiceResult<DiaryEntry> {
    Ok(DiaryDao::query_by_date(&workspace.connection(), date)?)
}

/// 保存日记，返回写入后的条目（字数按 Unicode 字符数计算）。
pub fn upsert(
    workspace: &Workspace,
    date: &str,
    content: &str,
    mood: &str,
) -> ServiceResult<DiaryEntry> {
    // 与原实现一致：GORM 在 Create 时会填充创建/更新时间，返回的结构体带着它们
    let now = tr_store::util::now_unix();
    let entry = DiaryEntry {
        id: tr_store::util::new_uuid(),
        date: date.to_string(),
        content: content.to_string(),
        word_count: tr_store::util::char_count(content),
        mood: mood.to_string(),
        created_at: now,
        updated_at: now,
    };
    DiaryDao::upsert(&workspace.connection(), &entry)?;
    Ok(entry)
}

/// 删除日记。
pub fn delete_by_date(workspace: &Workspace, date: &str) -> ServiceResult<()> {
    tracing::info!("删除日记, 日期: {}", date);
    DiaryDao::delete_by_date(&workspace.connection(), date)?;
    Ok(())
}

/// 递归扫描目录，找出 `YYYY-MM-DD.txt` / `YYYY-MM-DD.md` 并按日期升序返回。
pub fn scan_directory(directory: &str) -> ServiceResult<DiaryScanResponse> {
    let mut files: Vec<DiaryFileItem> = Vec::new();
    collect_diary_files(Path::new(directory), &mut files)
        .map_err(|error| ServiceError::Internal(format!("扫描目录失败: {error}")))?;
    files.sort_by(|left, right| left.date.cmp(&right.date));
    Ok(DiaryScanResponse { files })
}

/// 导入单个文件（编码自动识别），导入后返回条目。
pub fn import_file(workspace: &Workspace, path: &str, date: &str) -> ServiceResult<DiaryEntry> {
    let raw = std::fs::read(path)
        .map_err(|error| ServiceError::Internal(format!("读取文件失败 {path}: {error}")))?;
    let content = decode_text(&raw);
    upsert(workspace, date, &content, "")
}

/// 导出日记到目录：`year`/`month` 为 0 表示不限（month 需配合 year）。
pub fn export_to_directory(
    workspace: &Workspace,
    directory: &str,
    year: i64,
    month: i64,
) -> ServiceResult<DiaryExportResult> {
    let mut entries = DiaryDao::list_all(&workspace.connection())?;

    if year > 0 || month > 0 {
        entries.retain(|entry| {
            let Some((entry_year, entry_month)) = parse_year_month(&entry.date) else {
                return false; // 日期非法：与原实现一致，直接跳过
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

/// `YYYY-MM-DD` 严格校验（含闰年），等价 Go `time.Parse("2006-01-02", ...)` 的成败。
fn is_valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        if index == 4 || index == 7 {
            continue;
        }
        if !byte.is_ascii_digit() {
            return false;
        }
    }
    let year: i32 = value[0..4].parse().unwrap_or(0);
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    day <= days_in_month(year, month)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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

/// 带 BOM 的 UTF-16 解码；无 BOM 返回 `None`（与原实现一致）。
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

/// 把非法 UTF-8 序列替换为 `?`（对齐 Go `bytes.ToValidUTF8`）。
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

    fn workspace(tag: &str) -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-service-diary-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

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
        let entry = upsert(&workspace, "2026-01-01", "中文abc", "开心").unwrap();
        assert_eq!(entry.word_count, 5, "字数按 Unicode 字符数");
        assert_eq!(entry.mood, "开心");
        assert!(!entry.id.is_empty());

        let loaded = get_by_date(&workspace, "2026-01-01").unwrap();
        assert_eq!(loaded.content, "中文abc");
        assert_eq!(loaded.word_count, 5);

        // 覆盖保存：内容更新、id 保留
        upsert(&workspace, "2026-01-01", "改了", "").unwrap();
        let loaded = get_by_date(&workspace, "2026-01-01").unwrap();
        assert_eq!(loaded.content, "改了");
        assert_eq!(loaded.word_count, 2);

        let dates = list_dates(&workspace).unwrap();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].word_count, 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_entry_reports_error() {
        let (workspace, dir) = workspace("missing");
        let error = get_by_date(&workspace, "2000-01-01").unwrap_err();
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
            let entry = import_file(&workspace, path.to_str().unwrap(), date).unwrap();
            assert_eq!(entry.content, "中文内容", "{path:?} 解码失败");
            assert_eq!(entry.word_count, 4);
            assert_eq!(entry.mood, "");
        }

        let broken = import_file(&workspace, broken_path.to_str().unwrap(), "2026-01-04").unwrap();
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
        upsert(&workspace, "2025-12-31", "去年", "").unwrap();
        upsert(&workspace, "2026-01-01", "一月", "").unwrap();
        upsert(&workspace, "2026-02-01", "二月", "").unwrap();

        let out = temp_dir("export-out");
        // 全部导出
        let result = export_to_directory(&workspace, out.to_str().unwrap(), 0, 0).unwrap();
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
        let result = export_to_directory(&workspace, year_out.to_str().unwrap(), 2026, 0).unwrap();
        assert_eq!(result.total, 2);
        assert!(!year_out.join("2025-12-31.md").exists());

        // 按年月
        let month_out = temp_dir("export-month");
        let result = export_to_directory(&workspace, month_out.to_str().unwrap(), 2026, 2).unwrap();
        assert_eq!(result.total, 1);
        assert!(month_out.join("2026-02-01.md").exists());
        assert!(!month_out.join("2026-01-01.md").exists());

        // 目录不存在时自动创建
        let nested = out.join("a").join("b");
        assert!(export_to_directory(&workspace, nested.to_str().unwrap(), 2026, 1).is_ok());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
        std::fs::remove_dir_all(&year_out).ok();
        std::fs::remove_dir_all(&month_out).ok();
    }

    #[test]
    fn date_validation_matches_go_strictness() {
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

    #[test]
    fn delete_is_idempotent() {
        let (workspace, dir) = workspace("delete");
        upsert(&workspace, "2026-01-01", "内容", "").unwrap();
        delete_by_date(&workspace, "2026-01-01").unwrap();
        delete_by_date(&workspace, "2026-01-01").unwrap();
        assert!(list_dates(&workspace).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
