//! What an app needs, read from the project itself: its Dockerfile and an
//! optional `ltn.toml`. Nothing here is guessed from the directory name.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SPEC_FILE: &str = "ltn.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSpec {
    pub name: String,
    /// Absolute path of the build context.
    pub path: String,
    /// Dockerfile path relative to `path`.
    pub dockerfile: String,
    /// Port the app listens on inside the container (`ltn.toml` or the Dockerfile's EXPOSE).
    pub port: Option<u16>,
    /// Port published on the target host; defaults to `port`.
    pub host_port: Option<u16>,
    pub health_path: String,
    pub health_timeout_s: u64,
    pub cpu_millis: u64,
    pub ram_mb: u64,
    pub gpu_count: u64,
    pub vram_mb: u64,
    pub max_cost_cents_per_hour: u64,
    pub env: BTreeMap<String, String>,
    /// Manifest files found in the project, for the agent's information only.
    pub stack: Vec<String>,
    /// Where each value came from, so an agent can see what was defaulted.
    pub sources: BTreeMap<String, String>,
}

/// The optional `ltn.toml`. Unknown keys are rejected so typos fail loudly.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecFile {
    name: Option<String>,
    dockerfile: Option<String>,
    port: Option<u16>,
    host_port: Option<u16>,
    health_path: Option<String>,
    health_timeout_s: Option<u64>,
    cpu: Option<f64>,
    ram_mb: Option<u64>,
    gpu: Option<u64>,
    vram_mb: Option<u64>,
    max_cost_cents_per_hour: Option<u64>,
    env: Option<BTreeMap<String, String>>,
}

const DEFAULT_CPU_MILLIS: u64 = 500;
const DEFAULT_RAM_MB: u64 = 512;
const DEFAULT_HEALTH_TIMEOUT_S: u64 = 60;
const MANIFESTS: &[&str] = &[
    "package.json",
    "requirements.txt",
    "pyproject.toml",
    "go.mod",
    "Cargo.toml",
    "pom.xml",
    "build.gradle",
    "Gemfile",
    "composer.json",
    "docker-compose.yml",
    "compose.yaml",
];

pub fn analyze(path: &Path) -> Result<AppSpec> {
    let root: PathBuf = path
        .canonicalize()
        .with_context(|| format!("project path {} does not exist", path.display()))?;
    if !root.is_dir() {
        bail!("{} is not a directory", root.display());
    }

    let file: SpecFile = match std::fs::read_to_string(root.join(SPEC_FILE)) {
        Ok(text) => toml::from_str(&text)
            .map_err(|e| anyhow!("{} is invalid: {e}", root.join(SPEC_FILE).display()))?,
        Err(_) => SpecFile::default(),
    };
    let mut sources = BTreeMap::new();
    let mut note = |key: &str, from: &str| {
        sources.insert(key.to_string(), from.to_string());
    };

    let stack: Vec<String> = MANIFESTS
        .iter()
        .filter(|m| root.join(m).is_file())
        .map(|m| m.to_string())
        .collect();

    let dockerfile = file.dockerfile.clone().unwrap_or_else(|| "Dockerfile".into());
    let dockerfile_text = std::fs::read_to_string(root.join(&dockerfile)).map_err(|_| {
        anyhow!(
            "no {dockerfile} in {}. ltn deploys container images, so the project needs a \
             Dockerfile (detected manifests: {}). Add one, or set `dockerfile = \"...\"` in {SPEC_FILE}.",
            root.display(),
            if stack.is_empty() { "none".to_string() } else { stack.join(", ") }
        )
    })?;

    let name = match &file.name {
        Some(n) => {
            note("name", SPEC_FILE);
            sanitize_name(n)
        }
        None => {
            note("name", "directory name");
            sanitize_name(&root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
        }
    };

    let port = match file.port {
        Some(p) => {
            note("port", SPEC_FILE);
            Some(p)
        }
        None => match first_exposed_port(&dockerfile_text) {
            Some(p) => {
                note("port", "Dockerfile EXPOSE");
                Some(p)
            }
            None => {
                note("port", "none (no EXPOSE, health = container running)");
                None
            }
        },
    };

    let cpu_millis = match file.cpu {
        Some(c) if c > 0.0 => {
            note("cpu", SPEC_FILE);
            (c * 1000.0).round() as u64
        }
        Some(c) => bail!("cpu must be > 0 in {SPEC_FILE}, got {c}"),
        None => {
            note("cpu", "default");
            DEFAULT_CPU_MILLIS
        }
    };
    let ram_mb = file.ram_mb.map(|v| {
        note("ram_mb", SPEC_FILE);
        v
    });
    let ram_mb = ram_mb.unwrap_or_else(|| {
        note("ram_mb", "default");
        DEFAULT_RAM_MB
    });
    if ram_mb < 16 {
        bail!("ram_mb must be at least 16, got {ram_mb}");
    }

    Ok(AppSpec {
        name,
        path: root.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
        dockerfile,
        port,
        host_port: file.host_port.or(port),
        health_path: normalize_path(file.health_path.as_deref().unwrap_or("/")),
        health_timeout_s: file.health_timeout_s.unwrap_or(DEFAULT_HEALTH_TIMEOUT_S),
        cpu_millis,
        ram_mb,
        gpu_count: file.gpu.unwrap_or(0),
        vram_mb: file.vram_mb.unwrap_or(0),
        max_cost_cents_per_hour: file.max_cost_cents_per_hour.unwrap_or(u64::MAX),
        env: file.env.unwrap_or_default(),
        stack,
        sources,
    })
}

/// Docker image and container names must be lowercase `[a-z0-9-]`.
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "app".into()
    } else {
        out
    }
}

