//! HEIC/HEIF（iPhone 相机默认格式）→ RGBA8 像素。
//!
//! 纯函数：`&[u8]` 进、像素出，没有一行 I/O —— 所以界面侧能跑，
//! `cargo test -p tr-draw` 里也能拿真文件断言（[`tests`] 用的是
//! `fixtures/heic/` 里的合成图）。
//!
//! ## 为什么在这里解，而不是交给 WebView2
//!
//! 原先的做法是把 HEIC 丢给 `createImageBitmap` / `<img>`，指望 WebView2 自带的
//! 解码器解 HEIF。**这条路走不通**：Chromium 内核根本没有 HEIF 解码器
//! （HEVC 图片受专利约束），Windows 那边要靠商店里的「HEIF 图像扩展」，
//! 而它默认不装 —— 于是 iPhone 照片一律 `HEIC 转换失败: 图片解码失败`（用户报的就是这个）。
//!
//! 现在换成随程序一起分发的纯 Rust 解码器（`heic-rs`，MIT OR Apache-2.0）：
//! 不依赖系统装了什么、也没有 C 依赖，native 与 wasm32 共用这一份实现。
//!
//! ## 两条边界
//!
//! * **输出固定 RGBA8**：直接喂 `ImageData`（canvas 只吃这个），
//!   所以这一层不做 JPEG 编码 —— 编码那步留在界面（`canvas.toDataURL`）。
//! * **像素上限**：超大图先被 [`MAX_PIXELS`] 拦下（解码前判），
//!   免得一个几十 KB 的文件让进程去要几 GB 内存。

/// 单张图允许的最大像素数（1 亿）。
///
/// 参考：iPhone 主摄常见的 24MP（5712×4284），单反/中画幅到 1 亿像素。
/// 超过就报错，而不是先分配再失败。
pub const MAX_PIXELS: u64 = 100_000_000;

/// 解码结果：逐像素 RGBA（行优先、无行间填充），长度恒为 `width * height * 4`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// HEIC/HEIF 字节 → RGBA8。
///
/// 失败时返回**可读的原因**（不含 `HEIC 转换失败:` 前缀 —— 那是界面层加的，
/// 因为同一段文案在事件图片与工作空间图标两处都要用）。
///
/// 旋转（`irot`）与镜像（`imir`）由解码器按 `ipma` 顺序应用到像素上，
/// 所以竖着拍的 iPhone 照片出来就是竖的。
pub fn decode_rgba8(bytes: &[u8]) -> Result<Rgba8Image, String> {
    let options = heic_rs::DecodeOptions::default()
        .with_layout(heic_rs::PixelLayout::Rgba8)
        .with_max_pixels(Some(MAX_PIXELS));

    let image = heic_rs::decode(bytes, &options).map_err(|error| format!("{error}"))?;

    let (width, height) = (image.width, image.height);
    if width == 0 || height == 0 {
        return Err("图片尺寸无效".to_string());
    }
    if image.layout != heic_rs::PixelLayout::Rgba8 {
        return Err(format!("解码输出不是 RGBA8: {:?}", image.layout));
    }
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if image.data.len() != expected {
        return Err(format!(
            "解码输出长度不对: {} != {expected}",
            image.data.len()
        ));
    }

    Ok(Rgba8Image {
        width,
        height,
        pixels: image.data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `fixtures/heic/*` 的绝对路径。
    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/heic")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|error| panic!("读不到 {}: {error}", path.display()))
    }

    /// 读一个 8-bit RGB 参考 PNG（Apple 的解码结果）。
    fn reference(name: &str) -> (u32, u32, Vec<u8>) {
        let bytes = fixture(name);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("参考 PNG 读头");
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).expect("参考 PNG 解码");
        assert_eq!(info.color_type, png::ColorType::Rgb, "参照物应是 8-bit RGB");
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn decodes_flat_color_close_to_apples_reference() {
        let image = decode_rgba8(&fixture("flat-64.heic")).expect("解码 flat-64");
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(image.pixels.len(), 64 * 64 * 4);
        // 不透明：HEIC 没有 alpha 通道时补 255，canvas 才不会画成透明
        assert!(image.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));

        // 与 Apple 的解码结果比：HEVC 有损 + 4:2:0，允许几个码值的漂移
        let (width, height, reference) = reference("flat-64.ref.png");
        assert_eq!((width, height), (image.width, image.height));
        let mut total = 0u64;
        let mut worst = 0u32;
        for (index, reference_pixel) in reference.chunks_exact(3).enumerate() {
            let ours = &image.pixels[index * 4..index * 4 + 3];
            for (&ours_channel, &reference_channel) in ours.iter().zip(reference_pixel) {
                let diff = u32::from(ours_channel).abs_diff(u32::from(reference_channel));
                total += u64::from(diff);
                worst = worst.max(diff);
            }
        }
        let mean = total as f64 / f64::from(width * height * 3);
        assert!(mean < 2.0, "平均偏差 {mean} 太大");
        assert!(worst <= 8, "单个通道最大偏差 {worst} 太大");
    }

    #[test]
    fn decodes_gradient_monotonically() {
        let image = decode_rgba8(&fixture("gradient-512.heic")).expect("解码 gradient-512");
        assert_eq!((image.width, image.height), (512, 512));

        // 红向右递增、绿向下递增、蓝恒为 ~128（fixture 的定义）
        let pixel = |x: usize, y: usize| {
            let index = (y * 512 + x) * 4;
            (
                image.pixels[index],
                image.pixels[index + 1],
                image.pixels[index + 2],
            )
        };
        let (left, _, _) = pixel(0, 256);
        let (right, _, _) = pixel(511, 256);
        assert!(right > left + 200, "红应当向右递增: {left} → {right}");
        let (_, top, _) = pixel(256, 0);
        let (_, bottom, _) = pixel(256, 511);
        assert!(bottom > top + 200, "绿应当向下递增: {top} → {bottom}");
        let (_, _, blue) = pixel(256, 256);
        assert!((blue as i32 - 128).abs() <= 8, "蓝应恒为 128，实测 {blue}");
    }

    #[test]
    fn rejects_non_heic_bytes() {
        assert!(decode_rgba8(b"").is_err());
        assert!(decode_rgba8(b"not a heic file at all").is_err());
        // 一张真 PNG 也不行：这里只认 HEIC/HEIF（其它格式走读文件那条路）
        assert!(decode_rgba8(&fixture("flat-64.ref.png")).is_err());
    }
}
