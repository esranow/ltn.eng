use std::collections::HashMap;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct ProcessSandboxLimits {
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub max_pids: u32,
}

impl Default for ProcessSandboxLimits {
    fn default() -> Self {
        Self {
            cpu_cores: 1,
            ram_mb: 512,
            max_pids: 64,
        }
    }
}

pub struct ProcessSandbox {
    pub name: String,
    pub limits: ProcessSandboxLimits,
    pub env_vars: HashMap<String, String>,
}

impl ProcessSandbox {
    pub fn new(name: impl Into<String>, limits: ProcessSandboxLimits) -> Self {
        Self {
            name: name.into(),
            limits,
            env_vars: HashMap::new(),
        }
    }

    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.env_vars.insert(key.into(), value.into());
    }

    /// Spawns a sandboxed process with deterministic environment variables
    pub fn spawn_sandboxed(&self, program: &str, args: &[&str]) -> std::io::Result<Child> {
        let mut cmd = Command::new(program);
        cmd.args(args);

        // Inject deterministic runtime limits into child process environment
        cmd.env("LTN_DETERMINISTIC_SANDBOX", "1");
        cmd.env("LTN_CONTAINER_NAME", &self.name);
        cmd.env("LTN_LIMIT_CPU_CORES", self.limits.cpu_cores.to_string());
        cmd.env("LTN_LIMIT_RAM_MB", self.limits.ram_mb.to_string());

        for (k, v) in &self.env_vars {
            cmd.env(k, v);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        cmd.spawn()
    }
}
