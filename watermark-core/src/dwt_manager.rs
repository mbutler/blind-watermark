/// A lossless 2D Haar Wavelet Transform engine
pub struct DwtEngine;

impl DwtEngine {
    /// Performs a 1-level 2D Forward Haar Transform.
    /// Splits the matrix into LL, HL, LH, and HH quadrants.
    pub fn forward_2d(matrix: &mut [f64], width: usize, height: usize) {
        // Enforce even dimensions for the transform
        let w = width & !1;
        let h = height & !1;

        let mut temp = vec![0.0; w * h];
        let half_w = w / 2;
        let half_h = h / 2;

        // 1. Process Rows (Split into Low and High frequency columns)
        for y in 0..h {
            for x in 0..half_w {
                let a = matrix[y * width + 2 * x];
                let b = matrix[y * width + 2 * x + 1];
                temp[y * w + x] = (a + b) / 2.0;             // L
                temp[y * w + half_w + x] = (a - b) / 2.0;    // H
            }
        }

        // 2. Process Columns (Split into Low and High frequency rows)
        for x in 0..w {
            for y in 0..half_h {
                let a = temp[2 * y * w + x];
                let b = temp[(2 * y + 1) * w + x];
                matrix[y * width + x] = (a + b) / 2.0;             // LL
                matrix[(half_h + y) * width + x] = (a - b) / 2.0;  // LH / HL
            }
        }
    }

    /// Performs a 1-level 2D Inverse Haar Transform perfectly reconstructing the pixels.
    pub fn inverse_2d(matrix: &mut [f64], width: usize, height: usize) {
        let w = width & !1;
        let h = height & !1;

        let mut temp = vec![0.0; w * h];
        let half_w = w / 2;
        let half_h = h / 2;

        // 1. Inverse Columns
        for x in 0..w {
            for y in 0..half_h {
                let l = matrix[y * width + x];
                let h_freq = matrix[(half_h + y) * width + x];
                temp[2 * y * w + x] = l + h_freq;
                temp[(2 * y + 1) * w + x] = l - h_freq;
            }
        }

        // 2. Inverse Rows
        for y in 0..h {
            for x in 0..half_w {
                let l = temp[y * w + x];
                let h_freq = temp[y * w + half_w + x];
                matrix[y * width + 2 * x] = l + h_freq;
                matrix[y * width + 2 * x + 1] = l - h_freq;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dwt_roundtrip() {
        let width = 4;
        let height = 4;
        let original: Vec<f64> = (0..16).map(|i| i as f64).collect();
        let mut matrix = original.clone();

        DwtEngine::forward_2d(&mut matrix, width, height);
        assert_ne!(matrix, original, "Forward DWT should alter the matrix");

        DwtEngine::inverse_2d(&mut matrix, width, height);

        for i in 0..16 {
            assert!(
                (matrix[i] - original[i]).abs() < 0.0001,
                "IDWT failed to reconstruct pixels"
            );
        }
    }
}
