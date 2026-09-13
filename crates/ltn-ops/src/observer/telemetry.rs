use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketProbeResult {
    pub address: String,
    pub reachable: bool,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessProbeResult {
    pub name: String,
    pub pid: u32,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProbeResult {
    pub url: String,
    pub status_code: u16,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationTelemetry {
    pub socket_probes: Vec<SocketProbeResult>,
    pub process_probes: Vec<ProcessProbeResult>,
    pub http_probes: Vec<HttpProbeResult>,
    pub drift_detected: bool,
}

pub struct EmpiricalObserver;

impl EmpiricalObserver {
    pub fn probe_system(target_url: &str) -> ObservationTelemetry {
        use std::net::TcpStream;
        use std::time::{Duration, Instant};

        // 1. Real HTTP Probe
        let t0 = Instant::now();
        let http_res = ltn_providers::actuators::http_client::MinimalHttpClient::request(
            "GET",
            target_url,
            &[("User-Agent", "ltn-empirical-observer/1.0")],
            None,
        );
        let http_latency = t0.elapsed().as_secs_f64() * 1000.0;

        let (status_code, http_ok) = match http_res {
            Ok(resp) => (resp.status_code, resp.status_code >= 200 && resp.status_code < 400),
            Err(_) => (0, false),
        };

        let http_probes = vec![HttpProbeResult {
            url: target_url.to_string(),
            status_code,
            latency_ms: (http_latency * 10.0).round() / 10.0,
        }];

        // 2. Real Socket Probe
        let addr = target_url
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let host_port = addr.split('/').next().unwrap_or("127.0.0.1:4566");
        let target_socket = if host_port.contains(':') {
            host_port.to_string()
        } else {
            format!("{}:80", host_port)
        };

        let t_sock0 = Instant::now();
        let reachable = if let Ok(sock) = target_socket.parse() {
            TcpStream::connect_timeout(&sock, Duration::from_millis(800)).is_ok()
        } else {
            false
        };
        let sock_latency = t_sock0.elapsed().as_secs_f64() * 1000.0;

        let socket_probes = vec![SocketProbeResult {
            address: target_socket,
            reachable,
            latency_ms: (sock_latency * 10.0).round() / 10.0,
        }];

        // 3. Real Process Discovery via sysinfo
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let mut process_probes = Vec::new();

        for (pid, process) in sys.processes() {
            let pname = process.name().to_string();
            let pname_lower = pname.to_lowercase();
            if pname_lower.contains("ministack") || pname_lower.contains("python") || pname_lower.contains("cargo") || pname_lower.contains("ltn") {
                process_probes.push(ProcessProbeResult {
                    name: pname,
                    pid: pid.as_u32(),
                    running: true,
                });
                if process_probes.len() >= 3 {
                    break;
                }
            }
        }

        if process_probes.is_empty() {
            process_probes.push(ProcessProbeResult {
                name: "system-host".to_string(),
                pid: std::process::id(),
                running: true,
            });
        }

        let drift_detected = !http_ok && !reachable;

        ObservationTelemetry {
            socket_probes,
            process_probes,
            http_probes,
            drift_detected,
        }
    }
}
