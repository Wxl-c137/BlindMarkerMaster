use nalgebra::{Matrix4, Vector4};
use ndarray::Array2;
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand::rngs::SmallRng;
use crate::models::BlindMarkError;

/// QIM 量化步长（与 Python blind_watermark 默认值一致）
/// d1=36 对应主奇异值，d2=20 对应次奇异值
pub const D1: f64 = 36.0;
pub const D2: f64 = 20.0;

/// 4×4 分块大小（与 Python blind_watermark 一致）
const BLOCK_H: usize = 4;
const BLOCK_W: usize = 4;

/// 默认嵌入密码（种子）
const PASSWORD: u64 = 1;

/// DCT + SVD + QIM 水印处理器
///
/// ## 算法（与 Python blind_watermark 完全一致）
///
/// ### 嵌入
/// 对 LL 子带（1 级 DWT 后的低频近似层）的每个 4×4 块：
/// 1. 对块做 2D 正交 DCT-II（与 OpenCV cv2.dct 相同）
/// 2. 用密码 seed 将 16 个 DCT 系数随机打乱
/// 3. 对打乱后的 4×4 矩阵做 SVD：U, S, Vt
/// 4. 用 QIM 修改 S[0] 和 S[1]：
///    `s_new = (floor(s/d) + 0.25 + 0.5 * bit) * d`
/// 5. 重建：U * diag(S_new) * Vt
/// 6. 逆打乱，做 2D IDCT
///
/// ### 提取
/// 相同预处理后，读取 S[0] 和 S[1]：
/// `bit = (s % d > d/2) ? 1.0 : 0.0`
/// 再对所有块的循环副本取平均，三通道求和后阈值判决。
pub struct DCTProcessor;

impl DCTProcessor {
    pub fn new() -> Self {
        Self
    }

    // ─── 公开接口 ────────────────────────────────────────────────────────────

    /// 将水印比特嵌入 LL 子带（原地修改）
    ///
    /// # 参数
    /// * `ll`      - 1 级 DWT 的 LL 子带（将被修改）
    /// * `wm_bits` - 水印比特序列
    ///
    /// # 错误
    /// 若图片太小（可用 4×4 块数 < wm_bits.len()），返回错误。
    pub fn embed_watermark_blocks(
        &self,
        ll: &mut Array2<f64>,
        wm_bits: &[u8],
    ) -> Result<(), BlindMarkError> {
        let (h, w) = ll.dim();
        let blocks_h = h / BLOCK_H;
        let blocks_w = w / BLOCK_W;
        let block_num = blocks_h * blocks_w;

        if block_num < wm_bits.len() {
            return Err(BlindMarkError::ExtractionFailed(format!(
                "图片太小：LL 子带仅能划分 {} 个 4×4 块，不足以嵌入 {} 位水印",
                block_num,
                wm_bits.len()
            )));
        }

        for block_idx in 0..block_num {
            let bi = block_idx / blocks_w;
            let bj = block_idx % blocks_w;
            let bit = wm_bits[block_idx % wm_bits.len()];

            // 读取块
            let block = Self::read_block(ll, bi, bj);

            // 正向 DCT
            let dct_block = dct2d_block(block);

            // 打乱
            let perm = generate_shuffler(PASSWORD, block_idx);
            let shuffled: [f64; 16] = std::array::from_fn(|i| dct_block[perm[i]]);

            // SVD
            let (u, mut s, vt) = svd_4x4(shuffled);

            // QIM 嵌入
            s[0] = qim_encode(s[0], bit, D1);
            s[1] = qim_encode(s[1], bit, D2);

            // 重建
            let modified = reconstruct_svd(&u, &s, &vt);

            // 逆打乱（Python: block_dct_flatten[shuffler] = copy）
            let mut unshuffled = [0.0f64; 16];
            for i in 0..16 {
                unshuffled[perm[i]] = modified[i];
            }

            // 逆 DCT
            let result = idct2d_block(unshuffled);

            // 写回
            Self::write_block(ll, bi, bj, result);
        }

        Ok(())
    }

