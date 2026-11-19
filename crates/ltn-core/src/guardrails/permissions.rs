use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentCapability {
    ReadProject,
    ReadHardware,
    Architect,
    Validate,
    GenerateIac,
    DeployStaging,
    DeployProduction,
    Rollback,
    ModifyNetwork,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityContext {
    pub granted_capabilities: Vec<AgentCapability>,
    pub environment: String, // "development" | "staging" | "production"
    pub authorization_token: Option<String>,
}

impl Default for CapabilityContext {
    fn default() -> Self {
        Self {
            granted_capabilities: vec![
                AgentCapability::ReadProject,
                AgentCapability::ReadHardware,
                AgentCapability::Architect,
                AgentCapability::Validate,
                AgentCapability::GenerateIac,
                AgentCapability::DeployStaging,
                AgentCapability::Rollback,
            ],
            environment: "development".to_string(),
            authorization_token: None,
        }
    }
}

impl CapabilityContext {
    pub fn production_with_auth(auth_token: impl Into<String>) -> Self {
        Self {
            granted_capabilities: vec![
                AgentCapability::ReadProject,
                AgentCapability::ReadHardware,
                AgentCapability::Architect,
                AgentCapability::Validate,
                AgentCapability::GenerateIac,
                AgentCapability::DeployStaging,
                AgentCapability::DeployProduction,
                AgentCapability::Rollback,
                AgentCapability::ModifyNetwork,
            ],
            environment: "production".to_string(),
            authorization_token: Some(auth_token.into()),
        }
    }

    pub fn has_capability(&self, cap: AgentCapability) -> bool {
        self.granted_capabilities.contains(&cap)
    }

    pub fn require_capability(&self, cap: AgentCapability) -> Result<(), String> {
        if self.has_capability(cap) {
            Ok(())
        } else {
            Err(format!(
                "Permission Denied: Agent lacks required capability '{:?}' in environment '{}'",
                cap, self.environment
            ))
        }
    }
}
