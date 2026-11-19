use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DockerNetworkDriver {
    Bridge,
    Overlay,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerNetworkSpec {
    pub network_name: String,
    pub driver: DockerNetworkDriver,
    pub subnet: String,
    pub is_isolated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerActuatorResult {
    pub container_id: String,
    pub network_attached: String,
    pub status: String,
}

pub struct DockerActuator;

impl DockerActuator {
    /// Checks if Docker CLI is available and the daemon is responding
    pub fn is_docker_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create_standard_network(name: &str, driver: DockerNetworkDriver) -> DockerNetworkSpec {
        let driver_str = match &driver {
            DockerNetworkDriver::Bridge => "bridge",
            DockerNetworkDriver::Overlay => "overlay",
            DockerNetworkDriver::Custom(c) => c.as_str(),
        };

        tracing::info!(
            "Creating standard Docker container network '{}' ({})",
            name,
            driver_str
        );

        if Self::is_docker_available() {
            let _ = Command::new("docker")
                .args(["network", "create", "-d", driver_str, name])
                .output();
        }

        DockerNetworkSpec {
            network_name: name.to_string(),
            driver,
            subnet: "172.28.0.0/16".to_string(),
            is_isolated: true,
        }
    }

    pub fn deploy_container(image: &str, name: &str, port: &str) -> Result<DockerActuatorResult, String> {
        Self::deploy_container_on_network(image, name, port, "ltn-bridge")
    }

    pub fn deploy_container_on_network(
        image: &str,
        name: &str,
        port: &str,
        network_name: &str,
    ) -> Result<DockerActuatorResult, String> {
        if !Self::is_docker_available() {
            return Err(
                "DOCKER_DAEMON_OFFLINE: Docker daemon is offline. Start Docker Desktop or daemon."
                    .to_string(),
            );
        }

        // Clean up any stale container with the same name before launching
        let _ = Command::new("docker")
            .args(["rm", "-f", name])
            .output();

        let mut cmd = Command::new("docker");
        cmd.args(["run", "-d", "--name", name]);

        if !port.is_empty() {
            cmd.args(["-p", port]);
        }

        if !network_name.is_empty() {
            cmd.args(["--network", network_name]);
        }

        cmd.arg(image);

        let output = cmd.output().map_err(|e| {
            format!(
                "DOCKER_DAEMON_OFFLINE: Docker daemon is offline or execution failed: {}",
                e
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "DOCKER_DAEMON_OFFLINE: Docker daemon is offline or container run failed: {}",
                stderr.trim()
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let display_id = if container_id.is_empty() {
            format!("doc-{}", uuid::Uuid::new_v4().simple())
        } else {
            container_id
        };

        Ok(DockerActuatorResult {
            container_id: display_id,
            network_attached: network_name.to_string(),
            status: format!(
                "Container '{}' ({}) attached to network '{}' listening on {}",
                name, image, network_name, port
            ),
        })
    }
}
