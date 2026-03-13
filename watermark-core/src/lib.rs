pub mod dwt_manager;
pub mod image_manager;
pub mod schema;
pub mod transform_manager;
pub mod qim_manager;
pub mod embed_manager;

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum WatermarkError {
    #[error("Invalid payload length. Expected {0} bytes, got {1}")]
    InvalidPayloadLength(usize, usize),
    #[error("Reed-Solomon encoding/decoding failed")]
    FecError,
    #[error("Payload is too corrupted to reconstruct")]
    UnrecoverableCorruption,
    #[error("Image too small. Need at least ~344×344 pixels, got {width}×{height}")]
    ImageTooSmall { width: usize, height: usize },
    #[error("Invalid schema: {0}")]
    InvalidSchema(String),
    #[error("No valid provenance found at any step size")]
    NoProvenanceFound,
}

pub struct PayloadManager {
    rs: ReedSolomon,
    data_shards: usize,
    parity_shards: usize,
}

impl PayloadManager {
    /// Initializes a new Payload Manager with fixed 36 data bytes and 16 parity bytes (52 bytes total).
    pub fn new() -> Result<Self, WatermarkError> {
        Self::with_params(36, 16)
    }

    /// Initializes with custom data and parity shard counts.
    pub fn with_params(data_shards: usize, parity_shards: usize) -> Result<Self, WatermarkError> {
        let rs = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|_| WatermarkError::FecError)?;

        Ok(Self {
            rs,
            data_shards,
            parity_shards,
        })
    }

    pub fn total_payload_bytes(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    /// Takes exactly `data_shards` bytes and returns a robust payload.
    pub fn encode_payload(&self, data: &[u8]) -> Result<Vec<u8>, WatermarkError> {
        if data.len() != self.data_shards {
            return Err(WatermarkError::InvalidPayloadLength(
                self.data_shards,
                data.len(),
            ));
        }

        let mut shards: Vec<Vec<u8>> = data.iter().map(|&b| vec![b]).collect();

        for _ in 0..self.parity_shards {
            shards.push(vec![0]);
        }

        self.rs.encode(&mut shards).map_err(|_| WatermarkError::FecError)?;

        let payload: Vec<u8> = shards.into_iter().flatten().collect();
        Ok(payload)
    }

    /// Takes a payload where unsure/corrupted bytes are marked as `None`.
    /// Returns the original data if reconstruction is successful.
    pub fn decode_payload(&self, extracted_payload: &[Option<u8>]) -> Result<Vec<u8>, WatermarkError> {
        let total_shards = self.data_shards + self.parity_shards;

        if extracted_payload.len() != total_shards {
            return Err(WatermarkError::UnrecoverableCorruption);
        }

        let mut shards: Vec<Option<Vec<u8>>> = extracted_payload
            .iter()
            .map(|opt_byte| opt_byte.map(|b| vec![b]))
            .collect();

        self.rs
            .reconstruct(&mut shards)
            .map_err(|_| WatermarkError::UnrecoverableCorruption)?;

        let mut original_data = Vec::with_capacity(self.data_shards);
        for i in 0..self.data_shards {
            if let Some(shard) = &shards[i] {
                original_data.push(shard[0]);
            } else {
                return Err(WatermarkError::UnrecoverableCorruption);
            }
        }

        Ok(original_data)
    }
}

// =============================================================================
// High-Level Facade (Tauri / Trust Systems Integration)
// =============================================================================

use image::RgbImage;

pub use schema::ProvenancePayload;

/// Minimum image dimension (each axis) for 58-byte payload. HL band must hold 464 blocks.
const MIN_IMAGE_DIM: usize = 344;

/// Step sizes probed during extraction when caller does not specify.
const EXTRACT_STEP_SIZES: [f64; 7] = [25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0];

/// Embeds provenance into an image in-place. Accepts and mutates an `RgbImage` buffer.
/// Designed for in-memory pipelines (e.g., load → C2PA inject → watermark → single write).
pub fn apply_watermark(
    image: &mut RgbImage,
    payload: &ProvenancePayload,
    step_size: f64,
) -> Result<(), WatermarkError> {
    let (width, height) = image.dimensions();
    let width = width as usize;
    let height = height as usize;

    if width < MIN_IMAGE_DIM || height < MIN_IMAGE_DIM {
        return Err(WatermarkError::ImageTooSmall { width, height });
    }

    let payload_manager = PayloadManager::with_params(42, 16)?;
    let raw_bytes = payload.to_bytes();
    let robust_payload = payload_manager.encode_payload(&raw_bytes[..])?;

    let mut ycbcr = crate::image_manager::YCbCrImage::from_rgb(image);
    let engine = crate::embed_manager::WatermarkEngine::new(step_size);
    engine.embed(
        &mut ycbcr.y_channel,
        ycbcr.width as usize,
        ycbcr.height as usize,
        &robust_payload,
    );

    let output = ycbcr.to_rgb();
    for (dst, src) in image.pixels_mut().zip(output.pixels()) {
        *dst = *src;
    }

    Ok(())
}

