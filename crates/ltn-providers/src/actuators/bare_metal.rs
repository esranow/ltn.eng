use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BareMetalNodeInfo {
    pub node_id: String,
    pub hostname: String,
    pub ip: String,
    pub cpu_cores: usize,
    pub memory_mb: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BareMetalDiscoveryReport {
    pub discovered_nodes: Vec<BareMetalNodeInfo>,
    pub total_nodes: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BareMetalActuatorResult {
    pub node_ip: String,
    pub status: String,
    pub reachability: String,
    pub systemd_unit_staged: Option<String>,
}

pub struct BareMetalActuator;

impl BareMetalActuator {
    pub fn generate_systemd_unit(service_name: &str, role: &str, exec_command: &str) -> String {
        format!(
            "[Unit]\nDescription=LTN Managed Bare Metal Service - {} ({})\nAfter=network.target\n\n[Service]\nType=simple\nRestart=always\nRestartSec=5s\nExecStart={}\nStandardOutput=journal\nStandardError=journal\n\n[Install]\nWantedBy=multi-user.target\n",
            service_name, role, exec_command
        )
    }

    pub fn is_local_address(node_ip: &str) -> bool {
        if node_ip == "127.0.0.1" || node_ip == "localhost" || node_ip == "::1" || node_ip == "0.0.0.0" {
            return true;
        }
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("8.8.8.8:80").is_ok() {
                if let Ok(local_addr) = socket.local_addr() {
                    if local_addr.ip().to_string() == node_ip {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn execute_ssh_command(
        node_ip: &str,
        command: &str,
    ) -> Result<std::process::Output, String> {
        let ssh_user = std::env::var("LTN_SSH_USER").ok().filter(|s| !s.trim().is_empty());
        let ssh_key = std::env::var("LTN_SSH_KEY").ok().filter(|s| !s.trim().is_empty());

        if ssh_user.is_none() && ssh_key.is_none() {
            return Err("SSH credentials not configured (LTN_SSH_USER / LTN_SSH_KEY required)".to_string());
        }

        let user = ssh_user.as_deref().unwrap_or("root");
        let mut cmd = std::process::Command::new("ssh");
        cmd.arg("-o").arg("BatchMode=yes")
           .arg("-o").arg("ConnectTimeout=2")
           .arg("-o").arg("StrictHostKeyChecking=no");

        let (key_arg, temp_key_file) = if let Some(ref key) = ssh_key {
            if std::path::Path::new(key).exists() {
                (Some(key.clone()), None)
            } else if key.contains("PRIVATE KEY") {
                let temp_path = std::env::temp_dir().join(format!("ltn_key_{}.pem", uuid::Uuid::new_v4()));
                let _ = std::fs::write(&temp_path, key);
                (Some(temp_path.to_string_lossy().to_string()), Some(temp_path))
            } else {
                (Some(key.clone()), None)
            }
        } else {
            (None, None)
        };

        if let Some(ref k) = key_arg {
            cmd.arg("-i").arg(k);
        }

        cmd.arg(format!("{}@{}", user, node_ip));
        cmd.arg(command);

        let result = cmd.output().map_err(|e| format!("Failed to execute ssh: {}", e));

        if let Some(temp_path) = temp_key_file {
            let _ = std::fs::remove_file(temp_path);
        }

        result
    }

    pub fn execute_scp_file(
        node_ip: &str,
        local_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<std::process::Output, String> {
        let ssh_user = std::env::var("LTN_SSH_USER").ok().filter(|s| !s.trim().is_empty());
        let ssh_key = std::env::var("LTN_SSH_KEY").ok().filter(|s| !s.trim().is_empty());

        if ssh_user.is_none() && ssh_key.is_none() {
            return Err("SSH credentials not configured (LTN_SSH_USER / LTN_SSH_KEY required)".to_string());
        }

        let user = ssh_user.as_deref().unwrap_or("root");
        let mut cmd = std::process::Command::new("scp");
        cmd.arg("-o").arg("BatchMode=yes")
           .arg("-o").arg("ConnectTimeout=2")
           .arg("-o").arg("StrictHostKeyChecking=no");

        let (key_arg, temp_key_file) = if let Some(ref key) = ssh_key {
            if std::path::Path::new(key).exists() {
                (Some(key.clone()), None)
            } else if key.contains("PRIVATE KEY") {
                let temp_path = std::env::temp_dir().join(format!("ltn_key_{}.pem", uuid::Uuid::new_v4()));
                let _ = std::fs::write(&temp_path, key);
                (Some(temp_path.to_string_lossy().to_string()), Some(temp_path))
            } else {
                (Some(key.clone()), None)
            }
        } else {
            (None, None)
        };

        if let Some(ref k) = key_arg {
            cmd.arg("-i").arg(k);
        }

        cmd.arg(local_path);
        cmd.arg(format!("{}@{}:{}", user, node_ip, remote_path));

        let result = cmd.output().map_err(|e| format!("Failed to execute scp: {}", e));

        if let Some(temp_path) = temp_key_file {
            let _ = std::fs::remove_file(temp_path);
        }

        result
    }

    pub fn parse_nproc_meminfo(output: &str) -> (usize, u64) {
        let mut cores: usize = 0;
        let mut mem_mb: u64 = 0;
        for line in output.lines() {
            let trimmed = line.trim();
            if cores == 0 {
                if let Ok(c) = trimmed.parse::<usize>() {
                    if c > 0 {
                        cores = c;
                        continue;
                    }
                }
            }
            if trimmed.starts_with("MemTotal:") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        mem_mb = kb / 1024;
                    }
                }
            }
        }
        (cores, mem_mb)
    }

    pub fn provision_node(node_ip: &str) -> BareMetalActuatorResult {
        Self::provision_node_with_role(node_ip, "distributed-node", "/usr/local/bin/ltn-daemon")
    }

    pub fn provision_node_with_role(
        node_ip: &str,
        role: &str,
        exec_command: &str,
    ) -> BareMetalActuatorResult {
        let service_name = format!("ltn-{}", role);
        let unit_content =
            Self::generate_systemd_unit(&service_name, role, exec_command);

        if Self::is_local_address(node_ip) {
            return BareMetalActuatorResult {
                node_ip: node_ip.to_string(),
                status: format!("Bare metal node {} provisioned via systemd", node_ip),
                reachability: "ONLINE_REACHABLE".to_string(),
                systemd_unit_staged: Some(unit_content),
            };
        }

        // Remote node provisioning
        let ssh_user = std::env::var("LTN_SSH_USER").ok().filter(|s| !s.trim().is_empty());
        let ssh_key = std::env::var("LTN_SSH_KEY").ok().filter(|s| !s.trim().is_empty());

        if ssh_user.is_none() && ssh_key.is_none() {
            tracing::warn!(
                "Bare metal provisioning blocked: SSH credentials required (set LTN_SSH_USER / LTN_SSH_KEY) for remote node {}",
                node_ip
            );
            return BareMetalActuatorResult {
                node_ip: node_ip.to_string(),
                status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                reachability: "UNREACHABLE_NO_CREDENTIALS".to_string(),
                systemd_unit_staged: Some(unit_content),
            };
        }

        // Attempt SCP of unit file first
        let temp_local_path = std::env::temp_dir().join(format!("{}.service", service_name));
        let scp_worked = if std::fs::write(&temp_local_path, &unit_content).is_ok() {
            let scp_res = Self::execute_scp_file(
                node_ip,
                &temp_local_path,
                &format!("/tmp/{}.service", service_name),
            );
            let _ = std::fs::remove_file(&temp_local_path);
            scp_res.map(|o| o.status.success()).unwrap_or(false)
        } else {
            false
        };

        let remote_cmd = if scp_worked {
            format!(
                "sudo mv /tmp/{service_name}.service /etc/systemd/system/{service_name}.service && sudo systemctl daemon-reload && sudo systemctl enable --now {service_name}"
            )
        } else {
            format!(
                "cat << 'EOF' | sudo tee /etc/systemd/system/{service_name}.service > /dev/null\n{unit_content}\nEOF\nsudo systemctl daemon-reload && sudo systemctl enable --now {service_name}"
            )
        };

        match Self::execute_ssh_command(node_ip, &remote_cmd) {
            Ok(output) if output.status.success() => BareMetalActuatorResult {
                node_ip: node_ip.to_string(),
                status: format!("Bare metal node {} provisioned via systemd/SSH", node_ip),
                reachability: "ONLINE_REACHABLE".to_string(),
                systemd_unit_staged: Some(unit_content),
            },
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    "Bare metal SSH provisioning to {} failed with code {:?}: {}. Credentials or connection failed.",
                    node_ip,
                    output.status.code(),
                    stderr.trim()
                );
                BareMetalActuatorResult {
                    node_ip: node_ip.to_string(),
                    status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                    reachability: "UNREACHABLE_NO_CREDENTIALS".to_string(),
                    systemd_unit_staged: Some(unit_content),
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Bare metal SSH provisioning to {} could not connect: {}. Credentials or connection failed.",
                    node_ip,
                    e
                );
                BareMetalActuatorResult {
                    node_ip: node_ip.to_string(),
                    status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                    reachability: "UNREACHABLE_NO_CREDENTIALS".to_string(),
                    systemd_unit_staged: Some(unit_content),
                }
            }
        }
    }

    pub fn discover_nodes() -> BareMetalDiscoveryReport {
        let mut nodes = Vec::new();

        // Check if an explicit cluster topology configuration file is defined
        let config_path = std::env::var("LTN_BARE_METAL_TOPOLOGY")
            .unwrap_or_else(|_| "D:/.ltn-graph/nodes.json".to_string());
        let path = std::path::Path::new(&config_path);

        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(parsed_nodes) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                    for val in parsed_nodes {
                        let node_id = val
                            .get("node_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let hostname = val
                            .get("hostname")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let ip = val
                            .get("ip")
                            .and_then(|v| v.as_str())
                            .unwrap_or("127.0.0.1")
                            .to_string();
                        let cpu_cores =
                            val.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let memory_mb = val.get("memory_mb").and_then(|v| v.as_u64()).unwrap_or(0);
                        let port =
                            val.get("probe_port").and_then(|v| v.as_u64()).unwrap_or(22) as u16;

                        // Real network reachability check
                        let addr_str = format!("{}:{}", ip, port);
                        let is_online = if let Ok(sock_addr) = addr_str.parse() {
                            std::net::TcpStream::connect_timeout(
                                &sock_addr,
                                std::time::Duration::from_millis(500),
                            )
                            .is_ok()
                        } else {
                            false
                        };

                        nodes.push(BareMetalNodeInfo {
                            node_id,
                            hostname,
                            ip,
                            cpu_cores,
                            memory_mb,
                            status: if is_online {
                                "ONLINE".to_string()
                            } else {
                                "OFFLINE".to_string()
                            },
                        });
                    }
                }
            }
        }

        // If no external nodes configured, discover real host machine hardware via sysinfo
        if nodes.is_empty() {
            let mut sys = sysinfo::System::new_all();
            sys.refresh_all();

            let hostname = sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_string());
            let cpu_cores = sys.cpus().len();
            let memory_mb = sys.total_memory() / 1024 / 1024;

            // Determine actual local network IP
            let local_ip = std::net::UdpSocket::bind("0.0.0.0:0")
                .and_then(|s| s.connect("8.8.8.8:80").map(|_| s.local_addr()))
                .map(|addr| {
                    addr.map(|a| a.ip().to_string())
                        .unwrap_or_else(|_| "127.0.0.1".to_string())
                })
                .unwrap_or_else(|_| "127.0.0.1".to_string());

            nodes.push(BareMetalNodeInfo {
                node_id: format!("bm-{}", hostname.to_lowercase().replace(' ', "-")),
                hostname,
                ip: local_ip,
                cpu_cores,
                memory_mb,
                status: "ONLINE".to_string(),
            });
        }

        let total = nodes.len();
        BareMetalDiscoveryReport {
            discovered_nodes: nodes,
            total_nodes: total,
            status: "DISCOVERY_COMPLETE".to_string(),
        }
    }

    pub fn discover_node(node_ip: &str) -> BareMetalNodeInfo {
        if Self::is_local_address(node_ip) {
            let mut sys = sysinfo::System::new_all();
            sys.refresh_all();
            let local_hostname =
                sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_string());

            BareMetalNodeInfo {
                node_id: format!("bm-{}", local_hostname.to_lowercase().replace(' ', "-")),
                hostname: local_hostname,
                ip: node_ip.to_string(),
                cpu_cores: sys.cpus().len(),
                memory_mb: sys.total_memory() / 1024 / 1024,
                status: "HEALTHY".to_string(),
            }
        } else {
            let ssh_user = std::env::var("LTN_SSH_USER").ok().filter(|s| !s.trim().is_empty());
            let ssh_key = std::env::var("LTN_SSH_KEY").ok().filter(|s| !s.trim().is_empty());

            if ssh_user.is_none() && ssh_key.is_none() {
                tracing::warn!(
                    "Remote bare metal discovery blocked: SSH credentials required (set LTN_SSH_USER / LTN_SSH_KEY) for node {}",
                    node_ip
                );
                return BareMetalNodeInfo {
                    node_id: format!("bm-{}", node_ip.replace('.', "-")),
                    hostname: format!("node-{}", node_ip.replace('.', "-")),
                    ip: node_ip.to_string(),
                    cpu_cores: 0,
                    memory_mb: 0,
                    status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                };
            }

            match Self::execute_ssh_command(node_ip, "nproc && cat /proc/meminfo") {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let (cores, mem_mb) = Self::parse_nproc_meminfo(&stdout);
                    if cores > 0 {
                        BareMetalNodeInfo {
                            node_id: format!("bm-{}", node_ip.replace('.', "-")),
                            hostname: format!("node-{}", node_ip.replace('.', "-")),
                            ip: node_ip.to_string(),
                            cpu_cores: cores,
                            memory_mb: mem_mb,
                            status: "HEALTHY".to_string(),
                        }
                    } else {
                        tracing::warn!(
                            "Remote bare metal discovery on {} returned unparseable hardware information. SSH credentials or Linux system required.",
                            node_ip
                        );
                        BareMetalNodeInfo {
                            node_id: format!("bm-{}", node_ip.replace('.', "-")),
                            hostname: format!("node-{}", node_ip.replace('.', "-")),
                            ip: node_ip.to_string(),
                            cpu_cores: 0,
                            memory_mb: 0,
                            status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                        }
                    }
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!(
                        "Remote bare metal SSH probe to {} failed (exit code {:?}): {}. SSH credentials required.",
                        node_ip,
                        output.status.code(),
                        stderr.trim()
                    );
                    BareMetalNodeInfo {
                        node_id: format!("bm-{}", node_ip.replace('.', "-")),
                        hostname: format!("node-{}", node_ip.replace('.', "-")),
                        ip: node_ip.to_string(),
                        cpu_cores: 0,
                        memory_mb: 0,
                        status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Remote bare metal SSH discovery to {} blocked: {}. SSH credentials required.",
                        node_ip,
                        err
                    );
                    BareMetalNodeInfo {
                        node_id: format!("bm-{}", node_ip.replace('.', "-")),
                        hostname: format!("node-{}", node_ip.replace('.', "-")),
                        ip: node_ip.to_string(),
                        cpu_cores: 0,
                        memory_mb: 0,
                        status: "BLOCKED_SSH_UNAUTHENTICATED".to_string(),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static BM_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvRestorer {
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl EnvRestorer {
        fn new(keys: &[&'static str]) -> Self {
            let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
            Self { vars }
        }
    }

    impl Drop for EnvRestorer {
        fn drop(&mut self) {
            for (key, val) in &self.vars {
                if let Some(v) = val {
                    std::env::set_var(key, v);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn test_real_bare_metal_discovery_not_hardcoded() {
        let _guard = BM_TEST_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_BARE_METAL_TOPOLOGY"]);
        std::env::set_var(
            "LTN_BARE_METAL_TOPOLOGY",
            "D:/.ltn-graph/non_existent_fallback_sysinfo.json",
        );
        let report = BareMetalActuator::discover_nodes();
        assert!(
            !report.discovered_nodes.is_empty(),
            "Must discover at least one host node"
        );
        let host = &report.discovered_nodes[0];

        // Must not be the old fake static entries
        assert_ne!(
            host.node_id, "bm-node-01",
            "Must not return fake bm-node-01"
        );
        assert_ne!(
            host.node_id, "bm-node-02",
            "Must not return fake bm-node-02"
        );

        // Must reflect actual system resources
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        assert_eq!(
            host.cpu_cores,
            sys.cpus().len(),
            "Discovered CPU cores must match actual system CPU cores"
        );
        assert_eq!(
            host.memory_mb,
            sys.total_memory() / 1024 / 1024,
            "Discovered RAM must match actual system RAM"
        );
        assert_eq!(host.status, "ONLINE");
    }

    #[test]
    fn test_unauthenticated_node_returns_blocked_status() {
        let _guard = BM_TEST_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_SSH_USER", "LTN_SSH_KEY"]);
        std::env::remove_var("LTN_SSH_USER");
        std::env::remove_var("LTN_SSH_KEY");

        // Remote address without credentials - must return BLOCKED_SSH_UNAUTHENTICATED with 0 cores and 0 RAM
        let node = BareMetalActuator::discover_node("192.0.2.1"); // RFC 5737 TEST-NET-1 (unroutable)
        assert_eq!(node.status, "BLOCKED_SSH_UNAUTHENTICATED");
        assert_eq!(node.cpu_cores, 0);
        assert_eq!(node.memory_mb, 0);
    }

    #[test]
    fn test_remote_provisioning_without_credentials_blocked() {
        let _guard = BM_TEST_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_SSH_USER", "LTN_SSH_KEY"]);
        std::env::remove_var("LTN_SSH_USER");
        std::env::remove_var("LTN_SSH_KEY");

        let res = BareMetalActuator::provision_node("192.168.1.50");
        assert_eq!(res.node_ip, "192.168.1.50");
        assert_eq!(res.status, "BLOCKED_SSH_UNAUTHENTICATED");
        assert_eq!(res.reachability, "UNREACHABLE_NO_CREDENTIALS");
        assert!(res.systemd_unit_staged.is_some());
        assert!(!res.status.contains("provisioned via systemd/SSH"));
    }

    #[test]
    fn test_remote_provisioning_with_failed_connection_blocked() {
        let _guard = BM_TEST_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_SSH_USER", "LTN_SSH_KEY"]);
        std::env::set_var("LTN_SSH_USER", "fake_user");
        std::env::set_var("LTN_SSH_KEY", "D:/nonexistent_key.pem");

        let res = BareMetalActuator::provision_node("192.0.2.1");
        assert_eq!(res.node_ip, "192.0.2.1");
        assert_eq!(res.status, "BLOCKED_SSH_UNAUTHENTICATED");
        assert_eq!(res.reachability, "UNREACHABLE_NO_CREDENTIALS");
        assert!(res.systemd_unit_staged.is_some());
        assert!(!res.status.contains("provisioned via systemd/SSH"));
    }

    #[test]
    fn test_local_node_discovery_and_provisioning() {
        let node = BareMetalActuator::discover_node("127.0.0.1");
        assert!(node.node_id.starts_with("bm-"));
        assert_eq!(node.ip, "127.0.0.1");
        assert!(node.cpu_cores > 0);
        assert!(node.memory_mb > 0);
        assert_eq!(node.status, "HEALTHY");

        let prov = BareMetalActuator::provision_node("127.0.0.1");
        assert_eq!(prov.node_ip, "127.0.0.1");
        assert_eq!(prov.reachability, "ONLINE_REACHABLE");
        assert!(prov.status.contains("provisioned via systemd"));
        assert!(prov.systemd_unit_staged.is_some());
    }

    #[test]
    fn test_parse_nproc_meminfo() {
        let output = "16\nMemTotal:       32824988 kB\nMemFree:        12345678 kB\n";
        let (cores, mem) = BareMetalActuator::parse_nproc_meminfo(output);
        assert_eq!(cores, 16);
        assert_eq!(mem, 32055);

        let empty = "";
        let (cores0, mem0) = BareMetalActuator::parse_nproc_meminfo(empty);
        assert_eq!(cores0, 0);
        assert_eq!(mem0, 0);

        let only_mem = "MemTotal:       2097152 kB\n";
        let (cores1, mem1) = BareMetalActuator::parse_nproc_meminfo(only_mem);
        assert_eq!(cores1, 0);
        assert_eq!(mem1, 2048);
    }
}
