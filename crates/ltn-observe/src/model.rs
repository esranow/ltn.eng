//! Plain data and pure logic: probe results, `docker stats` parsing and the
//! status verdict. Nothing here touches Docker or the network.

use ltn_state::docker::ContainerState;
use serde::Serialize;
use serde_json::{json, Value};

/// Result of one real HTTP GET against the app's published health URL.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Probe {
    pub url: String,
    /// true for 2xx/3xx.
    pub ok: bool,
    pub status: Option<u16>,
    pub latency_ms: u64,
    /// Transport error text, or None when an HTTP response came back.
    pub error: Option<String>,
    /// `ok` | `http_status` | `refused` | `reset` | `timeout` | `error`.
    pub kind: String,
}

impl Probe {
    /// Classifies a transport error message (ureq / OS text) into a kind.
    pub fn classify_error(message: &str) -> &'static str {
        let m = message.to_lowercase();
        if m.contains("timed out") || m.contains("timeout") {
            "timeout"
        } else if m.contains("refused") || m.contains("10061") {
            "refused"
        } else if m.contains("reset")
            || m.contains("closed")
            || m.contains("eof")
            || m.contains("10054")
            || m.contains("broken pipe")
            || m.contains("aborted")
        {
            "reset"
        } else {
            "error"
        }
    }
}

/// Parsed `docker stats --no-stream --format "{{json .}}"`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Stats {
    pub cpu_percent: f64,
    pub memory_used_mb: f64,
    pub memory_limit_mb: f64,
    pub memory_percent: f64,
    pub pids: Option<u64>,
    /// The string docker printed, for quoting in evidence.
    pub raw_mem_usage: String,
}

/// "0.15%" -> 0.15. "--" (docker's placeholder) -> None.
pub fn parse_percent(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').trim().parse().ok()
}

/// "12.5MiB" -> bytes. Accepts binary (KiB, MiB, GiB, TiB) and decimal
/// (kB, KB, MB, GB, TB) units, and plain B.
pub fn parse_size_bytes(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mult = match unit.trim() {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0f64.powi(4),
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some(n * mult)
}

/// "12.5MiB / 512MiB" -> (used_mb, limit_mb).
pub fn parse_mem_usage(s: &str) -> Option<(f64, f64)> {
    let (used, limit) = s.split_once('/')?;
    let mb = 1024.0 * 1024.0;
    Some((parse_size_bytes(used)? / mb, parse_size_bytes(limit)? / mb))
}

pub fn parse_stats(v: &Value) -> Option<Stats> {
    let raw_mem = v["MemUsage"].as_str()?.to_string();
    let (used, limit) = parse_mem_usage(&raw_mem)?;
    let memory_percent = v["MemPerc"]
        .as_str()
        .and_then(parse_percent)
        .unwrap_or(if limit > 0.0 { used / limit * 100.0 } else { 0.0 });
    Some(Stats {
        cpu_percent: v["CPUPerc"].as_str().and_then(parse_percent)?,
        memory_used_mb: round2(used),
        memory_limit_mb: round2(limit),
        memory_percent: round2(memory_percent),
        pids: v["PIDs"].as_str().and_then(|p| p.trim().parse().ok()),
        raw_mem_usage: raw_mem,
    })
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Healthy,
    Degraded,
    Down,
    Missing,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Healthy => "healthy",
            Verdict::Degraded => "degraded",
            Verdict::Down => "down",
            Verdict::Missing => "missing",
        }
    }
}

pub const MEMORY_PRESSURE_PERCENT: f64 = 90.0;

