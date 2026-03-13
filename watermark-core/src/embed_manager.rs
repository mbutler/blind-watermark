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

    /// Extracts payload chunks from the Y channel. Runs DWT, reads from HL band.
    /// Returns multiple 58-byte chunks (spatial repetition); caller tries each until one passes CRC32.
    /// Requires &mut because it performs the forward DWT in place.
    pub fn extract(
        &self,
        y_channel: &mut [f64],
        width: usize,
        height: usize,
        expected_bytes: usize,
    ) -> Vec<Vec<Option<u8>>> {
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

        // 3. Extract all payload chunks from the HL band (hologram-style repetition)
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

        let total_bits = bits.len();
        let blocks_x = width / 8;
        let blocks_y = height / 8;
        let mut block_index = 0;

        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                let mut block = [0.0; 64];

                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        block[r * 8 + c] = band[px];
                    }
                }

                self.dct.forward_2d(&mut block);

                // Loop the payload; every 464-block chunk contains the full identity
                let current_bit = bits[block_index % total_bits];
                block[self.target_coeff] =
                    self.qim.embed_bit(block[self.target_coeff], current_bit);
                block_index += 1;

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
    ) -> Vec<Vec<Option<u8>>> {
        let expected_bits = expected_bytes * 8;
        let blocks_x = width / 8;
        let blocks_y = height / 8;
        let total_blocks = blocks_x * blocks_y;

        let mut all_bits = Vec::with_capacity(total_blocks);

        // Read EVERY bit from the entire sub-band
        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                let mut block = [0.0; 64];
                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        block[r * 8 + c] = band[px];
                    }
                }

                self.dct.forward_2d(&mut block);
                all_bits.push(self.qim.extract_bit(block[self.target_coeff], 0.45));
            }
        }

        let mut extracted_payloads = Vec::new();

        // Chunk the bits into complete payload packages
        for bit_chunk in all_bits.chunks(expected_bits) {
            if bit_chunk.len() < expected_bits {
                break;
            }

            let mut payload = Vec::with_capacity(expected_bytes);
            for byte_chunk in bit_chunk.chunks_exact(8) {
                let mut byte = 0u8;
                let mut valid_byte = true;

                for (i, &opt_bit) in byte_chunk.iter().enumerate() {
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
            extracted_payloads.push(payload);
        }

        extracted_payloads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_embed_extract_pipeline() {
        // DWT halves each dimension; HL band 176x176 = 484 blocks. 10 bytes = 80 bits.
        // Repetition produces multiple chunks; first chunk should decode correctly.
        let width = 352;
        let height = 352;
        let mut y_channel = vec![128.0; width * height];
        let payload = b"HELLOWORLD";

        let engine = WatermarkEngine::new(20.0);
        engine.embed(&mut y_channel, width, height, payload);

        let mut y_for_extract = y_channel.clone();
        let payload_chunks = engine.extract(&mut y_for_extract, width, height, payload.len());

        // At least one chunk should decode to our payload
        let extracted_bytes: Vec<u8> = payload_chunks[0]
            .iter()
            .map(|o| o.unwrap())
            .collect();
        assert_eq!(&extracted_bytes, payload, "Pipeline failed to extract exact payload");
    }
}
