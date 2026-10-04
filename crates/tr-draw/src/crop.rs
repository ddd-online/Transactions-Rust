//! 方形裁剪的纯几何：拖拽改位移、按钮改缩放，按"视口里看到什么"反解出源图上的那块矩形。
//!
//! 弹窗本身（`crates/tr-ui` 的 `components/ui/image_crop.rs`）只负责把指针事件喂进来、
//! 把 [`CropRect`] 交给 canvas 去裁 —— 几何留在这里，是为了能在 native 上断言：
//! `cargo test -p tr-draw`。

/// 裁剪视口的边长（CSS px）。最窄一档弹窗（`ModalSize::Small` = 400px）减去内边距后仍有余量。
pub const CROP_VIEWPORT: f64 = 260.0;

/// 源图上要取走的那一块（源图像素：左上角 + 边长）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub size: f64,
}

/// "刚好铺满视口"的比例（cover）：源图像素 → CSS px。
///
/// 取两个方向的**较大**者：小的那边铺满时，大的那边正好溢出 —— 溢出部分就是可拖动范围。
pub fn cover_scale(natural: (f64, f64)) -> f64 {
    if natural.0 <= 0.0 || natural.1 <= 0.0 {
        return 1.0;
    }
    (CROP_VIEWPORT / natural.0).max(CROP_VIEWPORT / natural.1)
}

/// 位移的合法范围（±，CSS px）。
///
/// 上限 = 溢出量的一半：再多拖一点，图的那条边就会离开视口边缘、露出底色。
pub fn max_offset(natural: (f64, f64), scale: f64) -> (f64, f64) {
    (
        ((natural.0 * scale - CROP_VIEWPORT) / 2.0).max(0.0),
        ((natural.1 * scale - CROP_VIEWPORT) / 2.0).max(0.0),
    )
}

/// 按"视口里看到什么"反解出源图上的裁剪框。
///
/// `offset`：图像中心相对视口中心的位移（CSS px，正 = 往右下）；
/// `scale`：源图像素 → CSS px 的比例（= [`cover_scale`] × 用户缩放）。
///
/// 视口在图像坐标系里的起点 = `(视口边长 - 图像显示边长) / 2 + 位移`，
/// 但那个值是**负的**（图像比视口大），所以取反再除以比例才是源图坐标。
pub fn crop_rect(natural: (f64, f64), offset: (f64, f64), scale: f64) -> CropRect {
    let (limit_x, limit_y) = max_offset(natural, scale);
    let dx = offset.0.clamp(-limit_x, limit_x);
    let dy = offset.1.clamp(-limit_y, limit_y);
    let left = (CROP_VIEWPORT - natural.0 * scale) / 2.0 + dx;
    let top = (CROP_VIEWPORT - natural.1 * scale) / 2.0 + dy;
    CropRect {
        x: -left / scale,
        y: -top / scale,
        size: CROP_VIEWPORT / scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 横图：铺满时是**高度**说了算（宽度溢出）。
    #[test]
    fn cover_scale_fills_the_shorter_side() {
        let wide = (400.0, 200.0);
        assert_eq!(cover_scale(wide), CROP_VIEWPORT / 200.0);
        let tall = (200.0, 400.0);
        assert_eq!(cover_scale(tall), CROP_VIEWPORT / 200.0);
        // 退化输入不 panic、也不产生 inf
        assert_eq!(cover_scale((0.0, 0.0)), 1.0);
    }

    /// 居中、铺满时取的是正中间那块方图 —— 裁剪框必须完整落在源图内。
    #[test]
    fn centered_crop_takes_the_middle_square() {
        let natural = (400.0, 200.0);
        let scale = cover_scale(natural);
        let rect = crop_rect(natural, (0.0, 0.0), scale);
        assert!((rect.size - 200.0).abs() < 1e-9, "取到的应是 200px 见方");
        assert!((rect.x - 100.0).abs() < 1e-9, "x 应贴着左右的等分处");
        assert!(rect.y.abs() < 1e-9);
        assert!(rect.x + rect.size <= natural.0 + 1e-9);
    }

    /// 位移被夹在溢出量之内：**图像任何时候都盖满视口**，不会露出底色。
    #[test]
    fn offset_is_clamped_to_the_overflow() {
        let natural = (400.0, 200.0);
        let scale = cover_scale(natural);
        // 往右拖 10000px：最多只能拖到左边缘贴住视口（100px 溢出的一半 = 50px）
        let rect = crop_rect(natural, (10000.0, 0.0), scale);
        assert!((rect.x - 0.0).abs() < 1e-9);
        let rect = crop_rect(natural, (-10000.0, 0.0), scale);
        assert!((rect.x + rect.size - natural.0).abs() < 1e-9);
    }

    /// 放大后允许的位移变大（可以去看图的四角），但裁剪框仍然在源图内。
    #[test]
    fn zooming_in_allows_larger_offsets_and_stays_inside() {
        let natural = (400.0, 200.0);
        let scale = cover_scale(natural) * 4.0;
        let (limit_x, _) = max_offset(natural, scale);
        assert!(limit_x > 0.0);
        for probe in [-limit_x, 0.0, limit_x] {
            let rect = crop_rect(natural, (probe, 0.0), scale);
            assert!(rect.x >= -1e-9, "裁剪框越过了左边界");
            assert!(rect.x + rect.size <= natural.0 + 1e-9, "裁剪框越过了右边界");
        }
    }
}
