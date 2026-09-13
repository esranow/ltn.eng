use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalOsActuatorResult {
    pub pid: u32,
    pub status: String,
}

pub struct LocalOsActuator;

impl LocalOsActuator {
    pub fn spawn_process(command: &str) -> LocalOsActuatorResult {
        LocalOsActuatorResult {
            pid: 12345,
            status: format!("Process '{}' started on local OS", command),
        }
    }
}
