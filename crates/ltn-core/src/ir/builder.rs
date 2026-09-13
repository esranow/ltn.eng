use super::types::{Constraint, Intent, Workload, WorkloadKind};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct WorkloadBuilder {
    name: String,
    kind: Option<WorkloadKind>,
    containers: Vec<String>,
    constraints: Vec<Constraint>,
}

impl WorkloadBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: None,
            containers: Vec::new(),
            constraints: Vec::new(),
        }
    }

    pub fn kind(mut self, kind: WorkloadKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn container(mut self, container_name: impl Into<String>) -> Self {
        self.containers.push(container_name.into());
        self
    }

    pub fn add_constraint(mut self, constraint: Constraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    pub fn cpu_cores(self, cores: u32) -> Self {
        self.add_constraint(Constraint::CpuCores(cores))
    }

    pub fn ram_mb(self, mb: u32) -> Self {
        self.add_constraint(Constraint::RamMb(mb))
    }

    pub fn vram_mb(self, mb: u32) -> Self {
        self.add_constraint(Constraint::VramMb(mb))
    }

    pub fn gpu_count(self, gpus: u32) -> Self {
        self.add_constraint(Constraint::GpuCount(gpus))
    }

    pub fn build(self) -> Workload {
        Workload {
            id: Uuid::new_v4(),
            name: self.name,
            kind: self.kind.unwrap_or(WorkloadKind::GenericContainer),
            containers: self.containers,
            constraints: self.constraints,
        }
    }
}

pub fn create_intent(workloads: Vec<Workload>) -> Intent {
    Intent {
        intent_id: Uuid::new_v4(),
        workloads,
    }
}
