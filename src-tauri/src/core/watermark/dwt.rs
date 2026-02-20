use ndarray::{Array2, ArrayView2, s};
use crate::models::BlindMarkError;

/// DWT (Discrete Wavelet Transform) processor using Haar wavelet
///
/// Implements 2-level Haar wavelet decomposition for image watermarking.
/// The Haar wavelet is the simplest wavelet and works well for watermarking.
pub struct DWTProcessor {
    level: usize,
}

/// Container for DWT decomposition components
///
/// For 2-level decomposition:
/// - Level 2: LL2, LH2, HL2, HH2 (from decomposing LL1)
/// - Level 1: LH1, HL1, HH1 (kept for reconstruction)
#[derive(Debug, Clone)]
pub struct DWTComponents {
    /// Low-low (approximation) - level 2
    pub ll: Array2<f64>,
    /// Low-high (horizontal details) - level 2, used for watermarking
    pub lh: Array2<f64>,
    /// High-low (vertical details) - level 2, alternative for watermarking
    pub hl: Array2<f64>,
    /// High-high (diagonal details) - level 2
    pub hh: Array2<f64>,
    /// Level 1 components needed for reconstruction
    pub lh1: Array2<f64>,
    pub hl1: Array2<f64>,
    pub hh1: Array2<f64>,
}

impl DWTProcessor {
    /// Create a new DWT processor with 2-level decomposition
    pub fn new() -> Self {
        Self { level: 2 }
    }

    /// Perform 2-level DWT decomposition on image data
    ///
    /// # Arguments
    /// * `image_data` - 2D array of image data (grayscale values as f64)
    ///
    /// # Returns
    /// * `DWTComponents` containing all frequency subbands
    ///
    /// # Algorithm
    /// 1. Apply 1D Haar transform to each row
    /// 2. Apply 1D Haar transform to each column
    /// 3. Repeat on LL subband for level 2
    pub fn decompose(&self, image_data: ArrayView2<f64>) -> Result<DWTComponents, BlindMarkError> {
        let (height, width) = image_data.dim();

        // Ensure dimensions are even for Haar transform
        if height % 2 != 0 || width % 2 != 0 {
            return Err(BlindMarkError::ImageProcessing(
                format!("Image dimensions must be even: {}x{}", height, width)
            ));
        }

        // Convert view to owned array for processing
        let image_owned = image_data.to_owned();

        // Level 1 decomposition
        let (ll1, lh1, hl1, hh1) = self.dwt_2d(&image_owned)?;

        // Level 2 decomposition on LL1 subband
        let (ll2, lh2, hl2, hh2) = self.dwt_2d(&ll1)?;

        Ok(DWTComponents {
            ll: ll2,
            lh: lh2,  // Mid-frequency horizontal - good for watermarking
            hl: hl2,  // Mid-frequency vertical - alternative for watermarking
            hh: hh2,
            lh1,
            hl1,
            hh1,
        })
    }

    /// Reconstruct image from DWT components
    ///
    /// # Arguments
    /// * `components` - DWT components including modified watermarked data
    ///
    /// # Returns
    /// * Reconstructed 2D array
    pub fn reconstruct(&self, components: DWTComponents) -> Result<Array2<f64>, BlindMarkError> {
        // Reconstruct level 2 LL subband from level 2 components
        let ll1_reconstructed = self.idwt_2d(
            &components.ll,
            &components.lh,
            &components.hl,
            &components.hh,
        )?;

        // Reconstruct final image from level 1 components
        let reconstructed = self.idwt_2d(
            &ll1_reconstructed,
            &components.lh1,
            &components.hl1,
            &components.hh1,
        )?;

        Ok(reconstructed)
    }

    /// Perform 1-level DWT decomposition, returning (LL, LH, HL, HH) subbands
    ///
    /// Used by the Python blind_watermark-compatible pipeline which embeds into the
    /// LL (low-frequency approximation) subband.
    pub fn decompose_1level(
        &self,
        image_data: ArrayView2<f64>,
    ) -> Result<(Array2<f64>, Array2<f64>, Array2<f64>, Array2<f64>), BlindMarkError> {
        let (height, width) = image_data.dim();
        if height % 2 != 0 || width % 2 != 0 {
            return Err(BlindMarkError::ImageProcessing(
                format!("Image dimensions must be even for DWT: {}x{}", height, width)
            ));
        }
        let image_owned = image_data.to_owned();
        self.dwt_2d(&image_owned)
    }