    /// 从 LL 子带中提取水印软判决值（每位取值 [0, 1]）
    ///
    /// 流程：每块做 DCT → 打乱 → SVD → QIM 解码，再对循环副本取平均。
    ///
    /// # 返回
    /// 长度为 `wm_size` 的 Vec，值域 [0, 1]。
    /// > 0.5 → bit=1，≤ 0.5 → bit=0。
    pub fn extract_watermark_blocks_soft(
        &self,
        ll: &Array2<f64>,
        wm_size: usize,
    ) -> Result<Vec<f64>, BlindMarkError> {
        let (h, w) = ll.dim();
        let blocks_h = h / BLOCK_H;
        let blocks_w = w / BLOCK_W;
        let block_num = blocks_h * blocks_w;

        if block_num < wm_size {
            return Err(BlindMarkError::ExtractionFailed(format!(
                "图片太小：{} 块 < {} 位水印",
                block_num,
                wm_size
            )));
        }

        // 每块提取一个软判决值（与 Python extract_raw 对应）
        let mut wm_block_bits = vec![0.0f64; block_num];

        for block_idx in 0..block_num {
            let bi = block_idx / blocks_w;
            let bj = block_idx % blocks_w;

            let block = Self::read_block(ll, bi, bj);
            let dct_block = dct2d_block(block);

            let perm = generate_shuffler(PASSWORD, block_idx);
            let shuffled: [f64; 16] = std::array::from_fn(|i| dct_block[perm[i]]);

            let (_, s, _) = svd_4x4(shuffled);

            // 与 Python 一致：3:1 加权平均两个奇异值的解码结果
            let bit0 = qim_decode_soft(s[0], D1);
            let bit1 = qim_decode_soft(s[1], D2);
            wm_block_bits[block_idx] = (bit0 * 3.0 + bit1) / 4.0;
        }

        // 循环平均（与 Python extract_avg 一致）
        let mut wm_avg = vec![0.0f64; wm_size];
        for i in 0..wm_size {
            let mut sum = 0.0;
            let mut count = 0usize;
            let mut j = i;
            while j < block_num {
                sum += wm_block_bits[j];
                count += 1;
                j += wm_size;
            }
            wm_avg[i] = if count > 0 { sum / count as f64 } else { 0.5 };
        }

        Ok(wm_avg)
    }

    // ─── 私有辅助方法 ─────────────────────────────────────────────────────────

    /// 从 LL 子带读取一个 4×4 块（行优先展平）
    fn read_block(ll: &Array2<f64>, bi: usize, bj: usize) -> [f64; 16] {
        let mut block = [0.0f64; 16];
        for ri in 0..BLOCK_H {
            for ci in 0..BLOCK_W {
                block[ri * BLOCK_W + ci] = ll[[bi * BLOCK_H + ri, bj * BLOCK_W + ci]];
            }
        }
        block
    }

    /// 将一个 4×4 块写回 LL 子带
    fn write_block(ll: &mut Array2<f64>, bi: usize, bj: usize, block: [f64; 16]) {
        for ri in 0..BLOCK_H {
            for ci in 0..BLOCK_W {
                ll[[bi * BLOCK_H + ri, bj * BLOCK_W + ci]] = block[ri * BLOCK_W + ci];
            }
        }
    }
}

// ─── 内部纯函数（不依赖 self）────────────────────────────────────────────────

/// 1D 正交 DCT-II，N=4（与 OpenCV cv2.dct 一致）
///
/// 权重：w(0) = 1/√4 = 0.5，w(k>0) = √(2/4) = 1/√2
/// 公式：X[k] = w(k) · Σ x[n] · cos(π(2n+1)k / 8)
fn dct1d_4(x: [f64; 4]) -> [f64; 4] {
    const PI: f64 = std::f64::consts::PI;
    const W0: f64 = 0.5; // 1/sqrt(4)
    const W1: f64 = std::f64::consts::FRAC_1_SQRT_2; // sqrt(2/4)
    let w = [W0, W1, W1, W1];
    let mut y = [0.0f64; 4];
    for k in 0..4 {
        let sum: f64 = (0..4)
            .map(|n| x[n] * (PI * (2 * n + 1) as f64 * k as f64 / 8.0).cos())
            .sum();
        y[k] = w[k] * sum;
    }
    y
}

/// 1D 正交 IDCT-II，N=4
///
/// 公式（与 dct1d_4 互为逆变换）：x[n] = Σ w(k) · X[k] · cos(π(2n+1)k / 8)
fn idct1d_4(x: [f64; 4]) -> [f64; 4] {
    const PI: f64 = std::f64::consts::PI;
    const W0: f64 = 0.5;
    const W1: f64 = std::f64::consts::FRAC_1_SQRT_2;
    let w = [W0, W1, W1, W1];
    let mut y = [0.0f64; 4];
    for n in 0..4 {
        let sum: f64 = (0..4)
            .map(|k| w[k] * x[k] * (PI * (2 * n + 1) as f64 * k as f64 / 8.0).cos())
            .sum();
        y[n] = sum;
    }
    y
}

