use super::super::auth::{AuthChecker, AuthStatus};
use super::http_client::MinimalHttpClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsActuatorResult {
    pub instance_id: String,
    pub region: String,
    pub status: String,
    pub endpoint: String,
    pub raw_response_body: String,
}

pub struct AwsActuator;

impl AwsActuator {
    pub fn get_endpoint() -> String {
        std::env::var("AWS_ENDPOINT_URL").unwrap_or_else(|_| "http://127.0.0.1:4566".to_string())
    }

    pub fn deploy_ecs_task(task_name: &str) -> Result<AwsActuatorResult, String> {
        match AuthChecker::check_provider_auth("AWS") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!("{}/ecs", endpoint.trim_end_matches('/'));
                let payload = serde_json::json!({
                    "taskDefinition": task_name,
                    "count": 1,
                    "launchType": "FARGATE"
                })
                .to_string();

                let resp = MinimalHttpClient::request(
                    "POST",
                    &url,
                    &[
                        ("X-Amz-Target", "AmazonEC2ContainerServiceV20141113.RunTask"),
                        ("Content-Type", "application/x-amz-json-1.1"),
                    ],
                    Some(&payload),
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&resp.body).unwrap_or_else(|_| serde_json::json!({}));
                    let req_id = parsed
                        .pointer("/details/request_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("req-real-ack");

                    Ok(AwsActuatorResult {
                        instance_id: format!("i-{}", req_id.replace('-', "")),
                        region: std::env::var("AWS_DEFAULT_REGION")
                            .unwrap_or_else(|_| "us-east-1".to_string()),
                        status: format!(
                            "HTTP {} OK: ECS Task '{}' deployed via {}",
                            resp.status_code, task_name, endpoint
                        ),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!(
                        "AWS ECS task deployment rejected with HTTP {}: {}",
                        resp.status_code, resp.body
                    ))
                }
            }
            AuthStatus::BlockedUnauthenticated(reason) => Err(reason),
        }
    }

    pub fn create_s3_bucket(bucket_name: &str) -> Result<AwsActuatorResult, String> {
        match AuthChecker::check_provider_auth("AWS") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!("{}/s3/{}", endpoint.trim_end_matches('/'), bucket_name);

                let resp = MinimalHttpClient::request(
                    "PUT",
                    &url,
                    &[("Content-Type", "application/xml")],
                    None,
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    Ok(AwsActuatorResult {
                        instance_id: format!("arn:aws:s3:::{}", bucket_name),
                        region: std::env::var("AWS_DEFAULT_REGION")
                            .unwrap_or_else(|_| "us-east-1".to_string()),
                        status: format!(
                            "HTTP {} OK: S3 Bucket '{}' verified via {}",
                            resp.status_code, bucket_name, endpoint
                        ),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!(
                        "AWS S3 Bucket creation rejected with HTTP {}: {}",
                        resp.status_code, resp.body
                    ))
                }
            }
            AuthStatus::BlockedUnauthenticated(reason) => Err(reason),
        }
    }

    pub fn deploy_rds_instance(db_name: &str) -> Result<AwsActuatorResult, String> {
        match AuthChecker::check_provider_auth("AWS") {
            AuthStatus::Authenticated => {
                let endpoint = Self::get_endpoint();
                let url = format!("{}/rds", endpoint.trim_end_matches('/'));
                let payload = serde_json::json!({
                    "DBInstanceIdentifier": db_name,
                    "Engine": "postgres",
                    "DBInstanceClass": "db.t3.medium"
                })
                .to_string();

                let resp = MinimalHttpClient::request(
                    "POST",
                    &url,
                    &[
                        ("X-Amz-Target", "AmazonRDSv20141031.CreateDBInstance"),
                        ("Content-Type", "application/x-amz-json-1.1"),
                    ],
                    Some(&payload),
                )?;

                if resp.status_code >= 200 && resp.status_code < 300 {
                    Ok(AwsActuatorResult {
                        instance_id: format!("rds-{}", db_name),
                        region: std::env::var("AWS_DEFAULT_REGION")
                            .unwrap_or_else(|_| "us-east-1".to_string()),
                        status: format!(
                            "HTTP {} OK: RDS PostgreSQL instance '{}' provisioned via {}",
                            resp.status_code, db_name, endpoint
                        ),
                        endpoint,
                        raw_response_body: resp.body,
                    })
                } else {
                    Err(format!(
                        "AWS RDS Instance creation rejected with HTTP {}: {}",
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

    static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_unreachable_endpoint_returns_error_never_synthetic_success() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AWS_ACCESS_KEY_ID", "mock_key");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "mock_secret");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:49999");
        let result = AwsActuator::deploy_ecs_task("test-task");
        assert!(
            result.is_err(),
            "Must return Err when endpoint is unreachable"
        );
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("Failed to connect")
                || err_msg.contains("Connection")
                || err_msg.contains("refused"),
            "Error must describe network failure, got: {}",
            err_msg
        );
    }

    #[test]
    #[ignore = "requires the MiniStack mock on 127.0.0.1:4566"]
    fn test_real_http_call_to_ministack() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AWS_ACCESS_KEY_ID", "mock_key");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "mock_secret");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");
        let ecs_res = AwsActuator::deploy_ecs_task("eval-task-001");
        assert!(
            ecs_res.is_ok(),
            "MiniStack ECS deployment should succeed via real HTTP: {:?}",
            ecs_res
        );
        let res = ecs_res.unwrap();
        assert!(
            res.status.contains("HTTP 200 OK"),
            "Status must confirm real HTTP 200 OK, got: {}",
            res.status
        );

        let s3_res = AwsActuator::create_s3_bucket("test-bucket-eval");
        assert!(
            s3_res.is_ok(),
            "MiniStack S3 bucket creation should succeed via real HTTP: {:?}",
            s3_res
        );

        let rds_res = AwsActuator::deploy_rds_instance("eval-db-01");
        assert!(
            rds_res.is_ok(),
            "MiniStack RDS deployment should succeed via real HTTP: {:?}",
            rds_res
        );
    }

    #[test]
    #[ignore = "requires the MiniStack mock on 127.0.0.1:4566"]
    fn test_ministack_request_id_changes_across_seconds() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AWS_ACCESS_KEY_ID", "mock_key");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "mock_secret");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");

        // Call 1
        let res1 = AwsActuator::deploy_ecs_task("call-1").expect("Call 1 failed");
        let body1 = res1.raw_response_body.clone();

        // 1.2s delay ensures distinct second timestamp from MiniStack
        std::thread::sleep(std::time::Duration::from_millis(1200));

        // Call 2
        let res2 = AwsActuator::deploy_ecs_task("call-2").expect("Call 2 failed");
        let body2 = res2.raw_response_body.clone();

        // 1.2s delay
        std::thread::sleep(std::time::Duration::from_millis(1200));

        // Call 3
        let res3 = AwsActuator::deploy_ecs_task("call-3").expect("Call 3 failed");
        let body3 = res3.raw_response_body.clone();

        eprintln!("\n=== CALL 1 RAW RESPONSE BODY ===");
        eprintln!("{}", body1);
        eprintln!("=== CALL 2 RAW RESPONSE BODY ===");
        eprintln!("{}", body2);
        eprintln!("=== CALL 3 RAW RESPONSE BODY ===");
        eprintln!("{}", body3);

        eprintln!("CALL 1 instance_id: {}", res1.instance_id);
        eprintln!("CALL 2 instance_id: {}", res2.instance_id);
        eprintln!("CALL 3 instance_id: {}", res3.instance_id);

        assert_ne!(
            res1.instance_id, res2.instance_id,
            "Instance IDs must differ after 1s gap"
        );
        assert_ne!(
            res2.instance_id, res3.instance_id,
            "Instance IDs must differ after 1s gap"
        );
    }
}
