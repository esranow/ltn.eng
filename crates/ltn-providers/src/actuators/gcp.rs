use super::super::auth::{AuthChecker, AuthStatus};
use super::http_client::MinimalHttpClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcpActuatorResult {
    pub cluster_id: String,
    pub region: String,
    pub status: String,
    pub endpoint: String,
    pub raw_response_body: String,
}

pub struct GcpActuator;

impl GcpActuator {
    pub fn get_endpoint() -> String {
        std::env::var("GCP_ENDPOINT_URL").unwrap_or_else(|_| {
            std::env::var("AWS_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4566".to_string())
        })
    }

    pub fn deploy_gke_workload(workload_name: &str) -> Result<GcpActuatorResult, String> {
        match AuthChecker::check_provider_auth("GCP") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!(
                    "{}/v1/projects/ltn-prod/zones/us-central1-a/clusters/{}",
                    endpoint.trim_end_matches('/'),
                    workload_name
                );
                let payload = serde_json::json!({
                    "name": workload_name,
                    "initialNodeCount": 3
                })
                .to_string();

                let resp = MinimalHttpClient::request(
                    "POST",
                    &url,
                    &[
                        ("X-Amz-Target", "GoogleKubernetesEngineV1.CreateCluster"),
                        ("Content-Type", "application/json"),
                    ],
                    Some(&payload),
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    Ok(GcpActuatorResult {
                        cluster_id: format!("gke-{}", workload_name),
                        region: "us-central1".to_string(),
                        status: format!(
                            "HTTP {} OK: GKE Workload '{}' deployed via {}",
                            resp.status_code, workload_name, endpoint
                        ),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!(
                        "GCP GKE deployment rejected with HTTP {}: {}",
                        resp.status_code, resp.body
                    ))
                }
            }
            AuthStatus::BlockedUnauthenticated(reason) => Err(reason),
        }
    }

    pub fn create_storage_bucket(bucket_name: &str) -> Result<GcpActuatorResult, String> {
        match AuthChecker::check_provider_auth("GCP") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!("{}/storage/v1/b/{}", endpoint.trim_end_matches('/'), bucket_name);
                let payload = serde_json::json!({
                    "name": bucket_name,
                    "location": "us-central1"
                })
                .to_string();

                let resp = MinimalHttpClient::request(
                    "POST",
                    &url,
                    &[
                        ("X-Amz-Target", "GoogleCloudStorageV1.CreateBucket"),
                        ("Content-Type", "application/json"),
                    ],
                    Some(&payload),
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    Ok(GcpActuatorResult {
                        cluster_id: format!("gs://{}", bucket_name),
                        region: "us-central1".to_string(),
                        status: format!(
                            "HTTP {} OK: GCS Bucket '{}' verified via {}",
                            resp.status_code, bucket_name, endpoint
                        ),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!(
                        "GCP GCS Bucket creation rejected with HTTP {}: {}",
                        resp.status_code, resp.body
                    ))
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
    fn test_unreachable_endpoint_returns_error_never_synthetic_success_gcp() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("GCP_ENDPOINT_URL", "http://127.0.0.1:49997");
        let result = GcpActuator::deploy_gke_workload("test-workload");
        assert!(
            result.is_err(),
            "Must return Err when GCP endpoint is unreachable"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to connect")
                || err.contains("refused")
                || err.contains("Connection"),
            "Error must report network failure, got: {}",
            err
        );
    }

    #[test]
    fn test_real_http_call_to_ministack_gcp() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("GCP_ENDPOINT_URL", "http://127.0.0.1:4566");
        let res = GcpActuator::deploy_gke_workload("eval-gke-001");
        assert!(
            res.is_ok(),
            "MiniStack GCP GKE deployment should succeed via real HTTP: {:?}",
            res
        );
        let val = res.unwrap();
        assert!(
            val.status.contains("HTTP 200 OK"),
            "Status must confirm HTTP 200 OK"
        );
        assert!(
            val.raw_response_body
                .contains("GoogleKubernetesEngineV1.CreateCluster"),
            "Raw body must confirm GCP action was handled"
        );
        eprintln!("\n=== GCP MINISTACK RAW RESPONSE ===");
        eprintln!("{}", val.raw_response_body);
    }
}