/// The status verdict and the reasons behind it, from observed facts only.
/// `probe` is None when the spec declares no port.
pub fn verdict(
    target_error: Option<&str>,
    container: Option<&ContainerState>,
    probe: Option<&Probe>,
    stats: Option<&Stats>,
) -> (Verdict, Vec<String>) {
    if let Some(e) = target_error {
        return (Verdict::Missing, vec![format!("target unreachable: {e}")]);
    }
    let Some(c) = container else {
        return (Verdict::Missing, vec!["no container exists for the current release".into()]);
    };
    if !c.running {
        let mut why = format!(
            "container exists but is not running (status {}, exit code {}",
            c.status, c.exit_code
        );
        if c.oom_killed {
            why.push_str(", OOMKilled");
        }
        why.push(')');
        return (Verdict::Down, vec![why]);
    }
    let mut reasons = Vec::new();
    if c.restarting {
        reasons.push(format!("container is restarting (restart count {})", c.restart_count));
    } else if c.restart_count > 0 {
        reasons.push(format!(
            "docker has restarted the container {} time(s) after it exited",
            c.restart_count
        ));
    }
    if let Some(p) = probe {
        if !p.ok {
            reasons.push(match (&p.status, &p.error) {
                (Some(s), _) => format!("GET {} returned HTTP {s}", p.url),
                (None, Some(e)) => format!("GET {} failed ({}): {e}", p.url, p.kind),
                _ => format!("GET {} failed", p.url),
            });
        }
    }
    if let Some(s) = stats {
        if s.memory_limit_mb > 0.0 && s.memory_percent > MEMORY_PRESSURE_PERCENT {
            reasons.push(format!("memory at {:.1}% of limit ({})", s.memory_percent, s.raw_mem_usage));
        }
    }
    if reasons.is_empty() {
        let mut ok = vec![format!("container running, restart count {}", c.restart_count)];
        match probe {
            Some(p) => ok.push(format!(
                "GET {} -> HTTP {} in {} ms",
                p.url,
                p.status.unwrap_or(0),
                p.latency_ms
            )),
            None => ok.push("spec declares no port; health = container running".into()),
        }
        (Verdict::Healthy, ok)
    } else {
        (Verdict::Degraded, reasons)
    }
}

/// Seconds since `started_at` (RFC 3339) for a running container.
pub fn uptime_s(c: &ContainerState, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    if !c.running {
        return None;
    }
    let t = chrono::DateTime::parse_from_rfc3339(&c.started_at).ok()?;
    Some((now - t.with_timezone(&chrono::Utc)).num_seconds().max(0))
}

/// `NetworkSettings.Ports` (live) or `HostConfig.PortBindings` (configured),
/// as "8080/tcp -> 0.0.0.0:18200" strings.
pub fn published_ports(inspect: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let live = inspect["NetworkSettings"]["Ports"].as_object().filter(|m| !m.is_empty());
    let source = live.or_else(|| inspect["HostConfig"]["PortBindings"].as_object());
    if let Some(map) = source {
        for (cport, binds) in map {
            match binds.as_array() {
                Some(list) if !list.is_empty() => {
                    for b in list {
                        out.push(format!(
                            "{cport} -> {}:{}",
                            b["HostIp"].as_str().filter(|s| !s.is_empty()).unwrap_or("0.0.0.0"),
                            b["HostPort"].as_str().unwrap_or("?")
                        ));
                    }
                }
                _ => out.push(format!("{cport} (not published)")),
            }
        }
    }
    out
}

/// `HostConfig.Memory` in MB, None when unlimited.
pub fn memory_limit_mb(inspect: &Value) -> Option<u64> {
    inspect["HostConfig"]["Memory"]
        .as_u64()
        .filter(|m| *m > 0)
        .map(|m| m / (1024 * 1024))
}