    /// Reconstruct image from 1-level DWT components
    pub fn reconstruct_1level(
        &self,
        ll: &Array2<f64>,
        lh: &Array2<f64>,
        hl: &Array2<f64>,
        hh: &Array2<f64>,
    ) -> Result<Array2<f64>, BlindMarkError> {
        self.idwt_2d(ll, lh, hl, hh)
    }

    /// Perform 2D Haar wavelet transform
    ///
    /// Returns (LL, LH, HL, HH) subbands
    fn dwt_2d(&self, data: &Array2<f64>) -> Result<(Array2<f64>, Array2<f64>, Array2<f64>, Array2<f64>), BlindMarkError> {
        let (height, width) = data.dim();
        let half_h = height / 2;
        let half_w = width / 2;

        // Step 1: Transform rows
        let mut row_transformed = Array2::zeros((height, width));
        for i in 0..height {
            let row = data.slice(s![i, ..]);
            let row_vec = row.to_vec();
            let (low, high) = self.haar_1d(&row_vec)?;

            // Place low frequencies in left half, high in right half
            for j in 0..half_w {
                row_transformed[[i, j]] = low[j];
                row_transformed[[i, j + half_w]] = high[j];
            }
        }

        // Step 2: Transform columns
        let mut result = Array2::zeros((height, width));
        for j in 0..width {
            let col = row_transformed.slice(s![.., j]);
            let col_vec = col.to_vec();
            let (low, high) = self.haar_1d(&col_vec)?;

            // Place low frequencies in top half, high in bottom half
            for i in 0..half_h {
                result[[i, j]] = low[i];
                result[[i + half_h, j]] = high[i];
            }
        }

        // Extract four subbands
        let ll = result.slice(s![0..half_h, 0..half_w]).to_owned();
        let lh = result.slice(s![0..half_h, half_w..width]).to_owned();
        let hl = result.slice(s![half_h..height, 0..half_w]).to_owned();
        let hh = result.slice(s![half_h..height, half_w..width]).to_owned();

        Ok((ll, lh, hl, hh))
    }

    /// Perform 2D inverse Haar wavelet transform
    fn idwt_2d(
        &self,
        ll: &Array2<f64>,
        lh: &Array2<f64>,
        hl: &Array2<f64>,
        hh: &Array2<f64>,
    ) -> Result<Array2<f64>, BlindMarkError> {
        let (half_h, half_w) = ll.dim();
        let height = half_h * 2;
        let width = half_w * 2;

        // Step 1: Combine subbands
        let mut combined = Array2::zeros((height, width));

        // Place subbands in their positions
        combined.slice_mut(s![0..half_h, 0..half_w]).assign(ll);
        combined.slice_mut(s![0..half_h, half_w..width]).assign(lh);
        combined.slice_mut(s![half_h..height, 0..half_w]).assign(hl);
        combined.slice_mut(s![half_h..height, half_w..width]).assign(hh);

        // Step 2: Inverse transform columns
        let mut col_transformed = Array2::zeros((height, width));
        for j in 0..width {
            let col = combined.slice(s![.., j]);
            let low = col.slice(s![0..half_h]).to_vec();
            let high = col.slice(s![half_h..height]).to_vec();

            let reconstructed = self.ihaar_1d(&low, &high)?;
            for i in 0..height {
                col_transformed[[i, j]] = reconstructed[i];
            }
        }

        // Step 3: Inverse transform rows
        let mut result = Array2::zeros((height, width));
        for i in 0..height {
            let row = col_transformed.slice(s![i, ..]);
            let low = row.slice(s![0..half_w]).to_vec();
            let high = row.slice(s![half_w..width]).to_vec();

            let reconstructed = self.ihaar_1d(&low, &high)?;
            for j in 0..width {
                result[[i, j]] = reconstructed[j];
            }
        }

        Ok(result)
    }

    /// 1D Haar wavelet transform
    ///
    /// Computes averages (low frequencies) and differences (high frequencies)
    /// Formula:
    /// - Low: (x[2i] + x[2i+1]) / sqrt(2)
    /// - High: (x[2i] - x[2i+1]) / sqrt(2)
    fn haar_1d(&self, signal: &[f64]) -> Result<(Vec<f64>, Vec<f64>), BlindMarkError> {
        let len = signal.len();
        if len % 2 != 0 {
            return Err(BlindMarkError::ImageProcessing(
                "Signal length must be even for Haar transform".to_string()
            ));
        }

        let half_len = len / 2;
        let mut low = Vec::with_capacity(half_len);
        let mut high = Vec::with_capacity(half_len);

        let sqrt2 = std::f64::consts::SQRT_2;

        for i in 0..half_len {
            let even = signal[2 * i];
            let odd = signal[2 * i + 1];

            // Averaging (approximation)
            low.push((even + odd) / sqrt2);

            // Differencing (detail)
            high.push((even - odd) / sqrt2);
        }

        Ok((low, high))
    }

