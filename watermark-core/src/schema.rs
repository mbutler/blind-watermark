use std::convert::TryInto;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum SchemaError {
    #[error("Invalid payload length. Expected 38 bytes, got {0}")]
    InvalidLength(usize),
    #[error("Unsupported schema version: {0}")]
    UnsupportedVersion(u8),
}

/// The exact 38-byte binary layout for our watermark payload.
#[derive(Debug, PartialEq)]
pub struct ProvenancePayload {
    pub version: u8,                // 1 byte
    pub compressed_pubkey: [u8; 33], // 33 bytes (P-256 compressed)
    pub asset_id: u32,              // 4 bytes
}

impl ProvenancePayload {
    /// Serializes the struct into exactly 38 bytes.
    pub fn to_bytes(&self) -> [u8; 38] {
        let mut bytes = [0u8; 38];

        bytes[0] = self.version;
        bytes[1..34].copy_from_slice(&self.compressed_pubkey);
        bytes[34..38].copy_from_slice(&self.asset_id.to_be_bytes());

        bytes
    }

    /// Deserializes a 38-byte slice back into the struct.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        if bytes.len() != 38 {
            return Err(SchemaError::InvalidLength(bytes.len()));
        }

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
            compressed_pubkey: [0x42; 33], // Dummy key
            asset_id: 999_999,
        };

        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), 38);

        let reconstructed = ProvenancePayload::from_bytes(&bytes).unwrap();
        assert_eq!(original, reconstructed);
    }
}
