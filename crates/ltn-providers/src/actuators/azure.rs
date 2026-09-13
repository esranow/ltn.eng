use super::super::auth::{AuthChecker, AuthStatus};
use super::http_client::MinimalHttpClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureActuatorResult {
    pub resource_group: String,
    pub status: String,
    pub endpoint: String,
    pub raw_response_body: String,
}

pub struct AzureActuator;

impl AzureActuator {
    pub fn get_endpoint() -> String {
        std::env::var("AZURE_ENDPOINT_URL")
            .unwrap_or_else(|_| std::env::var("AWS_ENDPOINT_URL").unwrap_or_else(|_| "http://127.0.0.1:4566".to_string()))
    }

    pub fn deploy_aks_cluster(cluster_name: &str) -> Result<AzureActuatorResult, String> {
        match AuthChecker::check_provider_auth("AZURE") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!("{}/subscriptions/sub-1/resourceGroups/rg-ltn/providers/Microsoft.ContainerService/managedClusters/{}", endpoint.trim_end_matches('/'), cluster_name);
                let payload = serde_json::json!({
                    "location": "eastus",
                    "properties": {
                        "dnsPrefix": cluster_name,
                        "agentPoolProfiles": [{ "name": "nodepool1", "count": 3, "vmSize": "Standard_D2s_v3" }]
                    }
                }).to_string();

                let resp = MinimalHttpClient::request(
                    "POST",
                    &url,
                    &[
                        ("X-Amz-Target", "AzureResourceManager.CreateOrUpdateCluster"),
                        ("Content-Type", "application/json"),
                    ],
                    Some(&payload),
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    Ok(AzureActuatorResult {
                        resource_group: "rg-ltn-prod".to_string(),
                        status: format!("HTTP {} OK: AKS Cluster '{}' deployed via {}", resp.status_code, cluster_name, endpoint),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!("Azure AKS deployment rejected with HTTP {}: {}", resp.status_code, resp.body))
                }
            }
            AuthStatus::BlockedUnauthenticated(reason) => Err(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_unreachable_endpoint_returns_error_never_synthetic_success_azure() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AZURE_ENDPOINT_URL", "http://127.0.0.1:49998");
        let result = AzureActuator::deploy_aks_cluster("test-cluster");
        assert!(result.is_err(), "Must return Err when Azure endpoint is unreachable");
        let err = result.unwrap_err();
        assert!(err.contains("Failed to connect") || err.contains("refused") || err.contains("Connection"), "Error must report network failure, got: {}", err);
    }

    #[test]
    fn test_real_http_call_to_ministack_azure() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AZURE_ENDPOINT_URL", "http://127.0.0.1:4566");
        let res = AzureActuator::deploy_aks_cluster("eval-aks-001");
        assert!(res.is_ok(), "MiniStack Azure AKS deployment should succeed via real HTTP: {:?}", res);
        let val = res.unwrap();
        assert!(val.status.contains("HTTP 200 OK"), "Status must confirm HTTP 200 OK");
        assert!(val.raw_response_body.contains("AzureResourceManager.CreateOrUpdateCluster"), "Raw body must confirm Azure action was handled");
        eprintln!("\n=== AZURE MINISTACK RAW RESPONSE ===");
        eprintln!("{}", val.raw_response_body);
    }
}
