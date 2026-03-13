pub struct QimEngine {
    pub step_size: f64,
}

impl QimEngine {
    /// Initializes a new QIM Engine with a specific quantization step size.
    /// A larger step size is more robust to compression but degrades image quality.
    pub fn new(step_size: f64) -> Self {
        Self { step_size }
    }

    /// Embeds a single bit (0 or 1) into a frequency coefficient.
    pub fn embed_bit(&self, coefficient: f64, bit: u8) -> f64 {
        let double_step = 2.0 * self.step_size;

        if bit == 0 {
            // Snap to the nearest EVEN multiple of the step size
            (coefficient / double_step).round() * double_step
        } else {
            // Snap to the nearest ODD multiple of the step size
            ((coefficient - self.step_size) / double_step).round() * double_step + self.step_size
        }
    }

    /// Extracts a bit, returning None if the value has drifted too far from a valid bucket center.
    pub fn extract_bit(&self, coefficient: f64, tolerance: f64) -> Option<u8> {
        let bucket_index = (coefficient.abs() / self.step_size).round() as i64;
        let expected_center = bucket_index as f64 * self.step_size;

        // How far did the JPEG compression push this value?
        let drift = (coefficient.abs() - expected_center).abs();

        // If it drifted beyond our acceptable noise threshold, declare an erasure.
        // tolerance should be a value like 0.35 (meaning it can drift up to 35% of S).
        if drift > self.step_size * tolerance {
            return None;
        }

        if bucket_index % 2 == 0 {
            Some(0)
        } else {
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qim_perfect_roundtrip() {
        let qim = QimEngine::new(20.0);

        let original_val = 45.0;

        // Embed a 0
        let embedded_0 = qim.embed_bit(original_val, 0);
        assert_eq!(embedded_0, 40.0); // 40 is the nearest even multiple of 20 (2 * 20)
        assert_eq!(qim.extract_bit(embedded_0, 0.35), Some(0));

        // Embed a 1
        let embedded_1 = qim.embed_bit(original_val, 1);
        assert_eq!(embedded_1, 60.0); // 60 is the nearest odd multiple of 20 (3 * 20)
        assert_eq!(qim.extract_bit(embedded_1, 0.35), Some(1));
    }

    #[test]
    fn test_qim_compression_resistance() {
        let qim = QimEngine::new(20.0);
        let original_val = 112.0;

        // Embed a 1 (snaps to 100.0, which is 5 * 20)
        let mut embedded = qim.embed_bit(original_val, 1);
        assert_eq!(embedded, 100.0);

        // SIMULATE JPEG COMPRESSION OR RESIZING NOISE
        // We alter the coefficient by 6.0 (within 35% of S=20, so drift tolerance is 7)
        embedded += 6.0;

        // Even with the noise, the coefficient (106.0) should still round to the
        // 100.0 bucket (bucket index 5, which is odd), successfully extracting our 1.
        assert_eq!(qim.extract_bit(embedded, 0.35), Some(1), "Failed to survive simulated noise");
    }
}
