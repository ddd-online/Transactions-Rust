//! 关键事件服务（含图片）。
//!
//! 几个必须遵守的点：
//! * 标题按**字符**（rune）截断到 200，避免在多字节 UTF-8 中间切断
//! * upsert 每次都生成新 id，但冲突时数据库保留原 id 与 created_at（见 `KeyEventDao::upsert`）
//! * 删除关键事件：先取图片列表 → 事务内删记录 → **提交后**再删磁盘文件
//! * 新增图片：先落盘 → 记录失败时回滚磁盘文件
//! * 图片排序号 = 该天现有最大 sort_order + 1

use tr_domain::models::{KeyEvent, KeyEventImage};
use tr_store::dao::is_not_found;
use tr_store::dao::key_event::{KeyEventDao, TITLE_MAX_CHARS};
use tr_store::dao::key_event_image::KeyEventImageDao;
use tr_store::Workspace;

use crate::{assets, ServiceError, ServiceResult};

/// 写入关键事件（标题按字符截断）。
pub fn upsert_key_event(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
    title: &str,
    content: &str,
    color: &str,
) -> ServiceResult<()> {
    let title = tr_store::util::truncate_chars(title, TITLE_MAX_CHARS);
    let event = KeyEvent {
        id: tr_store::util::new_uuid(),
        date: date.to_string(),
        title,
        content: content.to_string(),
        color: color.to_string(),
        ledger_id: ledger_id.to_string(),
        created_at: 0,
        updated_at: 0,
    };
    Ok(KeyEventDao::upsert(&workspace.connection(), &event)?)
}

/// 按日期取关键事件；不存在时报错。
pub fn query_by_date(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
) -> ServiceResult<KeyEvent> {
    Ok(KeyEventDao::query_by_date(
        &workspace.connection(),
        ledger_id,
        date,
    )?)
}

/// 某账本某年的全部关键事件。
pub fn query_by_year(
    workspace: &Workspace,
    ledger_id: &str,
    year: &str,
) -> ServiceResult<Vec<KeyEvent>> {
    Ok(KeyEventDao::query_by_year(
        &workspace.connection(),
        ledger_id,
        year,
    )?)
}

/// 某账本某年有事件的日期列表（顺序由查询决定）。
pub fn query_dates_by_year(
    workspace: &Workspace,
    ledger_id: &str,
    year: &str,
) -> ServiceResult<Vec<String>> {
    let events = query_by_year(workspace, ledger_id, year)?;
    Ok(events.into_iter().map(|event| event.date).collect())
}

/// 删除某天的关键事件：事务内删图片记录与事件本身，提交后清理磁盘文件。
pub fn delete_by_date(workspace: &Workspace, ledger_id: &str, date: &str) -> ServiceResult<()> {
    tracing::info!("删除关键事件, 日期: {}", date);

    let images = KeyEventImageDao::query_by_event_date(&workspace.connection(), ledger_id, date)
        .map_err(|error| ServiceError::Internal(format!("query images: {error}")))?;

    workspace.transaction(|conn| -> ServiceResult<()> {
        KeyEventImageDao::delete_by_event_date(conn, ledger_id, date)
            .map_err(|error| ServiceError::Internal(format!("delete image records: {error}")))?;
        KeyEventDao::delete_by_date(conn, ledger_id, date)
            .map_err(|error| ServiceError::Internal(format!("delete key event: {error}")))?;
        Ok(())
    })?;

    // 事务提交成功后再删文件（避免"记录已删但文件删除失败"造成状态不可恢复）
    for image in images {
        assets::remove_image_files(workspace, &image.file_path, &image.thumb_path);
    }
    Ok(())
}

/// 新增一张图片：落盘（含缩略图）→ 写记录；记录失败时回滚磁盘文件。
pub fn add_image(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
    data: &str,
) -> ServiceResult<KeyEventImage> {
    let image_id = tr_store::util::new_uuid();

    let (file_path, thumb_path) = assets::save_image(workspace, date, &image_id, data)?;

    let existing =
        match KeyEventImageDao::query_by_event_date(&workspace.connection(), ledger_id, date) {
            Ok(images) => images,
            Err(error) => {
                assets::remove_image_files(workspace, &file_path, &thumb_path);
                return Err(ServiceError::from(error));
            }
        };

    let next_sort_order = existing
        .iter()
        .map(|image| image.sort_order)
        .max()
        .unwrap_or(0)
        + 1;

    let image = KeyEventImage {
        id: image_id,
        ledger_id: ledger_id.to_string(),
        event_date: date.to_string(),
        file_path,
        thumb_path,
        sort_order: next_sort_order,
        created_at: 0,
    };

    if let Err(error) = KeyEventImageDao::create(&workspace.connection(), &image) {
        assets::remove_image_files(workspace, &image.file_path, &image.thumb_path);
        return Err(ServiceError::from(error));
    }

    Ok(image)
}