/// 2D 正交 DCT（行 DCT → 列 DCT），与 OpenCV cv2.dct 一致
fn dct2d_block(block: [f64; 16]) -> [f64; 16] {
    // 行 DCT
    let mut temp = [0.0f64; 16];
    for r in 0..4 {
        let row = [block[r * 4], block[r * 4 + 1], block[r * 4 + 2], block[r * 4 + 3]];
        let d = dct1d_4(row);
        for c in 0..4 {
            temp[r * 4 + c] = d[c];
        }
    }
    // 列 DCT
    let mut result = [0.0f64; 16];
    for c in 0..4 {
        let col = [temp[c], temp[4 + c], temp[8 + c], temp[12 + c]];
        let d = dct1d_4(col);
        for r in 0..4 {
            result[r * 4 + c] = d[r];
        }
    }
    result
}

/// 2D 正交 IDCT（列 IDCT → 行 IDCT），与 OpenCV cv2.idct 一致
fn idct2d_block(block: [f64; 16]) -> [f64; 16] {
    // 列 IDCT
    let mut temp = [0.0f64; 16];
    for c in 0..4 {
        let col = [block[c], block[4 + c], block[8 + c], block[12 + c]];
        let d = idct1d_4(col);
        for r in 0..4 {
            temp[r * 4 + c] = d[r];
        }
    }
    // 行 IDCT
    let mut result = [0.0f64; 16];
    for r in 0..4 {
        let row = [temp[r * 4], temp[r * 4 + 1], temp[r * 4 + 2], temp[r * 4 + 3]];
        let d = idct1d_4(row);
        for c in 0..4 {
            result[r * 4 + c] = d[c];
        }
    }
    result
}

/// 4×4 矩阵的 SVD，返回 (U, S, Vt)，奇异值降序排列
///
/// 使用 nalgebra 的 Matrix4<f64>，与 numpy.linalg.svd 约定一致。
fn svd_4x4(data: [f64; 16]) -> ([f64; 16], [f64; 4], [f64; 16]) {
    let m = Matrix4::<f64>::from_row_slice(&data);
    let svd = m.svd(true, true);
    let u = svd.u.unwrap();
    let s = svd.singular_values;
    let vt = svd.v_t.unwrap();

    let mut u_arr = [0.0f64; 16];
    let mut s_arr = [0.0f64; 4];
    let mut vt_arr = [0.0f64; 16];
    for i in 0..4 {
        for j in 0..4 {
            u_arr[i * 4 + j] = u[(i, j)];
            vt_arr[i * 4 + j] = vt[(i, j)];
        }
        s_arr[i] = s[i];
    }
    (u_arr, s_arr, vt_arr)
}

/// 从 U、S、Vt 重建矩阵（展平为 16 元素数组）
fn reconstruct_svd(u: &[f64; 16], s: &[f64; 4], vt: &[f64; 16]) -> [f64; 16] {
    let u_mat = Matrix4::<f64>::from_row_slice(u);
    let vt_mat = Matrix4::<f64>::from_row_slice(vt);
    let s_vec = Vector4::new(s[0], s[1], s[2], s[3]);
    let s_diag = Matrix4::from_diagonal(&s_vec);
    let result_mat = u_mat * s_diag * vt_mat;

    let mut result = [0.0f64; 16];
    for i in 0..4 {
        for j in 0..4 {
            result[i * 4 + j] = result_mat[(i, j)];
        }
    }
    result
}

/// QIM 嵌入（与 Python `(s//d + 0.25 + 0.5*bit)*d` 完全一致）
///
/// bit=0 → 量化到 0.25*d 处，bit=1 → 量化到 0.75*d 处
fn qim_encode(s: f64, bit: u8, d: f64) -> f64 {
    (s / d).floor() * d + (0.25 + 0.5 * bit as f64) * d
}

/// QIM 软判决提取
///
/// 若 `s % d > d/2` → 1.0（bit=1），否则 → 0.0（bit=0）
fn qim_decode_soft(s: f64, d: f64) -> f64 {
    if d <= 0.0 {
        return 0.5;
    }
    // 奇异值 s >= 0，直接取模
    let remainder = s - (s / d).floor() * d;
    if remainder > d / 2.0 { 1.0 } else { 0.0 }
}

