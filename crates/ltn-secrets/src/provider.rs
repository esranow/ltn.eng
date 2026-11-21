use anyhow::Result;
use std::collections::HashMap;

pub trait SecretsProvider: Send + Sync {
    fn fetch_secret(&self, app: &str, key: &str) -> Result<Option<String>>;
    fn put_secret(&self, app: &str, key: &str, value: &str) -> Result<()>;
    fn fetch_all(&self, app: &str) -> Result<HashMap<String, String>>;
}

pub struct EnvFallbackProvider;

impl SecretsProvider for EnvFallbackProvider {
    fn fetch_secret(&self, app: &str, key: &str) -> Result<Option<String>> {
        let env_key = format!("{}_{}", app.to_uppercase(), key.to_uppercase());
        Ok(std::env::var(env_key).or_else(|_| std::env::var(key)).ok())
    }

    fn put_secret(&self, _app: &str, _key: &str, _value: &str) -> Result<()> {
        Ok(())
    }

    fn fetch_all(&self, _app: &str) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
}
