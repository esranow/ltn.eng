use serde::{Deserialize, Serialize};

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
    pub fn create_standard_network(name: &str, driver: DockerNetworkDriver) -> DockerNetworkSpec {
        let driver_str = match &driver {
            DockerNetworkDriver::Bridge => "bridge",
            DockerNetworkDriver::Overlay => "overlay",
            DockerNetworkDriver::Custom(c) => c.as_str(),
        };

        tracing::info!("Creating standard Docker container network '{}' ({})", name, driver_str);

        DockerNetworkSpec {
            network_name: name.to_string(),
            driver,
            subnet: "172.28.0.0/16".to_string(),
            is_isolated: true,
        }
    }

    pub fn deploy_container(image: &str, name: &str, port: &str) -> DockerActuatorResult {
        Self::deploy_container_on_network(image, name, port, "ltn-bridge")
    }

    pub fn deploy_container_on_network(
        image: &str,
        name: &str,
        port: &str,
        network_name: &str,
    ) -> DockerActuatorResult {
        DockerActuatorResult {
            container_id: format!("doc-{}", uuid::Uuid::new_v4().simple()),
            network_attached: network_name.to_string(),
            status: format!(
                "Container '{}' ({}) attached to network '{}' listening on {}",
                name, image, network_name, port
            ),
        }
    }
}
