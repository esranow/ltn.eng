use anyhow::Result;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct AcmeChallenge {
    pub token: String,
    pub key_authorization: String,
}

impl AcmeChallenge {
    pub fn new(token: &str, account_thumbprint: &str) -> Self {
        let key_auth = format!("{}.{}", token, account_thumbprint);
        Self {
            token: token.to_string(),
            key_authorization: key_auth,
        }
    }

    pub fn http_01_path(&self) -> String {
        format!("/.well-known/acme-challenge/{}", self.token)
    }

    pub fn dns_01_txt_record(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.key_authorization.as_bytes());
        let digest = hasher.finalize();
        // base64 url-safe
        base64_url_encode(&digest)
    }
}

fn base64_url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    let mut val = 0u32;
    let mut bits = 0;

    for &b in data {
        val = (val << 8) | (b as u32);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            let idx = ((val >> bits) & 0x3F) as usize;
            result.push(CHARS[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((val << (6 - bits)) & 0x3F) as usize;
        result.push(CHARS[idx] as char);
    }
    result
}

pub struct SelfSignedCertBundle {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn generate_dev_certificate(domain: &str) -> Result<SelfSignedCertBundle> {
    // Generate self-signed PEM placeholder
    let cert_pem = format!(
        "-----BEGIN CERTIFICATE-----\n# Dev certificate for {}\nMIICpDCCAYwCAQAwDQYJKoZIhvcNAQELBQAwADAeFw0yNjExMjIwMDAwMDBaFw0y\nNzExMjIwMDAwMDBaMAAwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQC6\n-----END CERTIFICATE-----\n",
        domain
    );
    let key_pem = format!(
        "-----BEGIN PRIVATE KEY-----\n# Dev private key for {}\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC6rR8b...==\n-----END PRIVATE KEY-----\n",
        domain
    );
    Ok(SelfSignedCertBundle { cert_pem, key_pem })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acme_challenges() {
        let challenge = AcmeChallenge::new("token123", "thumbprint456");
        assert_eq!(challenge.http_01_path(), "/.well-known/acme-challenge/token123");
        assert_eq!(challenge.key_authorization, "token123.thumbprint456");

        let txt = challenge.dns_01_txt_record();
        assert!(!txt.is_empty());
    }

    #[test]
    fn test_dev_certificate() {
        let bundle = generate_dev_certificate("api.local").unwrap();
        assert!(bundle.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(bundle.key_pem.contains("BEGIN PRIVATE KEY"));
    }
}
