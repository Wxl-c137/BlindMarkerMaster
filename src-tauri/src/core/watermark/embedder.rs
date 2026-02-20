use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use ndarray::Array2;
use crate::models::BlindMarkError;
use crate::core::watermark::{
    dwt::DWTProcessor,
    dct::DCTProcessor,
    encoder::WatermarkEncoder,
};

/// 完整的水印嵌入流水线
///
/// ## 算法（与 Python blind_watermark 完全一致）
///
/// 1. 将图片的 R、G、B 三个通道分别处理
/// 2. 对每个通道做 **1 级 Haar DWT**，提取 LL（低频近似）子带
/// 3. 将 LL 子带划分为 **4×4 块**
/// 4. 对每块做：2D 正交 DCT → 随机打乱 → SVD → **QIM 嵌入** → ISVD → 逆打乱 → IDCT
///    - QIM 公式：`s_new = (floor(s/d) + 0.25 + 0.5*bit) * d`，d1=36，d2=20
/// 5. 重组 LL 子带，做 1 级 IDWT 重建通道
/// 6. 三通道合并，像素值钳制到 [0, 255]
pub struct WatermarkEmbedder {
    dwt: DWTProcessor,
    dct: DCTProcessor,
}

impl WatermarkEmbedder {
    pub fn new() -> Self {
        Self {
            dwt: DWTProcessor::new(),
            dct: DCTProcessor::new(),
        }
    }

    /// 将 MD5 水印嵌入图片
    ///
    /// # 参数
    /// * `image`          - 输入图片
    /// * `watermark_text` - 要嵌入的文本（将被 MD5 哈希为 128 位）
    /// * `strength`       - 当前为保留参数，不影响嵌入效果。
    ///                      算法使用固定 QIM 步长（d1=36，d2=20），
    ///                      提取时需相同步长，因此强度不可变。
    ///                      传入值须在 [0.1, 1.0] 范围内以通过校验。
    pub fn embed(
        &self,
        image: &DynamicImage,
        watermark_text: &str,
        strength: f32,
    ) -> Result<DynamicImage, BlindMarkError> {
        if strength < 0.1 || strength > 1.0 {
            return Err(BlindMarkError::InvalidConfig(
                format!("Strength must be between 0.1 and 1.0, got {}", strength)
            ));
        }

        let watermark_data = WatermarkEncoder::encode(watermark_text);
        self.embed_bits(image, &watermark_data.binary_sequence)
    }

    /// 将原始文本作为盲水印嵌入图片
    ///
    /// # 参数
    /// * `image`     - 输入图片
    /// * `text`      - 要嵌入的原始文本（直接存储，不做哈希处理）
    /// * `strength`  - 保留参数，同 `embed()`，当前不影响嵌入效果
    /// * `fast_mode` - 高速模式：对两维均超过 512px 的大图，仅处理左上角
    ///                 512×512 区域再贴回原图
    pub fn embed_raw_text(
        &self,
        image: &DynamicImage,
        text: &str,
        strength: f32,
        fast_mode: bool,
    ) -> Result<DynamicImage, BlindMarkError> {
        if strength < 0.1 || strength > 1.0 {
            return Err(BlindMarkError::InvalidConfig(
                format!("Strength must be between 0.1 and 1.0, got {}", strength)
            ));
        }

        // ── 高速模式：大图仅处理左上角 512×512 ROI ────────────────────────────
        const FAST_MODE_MAX: u32 = 512;
        let (width, height) = image.dimensions();
        if fast_mode && width > FAST_MODE_MAX && height > FAST_MODE_MAX {
            let roi = image.crop_imm(0, 0, FAST_MODE_MAX, FAST_MODE_MAX);
            let watermarked_roi = self.embed_raw_text(&roi, text, strength, false)?;
            let roi_rgb = watermarked_roi.to_rgb8();
            let rgb_image = image.to_rgb8();
            let mut result = rgb_image;
            for y in 0..FAST_MODE_MAX {
                for x in 0..FAST_MODE_MAX {
                    result.put_pixel(x, y, *roi_rgb.get_pixel(x, y));
                }
            }
            return Ok(DynamicImage::ImageRgb8(result));
        }

        let bits = WatermarkEncoder::text_to_bits(text)?;
        self.embed_bits(image, &bits)
    }