pub fn container_json(c: &ContainerState, uptime: Option<i64>, ports: &[String]) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
    v["uptime_s"] = json!(uptime);
    v["published_ports"] = json!(ports);
    v
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn state(running: bool, restarting: bool, exit: i64, oom: bool, restarts: i64) -> ContainerState {
        ContainerState {
            status: if running { "running".into() } else { "exited".into() },
            running,
            restarting,
            exit_code: exit,
            oom_killed: oom,
            restart_count: restarts,
            started_at: "2026-11-20T00:00:00.123456789Z".into(),
            finished_at: "0001-01-01T00:00:00Z".into(),
            error: String::new(),
        }
    }

    pub fn probe(ok: bool, status: Option<u16>, kind: &str, error: Option<&str>) -> Probe {
        Probe {
            url: "http://127.0.0.1:18200/".into(),
            ok,
            status,
            latency_ms: 3,
            error: error.map(str::to_string),
            kind: kind.into(),
        }
    }

    #[test]
    fn parses_sizes_and_percents() {
        assert_eq!(parse_percent("0.15%"), Some(0.15));
        assert_eq!(parse_percent("--"), None);
        let (u, l) = parse_mem_usage("12.5MiB / 512MiB").unwrap();
        assert!((u - 12.5).abs() < 1e-9 && (l - 512.0).abs() < 1e-9);
        let (u, l) = parse_mem_usage("1.5GiB / 7.6GiB").unwrap();
        assert!((u - 1536.0).abs() < 1e-6 && (l - 7782.4).abs() < 1e-3);
        let (u, _) = parse_mem_usage("512KiB / 1GiB").unwrap();
        assert!((u - 0.5).abs() < 1e-9);
        assert_eq!(parse_size_bytes("0B"), Some(0.0));
        assert_eq!(parse_size_bytes("2MB"), Some(2e6));
        assert_eq!(parse_size_bytes("12XB"), None);
    }

    #[test]
    fn parses_docker_stats_json() {
        let v: Value = serde_json::from_str(
            r#"{"BlockIO":"0B / 0B","CPUPerc":"0.15%","Container":"abc","ID":"abc","MemPerc":"2.44%","MemUsage":"12.5MiB / 512MiB","Name":"ltn-x-r1","NetIO":"1kB / 0B","PIDs":"3"}"#,
        )
        .unwrap();
        let s = parse_stats(&v).unwrap();
        assert_eq!(s.cpu_percent, 0.15);
        assert_eq!(s.memory_used_mb, 12.5);
        assert_eq!(s.memory_limit_mb, 512.0);
        assert_eq!(s.memory_percent, 2.44);
        assert_eq!(s.pids, Some(3));
        assert!(parse_stats(&json!({"CPUPerc": "--", "MemUsage": "-- / --"})).is_none());
    }

    #[test]
    fn classifies_probe_errors() {
        assert_eq!(Probe::classify_error("Connection Failed: Connect error: connection refused"), "refused");
        assert_eq!(
            Probe::classify_error(
                "No connection could be made because the target machine actively refused it. (os error 10061)"
            ),
            "refused"
        );
        assert_eq!(Probe::classify_error("Network Error: timed out reading response"), "timeout");
        assert_eq!(Probe::classify_error("Connection closed before receiving response"), "reset");
        assert_eq!(Probe::classify_error("dns failure"), "error");
    }

    #[test]
    fn verdict_vocabulary() {
        assert_eq!(verdict(Some("ssh: connect refused"), None, None, None).0, Verdict::Missing);
        assert_eq!(verdict(None, None, None, None).0, Verdict::Missing);
        assert_eq!(verdict(None, Some(&state(false, false, 1, false, 0)), None, None).0, Verdict::Down);
        let ok_probe = probe(true, Some(200), "ok", None);
        let healthy = verdict(None, Some(&state(true, false, 0, false, 0)), Some(&ok_probe), None);
        assert_eq!(healthy.0, Verdict::Healthy);
        assert!(healthy.1.iter().any(|r| r.contains("HTTP 200")));
        assert_eq!(verdict(None, Some(&state(true, false, 0, false, 0)), None, None).0, Verdict::Healthy);
        let refused = probe(false, None, "refused", Some("connection refused"));
        assert_eq!(
            verdict(None, Some(&state(true, false, 0, false, 0)), Some(&refused), None).0,
            Verdict::Degraded
        );
        let p503 = probe(false, Some(503), "http_status", None);
        assert_eq!(verdict(None, Some(&state(true, false, 0, false, 0)), Some(&p503), None).0, Verdict::Degraded);
        assert_eq!(verdict(None, Some(&state(true, true, 1, false, 2)), None, None).0, Verdict::Degraded);
        assert_eq!(verdict(None, Some(&state(true, false, 0, false, 1)), None, None).0, Verdict::Degraded);
        let hot = Stats {
            cpu_percent: 1.0,
            memory_used_mb: 470.0,
            memory_limit_mb: 512.0,
            memory_percent: 91.8,
            pids: None,
            raw_mem_usage: "470MiB / 512MiB".into(),
        };
        let (v, why) = verdict(None, Some(&state(true, false, 0, false, 0)), None, Some(&hot));
        assert_eq!(v, Verdict::Degraded);
        assert!(why[0].contains("470MiB / 512MiB"));
    }

    #[test]
    fn uptime_and_ports_from_inspect() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-20T00:01:40Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        // started 00:00:00.123, now 00:01:40 -> 99 whole seconds
        assert_eq!(uptime_s(&state(true, false, 0, false, 0), now), Some(99));
        assert_eq!(uptime_s(&state(false, false, 0, false, 0), now), None);
        let insp = json!({
            "NetworkSettings": {"Ports": {"8080/tcp": [{"HostIp": "0.0.0.0", "HostPort": "18200"}], "9000/tcp": null}},
            "HostConfig": {"Memory": 536870912}
        });
        let mut ports = published_ports(&insp);
        ports.sort();
        assert_eq!(
            ports,
            vec!["8080/tcp -> 0.0.0.0:18200".to_string(), "9000/tcp (not published)".to_string()]
        );
        assert_eq!(memory_limit_mb(&insp), Some(512));
        let stopped = json!({"NetworkSettings": {"Ports": {}}, "HostConfig": {"Memory": 0, "PortBindings": {"8080/tcp": [{"HostIp": "", "HostPort": "18201"}]}}});
        assert_eq!(published_ports(&stopped), vec!["8080/tcp -> 0.0.0.0:18201".to_string()]);
        assert_eq!(memory_limit_mb(&stopped), None);
    }
}