fn first_exposed_port(dockerfile: &str) -> Option<u16> {
    dockerfile.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if !parts.next()?.eq_ignore_ascii_case("EXPOSE") {
            return None;
        }
        parts.next()?.split('/').next()?.parse().ok()
    })
}

fn normalize_path(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ltn-spec-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        dir
    }

    #[test]
    fn reads_port_from_expose_and_applies_defaults() {
        let dir = project(&[("Dockerfile", "FROM busybox\nEXPOSE 8080/tcp\n"), ("package.json", "{}")]);
        let s = analyze(&dir).unwrap();
        assert_eq!(s.port, Some(8080));
        assert_eq!(s.host_port, Some(8080));
        assert_eq!(s.cpu_millis, 500);
        assert_eq!(s.ram_mb, 512);
        assert_eq!(s.stack, vec!["package.json".to_string()]);
        assert_eq!(s.sources["port"], "Dockerfile EXPOSE");
    }

    #[test]
    fn ltn_toml_overrides_everything() {
        let dir = project(&[
            ("Dockerfile", "FROM busybox\nEXPOSE 80\n"),
            (
                "ltn.toml",
                "name = \"My API\"\nport = 9000\nhost_port = 19000\ncpu = 1.5\nram_mb = 256\nhealth_path = \"healthz\"\n[env]\nMODE = \"prod\"\n",
            ),
        ]);
        let s = analyze(&dir).unwrap();
        assert_eq!(s.name, "my-api");
        assert_eq!((s.port, s.host_port), (Some(9000), Some(19000)));
        assert_eq!((s.cpu_millis, s.ram_mb), (1500, 256));
        assert_eq!(s.health_path, "/healthz");
        assert_eq!(s.env["MODE"], "prod");
    }

    #[test]
    fn missing_dockerfile_is_an_error_naming_the_stack() {
        let dir = project(&[("go.mod", "module x")]);
        let err = analyze(&dir).unwrap_err().to_string();
        assert!(err.contains("Dockerfile") && err.contains("go.mod"), "{err}");
    }

    #[test]
    fn typo_in_ltn_toml_is_rejected() {
        let dir = project(&[("Dockerfile", "FROM busybox"), ("ltn.toml", "ram_gb = 4\n")]);
        assert!(analyze(&dir).unwrap_err().to_string().contains("invalid"));
    }

    #[test]
    fn sanitizes_names_for_docker() {
        assert_eq!(sanitize_name("My_Cool App!!"), "my-cool-app");
        assert_eq!(sanitize_name("***"), "app");
    }
}
