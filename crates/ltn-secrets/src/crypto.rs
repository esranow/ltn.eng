use anyhow::{anyhow, Result};
use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, AES_256_GCM,
    NONCE_LEN,
};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};

struct OneNonce(Option<Nonce>);

impl NonceSequence for OneNonce {
    fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
        self.0.take().ok_or(ring::error::Unspecified)
    }
}

pub fn generate_random_nonce() -> Result<[u8; NONCE_LEN]> {
    let rng = SystemRandom::new();
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut nonce)
        .map_err(|_| anyhow!("Failed to generate secure random nonce"))?;
    Ok(nonce)
}

pub fn derive_key_from_passphrase(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ltn-secrets-v1:");
    hasher.update(passphrase.as_bytes());
    hasher.update(salt);
    let first = hasher.finalize();

    let mut second_hasher = Sha256::new();
    second_hasher.update(b"ltn-secrets-round2:");
    second_hasher.update(&first);
    let second = second_hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&second);
    key
}

pub fn encrypt_aes_gcm(
    key: &[u8; 32],
    nonce_bytes: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| anyhow!("Invalid key for AES-256-GCM"))?;
    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes)
        .map_err(|_| anyhow!("Invalid nonce"))?;
    let mut sealing_key = SealingKey::new(unbound_key, OneNonce(Some(nonce)));

    let mut in_out = plaintext.to_vec();
    sealing_key
        .seal_in_place_append_tag(Aad::from(aad), &mut in_out)
        .map_err(|_| anyhow!("AES-256-GCM encryption failed"))?;

    Ok(in_out)
}

pub fn decrypt_aes_gcm(
    key: &[u8; 32],
    nonce_bytes: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| anyhow!("Invalid key for AES-256-GCM"))?;
    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes)
        .map_err(|_| anyhow!("Invalid nonce"))?;
    let mut opening_key = OpeningKey::new(unbound_key, OneNonce(Some(nonce)));

    let mut in_out = ciphertext.to_vec();
    let decrypted = opening_key
        .open_in_place(Aad::from(aad), &mut in_out)
        .map_err(|_| anyhow!("AES-256-GCM decryption failed: invalid key or tampered ciphertext"))?;

    Ok(decrypted.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_gcm_roundtrip() {
        let key = derive_key_from_passphrase("test-passphrase", b"salt-1234");
        let nonce = generate_random_nonce().unwrap();
        let aad = b"app:web,key:DB_PASSWORD";
        let plaintext = b"super-secret-postgres-password-9821";

        let ciphertext = encrypt_aes_gcm(&key, &nonce, aad, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext);

        let decrypted = decrypt_aes_gcm(&key, &nonce, aad, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = derive_key_from_passphrase("test-passphrase", b"salt-1234");
        let nonce = generate_random_nonce().unwrap();
        let aad = b"app:web";
        let plaintext = b"my-secret";

        let mut ciphertext = encrypt_aes_gcm(&key, &nonce, aad, plaintext).unwrap();
        ciphertext[0] ^= 0xFF; // tamper

        let res = decrypt_aes_gcm(&key, &nonce, aad, &ciphertext);
        assert!(res.is_err());
    }

    #[test]
    fn test_tampered_aad_fails() {
        let key = derive_key_from_passphrase("test-passphrase", b"salt-1234");
        let nonce = generate_random_nonce().unwrap();
        let aad1 = b"app:web";
        let aad2 = b"app:db";
        let plaintext = b"my-secret";

        let ciphertext = encrypt_aes_gcm(&key, &nonce, aad1, plaintext).unwrap();
        let res = decrypt_aes_gcm(&key, &nonce, aad2, &ciphertext);
        assert!(res.is_err());
    }
}
