use sha2::{Digest, Sha256};
use std::fmt::Write;

pub struct StateHasher;

impl StateHasher {
    pub fn hash_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut hex_str = String::with_capacity(result.len() * 2);
        for byte in result {
            write!(&mut hex_str, "{:02x}", byte).unwrap();
        }
        hex_str
    }

    pub fn hash_str(s: &str) -> String {
        Self::hash_bytes(s.as_bytes())
    }

    pub fn hash_json<T: serde::Serialize>(val: &T) -> String {
        let json_bytes = serde_json::to_vec(val).unwrap_or_default();
        Self::hash_bytes(&json_bytes)
    }

    pub fn combine_hashes(hashes: &[String]) -> String {
        let mut hasher = Sha256::new();
        for h in hashes {
            hasher.update(h.as_bytes());
        }
        let result = hasher.finalize();
        let mut hex_str = String::with_capacity(result.len() * 2);
        for byte in result {
            write!(&mut hex_str, "{:02x}", byte).unwrap();
        }
        hex_str
    }
}
