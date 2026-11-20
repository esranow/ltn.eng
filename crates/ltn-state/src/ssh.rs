//! ltn's own SSH setup for `ssh://` Docker hosts.
//!
//! The docker CLI runs `ssh` from `PATH` and cannot be given `-F`, so ltn puts
//! a small wrapper named `ssh` first on the child's `PATH`. The wrapper calls
//! the real ssh with ltn's config file (`<home>/ssh_config`), ltn's own
//! `known_hosts`, non-interactive options and connect/keepalive timeouts. The
//! user's `~/.ssh/config` and `known_hosts` are never read or written.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};

/// ltn's ssh client config. Host blocks for targets live here.
pub fn config_path(home: &Path) -> PathBuf {
    home.join("ssh_config")
}

/// Host keys of ltn targets, accepted on first contact.
pub fn known_hosts_path(home: &Path) -> PathBuf {
    home.join("known_hosts")
}

/// Directory holding the `ssh` wrapper; goes first on the docker child's PATH.
pub fn shim_dir(home: &Path) -> PathBuf {
    home.join("ssh-shim")
}

/// OpenSSH-friendly path: forward slashes, quoted.
pub fn ssh_path(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "/"))
}

fn begin_marker(alias: &str) -> String {
    format!("# >>> ltn {alias}")
}
fn end_marker(alias: &str) -> String {
    format!("# <<< ltn {alias}")
}

/// One managed `Host` block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEntry {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

impl HostEntry {
    pub fn block(&self) -> String {
        let mut s = format!("{}\nHost {}\n    HostName {}\n", begin_marker(&self.alias), self.alias, self.host_name);
        if let Some(u) = &self.user {
            s.push_str(&format!("    User {u}\n"));
        }
        if let Some(p) = self.port {
            s.push_str(&format!("    Port {p}\n"));
        }
        if let Some(k) = &self.identity_file {
            s.push_str(&format!("    IdentityFile {}\n    IdentitiesOnly yes\n", ssh_path(k)));
        }
        s.push_str("    StrictHostKeyChecking accept-new\n");
        s.push_str(&end_marker(&self.alias));
        s.push('\n');
        s
    }
}

fn strip_block(text: &str, alias: &str) -> String {
    let (b, e) = (begin_marker(alias), end_marker(alias));
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim() == b {
            inside = true;
            continue;
        }
        if inside {
            if line.trim() == e {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Adds or replaces the block for `entry.alias` in `config`.
pub fn upsert_host(config: &Path, entry: &HostEntry) -> Result<()> {
    let existing = std::fs::read_to_string(config).unwrap_or_default();
    let mut text = strip_block(&existing, &entry.alias);
    if text.trim().is_empty() {
        text = "# Managed by ltn. Edits inside ltn blocks are overwritten.\n".into();
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&entry.block());
    write(config, &text)
}

/// Removes the block for `alias` (no-op when absent).
pub fn remove_host(config: &Path, alias: &str) -> Result<()> {
    match std::fs::read_to_string(config) {
        Ok(existing) => write(config, &strip_block(&existing, alias)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("reading {}", config.display())),
    }
}

/// Splits `ssh://[user@]host[:port]` into its parts.
pub fn parse_ssh_url(url: &str) -> Result<(Option<String>, String, Option<u16>)> {
    let rest = url
        .trim()
        .strip_prefix("ssh://")
        .ok_or_else(|| anyhow!("`{url}` is not an ssh:// URL"))?
        .trim_end_matches('/');
    if rest.is_empty() || rest.contains('/') {
        bail!("`{url}` must look like ssh://user@host or ssh://user@host:port");
    }
    let (user, hostport) = match rest.rsplit_once('@') {
        Some((u, h)) if !u.is_empty() => (Some(u.to_string()), h),
        Some(_) => bail!("`{url}` has an empty user"),
        None => (None, rest),
    };
    let (host, port) = if let Some(inner) = hostport.strip_prefix('[') {
        let (h, after) = inner.split_once(']').ok_or_else(|| anyhow!("`{url}` has an unclosed ["))?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse::<u16>().map_err(|_| anyhow!("bad port in `{url}`"))?),
            None => None,
        };
        (h.to_string(), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.parse::<u16>().map_err(|_| anyhow!("bad port in `{url}`"))?)),
            None => (hostport.to_string(), None),
        }
    };
    if host.is_empty() {
        bail!("`{url}` has no host");
    }
    Ok((user, host, port))
}

/// The real ssh client: `LTN_SSH_BIN`, else the first `ssh` on PATH that is
/// not ltn's own wrapper.
pub fn real_ssh(home: &Path) -> Result<PathBuf> {
    if let Some(b) = std::env::var_os("LTN_SSH_BIN") {
        return Ok(PathBuf::from(b));
    }
    let shim = shim_dir(home);
    let name = if cfg!(windows) { "ssh.exe" } else { "ssh" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir == shim {
                continue;
            }
            let c = dir.join(name);
            if c.is_file() {
                return Ok(c);
            }
        }
    }
    #[cfg(windows)]
    {
        let sys = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32")
            .join("OpenSSH")
            .join("ssh.exe");
        if sys.is_file() {
            return Ok(sys);
        }
    }
    bail!("no ssh client found on PATH (install OpenSSH or set LTN_SSH_BIN)")
}

const COMMON_OPTS: &[&str] = &[
    "StrictHostKeyChecking=accept-new",
    "ConnectTimeout=10",
    "ServerAliveInterval=15",
    "ServerAliveCountMax=3",
    "BatchMode=yes",
];