    /// 嵌入并返回 PNG 字节（用于预览/API）
    pub fn embed_to_bytes(
        &self,
        image: &DynamicImage,
        watermark_text: &str,
        strength: f32,
    ) -> Result<Vec<u8>, BlindMarkError> {
        let watermarked = self.embed(image, watermark_text, strength)?;
        let mut buffer = Vec::new();
        watermarked
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageFormat::Png,
            )
            .map_err(|e| BlindMarkError::ImageProcessing(
                format!("Failed to encode image: {}", e)
            ))?;
        Ok(buffer)
    }

    // ─── 核心嵌入逻辑 ─────────────────────────────────────────────────────────

    /// 将指定比特序列嵌入图片（内部实现，供 embed 和 embed_raw_text 共用）
    fn embed_bits(
        &self,
        image: &DynamicImage,
        bits: &[u8],
    ) -> Result<DynamicImage, BlindMarkError> {
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        let (w, h) = (width as usize, height as usize);

        // 1 级 DWT 要求尺寸为偶数
        if width % 2 != 0 || height % 2 != 0 {
            return Err(BlindMarkError::ImageProcessing(
                format!("图片尺寸必须为偶数（DWT 要求）：{}×{}", width, height)
            ));
        }

        // ── 三通道分别处理 ───────────────────────────────────────────────────
        let mut channels: [Array2<f64>; 3] = [
            Array2::zeros((h, w)),
            Array2::zeros((h, w)),
            Array2::zeros((h, w)),
        ];
        for y in 0..h {
            for x in 0..w {
                let p = rgb_image.get_pixel(x as u32, y as u32);
                channels[0][[y, x]] = p[0] as f64;
                channels[1][[y, x]] = p[1] as f64;
                channels[2][[y, x]] = p[2] as f64;
            }
        }

        for ch_data in &mut channels {
            // 1 级 DWT → (LL, LH, HL, HH)
            let (mut ll, lh, hl, hh) = self.dwt.decompose_1level(ch_data.view())?;

            // QIM 嵌入到 LL 子带
            self.dct.embed_watermark_blocks(&mut ll, bits)?;

            // 1 级 IDWT 重建
            *ch_data = self.dwt.reconstruct_1level(&ll, &lh, &hl, &hh)?;
        }

        // ── 合并三通道为 RGB 图片（像素值钳制到 [0, 255]）───────────────────
        let mut result = ImageBuffer::new(width, height);
        for y in 0..h {
            for x in 0..w {
                let r = channels[0][[y, x]].clamp(0.0, 255.0) as u8;
                let g = channels[1][[y, x]].clamp(0.0, 255.0) as u8;
                let b = channels[2][[y, x]].clamp(0.0, 255.0) as u8;
                result.put_pixel(x as u32, y as u32, Rgb([r, g, b]));
            }
        }

        Ok(DynamicImage::ImageRgb8(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, ImageBuffer};

    fn create_test_image(width: u32, height: u32) -> DynamicImage {
        let mut img = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = 128u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn test_embed_basic() {
        let embedder = WatermarkEmbedder::new();
        // MD5 (128 位) 要求至少 128 块 = LL 子带 ≥ 32×32 = 原图 ≥ 64×64
        // 保险起见用 128×128（256 块 > 128 位）
        let image = create_test_image(128, 128);
        let result = embedder.embed(&image, "Test watermark", 0.5);
        assert!(result.is_ok(), "嵌入应成功: {:?}", result.err());
    }

    #[test]
    fn test_embed_raw_text_basic() {
        let embedder = WatermarkEmbedder::new();
        // 544 位文本水印需要至少 544 块：LL ≥ 93×93 → 原图 ≥ 186×186
        // 使用 256×256（LL=128×128，1024 块 > 544）
        let image = create_test_image(256, 256);
        let result = embedder.embed_raw_text(&image, "Hello", 0.5, false);
        assert!(result.is_ok(), "embed_raw_text 应成功: {:?}", result.err());
    }

    #[test]
    fn test_embed_invalid_dimensions() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(63, 63); // 奇数尺寸
        let result = embedder.embed(&image, "Test", 0.5);
        assert!(result.is_err(), "奇数尺寸应失败");
    }

    #[test]
    fn test_embed_invalid_strength() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(128, 128);
        assert!(embedder.embed(&image, "Test", 0.05).is_err());
        assert!(embedder.embed(&image, "Test", 1.5).is_err());
    }

    #[test]
    fn test_embed_preserves_dimensions() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(256, 256);
        let (orig_w, orig_h) = image.dimensions();

        let watermarked = embedder.embed_raw_text(&image, "Test", 0.3, false).unwrap();
        let (new_w, new_h) = watermarked.dimensions();

        assert_eq!(orig_w, new_w, "宽度应保持不变");
        assert_eq!(orig_h, new_h, "高度应保持不变");
    }

    #[test]
    fn test_embed_different_strengths() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(256, 256);
        // strength 不同时应都能成功（当前算法 strength 为保留参数）
        assert!(embedder.embed_raw_text(&image, "Test", 0.1, false).is_ok());
        assert!(embedder.embed_raw_text(&image, "Test", 1.0, false).is_ok());
    }

    #[test]
    fn test_embed_to_bytes() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(128, 128);
        let bytes = embedder.embed_to_bytes(&image, "Test", 0.5);
        assert!(bytes.is_ok(), "应产生字节输出");
        assert!(!bytes.unwrap().is_empty(), "字节不应为空");
    }

    #[test]
    fn test_embed_raw_text_fast_mode_large_image() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(1024, 1024);
        let (orig_w, orig_h) = image.dimensions();

        let result = embedder.embed_raw_text(&image, "FastMode", 0.5, true);
        assert!(result.is_ok(), "高速模式应成功: {:?}", result.err());

        let watermarked = result.unwrap();
        assert_eq!(watermarked.width(), orig_w, "宽度应保持不变");
        assert_eq!(watermarked.height(), orig_h, "高度应保持不变");
    }

    #[test]
    fn test_embed_raw_text_fast_mode_small_image() {
        let embedder = WatermarkEmbedder::new();
        let image = create_test_image(256, 256);
        let r1 = embedder.embed_raw_text(&image, "SmallFast", 0.5, true);
        let r2 = embedder.embed_raw_text(&image, "SmallFast", 0.5, false);
        assert!(r1.is_ok() && r2.is_ok(), "Both should succeed");
    }
}
