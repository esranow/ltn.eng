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
        let user_home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        match provider_name.to_uppercase().as_str() {
            "LOCAL_OS" | "BARE_METAL" => AuthStatus::Authenticated,
            "LOCAL_DOCKER" => {
                if std::process::Command::new("docker")
                    .arg("--version")
                    .output()
                    .is_ok()
                {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("Docker CLI not found on host".to_string())
                }
            }
            "AWS" => {
                let has_env = std::env::var("AWS_ACCESS_KEY_ID").is_ok()
                    || std::env::var("AWS_PROFILE").is_ok()
                    || std::env::var("AWS_ENDPOINT_URL").is_ok();
                let has_config = !user_home.is_empty()
                    && std::path::Path::new(&user_home)
                        .join(".aws")
                        .join("credentials")
                        .exists();
                let has_cli = std::process::Command::new("aws")
                    .args(["sts", "get-caller-identity"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if has_env || has_config || has_cli {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("AWS credentials missing in environment (set AWS_ACCESS_KEY_ID, AWS_PROFILE, or ~/.aws/credentials)".to_string())
                }
            }
            "GCP" => {
                let has_env = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok()
                    || std::env::var("GEMINI_API_KEY").is_ok()
                    || std::env::var("GOOGLE_API_KEY").is_ok()
                    || std::env::var("GCP_ENDPOINT_URL").is_ok();
                let has_config = !user_home.is_empty()
                    && (std::path::Path::new(&user_home)
                        .join(".config")
                        .join("gcloud")
                        .join("application_default_credentials.json")
                        .exists()
                        || std::path::Path::new(&user_home)
                            .join("AppData")
                            .join("Roaming")
                            .join("gcloud")
                            .join("application_default_credentials.json")
                            .exists());
                let has_cli = std::process::Command::new("gcloud")
                    .args(["auth", "print-access-token"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if has_env || has_config || has_cli {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("GCP credentials missing in environment (set GOOGLE_APPLICATION_CREDENTIALS, GEMINI_API_KEY, or gcloud auth)".to_string())
                }
            }
            "AZURE" => {
                let has_env = std::env::var("AZURE_SUBSCRIPTION_ID").is_ok()
                    || std::env::var("AZURE_CLIENT_ID").is_ok()
                    || std::env::var("AZURE_ENDPOINT_URL").is_ok();
                let has_config = !user_home.is_empty()
                    && std::path::Path::new(&user_home)
                        .join(".azure")
                        .join("azureProfile.json")
                        .exists();
                let has_cli = std::process::Command::new("az")
                    .args(["account", "show"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if has_env || has_config || has_cli {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::BlockedUnauthenticated("Azure credentials missing in environment (set AZURE_SUBSCRIPTION_ID, AZURE_CLIENT_ID, or az login)".to_string())
                }
            }
            _ => {
                AuthStatus::BlockedUnauthenticated(format!("Unknown provider '{}'", provider_name))
            }
        }
    }
}