    /// 1D inverse Haar wavelet transform
    ///
    /// Reconstructs signal from low and high frequency components
    /// Formula:
    /// - x[2i] = (low[i] + high[i]) / sqrt(2)
    /// - x[2i+1] = (low[i] - high[i]) / sqrt(2)
    fn ihaar_1d(&self, low: &[f64], high: &[f64]) -> Result<Vec<f64>, BlindMarkError> {
        if low.len() != high.len() {
            return Err(BlindMarkError::ImageProcessing(
                "Low and high frequency components must have same length".to_string()
            ));
        }

        let half_len = low.len();
        let mut signal = Vec::with_capacity(half_len * 2);

        let sqrt2 = std::f64::consts::SQRT_2;

        for i in 0..half_len {
            let l = low[i];
            let h = high[i];

            // Reconstruct even sample
            signal.push((l + h) / sqrt2);

            // Reconstruct odd sample
            signal.push((l - h) / sqrt2);
        }

        Ok(signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn test_haar_1d_transform() {
        let processor = DWTProcessor::new();
        let signal = vec![1.0, 2.0, 3.0, 4.0];

        let (low, high) = processor.haar_1d(&signal).unwrap();

        assert_eq!(low.len(), 2);
        assert_eq!(high.len(), 2);

        // Check approximate values
        let sqrt2 = std::f64::consts::SQRT_2;
        assert!((low[0] - 3.0 / sqrt2).abs() < 0.001);
        assert!((high[0] - (-1.0) / sqrt2).abs() < 0.001);
    }

    #[test]
    fn test_haar_1d_roundtrip() {
        let processor = DWTProcessor::new();
        let original = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

        let (low, high) = processor.haar_1d(&original).unwrap();
        let reconstructed = processor.ihaar_1d(&low, &high).unwrap();

        for i in 0..original.len() {
            assert!((original[i] - reconstructed[i]).abs() < 0.0001);
        }
    }

    #[test]
    fn test_dwt_2d_decomposition() {
        let processor = DWTProcessor::new();

        // Create a simple 4x4 test image
        let data = Array2::from_shape_vec(
            (4, 4),
            vec![
                1.0, 2.0, 3.0, 4.0,
                5.0, 6.0, 7.0, 8.0,
                9.0, 10.0, 11.0, 12.0,
                13.0, 14.0, 15.0, 16.0,
            ],
        ).unwrap();

        let (ll, lh, hl, hh) = processor.dwt_2d(&data).unwrap();

        // Check dimensions
        assert_eq!(ll.dim(), (2, 2));
        assert_eq!(lh.dim(), (2, 2));
        assert_eq!(hl.dim(), (2, 2));
        assert_eq!(hh.dim(), (2, 2));
    }

    #[test]
    fn test_dwt_2d_roundtrip() {
        let processor = DWTProcessor::new();

        // Create 8x8 test image
        let mut data = Array2::zeros((8, 8));
        for i in 0..8 {
            for j in 0..8 {
                data[[i, j]] = (i * 8 + j) as f64;
            }
        }

        let (ll, lh, hl, hh) = processor.dwt_2d(&data).unwrap();
        let reconstructed = processor.idwt_2d(&ll, &lh, &hl, &hh).unwrap();

        // Check reconstruction accuracy
        for i in 0..8 {
            for j in 0..8 {
                assert!((data[[i, j]] - reconstructed[[i, j]]).abs() < 0.001);
            }
        }
    }

    #[test]
    fn test_full_decompose_reconstruct() {
        let processor = DWTProcessor::new();

        // Create 16x16 test image (needs to be divisible by 4 for 2-level)
        let mut data = Array2::zeros((16, 16));
        for i in 0..16 {
            for j in 0..16 {
                data[[i, j]] = ((i + j) % 256) as f64;
            }
        }

        let components = processor.decompose(data.view()).unwrap();
        let reconstructed = processor.reconstruct(components).unwrap();

        // Check reconstruction accuracy
        for i in 0..16 {
            for j in 0..16 {
                let diff = (data[[i, j]] - reconstructed[[i, j]]).abs();
                assert!(diff < 0.01, "Mismatch at ({}, {}): {} vs {}", i, j, data[[i, j]], reconstructed[[i, j]]);
            }
        }
    }

    #[test]
    fn test_decompose_odd_dimensions() {
        let processor = DWTProcessor::new();
        let data = Array2::zeros((15, 15)); // Odd dimensions

        let result = processor.decompose(data.view());
        assert!(result.is_err());
    }
}