/// 某账本某天的图片列表。
pub fn list_images(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
) -> ServiceResult<Vec<KeyEventImage>> {
    Ok(KeyEventImageDao::query_by_event_date(
        &workspace.connection(),
        ledger_id,
        date,
    )?)
}

/// 删除一张图片（记录 + 磁盘文件）。
pub fn delete_image(workspace: &Workspace, image_id: &str) -> ServiceResult<()> {
    let conn = workspace.connection();
    // 先查文件路径：记录删掉后就找不到文件了；查不到记录时继续删记录（幂等）
    match KeyEventImageDao::query_by_id(&conn, image_id) {
        Ok(image) => assets::remove_image_files(workspace, &image.file_path, &image.thumb_path),
        Err(error) if is_not_found(&error) => {}
        Err(error) => return Err(ServiceError::from(error)),
    }
    Ok(KeyEventImageDao::delete_by_id(&conn, image_id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;
    use base64::Engine;

    /// 生成一个最小的合法 PNG（1×1）data URI。
    fn tiny_png_data_uri() -> String {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    /// 生成一个宽 600 的 JPEG（用于验证缩略图缩放到 300 宽）。
    fn wide_jpeg_data_uri(width: u32, height: u32) -> String {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([10, 20, 30]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    #[test]
    fn upsert_truncates_title_by_character_not_byte() {
        let (workspace, dir) = workspace("truncate");
        // 300 个汉字 → 截断到 200 个字符
        let long_title = "中".repeat(300);
        upsert_key_event(&workspace, "l1", "2026-01-01", &long_title, "正文", "red").unwrap();

        let event = query_by_date(&workspace, "l1", "2026-01-01").unwrap();
        assert_eq!(event.title.chars().count(), 200);
        assert_eq!(event.content, "正文");
        assert_eq!(event.color, "red");

        // 200 以内不截断；跨账本同日互不影响
        upsert_key_event(&workspace, "l1", "2026-01-02", &"短".repeat(200), "", "").unwrap();
        assert_eq!(
            query_by_date(&workspace, "l1", "2026-01-02")
                .unwrap()
                .title
                .chars()
                .count(),
            200
        );
        upsert_key_event(&workspace, "l2", "2026-01-01", "别的账本", "", "").unwrap();
        assert_eq!(
            query_by_date(&workspace, "l2", "2026-01-01").unwrap().title,
            "别的账本"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_keeps_original_id_and_created_at() {
        let (workspace, dir) = workspace("upsert");
        upsert_key_event(&workspace, "l1", "2026-03-03", "一", "", "").unwrap();
        let first = query_by_date(&workspace, "l1", "2026-03-03").unwrap();

        upsert_key_event(&workspace, "l1", "2026-03-03", "二", "新正文", "blue").unwrap();
        let second = query_by_date(&workspace, "l1", "2026-03-03").unwrap();

        assert_eq!(second.id, first.id);
        assert_eq!(second.created_at, first.created_at);
        assert_eq!(second.title, "二");
        assert_eq!(second.content, "新正文");
        assert_eq!(second.color, "blue");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn year_queries_and_dates() {
        let (workspace, dir) = workspace("year");
        upsert_key_event(&workspace, "l1", "2025-12-31", "去年", "", "").unwrap();
        upsert_key_event(&workspace, "l1", "2026-01-01", "今年", "", "").unwrap();

        assert_eq!(query_by_year(&workspace, "l1", "2026").unwrap().len(), 1);
        assert_eq!(
            query_dates_by_year(&workspace, "l1", "2026").unwrap(),
            vec!["2026-01-01".to_string()]
        );
        assert!(query_dates_by_year(&workspace, "l1", "2024")
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_image_writes_files_and_increments_sort_order() {
        let (workspace, dir) = workspace("image");
        let data = tiny_png_data_uri();

        let first = add_image(&workspace, "l1", "2026-01-01", &data).unwrap();
        assert_eq!(first.sort_order, 1);
        assert!(first.file_path.starts_with("key_events/2026-01-01/"));
        assert!(first.file_path.ends_with(".png"));
        assert!(first.thumb_path.ends_with(".jpg"));

        // 原图与缩略图都真的落盘了
        assert!(assets::resolve(&workspace, &first.file_path).exists());
        assert!(assets::resolve(&workspace, &first.thumb_path).exists());

        let second = add_image(&workspace, "l1", "2026-01-01", &data).unwrap();
        assert_eq!(second.sort_order, 2);

        let images = list_images(&workspace, "l1", "2026-01-01").unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].sort_order, 1);
        assert_eq!(images[1].sort_order, 2);

        // 另一个账本同一天的排序号独立
        let other = add_image(&workspace, "l2", "2026-01-01", &data).unwrap();
        assert_eq!(other.sort_order, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn thumbnail_scales_down_to_three_hundred_pixels_wide() {
        let (workspace, dir) = workspace("thumb");
        let data = wide_jpeg_data_uri(600, 300);
        let image = add_image(&workspace, "l1", "2026-02-02", &data).unwrap();

        let thumb = assets::resolve(&workspace, &image.thumb_path);
        let decoded = image::open(&thumb).unwrap();
        assert_eq!(decoded.width(), 300);
        assert_eq!(decoded.height(), 150, "等比例缩放");

        // 小图不放大
        let small = tiny_png_data_uri();
        let small_image = add_image(&workspace, "l1", "2026-02-03", &small).unwrap();
        let decoded = image::open(assets::resolve(&workspace, &small_image.thumb_path)).unwrap();
        assert_eq!(decoded.width(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_image_rejects_broken_payload_and_leaves_no_files() {
        let (workspace, dir) = workspace("broken");

        // 非 data URI
        let error = add_image(&workspace, "l1", "2026-01-01", "not-a-data-uri").unwrap_err();
        assert!(
            error.to_string().contains("no comma separator"),
            "msg = {error}"
        );

        // base64 合法但不是图片 → 缩略图阶段失败并回滚原图
        let payload = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"not an image")
        );
        let error = add_image(&workspace, "l1", "2026-01-01", &payload).unwrap_err();
        assert!(
            error.to_string().contains("generate thumbnail"),
            "msg = {error}"
        );

        let directory = workspace
            .assets_directory()
            .join("key_events")
            .join("2026-01-01");
        let leftovers: Vec<_> = std::fs::read_dir(&directory)
            .map(|entries| entries.filter_map(Result::ok).collect())
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "失败的写入不得留下文件");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_image_removes_record_and_files_and_is_idempotent() {
        let (workspace, dir) = workspace("delete-image");
        let data = tiny_png_data_uri();
        let image = add_image(&workspace, "l1", "2026-01-01", &data).unwrap();
        let original = assets::resolve(&workspace, &image.file_path);
        let thumb = assets::resolve(&workspace, &image.thumb_path);

        delete_image(&workspace, &image.id).unwrap();
        assert!(!original.exists());
        assert!(!thumb.exists());
        assert!(list_images(&workspace, "l1", "2026-01-01")
            .unwrap()
            .is_empty());

        // 重复删除不报错
        delete_image(&workspace, &image.id).unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_by_date_removes_event_records_and_files() {
        let (workspace, dir) = workspace("delete-date");
        let data = tiny_png_data_uri();
        upsert_key_event(&workspace, "l1", "2026-01-01", "事件", "正文", "").unwrap();
        let image = add_image(&workspace, "l1", "2026-01-01", &data).unwrap();
        let original = assets::resolve(&workspace, &image.file_path);
        assert!(original.exists());

        delete_by_date(&workspace, "l1", "2026-01-01").unwrap();

        assert!(!original.exists(), "磁盘文件应在事务提交后被清理");
        assert!(list_images(&workspace, "l1", "2026-01-01")
            .unwrap()
            .is_empty());
        assert!(query_by_date(&workspace, "l1", "2026-01-01").is_err());

        // 另一个账本未被牵连
        upsert_key_event(&workspace, "l2", "2026-01-01", "保留", "", "").unwrap();
        assert!(query_by_date(&workspace, "l2", "2026-01-01").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
