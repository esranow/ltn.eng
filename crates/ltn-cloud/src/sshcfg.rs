//! SSH host aliases for cloud targets. Aliases live in ltn's own ssh config
//! (`<ltn home>/ssh_config`, see `ltn_state::ssh`), which ltn's ssh wrapper
//! passes to ssh with `-F`. The user's ssh config is not touched; the
//! `ensure_include` helper below is kept only for users who want to opt in
//! to using the aliases from their own shell.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// OpenSSH-friendly path: forward slashes, quoted.
pub fn ssh_path(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "/"))
}

/// Default location of the user's ssh client config.
pub fn user_ssh_config() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".ssh").join("config"))
}

pub fn alias_block(alias: &str, host: &str, user: &str, identity: &Path) -> String {
    entry(alias, host, user, identity).block()
}

fn entry(alias: &str, host: &str, user: &str, identity: &Path) -> ltn_state::ssh::HostEntry {
    ltn_state::ssh::HostEntry {
        alias: alias.to_string(),
        host_name: host.to_string(),
        user: Some(user.to_string()),
        port: None,
        identity_file: Some(identity.to_path_buf()),
    }
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Adds or replaces the alias block in the managed file.
pub fn upsert_alias(managed: &Path, alias: &str, host: &str, user: &str, identity: &Path) -> Result<()> {
    ltn_state::ssh::upsert_host(managed, &entry(alias, host, user, identity))
}

/// Removes the alias block (no-op when absent).
pub fn remove_alias(managed: &Path, alias: &str) -> Result<()> {
    ltn_state::ssh::remove_host(managed, alias)
}

pub fn include_line(managed: &Path) -> String {
    format!("Include {}", ssh_path(managed))
}

/// Ensures `user_config` has exactly one `Include` line for `managed`, at the
/// top (an Include after a `Host` line would only apply inside that host block).
/// Returns true when the file was changed.
pub fn ensure_include(user_config: &Path, managed: &Path) -> Result<bool> {
    let line = include_line(managed);
    let existing = match std::fs::read_to_string(user_config) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", user_config.display())),
    };
    let ours = |l: &str| l.trim().eq_ignore_ascii_case(&line);
    let count = existing.lines().filter(|l| ours(l)).count();
    let first_is_ours = existing.lines().find(|l| !l.trim().is_empty()).map_or(false, ours);
    if count == 1 && first_is_ours {
        return Ok(false);
    }
    let mut text = format!("{line}\n");
    let rest: Vec<&str> = existing.lines().filter(|l| !ours(l)).collect();
    if !rest.is_empty() {
        text.push('\n');
        text.push_str(rest.join("\n").trim_start_matches('\n'));
        text.push('\n');
    }
    write(user_config, &text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ltn-cloud-ssh-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn include_is_single_and_idempotent() {
        let d = tmp();
        let user = d.join(".ssh").join("config");
        let managed = d.join("ltn home").join("ssh_config");
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        std::fs::write(&user, "Host old\n    HostName 10.0.0.1\n").unwrap();

        assert!(ensure_include(&user, &managed).unwrap());
        let once = std::fs::read_to_string(&user).unwrap();
        assert!(!ensure_include(&user, &managed).unwrap());
        assert!(!ensure_include(&user, &managed).unwrap());
        let thrice = std::fs::read_to_string(&user).unwrap();
        assert_eq!(once, thrice);
        assert_eq!(thrice.matches("Include ").count(), 1);
        assert!(thrice.starts_with("Include \""), "{thrice}");
        assert!(thrice.contains("Host old"));

        // A duplicated or misplaced line is collapsed back to one at the top.
        std::fs::write(&user, format!("Host a\n{}\n{}\n", include_line(&managed), include_line(&managed))).unwrap();
        assert!(ensure_include(&user, &managed).unwrap());
        let fixed = std::fs::read_to_string(&user).unwrap();
        assert_eq!(fixed.matches("Include ").count(), 1);
        assert!(fixed.starts_with("Include "));

        // Missing file is created.
        let fresh = d.join("new").join("config");
        assert!(ensure_include(&fresh, &managed).unwrap());
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), format!("{}\n", include_line(&managed)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn alias_upsert_replace_and_remove() {
        let d = tmp();
        let managed = d.join("ssh_config");
        let key = d.join("cloud").join("web").join("id_ed25519");
        upsert_alias(&managed, "ltn-web", "198.51.100.4", "ec2-user", &key).unwrap();
        upsert_alias(&managed, "ltn-api", "198.51.100.5", "ec2-user", &key).unwrap();
        upsert_alias(&managed, "ltn-web", "198.51.100.9", "ec2-user", &key).unwrap();
        let text = std::fs::read_to_string(&managed).unwrap();
        assert_eq!(text.matches("Host ltn-web\n").count(), 1);
        assert!(text.contains("HostName 198.51.100.9") && !text.contains("198.51.100.4"));
        assert!(text.contains("User ec2-user"));
        assert!(text.contains("StrictHostKeyChecking accept-new"));
        assert!(text.contains("IdentityFile \"") && text.contains("id_ed25519\""));
        assert!(!text.contains('\\'), "paths use forward slashes: {text}");
        remove_alias(&managed, "ltn-web").unwrap();
        let text = std::fs::read_to_string(&managed).unwrap();
        assert!(!text.contains("ltn-web") && text.contains("Host ltn-api"));
        remove_alias(&d.join("absent"), "ltn-x").unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
