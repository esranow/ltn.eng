use ltn_container::runtime::{
    ContainerStatus, DeterministicContainer, DeterministicContainerEngine,
};
use ltn_core::crypto::StateHasher;
use ltn_core::ir::WorkloadDefinition;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterministicContainerActuatorResult {
    pub container_id: Uuid,
    pub name: String,
    pub image: String,
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub status: ContainerStatus,
    pub state_hash: String,
    pub verified: bool,
}

impl From<DeterministicContainer> for DeterministicContainerActuatorResult {
    fn from(c: DeterministicContainer) -> Self {
        Self {
            container_id: c.id,
            name: c.name,
            image: c.image,
            cpu_cores: c.cpu_cores,
            ram_mb: c.ram_mb,
            status: c.status,
            state_hash: c.state_hash,
            verified: true,
        }
    }
}

impl From<&DeterministicContainer> for DeterministicContainerActuatorResult {
    fn from(c: &DeterministicContainer) -> Self {
        Self {
            container_id: c.id,
            name: c.name.clone(),
            image: c.image.clone(),
            cpu_cores: c.cpu_cores,
            ram_mb: c.ram_mb,
            status: c.status.clone(),
            state_hash: c.state_hash.clone(),
            verified: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerStateVerification {
    pub container_id: Uuid,
    pub name: String,
    pub status: ContainerStatus,
    pub state_hash: String,
    pub is_valid: bool,
}

/// Actuator interfacing with `DeterministicContainerEngine` and `WorkloadDefinition`
/// to launch, stop, and verify container states.
#[derive(Default)]
pub struct DeterministicContainerActuator {
    pub engine: DeterministicContainerEngine,
}

impl DeterministicContainerActuator {
    /// Creates a new DeterministicContainerActuator with an empty container engine.
    pub fn new() -> Self {
        Self {
            engine: DeterministicContainerEngine::new(),
        }
    }

    /// Creates an actuator wrapping an existing DeterministicContainerEngine instance.
    pub fn with_engine(engine: DeterministicContainerEngine) -> Self {
        Self { engine }
    }

    /// Returns an immutable reference to the underlying engine.
    pub fn engine(&self) -> &DeterministicContainerEngine {
        &self.engine
    }

    /// Returns a mutable reference to the underlying engine.
    pub fn engine_mut(&mut self) -> &mut DeterministicContainerEngine {
        &mut self.engine
    }

    /// Launches a workload under deterministic sandbox execution, recording its state hash.
    pub fn launch_workload(
        &mut self,
        workload: &WorkloadDefinition,
    ) -> Result<DeterministicContainer, String> {
        self.engine.launch_workload(workload)
    }

    /// Alias for launch_workload.
    pub fn launch(
        &mut self,
        workload: &WorkloadDefinition,
    ) -> Result<DeterministicContainer, String> {
        self.launch_workload(workload)
    }

    /// Deploys a workload and returns a structured actuator result.
    pub fn deploy_workload(
        &mut self,
        workload: &WorkloadDefinition,
    ) -> Result<DeterministicContainerActuatorResult, String> {
        self.launch_workload(workload).map(Into::into)
    }

    /// Stops a container by ID, transitioning its status to Stopped.
    pub fn stop_container(&mut self, id: &Uuid) -> Result<(), String> {
        self.engine.stop_container(id)
    }

    /// Alias for stop_container.
    pub fn stop(&mut self, id: &Uuid) -> Result<(), String> {
        self.stop_container(id)
    }

    /// Verifies the current status of a container by ID.
    pub fn verify_container_state(&self, id: &Uuid) -> Result<ContainerStatus, String> {
        self.engine
            .containers
            .get(id)
            .map(|c| c.status.clone())
            .ok_or_else(|| format!("Container with ID '{}' not found", id))
    }

    /// Verifies detailed state and integrity of a container by ID.
    pub fn verify_container_state_detailed(
        &self,
        id: &Uuid,
    ) -> Result<ContainerStateVerification, String> {
        let container = self
            .engine
            .containers
            .get(id)
            .ok_or_else(|| format!("Container with ID '{}' not found", id))?;

        let expected_hash = StateHasher::hash_str(&format!(
            "{}:{}:{}:{}",
            container.name, container.image, container.cpu_cores, container.ram_mb
        ));

        let is_valid = container.state_hash == expected_hash;

        Ok(ContainerStateVerification {
            container_id: container.id,
            name: container.name.clone(),
            status: container.status.clone(),
            state_hash: container.state_hash.clone(),
            is_valid,
        })
    }

    /// Verifies whether the recorded state hash of a container matches an expected hash.
    pub fn verify_state_hash(&self, id: &Uuid, expected_hash: &str) -> Result<bool, String> {
        self.engine
            .containers
            .get(id)
            .map(|c| c.state_hash == expected_hash)
            .ok_or_else(|| format!("Container with ID '{}' not found", id))
    }

    /// Verifies whether the container's state matches the WorkloadDefinition's expected state hash.
    pub fn verify_workload_state(&self, workload: &WorkloadDefinition) -> Result<bool, String> {
        let hash_input = format!(
            "{}:{}:{}:{}",
            workload.name,
            workload.container_image,
            workload.compute.cpu_cores,
            workload.compute.ram_mb
        );
        let expected_hash = StateHasher::hash_str(&hash_input);
        self.verify_state_hash(&workload.id, &expected_hash)
    }

    /// Verifies cumulative state hash across all containers in the engine.
    pub fn verify_cluster_hash(&self) -> String {
        self.engine.verify_cluster_hash()
    }

    /// Returns a reference to a container by ID if it exists.
    pub fn get_container(&self, id: &Uuid) -> Option<&DeterministicContainer> {
        self.engine.containers.get(id)
    }

    /// Lists all containers currently registered in the engine.
    pub fn list_containers(&self) -> Vec<&DeterministicContainer> {
        self.engine.list_containers()
    }

    /// Checks if a container is currently running.
    pub fn is_running(&self, id: &Uuid) -> bool {
        self.engine
            .containers
            .get(id)
            .is_some_and(|c| matches!(c.status, ContainerStatus::Running))
    }

    /// Checks if a container is currently stopped.
    pub fn is_stopped(&self, id: &Uuid) -> bool {
        self.engine
            .containers
            .get(id)
            .is_some_and(|c| matches!(c.status, ContainerStatus::Stopped))
    }

    /// Helper to launch an ephemeral workload under a fresh engine instance.
    pub fn launch_ephemeral(
        workload: &WorkloadDefinition,
    ) -> Result<DeterministicContainer, String> {
        let mut engine = DeterministicContainerEngine::new();
        engine.launch_workload(workload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltn_core::ir::{
        ComputeRequirements, NetworkRequirements, ReplicaPolicy, StorageRequirements, WorkloadKind,
    };

    fn make_test_workload(name: &str, cpu: u32, ram: u32) -> WorkloadDefinition {
        WorkloadDefinition {
            id: Uuid::new_v4(),
            name: name.to_string(),
            kind: WorkloadKind::GenericContainer,
            container_image: format!("{}:v1", name),
            entrypoint: None,
            ports: vec![],
            secrets: vec![],
            compute: ComputeRequirements {
                cpu_cores: cpu,
                ram_mb: ram,
                gpu_count: 0,
                vram_mb: 0,
                gpu_model_preference: None,
            },
            storage: StorageRequirements::default(),
            network: NetworkRequirements::default(),
            replicas: ReplicaPolicy::default(),
            placement: Default::default(),
        }
    }

    #[test]
    fn test_actuator_new_and_lifecycle() {
        let mut actuator = DeterministicContainerActuator::new();
        assert_eq!(actuator.list_containers().len(), 0);

        let wl = make_test_workload("api-service", 2, 1024);
        let launch_res = actuator.launch_workload(&wl);
        let container = launch_res.expect("Should launch workload via docker or native sandbox");

        assert_eq!(container.name, "api-service");
        assert_eq!(container.cpu_cores, 2);
        assert_eq!(container.ram_mb, 1024);
        assert_eq!(container.status, ContainerStatus::Running);
        assert!(actuator.is_running(&container.id));
        assert!(!actuator.is_stopped(&container.id));

        // Verify container state
        let status = actuator.verify_container_state(&container.id).unwrap();
        assert_eq!(status, ContainerStatus::Running);

        // Verify workload state integrity
        assert!(actuator.verify_workload_state(&wl).unwrap());

        // Verify detailed state
        let detailed = actuator
            .verify_container_state_detailed(&container.id)
            .unwrap();
        assert!(detailed.is_valid);
        assert_eq!(detailed.status, ContainerStatus::Running);

        // Stop container
        actuator
            .stop_container(&container.id)
            .expect("Should stop container");
        assert!(actuator.is_stopped(&container.id));
        assert!(!actuator.is_running(&container.id));

        let updated_status = actuator.verify_container_state(&container.id).unwrap();
        assert_eq!(updated_status, ContainerStatus::Stopped);
    }

    #[test]
    fn test_actuator_deploy_workload_result() {
        let mut actuator = DeterministicContainerActuator::new();
        let wl = make_test_workload("worker", 4, 2048);
        let res = actuator.deploy_workload(&wl).expect("Should deploy workload");
        assert_eq!(res.container_id, wl.id);
        assert_eq!(res.name, "worker");
        assert_eq!(res.cpu_cores, 4);
        assert_eq!(res.ram_mb, 2048);
        assert_eq!(res.status, ContainerStatus::Running);
        assert!(res.verified);
    }

    #[test]
    fn test_actuator_stop_non_existent_returns_err() {
        let mut actuator = DeterministicContainerActuator::new();
        let fake_id = Uuid::new_v4();
        let res = actuator.stop_container(&fake_id);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_actuator_verify_non_existent_returns_err() {
        let actuator = DeterministicContainerActuator::new();
        let fake_id = Uuid::new_v4();
        assert!(actuator.verify_container_state(&fake_id).is_err());
        assert!(actuator.verify_state_hash(&fake_id, "abc").is_err());
        assert!(actuator.verify_container_state_detailed(&fake_id).is_err());
    }

    #[test]
    fn test_actuator_verify_cluster_hash() {
        let mut actuator = DeterministicContainerActuator::new();
        let wl1 = make_test_workload("svc-a", 1, 512);
        let wl2 = make_test_workload("svc-b", 2, 1024);

        let empty_hash = actuator.verify_cluster_hash();
        assert_eq!(empty_hash.len(), 64);

        actuator.launch_workload(&wl1).unwrap();
        let hash1 = actuator.verify_cluster_hash();
        assert_ne!(hash1, empty_hash);

        actuator.launch_workload(&wl2).unwrap();
        let hash2 = actuator.verify_cluster_hash();
        assert_ne!(hash2, hash1);
    }

    #[test]
    fn test_actuator_launch_ephemeral() {
        let wl = make_test_workload("ephemeral-job", 1, 256);
        let res = DeterministicContainerActuator::launch_ephemeral(&wl);
        let container = res.expect("Ephemeral launch should succeed");
        assert_eq!(container.name, "ephemeral-job");
        assert_eq!(container.status, ContainerStatus::Running);
    }
}
