use crate::crypto::{
    decrypt_aes_gcm, derive_key_from_passphrase, encrypt_aes_gcm, generate_random_nonce,
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSecretRecord {
    pub app: String,
    pub key: String,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretBundle {
    pub version: u32,
    pub records: HashMap<String, EncryptedSecretRecord>, // "app:key" -> record
}

pub struct SecretStore {
    master_key: [u8; 32],
    bundle: SecretBundle,
}

impl SecretStore {
    pub fn new(master_key: [u8; 32]) -> Self {
        Self {
            master_key,
            bundle: SecretBundle {
                version: 1,
                records: HashMap::new(),
            },
        }
    }

    pub fn with_passphrase(passphrase: &str, salt: &[u8]) -> Self {
        let key = derive_key_from_passphrase(passphrase, salt);
        Self::new(key)
    }

    fn record_id(app: &str, key: &str) -> String {
        format!("{}:{}", app.to_lowercase(), key)
    }

    pub fn set(&mut self, app: &str, key: &str, value: &str) -> Result<()> {
        let nonce = generate_random_nonce()?;
        let aad = format!("app:{},key:{}", app.to_lowercase(), key);
        let ciphertext = encrypt_aes_gcm(&self.master_key, &nonce, aad.as_bytes(), value.as_bytes())?;

        let id = Self::record_id(app, key);
        let now = Utc::now();
        let record = EncryptedSecretRecord {
            app: app.to_lowercase(),
            key: key.to_string(),
            nonce_hex: hex_encode(&nonce),
            ciphertext_hex: hex_encode(&ciphertext),
            created_at: self.bundle.records.get(&id).map(|r| r.created_at).unwrap_or(now),
            updated_at: now,
        };

        self.bundle.records.insert(id, record);
        Ok(())
    }

    pub fn get(&self, app: &str, key: &str) -> Result<Option<String>> {
        let id = Self::record_id(app, key);
        let Some(record) = self.bundle.records.get(&id) else {
            return Ok(None);
        };

        let nonce = hex_decode(&record.nonce_hex)?;
        let ciphertext = hex_decode(&record.ciphertext_hex)?;
        if nonce.len() != 12 {
            return Err(anyhow!("Corrupt nonce length in secret record"));
        }
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(&nonce);

        let aad = format!("app:{},key:{}", app.to_lowercase(), key);
        let plaintext = decrypt_aes_gcm(&self.master_key, &nonce_arr, aad.as_bytes(), &ciphertext)
            .context("Decryption failed for secret")?;

        let value = String::from_utf8(plaintext)
            .context("Secret is not valid UTF-8 string")?;
        Ok(Some(value))
    }

    pub fn list_keys(&self, app: &str) -> Vec<String> {
        let target_app = app.to_lowercase();
        self.bundle
            .records
            .values()
            .filter(|r| r.app == target_app)
            .map(|r| r.key.clone())
            .collect()
    }

    pub fn delete(&mut self, app: &str, key: &str) -> bool {
        let id = Self::record_id(app, key);
        self.bundle.records.remove(&id).is_some()
    }

    pub fn get_all_for_app(&self, app: &str) -> Result<HashMap<String, String>> {
        let keys = self.list_keys(app);
        let mut map = HashMap::new();
        for k in keys {
            if let Some(v) = self.get(app, &k)? {
                map.insert(k, v);
            }
        }
        Ok(map)
    }

    pub fn inject_tmpfs(&self, app: &str, target_dir: &Path) -> Result<Vec<PathBuf>> {
        if !target_dir.exists() {
            fs::create_dir_all(target_dir)?;
        }
        let secrets = self.get_all_for_app(app)?;
        let mut written = Vec::new();
        for (k, v) in secrets {
            let file_path = target_dir.join(&k);
            fs::write(&file_path, v.as_bytes())
                .with_context(|| format!("Failed to write tmpfs secret file: {:?}", file_path))?;
            written.push(file_path);
        }
        Ok(written)
    }

    pub fn masked_env(&self, app: &str) -> Result<HashMap<String, String>> {
        let keys = self.list_keys(app);
        let mut masked = HashMap::new();
        for k in keys {
            masked.insert(k, "******** [ENCRYPTED_SECRET]".to_string());
        }
        Ok(masked)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.bundle)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path, master_key: [u8; 32]) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new(master_key));
        }
        let content = fs::read_to_string(path)?;
        let bundle: SecretBundle = serde_json::from_str(&content)?;
        Ok(Self { master_key, bundle })
    }
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err(anyhow!("Invalid hex length"));
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|e| anyhow!("Invalid hex char: {}", e))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_store_lifecycle() {
        let key = derive_key_from_passphrase("store-passphrase", b"salt-999");
        let mut store = SecretStore::new(key);

        store.set("payment-svc", "STRIPE_KEY", "sk_live_123456789").unwrap();
        store.set("payment-svc", "DB_URI", "postgres://user:pass@10.0.0.1/db").unwrap();

        let stripe = store.get("payment-svc", "STRIPE_KEY").unwrap();
        assert_eq!(stripe, Some("sk_live_123456789".to_string()));

        let keys = store.list_keys("payment-svc");
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"STRIPE_KEY".to_string()));
        assert!(keys.contains(&"DB_URI".to_string()));

        let masked = store.masked_env("payment-svc").unwrap();
        assert_eq!(masked.get("STRIPE_KEY").unwrap(), "******** [ENCRYPTED_SECRET]");

        // test tmpfs injection
        let tmp = std::env::temp_dir().join(format!("ltn_test_tmpfs_{}", uuid::Uuid::new_v4()));
        let files = store.inject_tmpfs("payment-svc", &tmp).unwrap();
        assert_eq!(files.len(), 2);
        let read_val = fs::read_to_string(tmp.join("STRIPE_KEY")).unwrap();
        assert_eq!(read_val, "sk_live_123456789");
        let _ = fs::remove_dir_all(&tmp);

        // delete
        assert!(store.delete("payment-svc", "STRIPE_KEY"));
        assert_eq!(store.get("payment-svc", "STRIPE_KEY").unwrap(), None);
    }
}