/// Wrapper script text for this platform.
pub fn shim_script(real: &Path, config: &Path, known_hosts: &Path) -> String {
    let kh = known_hosts.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        // cmd: `%` must be doubled. The -o value keeps literal quotes for
        // OpenSSH's own tokenizer (\" survives the C runtime's argv parsing),
        // so a known_hosts path with spaces stays one value.
        let esc = |s: &str| s.replace('%', "%%");
        let mut line = format!(
            "@echo off\r\n\"{}\" -F \"{}\" -o \"UserKnownHostsFile=\\\"{}\\\"\"",
            esc(&real.to_string_lossy()),
            esc(&config.to_string_lossy()),
            esc(&kh)
        );
        for o in COMMON_OPTS {
            line.push_str(&format!(" -o {o}"));
        }
        line.push_str(" %*\r\n");
        line
    } else {
        let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let mut line = format!(
            "#!/bin/sh\nexec {} -F {} -o {}",
            q(&real.to_string_lossy()),
            q(&config.to_string_lossy()),
            q(&format!("UserKnownHostsFile=\"{kh}\""))
        );
        for o in COMMON_OPTS {
            line.push_str(&format!(" -o {o}"));
        }
        line.push_str(" \"$@\"\n");
        line
    }
}

/// Writes (when changed) the `ssh` wrapper under `home` and returns its
/// directory. Also creates an empty ltn ssh config if none exists.
pub fn ensure_ssh_shim(home: &Path) -> Result<PathBuf> {
    let dir = shim_dir(home);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let config = config_path(home);
    if !config.exists() {
        write(&config, "# Managed by ltn. Edits inside ltn blocks are overwritten.\n")?;
    }
    let real = real_ssh(home)?;
    let text = shim_script(&real, &config, &known_hosts_path(home));
    let file = dir.join(if cfg!(windows) { "ssh.cmd" } else { "ssh" });
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        write(&file, &text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ltn ssh {}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_ssh_urls() {
        assert_eq!(parse_ssh_url("ssh://root@10.0.0.5:2222").unwrap(), (Some("root".into()), "10.0.0.5".into(), Some(2222)));
        assert_eq!(parse_ssh_url("ssh://box").unwrap(), (None, "box".into(), None));
        assert_eq!(parse_ssh_url("ssh://u@[::1]:22").unwrap(), (Some("u".into()), "::1".into(), Some(22)));
        assert!(parse_ssh_url("tcp://x:2375").is_err());
        assert!(parse_ssh_url("ssh://u@h:notaport").is_err());
        assert!(parse_ssh_url("ssh://").is_err());
    }

    #[test]
    fn host_block_upsert_replace_remove() {
        let d = tmp();
        let cfg = config_path(&d);
        let e = |alias: &str, host: &str| HostEntry {
            alias: alias.into(),
            host_name: host.into(),
            user: Some("root".into()),
            port: Some(18422),
            identity_file: Some(d.join("keys").join("id_ed25519")),
        };
        upsert_host(&cfg, &e("node-a", "127.0.0.1")).unwrap();
        upsert_host(&cfg, &e("node-b", "127.0.0.2")).unwrap();
        upsert_host(&cfg, &e("node-a", "127.0.0.9")).unwrap();
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert_eq!(t.matches("Host node-a\n").count(), 1, "{t}");
        assert!(t.contains("HostName 127.0.0.9") && !t.contains("127.0.0.1"));
        assert!(t.contains("Port 18422") && t.contains("User root") && t.contains("IdentitiesOnly yes"));
        assert!(!t.contains('\\'), "{t}");
        remove_host(&cfg, "node-a").unwrap();
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert!(!t.contains("node-a") && t.contains("Host node-b"));
        remove_host(&d.join("absent"), "x").unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shim_is_written_and_points_at_ltn_files() {
        let d = tmp();
        let real = d.join("real ssh").join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "").unwrap();
        let text = shim_script(&real, &config_path(&d), &known_hosts_path(&d));
        assert!(text.contains(&real.to_string_lossy().to_string()), "{text}");
        assert!(text.contains(&config_path(&d).to_string_lossy().to_string()), "{text}");
        assert!(text.contains("known_hosts") && text.contains("BatchMode=yes") && text.contains("ConnectTimeout=10"));
        assert!(!text.contains(".ssh"), "must not reference the user's ssh dir: {text}");
        #[cfg(windows)]
        {
            assert!(text.starts_with("@echo off\r\n") && text.trim_end().ends_with("%*"));
            assert!(text.contains("-o \"UserKnownHostsFile=\\\""), "{text}");
        }
        #[cfg(not(windows))]
        assert!(text.starts_with("#!/bin/sh\nexec '") && text.trim_end().ends_with("\"$@\""));

        // ensure_ssh_shim finds a real ssh on this machine and is idempotent.
        if let Ok(dir) = ensure_ssh_shim(&d) {
            let file = dir.join(if cfg!(windows) { "ssh.cmd" } else { "ssh" });
            let first = std::fs::read_to_string(&file).unwrap();
            ensure_ssh_shim(&d).unwrap();
            assert_eq!(first, std::fs::read_to_string(&file).unwrap());
            assert!(config_path(&d).exists());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn windows_shim_passes_quoted_options_through() {
        // A stand-in "ssh" that echoes its arguments proves the quoting survives cmd.
        let d = tmp();
        let echo = d.join("echo-args.cmd");
        std::fs::write(&echo, "@echo off\r\necho %*\r\n").unwrap();
        let shim = d.join("ssh.cmd");
        std::fs::write(&shim, shim_script(&echo, &d.join("ssh_config"), &d.join("known hosts"))).unwrap();
        let out = std::process::Command::new("cmd").arg("/C").arg(&shim).arg("-p").arg("22").arg("host").output().unwrap();
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains("known hosts") && s.contains("BatchMode=yes"), "{s}");
        assert!(s.trim_end().ends_with("-p 22 host"), "{s}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
