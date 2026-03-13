use crate::dwt_manager::DwtEngine;
use crate::qim_manager::QimEngine;
use crate::transform_manager::BlockDctEngine;

pub struct WatermarkEngine {
    dct: BlockDctEngine,
    qim: QimEngine,
    target_coeff: usize,
}

impl WatermarkEngine {
    pub fn new(step_size: f64) -> Self {
        Self {
            dct: BlockDctEngine::new(),
            qim: QimEngine::new(step_size),
            target_coeff: 27,
        }
    }

    /// Embeds a byte payload into the Y channel. Wraps in DWT, targets HL band.
    pub fn embed(&self, y_channel: &mut [f64], width: usize, height: usize, payload: &[u8]) {
        let w = width & !1;
        let h = height & !1;
        let half_w = w / 2;
        let half_h = h / 2;

        // 1. Convert Spatial Image to DWT Frequencies
        DwtEngine::forward_2d(y_channel, width, height);

        // 2. Extract the HL Sub-band (Top-Right Quadrant)
        let mut hl_band = vec![0.0; half_w * half_h];
        for y in 0..half_h {
            for x in 0..half_w {
                hl_band[y * half_w + x] = y_channel[y * width + (half_w + x)];
            }
        }

        // 3. Run DCT-QIM embedding on the HL band only
        self.embed_dct_blocks(&mut hl_band, half_w, half_h, payload);

        // 4. Write the watermarked HL band back into the DWT grid
        for y in 0..half_h {
            for x in 0..half_w {
                y_channel[y * width + (half_w + x)] = hl_band[y * half_w + x];
            }
        }

        // 5. Reconstruct the image back to Spatial Pixels
        DwtEngine::inverse_2d(y_channel, width, height);
    }

    /// Extracts payload from the Y channel. Runs DWT, reads from HL band.
    /// Requires &mut because it performs the forward DWT in place.
    pub fn extract(
        &self,
        y_channel: &mut [f64],
        width: usize,
        height: usize,
        expected_bytes: usize,
    ) -> Vec<Option<u8>> {
        let w = width & !1;
        let h = height & !1;
        let half_w = w / 2;
        let half_h = h / 2;

        // 1. Convert Spatial Image to DWT Frequencies
        DwtEngine::forward_2d(y_channel, width, height);

        // 2. Extract the HL Sub-band
        let mut hl_band = vec![0.0; half_w * half_h];
        for y in 0..half_h {
            for x in 0..half_w {
                hl_band[y * half_w + x] = y_channel[y * width + (half_w + x)];
            }
        }

        // 3. Extract payload from the HL band
        self.extract_dct_blocks(&hl_band, half_w, half_h, expected_bytes)
    }

    fn embed_dct_blocks(
        &self,
        band: &mut [f64],
        width: usize,
        height: usize,
        payload: &[u8],
    ) {
        let mut bits = Vec::with_capacity(payload.len() * 8);
        for byte in payload {
            for i in 0..8 {
                bits.push((byte >> i) & 1);
            }
        }

        let blocks_x = width / 8;
        let blocks_y = height / 8;
        let mut bit_index = 0;

        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                if bit_index >= bits.len() {
                    return;
                }

                let mut block = [0.0; 64];
                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        block[r * 8 + c] = band[px];
                    }
                }

                self.dct.forward_2d(&mut block);
                block[self.target_coeff] =
                    self.qim.embed_bit(block[self.target_coeff], bits[bit_index]);
                bit_index += 1;
                self.dct.inverse_2d(&mut block);

                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        band[px] = block[r * 8 + c];
                    }
                }
            }
        }
    }

    fn extract_dct_blocks(
        &self,
        band: &[f64],
        width: usize,
        height: usize,
        expected_bytes: usize,
    ) -> Vec<Option<u8>> {
        let expected_bits = expected_bytes * 8;
        let mut bits = Vec::with_capacity(expected_bits);

        let blocks_x = width / 8;
        let blocks_y = height / 8;

        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                if bits.len() >= expected_bits {
                    break;
                }

                let mut block = [0.0; 64];
                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        block[r * 8 + c] = band[px];
                    }
                }

                self.dct.forward_2d(&mut block);
                let opt_bit = self.qim.extract_bit(block[self.target_coeff], 0.50);
                bits.push(opt_bit);
            }
        }

        let mut payload = Vec::with_capacity(expected_bytes);
        for bit_chunk in bits.chunks_exact(8) {
            let mut byte = 0u8;
            let mut valid_byte = true;
            for (i, &opt_bit) in bit_chunk.iter().enumerate() {
                if let Some(bit) = opt_bit {
                    byte |= bit << i;
                } else {
                    valid_byte = false;
                    break;
                }
            }
            if valid_byte {
                payload.push(Some(byte));
            } else {
                payload.push(None);
            }
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_embed_extract_pipeline() {
        // DWT halves each dimension; HL band is 1/4 of original. Need 80 blocks for 10 bytes.
        // 80 blocks = 8*10, so HL needs 80x64 min -> original 160x128. Use 352x352 for margin.
        let width = 352;
        let height = 352;
        let mut y_channel = vec![128.0; width * height];
        let payload = b"HELLOWORLD";

        let engine = WatermarkEngine::new(20.0);
        engine.embed(&mut y_channel, width, height, payload);

        let mut y_for_extract = y_channel.clone();
        let extracted = engine.extract(&mut y_for_extract, width, height, payload.len());

        let extracted_bytes: Vec<u8> = extracted.into_iter().map(|o| o.unwrap()).collect();
        assert_eq!(&extracted_bytes, payload, "Pipeline failed to extract exact payload");
    }
}
