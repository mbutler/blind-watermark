use rustdct::{DctPlanner, TransformType2And3};
use std::sync::Arc;

/// A specialized engine for performing 2D DCTs on 8x8 blocks.
pub struct BlockDctEngine {
    dct_forward: Arc<dyn TransformType2And3<f64>>,
    dct_inverse: Arc<dyn TransformType2And3<f64>>,
}

impl BlockDctEngine {
    pub fn new() -> Self {
        let mut planner = DctPlanner::new();
        // JPEG and our watermark use 8x8 blocks
        let dct_forward = planner.plan_dct2(8);
        let dct_inverse = planner.plan_dct3(8);

        Self {
            dct_forward,
            dct_inverse,
        }
    }

    /// Performs a Forward 2D DCT on an 8x8 block (in-place).
    /// Converts Spatial pixels -> Frequency coefficients.
    pub fn forward_2d(&self, block: &mut [f64; 64]) {
        // 1. 1D DCT on each row
        for row in 0..8 {
            let start = row * 8;
            self.dct_forward.process_dct2(&mut block[start..start + 8]);
        }

        // 2. Transpose, 1D DCT on columns, and Transpose back
        self.transpose_8x8(block);
        for row in 0..8 {
            let start = row * 8;
            self.dct_forward.process_dct2(&mut block[start..start + 8]);
        }
        self.transpose_8x8(block);
    }

    /// Performs an Inverse 2D DCT on an 8x8 block (in-place).
    /// Converts Frequency coefficients -> Spatial pixels.
    /// rustdct's unscaled DCT2/DCT3 scale the roundtrip; for 8x8 blocks we normalize by 1/16.
    pub fn inverse_2d(&self, block: &mut [f64; 64]) {
        // 1. 1D IDCT on each row
        for row in 0..8 {
            let start = row * 8;
            self.dct_inverse.process_dct3(&mut block[start..start + 8]);
        }

        // 2. Transpose, 1D IDCT on columns, and Transpose back
        self.transpose_8x8(block);
        for row in 0..8 {
            let start = row * 8;
            self.dct_inverse.process_dct3(&mut block[start..start + 8]);
        }
        self.transpose_8x8(block);

        // rustdct roundtrip scaling: empirically 16 for 8x8 (not 64)
        for val in block.iter_mut() {
            *val /= 16.0;
        }
    }

    /// Helper to transpose an 8x8 flat array
    fn transpose_8x8(&self, block: &mut [f64; 64]) {
        for i in 0..8 {
            for j in (i + 1)..8 {
                block.swap(i * 8 + j, j * 8 + i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dct_roundtrip() {
        let engine = BlockDctEngine::new();

        // Create a dummy 8x8 block representing spatial pixel values (Luminance)
        let mut original_block: [f64; 64] = [0.0; 64];
        for i in 0..64 {
            original_block[i] = (i as f64) + 100.0; // Values from 100 to 163
        }

        let mut block = original_block.clone();

        // 1. Transform to Frequency Domain
        engine.forward_2d(&mut block);

        // At this point, block[0] is the DC coefficient (average brightness),
        // and the rest are AC coefficients (frequencies).
        assert_ne!(block, original_block, "DCT should alter the block values");

        // 2. Transform back to Spatial Domain
        engine.inverse_2d(&mut block);

        // 3. Verify Roundtrip (allow for minor floating point drift)
        for i in 0..64 {
            let diff = (original_block[i] - block[i]).abs();
            assert!(
                diff < 0.0001,
                "IDCT failed to perfectly reconstruct the spatial block"
            );
        }
    }
}
