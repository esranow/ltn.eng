use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthStatus {
    Authenticated,
    BlockedUnauthenticated(String),
}

#[derive(Error, Debug)]
pub enum ProviderAuthError {
    #[error("BLOCKED_UNAUTHENTICATED: {0}")]
    BlockedUnauthenticated(String),
}

pub struct AuthChecker;

impl AuthChecker {
    pub fn check_provider_auth(provider_name: &str) -> AuthStatus {
        match provider_name.to_uppercase().as_str() {
            "LOCAL_DOCKER" | "LOCAL_OS" | "BARE_METAL" => AuthStatus::Authenticated,
            "AWS" => {
                if std::env::var("AWS_ACCESS_KEY_ID").is_ok()
                    || std::env::var("AWS_PROFILE").is_ok()
                    || std::env::var("AWS_ENDPOINT_URL").is_ok()
                {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("AWS credentials missing in environment".to_string())
                }
            }
            "GCP" => {
                if std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok()
                    || std::env::var("GEMINI_API_KEY").is_ok()
                    || std::env::var("GOOGLE_API_KEY").is_ok()
                    || std::env::var("GCP_ENDPOINT_URL").is_ok()
                {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("GCP credentials missing in environment".to_string())
                }
            }
            "AZURE" => {
                if std::env::var("AZURE_SUBSCRIPTION_ID").is_ok()
                    || std::env::var("AZURE_CLIENT_ID").is_ok()
                    || std::env::var("AZURE_ENDPOINT_URL").is_ok()
                {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("Azure credentials missing in environment".to_string())
                }
            }
            _ => AuthStatus::BlockedUnauthenticated(format!("Unknown provider '{}'", provider_name)),
        }
    }
}
