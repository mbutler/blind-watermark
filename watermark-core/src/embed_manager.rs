use crate::transform_manager::BlockDctEngine;
use crate::qim_manager::QimEngine;

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
            // Index 27 is a mid-frequency AC coefficient [row 3, col 3].
            // It balances visual imperceptibility with survival against JPEG compression.
            target_coeff: 27,
        }
    }

    /// Embeds a byte payload into the Y channel. Modifies the channel in-place.
    pub fn embed(&self, y_channel: &mut [f64], width: usize, height: usize, payload: &[u8]) {
        // Break payload down into a flat vector of bits (0s and 1s)
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
                    return; // All bits have been embedded
                }

                let mut block = [0.0; 64];

                // 1. Extract the 8x8 spatial block from the flat Y channel
                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        block[r * 8 + c] = y_channel[px];
                    }
                }

                // 2. Transform into Frequency Domain
                self.dct.forward_2d(&mut block);

                // 3. Embed the bit using QIM
                block[self.target_coeff] =
                    self.qim.embed_bit(block[self.target_coeff], bits[bit_index]);
                bit_index += 1;

                // 4. Transform back to Spatial Domain
                self.dct.inverse_2d(&mut block);

                // 5. Write the modified block back into the Y channel
                for r in 0..8 {
                    for c in 0..8 {
                        let px = (by * 8 + r) * width + (bx * 8 + c);
                        y_channel[px] = block[r * 8 + c];
                    }
                }
            }
        }
    }

    /// Extracts bits from the Y channel and reassembles them into a byte payload.
    /// Uses erasure signaling: if any bit in a byte is uncertain, the whole byte is marked None.
    pub fn extract(
        &self,
        y_channel: &[f64],
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
                        block[r * 8 + c] = y_channel[px];
                    }
                }

                self.dct.forward_2d(&mut block);

                // Extract with a 50% tolerance threshold (marks uncertain bits as erasures)
                let opt_bit = self.qim.extract_bit(block[self.target_coeff], 0.50);
                bits.push(opt_bit);
            }
        }

        // Reassemble bits into bytes; one bad bit marks the whole byte as an erasure
        let mut payload = Vec::with_capacity(expected_bytes);
        for bit_chunk in bits.chunks_exact(8) {
            let mut byte = 0u8;
            let mut valid_byte = true;

            for (i, &opt_bit) in bit_chunk.iter().enumerate() {
                if let Some(bit) = opt_bit {
                    byte |= bit << i;
                } else {
                    valid_byte = false;
                    break; // One bad bit ruins the byte
                }
            }

            if valid_byte {
                payload.push(Some(byte));
            } else {
                payload.push(None); // Signal the erasure to Reed-Solomon
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
        let engine = WatermarkEngine::new(20.0);
        let width = 128;  // 16 blocks across
        let height = 128; // 16 blocks down (256 blocks total)

        // Create a fake Y-channel (flat gray image)
        let mut y_channel = vec![128.0; width * height];

        // A 10-byte dummy payload (requires 80 blocks to embed)
        let payload = b"HELLOWORLD";

        // Embed
        engine.embed(&mut y_channel, width, height, payload);

        // Extract
        let extracted = engine.extract(&y_channel, width, height, payload.len());

        // Verify all bytes survived the roundtrip across 80 different frequency blocks
        let extracted_bytes: Vec<u8> = extracted.into_iter().map(|o| o.unwrap()).collect();
        assert_eq!(
            &extracted_bytes, payload,
            "Pipeline failed to extract exact payload"
        );
    }
}
