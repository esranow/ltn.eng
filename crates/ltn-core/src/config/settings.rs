use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub default_provider: String,
    pub default_tier: String,
    pub aws_region: Option<String>,
    pub gcp_project: Option<String>,
    pub azure_resource_group: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            default_provider: "LOCAL_DOCKER".to_string(),
            default_tier: "BALANCED".to_string(),
            aws_region: Some("us-east-1".to_string()),
            gcp_project: None,
            azure_resource_group: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub probe_interval_sec: u64,
    pub auto_heal_enabled: bool,
    pub max_fix_attempts: u8,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            probe_interval_sec: 10,
            auto_heal_enabled: true,
            max_fix_attempts: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub active_model: String,
    pub llm_provider: String,
    pub local_endpoint: String,
    pub rig_compatibility_mode: bool,
    #[serde(default)]
    pub gemini_api_key: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            active_model: "gemini-2.5-flash".to_string(),
            llm_provider: "Gemini".to_string(),
            local_endpoint: "http://localhost:11434".to_string(),
            rig_compatibility_mode: true,
            gemini_api_key: std::env::var("GEMINI_API_KEY").ok(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LtnSettings {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub llm: LlmConfig,
}

impl Default for LtnSettings {
    fn default() -> Self {
        Self {
            provider: ProviderConfig::default(),
            telemetry: TelemetryConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

impl LtnSettings {
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            let default_settings = Self::default();
            let _ = default_settings.save_to_file(path);
            return Ok(default_settings);
        }

        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        
        let is_toml = path.extension().map_or(false, |ext| ext == "toml");
        if is_toml {
            if let Ok(settings) = toml::from_str::<Self>(&content) {
                return Ok(settings);
            }
        }

        if let Ok(settings) = serde_json::from_str::<Self>(&content) {
            return Ok(settings);
        }

        // Fallback to toml if json failed or vice-versa
        toml::from_str::<Self>(&content).map_err(|e| e.to_string())
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let is_toml = path.extension().map_or(false, |ext| ext == "toml");
        let content = if is_toml {
            toml::to_string_pretty(self).map_err(|e| e.to_string())?
        } else {
            serde_json::to_string_pretty(self).map_err(|e| e.to_string())?
        };
        fs::write(path, content).map_err(|e| e.to_string())
    }
}

