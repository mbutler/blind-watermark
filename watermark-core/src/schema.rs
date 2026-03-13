use std::convert::TryInto;
use thiserror::Error;
use crc32fast::Hasher;

#[derive(Error, Debug, PartialEq)]
pub enum SchemaError {
    #[error("Invalid payload length. Expected 42 bytes, got {0}")]
    InvalidLength(usize),
    #[error("Unsupported schema version: {0}")]
    UnsupportedVersion(u8),
    #[error("Checksum validation failed. Payload is corrupted.")]
    ChecksumMismatch,
}

#[derive(Debug, PartialEq)]
pub struct ProvenancePayload {
    pub version: u8,
    pub compressed_pubkey: [u8; 33],
    pub asset_id: u32,
    /// The CRC32 checksum of the first 38 bytes (used when parsing; set to 0 when constructing for embed).
    pub checksum: u32,
}

impl ProvenancePayload {
    /// Serializes the struct into exactly 42 bytes (38 data + 4 CRC32).
    pub fn to_bytes(&self) -> [u8; 42] {
        let mut bytes = [0u8; 42];

        bytes[0] = self.version;
        bytes[1..34].copy_from_slice(&self.compressed_pubkey);
        bytes[34..38].copy_from_slice(&self.asset_id.to_be_bytes());

        // Calculate CRC32 of the data (first 38 bytes)
        let mut hasher = Hasher::new();
        hasher.update(&bytes[0..38]);
        let calc_checksum = hasher.finalize();

        bytes[38..42].copy_from_slice(&calc_checksum.to_be_bytes());

        bytes
    }

    /// Deserializes a 42-byte slice back into the struct. Verifies checksum first.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        if bytes.len() != 42 {
            return Err(SchemaError::InvalidLength(bytes.len()));
        }

        // 1. Verify checksum FIRST
        let mut hasher = Hasher::new();
        hasher.update(&bytes[0..38]);
        let expected_checksum = hasher.finalize();

        let actual_checksum_bytes: [u8; 4] = bytes[38..42].try_into().unwrap();
        let actual_checksum = u32::from_be_bytes(actual_checksum_bytes);

        if expected_checksum != actual_checksum {
            return Err(SchemaError::ChecksumMismatch);
        }

        // 2. Parse the rest
        if bytes[0] != 1 {
            return Err(SchemaError::UnsupportedVersion(bytes[0]));
        }

        let mut compressed_pubkey = [0u8; 33];
        compressed_pubkey.copy_from_slice(&bytes[1..34]);

        let asset_id_bytes: [u8; 4] = bytes[34..38].try_into().unwrap();
        let asset_id = u32::from_be_bytes(asset_id_bytes);

        Ok(Self {
            version: bytes[0],
            compressed_pubkey,
            asset_id,
            checksum: actual_checksum,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_roundtrip() {
        let original = ProvenancePayload {
            version: 1,
            compressed_pubkey: [0x42; 33],
            asset_id: 999_999,
            checksum: 0,
        };

        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), 42);

        let reconstructed = ProvenancePayload::from_bytes(&bytes).unwrap();
        assert_eq!(reconstructed.version, original.version);
        assert_eq!(reconstructed.compressed_pubkey, original.compressed_pubkey);
        assert_eq!(reconstructed.asset_id, original.asset_id);
    }

    #[test]
    fn test_schema_rejects_corrupted_checksum() {
        let payload = ProvenancePayload {
            version: 1,
            compressed_pubkey: [0x42; 33],
            asset_id: 999_999,
            checksum: 0,
        };
        let mut bytes = payload.to_bytes();
        bytes[41] ^= 0xFF; // Corrupt the checksum
        assert_eq!(
            ProvenancePayload::from_bytes(&bytes).unwrap_err(),
            SchemaError::ChecksumMismatch
        );
    }
}