/// Tries to decode a 58-byte chunk with phase search (464 bit rotations).
/// Cropping misaligns the block grid; one rotation yields the correct payload.
fn try_decode_chunk_with_phase_search(
    chunk: &[Option<u8>],
    payload_manager: &PayloadManager,
) -> Option<ProvenancePayload> {
    const BITS_PER_PAYLOAD: usize = 464; // 58 bytes * 8

    // Expand 58 bytes -> 464 bits (LSB first, matching embed)
    let mut bits = Vec::with_capacity(BITS_PER_PAYLOAD);
    for opt_byte in chunk.iter().take(58) {
        if let Some(b) = opt_byte {
            for i in 0..8 {
                bits.push(Some((b >> i) & 1));
            }
        } else {
            for _ in 0..8 {
                bits.push(None);
            }
        }
    }
    if bits.len() < BITS_PER_PAYLOAD {
        return None;
    }

    for phase in 0..BITS_PER_PAYLOAD {
        let mut rotated = Vec::with_capacity(58);
        for byte_idx in 0..58 {
            let mut byte_val: Option<u8> = Some(0);
            for bit_idx in 0..8 {
                let pos = (phase + byte_idx * 8 + bit_idx) % BITS_PER_PAYLOAD;
                if let Some(Some(b)) = bits.get(pos).copied() {
                    if let Some(ref mut v) = byte_val {
                        *v |= b << bit_idx;
                    }
                } else {
                    byte_val = None;
                    break;
                }
            }
            rotated.push(byte_val);
        }

        if let Ok(raw_bytes) = payload_manager.decode_payload(&rotated) {
            if let Ok(provenance) = ProvenancePayload::from_bytes(&raw_bytes) {
                if provenance.version == 1 {
                    return Some(provenance);
                }
            }
        }
    }
    None
}

/// Extracts provenance from an image. Does not mutate the input; clones internally for
/// step-size probing. Auto-probes common step sizes [25, 30, 35, 40, 45, 50, 55].
/// Phase search handles cropped images where block grid is misaligned.
/// Returns `(ProvenancePayload, step_size)` where `step_size` is the value that succeeded.
pub fn extract_watermark(
    image: &RgbImage,
) -> Result<(ProvenancePayload, f64), WatermarkError> {
    let (width, height) = image.dimensions();
    let width = width as usize;
    let height = height as usize;

    if width < MIN_IMAGE_DIM || height < MIN_IMAGE_DIM {
        return Err(WatermarkError::ImageTooSmall { width, height });
    }

    let payload_manager = PayloadManager::with_params(42, 16)?;
    let expected_bytes = payload_manager.total_payload_bytes();
    let ycbcr = crate::image_manager::YCbCrImage::from_rgb(image);

    for &step_size in &EXTRACT_STEP_SIZES {
        let engine = crate::embed_manager::WatermarkEngine::new(step_size);
        let mut y_mut = ycbcr.y_channel.clone();
        let payload_chunks = engine.extract(&mut y_mut, width, height, expected_bytes);

        // Try each spatially repeated chunk until CRC32 clicks.
        // Phase search: cropping misaligns block grid; try all 464 bit-offset rotations.
        for chunk in payload_chunks {
            if let Some(provenance) =
                try_decode_chunk_with_phase_search(&chunk, &payload_manager)
            {
                return Ok((provenance, step_size));
            }
        }
    }

    Err(WatermarkError::NoProvenanceFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_perfect() {
        let manager = PayloadManager::new().unwrap();
        let original_cid = b"QmYwAPJzv5CZsnA625s3Xf2yx2Kx4RkQ9zUq"; // 36 byte CID-like string

        let payload = manager.encode_payload(original_cid).unwrap();
        assert_eq!(payload.len(), 52);

        // Simulate a perfect extraction
        let extracted: Vec<Option<u8>> = payload.into_iter().map(Some).collect();
        let decoded = manager.decode_payload(&extracted).unwrap();

        assert_eq!(decoded, original_cid);
    }

    #[test]
    fn test_recover_from_erasures() {
        let manager = PayloadManager::new().unwrap();
        let original_cid = b"QmYwAPJzv5CZsnA625s3Xf2yx2Kx4RkQ9zUq";

        let payload = manager.encode_payload(original_cid).unwrap();

        // Simulate extraction where 16 bytes were destroyed/unreadable (at parity limit)
        let mut extracted: Vec<Option<u8>> = payload.into_iter().map(Some).collect();
        for i in (0..52).step_by(3).take(16) {
            extracted[i] = None;
        }

        let decoded = manager.decode_payload(&extracted).unwrap();
        assert_eq!(
            decoded, original_cid,
            "Failed to recover from 16 missing bytes"
        );
    }

    #[test]
    fn test_unrecoverable_corruption() {
        let manager = PayloadManager::new().unwrap();
        let original_cid = b"QmYwAPJzv5CZsnA625s3Xf2yx2Kx4RkQ9zUq";

        let payload = manager.encode_payload(original_cid).unwrap();

        // Erase 17 bytes (1 over our 16 parity limit)
        let mut extracted: Vec<Option<u8>> = payload.into_iter().map(Some).collect();
        for i in 0..17 {
            extracted[i] = None;
        }

        let result = manager.decode_payload(&extracted);
        assert_eq!(
            result.unwrap_err(),
            WatermarkError::UnrecoverableCorruption
        );
    }
}
