use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TlsMode {
    AutoAcme,
    SelfSigned,
    Custom { cert_path: String, key_path: String },
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressRoute {
    pub domain: String,
    pub path_prefix: String,
    pub target_app: String,
    pub target_port: u16,
    pub tls_mode: TlsMode,
    pub rate_limit_per_min: Option<u32>,
    pub cors_allowed_origins: Vec<String>,
}

impl IngressRoute {
    pub fn new(domain: &str, target_app: &str, target_port: u16) -> Self {
        Self {
            domain: domain.to_lowercase(),
            path_prefix: "/".to_string(),
            target_app: target_app.to_string(),
            target_port,
            tls_mode: TlsMode::AutoAcme,
            rate_limit_per_min: None,
            cors_allowed_origins: Vec::new(),
        }
    }

    pub fn matches(&self, host: &str, path: &str) -> bool {
        let clean_host = host.split(':').next().unwrap_or("").to_lowercase();
        if self.domain != clean_host && !self.domain.starts_with("*.") {
            return false;
        }
        if self.domain.starts_with("*.") {
            let suffix = &self.domain[1..]; // ".example.com"
            if !clean_host.ends_with(suffix) {
                return false;
            }
        }
        path.starts_with(&self.path_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_matching() {
        let route = IngressRoute::new("api.example.com", "my-api", 8080);
        assert!(route.matches("api.example.com", "/v1/users"));
        assert!(route.matches("api.example.com:443", "/"));
        assert!(!route.matches("web.example.com", "/"));

        let wildcard = IngressRoute::new("*.internal.net", "int-app", 9000);
        assert!(wildcard.matches("db.internal.net", "/query"));
        assert!(wildcard.matches("cache.internal.net", "/health"));
        assert!(!wildcard.matches("external.net", "/"));
    }
}
