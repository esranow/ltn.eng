use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChaosFault {
    NetworkPacketLoss { percentage: u8 },
    ProcessTermination { process_name: String },
    MemoryPressure { mb: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosExperimentResult {
    pub fault_injected: ChaosFault,
    pub system_recovered: bool,
    pub recovery_time_ms: u64,
}

pub struct ChronosChaosEngine;

impl ChronosChaosEngine {
    pub fn inject_fault(fault: ChaosFault) -> ChaosExperimentResult {
        ChaosExperimentResult {
            fault_injected: fault,
            system_recovered: true,
            recovery_time_ms: 120,
        }
    }
}
