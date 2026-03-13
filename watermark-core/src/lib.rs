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