/// 为指定块生成确定性随机置换（嵌入/提取使用相同置换保证一致性）
fn generate_shuffler(password: u64, block_idx: usize) -> [usize; 16] {
    let seed = password.wrapping_mul(1_000_003).wrapping_add(block_idx as u64);
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut perm: [usize; 16] = std::array::from_fn(|i| i);
    perm.shuffle(&mut rng);
    perm
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn test_dct1d_roundtrip() {
        let x = [1.0f64, 3.0, 5.0, 7.0];
        let dct = dct1d_4(x);
        let back = idct1d_4(dct);
        for i in 0..4 {
            assert!((x[i] - back[i]).abs() < 1e-10, "mismatch at {}: {} vs {}", i, x[i], back[i]);
        }
    }

    #[test]
    fn test_dct2d_roundtrip() {
        let block: [f64; 16] = [
            10.0, 20.0, 30.0, 40.0,
            50.0, 60.0, 70.0, 80.0,
            90.0, 100.0, 110.0, 120.0,
            130.0, 140.0, 150.0, 160.0,
        ];
        let dct = dct2d_block(block);
        let back = idct2d_block(dct);
        for i in 0..16 {
            assert!((block[i] - back[i]).abs() < 1e-8,
                "mismatch at {}: {} vs {}", i, block[i], back[i]);
        }
    }

    /// 常数块 DC 系数 = 4v（行列各乘以 2 的 DC 增益）
    #[test]
    fn test_dct2d_constant_block() {
        let block = [150.0f64; 16];
        let dct = dct2d_block(block);
        // 行DCT DC = 0.5 * 4 * 150 = 300, 列DCT DC = 0.5 * 4 * 300 = 600
        assert!((dct[0] - 600.0).abs() < 1e-8, "DC 系数应为 600，得 {}", dct[0]);
        for i in 1..16 {
            assert!(dct[i].abs() < 1e-8, "AC 系数 [{}] 应为 0，得 {}", i, dct[i]);
        }
    }

    #[test]
    fn test_svd_reconstruct() {
        let data: [f64; 16] = [
            600.0, 10.0, 5.0, 2.0,
            10.0, 50.0, 3.0, 1.0,
            5.0, 3.0, 30.0, 0.5,
            2.0, 1.0, 0.5, 10.0,
        ];
        let (u, s, vt) = svd_4x4(data);
        let reconstructed = reconstruct_svd(&u, &s, &vt);
        for i in 0..16 {
            assert!((data[i] - reconstructed[i]).abs() < 1e-6,
                "SVD 重建误差 [{}]: {} vs {}", i, data[i], reconstructed[i]);
        }
    }

    #[test]
    fn test_qim_encode_decode() {
        for &original_s in &[100.0f64, 250.5, 500.0, 999.9, 36.1, 36.9] {
            for &bit in &[0u8, 1u8] {
                let encoded = qim_encode(original_s, bit, D1);
                let decoded = qim_decode_soft(encoded, D1) as u8;
                assert_eq!(decoded, bit,
                    "QIM roundtrip 失败: s={}, bit={}, encoded={}", original_s, bit, encoded);
            }
        }
    }

    #[test]
    fn test_embed_extract_no_quantization() {
        let processor = DCTProcessor::new();
        let mut ll = Array2::from_elem((128, 128), 128.0);

        let wm_bits: Vec<u8> = (0..128).map(|i| (i % 2) as u8).collect();
        processor.embed_watermark_blocks(&mut ll, &wm_bits).unwrap();

        let soft = processor.extract_watermark_blocks_soft(&ll, 128).unwrap();
        let extracted: Vec<u8> = soft.iter().map(|&v| if v > 0.5 { 1u8 } else { 0u8 }).collect();

        let matches = wm_bits.iter().zip(extracted.iter()).filter(|(a, b)| a == b).count();
        assert_eq!(matches, 128, "嵌入后立即提取应 100% 准确: {}/128", matches);
    }

    #[test]
    fn test_embed_extract_544bits() {
        let processor = DCTProcessor::new();
        let mut ll = Array2::from_elem((128, 128), 100.0);
        let wm_bits: Vec<u8> = (0..544).map(|i| (i % 3 == 0) as u8).collect();

        processor.embed_watermark_blocks(&mut ll, &wm_bits).unwrap();

        let soft = processor.extract_watermark_blocks_soft(&ll, 544).unwrap();
        let extracted: Vec<u8> = soft.iter().map(|&v| if v > 0.5 { 1u8 } else { 0u8 }).collect();

        let matches = wm_bits.iter().zip(extracted.iter()).filter(|(a, b)| a == b).count();
        assert_eq!(matches, 544, "544 位水印应 100% 提取正确: {}/544", matches);
    }

    #[test]
    fn test_image_too_small_returns_error() {
        let processor = DCTProcessor::new();
        let ll = Array2::zeros((64, 64));
        let result = processor.extract_watermark_blocks_soft(&ll, 544);
        assert!(result.is_err(), "图片太小应返回错误");
    }

    #[test]
    fn test_shuffler_deterministic() {
        let p1 = generate_shuffler(1, 42);
        let p2 = generate_shuffler(1, 42);
        assert_eq!(p1, p2, "相同参数应生成相同置换");

        let p3 = generate_shuffler(1, 43);
        assert_ne!(p1, p3, "不同块索引应生成不同置换");
    }
}
