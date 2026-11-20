//! The AWS provisioner: OpenTofu creates a VM with Docker and SSH, which is
//! then returned as an ordinary Docker target reached via `ssh://ltn-<name>`.

use crate::auth::{self, AuthStatus};
use crate::catalog;
use crate::exec::{self, Bins};
use crate::hcl::{self, ModuleParams};
use crate::sshcfg;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use ltn_state::store::{State, Target};
use ltn_state::{Ctx, Offer, Provisioner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const SPEND_ENV: &str = "LTN_ALLOW_CLOUD_SPEND";
pub const CHECKIP_URL: &str = "https://checkip.amazonaws.com";

/// Everything provisioning depends on outside the state store. Built from
/// the process environment in production; tests construct it with fake binaries.
#[derive(Debug, Clone)]
pub struct ProvisionEnv {
    pub bins: Bins,
    /// True only when `LTN_ALLOW_CLOUD_SPEND=1`.
    pub allow_spend: bool,
    /// The user's ssh client config. Not written by provisioning (ltn's ssh
    /// wrapper uses ltn's own config); kept for `sshcfg::ensure_include` opt-in.
    pub user_ssh_config: PathBuf,
    pub docker_wait: Duration,
    pub docker_poll: Duration,
    pub app_ports: Vec<u16>,
    pub root_volume_gb: u32,
}

impl ProvisionEnv {
    pub fn from_env() -> Result<Self> {
        let user_ssh_config = std::env::var_os("LTN_SSH_CONFIG")
            .map(PathBuf::from)
            .or_else(sshcfg::user_ssh_config)
            .ok_or_else(|| anyhow!("cannot locate the user's ssh config (set USERPROFILE/HOME or LTN_SSH_CONFIG)"))?;
        Ok(Self {
            bins: Bins::from_env(),
            allow_spend: std::env::var(SPEND_ENV).map_or(false, |v| v.trim() == "1"),
            user_ssh_config,
            // `LTN_CLOUD_DOCKER_WAIT_S` shortens/lengthens the wait for Docker on a new VM.
            docker_wait: Duration::from_secs(
                std::env::var("LTN_CLOUD_DOCKER_WAIT_S").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(300),
            ),
            docker_poll: Duration::from_secs(10),
            app_ports: hcl::DEFAULT_APP_PORTS.to_vec(),
            root_volume_gb: hcl::DEFAULT_ROOT_VOLUME_GB,
        })
    }
}

/// One piece of cloud infrastructure ltn created, kept in `State.ext["cloud"]["infra"][name]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfraRecord {
    pub name: String,
    pub offer_id: String,
    pub instance_type: String,
    pub region: String,
    /// applying | ready | cleanup_failed | destroy_failed
    pub status: String,
    pub instance_id: Option<String>,
    pub public_ip: Option<String>,
    pub module_dir: String,
    pub ssh_alias: String,
    pub docker_host: String,
    pub created_at: DateTime<Utc>,
    pub cost_cents_per_hour: u64,
    pub usd_per_hour: String,
}

pub fn infra_records(state: &State) -> Vec<InfraRecord> {
    state
        .ext
        .get("cloud")
        .and_then(|c| c.get("infra"))
        .and_then(Value::as_object)
        .map(|m| m.values().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
        .unwrap_or_default()
}

fn infra_map(state: &mut State) -> &mut serde_json::Map<String, Value> {
    let cloud = state.ext.entry("cloud".into()).or_insert_with(|| json!({}));
    if !cloud.is_object() {
        *cloud = json!({});
    }
    let infra = cloud.as_object_mut().expect("object").entry("infra").or_insert_with(|| json!({}));
    if !infra.is_object() {
        *infra = json!({});
    }
    infra.as_object_mut().expect("object")
}

pub fn put_record(state: &mut State, rec: &InfraRecord) -> Result<()> {
    infra_map(state).insert(rec.name.clone(), serde_json::to_value(rec)?);
    Ok(())
}

pub fn remove_record(state: &mut State, name: &str) {
    infra_map(state).remove(name);
}

/// Parses `tofu output -json` into (instance_id, public_ip).
pub fn parse_outputs(text: &str) -> Result<(String, String)> {
    let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("unreadable `tofu output -json`: {e}"))?;
    let get = |k: &str| v[k]["value"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let id = get("instance_id").ok_or_else(|| anyhow!("tofu outputs have no instance_id"))?;
    let ip = get("public_ip").ok_or_else(|| anyhow!("tofu outputs have no public_ip (instance has no public address)"))?;
    ip.parse::<Ipv4Addr>().map_err(|_| anyhow!("tofu output public_ip `{ip}` is not an IPv4 address"))?;
    Ok((id, ip))
}

/// The caller's public IPv4 address as a /32, via `curl` to checkip.amazonaws.com.
pub fn detect_ssh_cidr(curl: &str) -> Result<String> {
    let out = exec::run(curl, &["-fsS", "--max-time", "15", CHECKIP_URL], None)
        .context("detecting this machine's public IP for the SSH rule")?;
    if !out.ok {
        bail!(
            "could not determine this machine's public IP from {CHECKIP_URL} ({}). Refusing to open SSH to 0.0.0.0/0; fix network access and retry.",
            out.evidence()
        );
    }
    let ip: Ipv4Addr = out.stdout.trim().parse().map_err(|_| {
        anyhow!("{CHECKIP_URL} returned `{}`, not an IPv4 address. Refusing to open SSH to 0.0.0.0/0.", out.stdout.trim())
    })?;
    Ok(format!("{ip}/32"))
}

fn tofu(bin: &str, dir: &Path, args: &[&str]) -> Result<exec::Out> {
    exec::run(bin, args, Some(dir))
}

fn alias_for(name: &str) -> String {
    format!("ltn-{name}")
}

fn managed_ssh_config(ctx: &Ctx) -> PathBuf {
    ctx.home.join("ssh_config")
}

/// Provision with an explicit environment. `Provisioner::provision` calls this
/// with `ProvisionEnv::from_env()`.
pub fn provision_with(ctx: &Ctx, env: &ProvisionEnv, offer_id: &str, name: &str) -> Result<Target> {
    if !env.allow_spend {
        bail!(
            "refused: provisioning `{offer_id}` creates billable AWS resources. Set {SPEND_ENV}=1 in the \
             ltn server's environment to allow cloud spend, then retry."
        );
    }
    hcl::validate_name(name)?;
    let (instance_type, region) = catalog::parse_offer_id(offer_id)?;
    if let AuthStatus::Unconfigured(reason) = auth::check(&env.bins.aws) {
        bail!("refused: AWS credentials are not configured. {reason}");
    }
    let tofu_bin = env.bins.tofu.clone().ok_or_else(|| {
        anyhow!("refused: OpenTofu not found. Install OpenTofu (https://opentofu.org) or Terraform, or set LTN_TOFU_BIN to its path.")
    })?;

    let store = ctx.store();
    let state = store.load()?;
    let cat = catalog::cached_catalog(&state, &region).ok_or_else(|| {
        anyhow!("no cached AWS catalog for {region}; call ltn_cloud_catalog with refresh=true and region={region} first")
    })?;
    let entry = cat
        .entries
        .iter()
        .find(|e| e.spec.instance_type == instance_type)
        .cloned()
        .ok_or_else(|| anyhow!("`{instance_type}` is not in the cached {region} catalog; refresh it with ltn_cloud_catalog"))?;
    if infra_records(&state).iter().any(|r| r.name == name) || state.targets.iter().any(|t| t.id == name) {
        bail!("a target or cloud record named `{name}` already exists; choose another name");
    }

    let module_dir = ctx.home.join("cloud").join(name);
    if module_dir.join("terraform.tfstate").exists() {
        bail!(
            "{} already holds OpenTofu state from an earlier run. Inspect it and run `tofu destroy` there before reusing the name.",
            module_dir.display()
        );
    }
    std::fs::create_dir_all(&module_dir).with_context(|| format!("creating {}", module_dir.display()))?;

    // SSH key pair; the private key never leaves the module directory.
    let key = module_dir.join("id_ed25519");
    let _ = std::fs::remove_file(&key);
    let _ = std::fs::remove_file(key.with_extension("pub"));
    let key_s = key.to_string_lossy().into_owned();
    let comment = alias_for(name);
    let kg = exec::run(&env.bins.ssh_keygen, &["-f", &key_s, "-t", "ed25519", "-N", "", "-q", "-C", &comment], None)
        .context("generating the SSH key (is OpenSSH's ssh-keygen installed? set LTN_SSH_KEYGEN_BIN)")?;
    if !kg.ok {
        bail!("ssh-keygen failed: {}", kg.evidence());
    }
    let public_key = std::fs::read_to_string(format!("{key_s}.pub"))
        .with_context(|| format!("reading {key_s}.pub after ssh-keygen"))?
        .trim()
        .to_string();

    let ssh_cidr = detect_ssh_cidr(&env.bins.curl)?;
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let hcl_text = hcl::render_module(&ModuleParams {
        target_name: name.into(),
        unique_suffix: suffix,
        region: region.clone(),
        instance_type: instance_type.clone(),
        public_key,
        ssh_cidr: ssh_cidr.clone(),
        app_ports: env.app_ports.clone(),
        root_volume_gb: env.root_volume_gb,
    })?;
    std::fs::write(module_dir.join("main.tf"), hcl_text).context("writing main.tf")?;

    let alias = alias_for(name);
    let docker_host = format!("ssh://{alias}");
    let mut rec = InfraRecord {
        name: name.into(),
        offer_id: offer_id.into(),
        instance_type: instance_type.clone(),
        region: region.clone(),
        status: "applying".into(),
        instance_id: None,
        public_ip: None,
        module_dir: module_dir.to_string_lossy().into_owned(),
        ssh_alias: alias.clone(),
        docker_host: docker_host.clone(),
        created_at: Utc::now(),
        cost_cents_per_hour: entry.cents_per_hour,
        usd_per_hour: entry.usd_per_hour.clone(),
    };

    let init = tofu(&tofu_bin, &module_dir, &["init", "-input=false", "-no-color"])?;
    if !init.ok {
        bail!("`tofu init` failed in {} (nothing was created): {}", module_dir.display(), init.evidence());
    }

    store.update(|st| put_record(st, &rec))?;
    store.audit(None, "cloud_provision_start", json!({ "name": name, "offer_id": offer_id, "ssh_cidr": ssh_cidr }));

    // From here on, any failure triggers `tofu destroy`.
    let result = (|| -> Result<(String, String)> {
        let apply = tofu(&tofu_bin, &module_dir, &["apply", "-auto-approve", "-input=false", "-no-color"])?;
        if !apply.ok {
            bail!("`tofu apply` failed: {}", apply.evidence());
        }
        let out = tofu(&tofu_bin, &module_dir, &["output", "-json", "-no-color"])?;
        if !out.ok {
            bail!("`tofu output` failed: {}", out.evidence());
        }
        let (instance_id, ip) = parse_outputs(&out.stdout)?;
        sshcfg::upsert_alias(&managed_ssh_config(ctx), &alias, &ip, "ec2-user", &key)?;
        wait_for_docker(ctx, env, &docker_host)?;
        Ok((instance_id, ip))
    })();

    match result {
        Ok((instance_id, ip)) => {
            rec.status = "ready".into();
            rec.instance_id = Some(instance_id.clone());
            rec.public_ip = Some(ip.clone());
            store.update(|st| put_record(st, &rec))?;
            store.audit(None, "cloud_provision", serde_json::to_value(&rec)?);
            let mut labels = BTreeMap::new();
            labels.insert("instance_id".into(), instance_id);
            labels.insert("region".into(), region);
            labels.insert("instance_type".into(), instance_type);
            labels.insert("module_dir".into(), rec.module_dir.clone());
            labels.insert("offer_id".into(), offer_id.into());
            Ok(Target {
                id: name.into(),
                docker_host: Some(docker_host),
                address: ip,
                cost_cents_per_hour: entry.cents_per_hour,
                gpu_count: entry.spec.gpu_count,
                vram_mb: entry.spec.gpu_vram_mib,
                provider: "aws".into(),
                labels,
            })
        }
        Err(e) => {
            let destroy = tofu(&tofu_bin, &module_dir, &["destroy", "-auto-approve", "-input=false", "-no-color"]);
            let _ = sshcfg::remove_alias(&managed_ssh_config(ctx), &alias);
            let cleanup = match destroy {
                Ok(d) if d.ok => {
                    let _ = store.update(|st| {
                        remove_record(st, name);
                        Ok(())
                    });
                    // Nothing is left in AWS; drop the state dir and key so the name can be reused.
                    let _ = std::fs::remove_dir_all(&module_dir);
                    "cleanup succeeded: `tofu destroy` removed what was created".to_string()
                }
                Ok(d) => {
                    rec.status = "cleanup_failed".into();
                    let _ = store.update(|st| put_record(st, &rec));
                    format!(
                        "CLEANUP FAILED: `tofu destroy` exited {:?}: {}. Resources may still exist and bill; run `tofu destroy` in {}",
                        d.code,
                        d.evidence(),
                        module_dir.display()
                    )
                }
                Err(de) => {
                    rec.status = "cleanup_failed".into();
                    let _ = store.update(|st| put_record(st, &rec));
                    format!(
                        "CLEANUP FAILED: could not run `tofu destroy` ({de}). Resources may still exist and bill; run `tofu destroy` in {}",
                        module_dir.display()
                    )
                }
            };
            store.audit(None, "cloud_provision_failed", json!({ "name": name, "error": e.to_string(), "cleanup": cleanup }));
            Err(anyhow!("provisioning `{name}` ({offer_id}) failed: {e:#}. {cleanup}"))
        }
    }
}

fn wait_for_docker(ctx: &Ctx, env: &ProvisionEnv, docker_host: &str) -> Result<()> {
    let bin = env.bins.docker.clone().unwrap_or_else(|| ctx.docker_bin.clone());
    let deadline = Instant::now() + env.docker_wait;
    let mut last: String;
    loop {
        match exec::run(&bin, &["-H", docker_host, "info", "--format", "{{.ServerVersion}}"], None) {
            Ok(o) if o.ok && !o.stdout.trim().is_empty() => return Ok(()),
            Ok(o) => last = o.evidence(),
            Err(e) => last = e.to_string(),
        }
        if Instant::now() + env.docker_poll > deadline {
            bail!(
                "Docker on {docker_host} did not become reachable within {}s (last error: {last})",
                env.docker_wait.as_secs()
            );
        }
        std::thread::sleep(env.docker_poll);
    }
}

pub fn deprovision_with(ctx: &Ctx, env: &ProvisionEnv, target: &Target) -> Result<()> {
    let store = ctx.store();
    let state = store.load()?;
    let mut rec = infra_records(&state)
        .into_iter()
        .find(|r| r.name == target.id)
        .ok_or_else(|| anyhow!("no recorded AWS infrastructure for target `{}`", target.id))?;
    let tofu_bin = env.bins.tofu.clone().ok_or_else(|| {
        anyhow!("OpenTofu not found; install it or set LTN_TOFU_BIN, then retry (the instance is still running)")
    })?;
    let dir = PathBuf::from(&rec.module_dir);
    let d = tofu(&tofu_bin, &dir, &["destroy", "-auto-approve", "-input=false", "-no-color"])?;
    if !d.ok {
        rec.status = "destroy_failed".into();
        store.update(|st| put_record(st, &rec))?;
        store.audit(None, "cloud_deprovision_failed", json!({ "name": rec.name, "error": d.evidence() }));
        bail!("`tofu destroy` failed in {} (resources may still exist and bill): {}", dir.display(), d.evidence());
    }
    sshcfg::remove_alias(&managed_ssh_config(ctx), &rec.ssh_alias)?;
    // The instance is gone: remove the (now empty) state and the private key.
    let _ = std::fs::remove_dir_all(&dir);
    store.update(|st| {
        remove_record(st, &rec.name);
        Ok(())
    })?;
    store.audit(None, "cloud_deprovision", json!({ "name": rec.name, "instance_id": rec.instance_id }));
    Ok(())
}

/// AWS provisioner registered in `Ctx::provisioners`.
#[derive(Debug, Default)]
pub struct AwsProvisioner;

impl Provisioner for AwsProvisioner {
    fn provider(&self) -> &'static str {
        "aws"
    }

    fn offers(&self, ctx: &Ctx) -> Result<Vec<Offer>> {
        let state = ctx.store().load()?;
        let offers = catalog::offers_from_state(&state);
        if offers.is_empty() || !auth::check(&exec::aws_bin()).is_configured() {
            return Ok(Vec::new());
        }
        Ok(offers)
    }

    fn provision(&self, ctx: &Ctx, offer_id: &str, name: &str) -> Result<Target> {
        provision_with(ctx, &ProvisionEnv::from_env()?, offer_id, name)
    }

    fn deprovision(&self, ctx: &Ctx, target: &Target) -> Result<()> {
        deprovision_with(ctx, &ProvisionEnv::from_env()?, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, CatalogEntry, InstanceType};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ltn-cloud-prov-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(windows)]
    fn seed_catalog(ctx: &Ctx) {
        let cat = Catalog {
            region: "us-east-1".into(),
            fetched_at: Utc::now(),
            families: vec!["t3".into()],
            entries: vec![CatalogEntry {
                spec: InstanceType {
                    instance_type: "t3.small".into(),
                    vcpus: 2,
                    memory_mib: 2048,
                    gpu_count: 0,
                    gpu_vram_mib: 0,
                    gpu_name: None,
                },
                usd_per_hour: "0.0208000000".into(),
                cents_per_hour: 3,
            }],
            unpriced: vec![],
        };
        ctx.store().update(|st| catalog::store_catalog(st, &cat)).unwrap();
    }

    #[cfg(windows)]
    /// Writes fake `.cmd` binaries into `dir`. Nothing here contacts any network.
    fn fakes(dir: &Path) -> Bins {
        let w = |n: &str, body: &str| {
            let p = dir.join(n);
            std::fs::write(&p, format!("@echo off\r\n{body}")).unwrap();
            p.to_string_lossy().into_owned()
        };
        let aws = w(
            "aws.cmd",
            "echo {\"UserId\":\"FAKE\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:iam::123456789012:user/fake\"}\r\n",
        );
        let curl = w("curl.cmd", "echo 203.0.113.7\r\n");
        let keygen = w(
            "ssh-keygen.cmd",
            "echo FAKE PRIVATE KEY>\"%~2\"\r\necho ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKey ltn>\"%~2.pub\"\r\n",
        );
        std::fs::write(
            dir.join("outputs.json"),
            r#"{"instance_id":{"sensitive":false,"type":"string","value":"i-0fake1234"},"public_ip":{"sensitive":false,"type":"string","value":"198.51.100.20"}}"#,
        )
        .unwrap();
        let tofu = w(
            "tofu.cmd",
            "echo %1>>\"%~dp0tofu.log\"\r\n\
             if \"%1\"==\"apply\" if exist \"%~dp0fail_apply\" (echo apply exploded 1>&2 & exit /b 1)\r\n\
             if \"%1\"==\"destroy\" if exist \"%~dp0fail_destroy\" (echo destroy exploded 1>&2 & exit /b 1)\r\n\
             if \"%1\"==\"output\" type \"%~dp0outputs.json\"\r\n\
             exit /b 0\r\n",
        );
        let docker = w(
            "docker.cmd",
            "echo %*>>\"%~dp0docker.log\"\r\nif exist \"%~dp0fail_docker\" (echo ssh: connect refused 1>&2 & exit /b 1)\r\necho 27.3.1\r\n",
        );
        Bins { aws, tofu: Some(tofu), ssh_keygen: keygen, curl, docker: Some(docker) }
    }

    #[cfg(windows)]
    fn env_for(dir: &Path) -> ProvisionEnv {
        ProvisionEnv {
            bins: fakes(dir),
            allow_spend: true,
            user_ssh_config: dir.join("user-ssh").join("config"),
            docker_wait: Duration::from_millis(300),
            docker_poll: Duration::from_millis(100),
            app_ports: hcl::DEFAULT_APP_PORTS.to_vec(),
            root_volume_gb: 20,
        }
    }

    #[cfg(windows)]
    fn tofu_log(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("tofu.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .collect()
    }

    #[test]
    fn refuses_without_spend_switch_via_trait() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var_os(SPEND_ENV);
        std::env::remove_var(SPEND_ENV);
        let home = tmp();
        let ctx = Ctx::for_home(&home);
        let err = AwsProvisioner.provision(&ctx, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        if let Some(v) = prev {
            std::env::set_var(SPEND_ENV, v);
        }
        assert!(err.contains("LTN_ALLOW_CLOUD_SPEND=1"), "{err}");
        assert!(!home.join("cloud").exists(), "nothing may be written before the spend check");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    #[cfg(windows)] // fake binaries are .cmd scripts
    fn refuses_without_credentials_or_tofu() {
        let dir = tmp();
        let ctx = Ctx::for_home(&dir.join("home"));
        seed_catalog(&ctx);
        let mut env = env_for(&dir);
        env.bins.aws = dir.join("no-such-aws.exe").to_string_lossy().into_owned();
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("credentials are not configured"), "{err}");

        let mut env = env_for(&dir);
        env.bins.tofu = None;
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("OpenTofu not found"), "{err}");
        assert!(tofu_log(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)] // fake binaries are .cmd scripts
    fn refuses_unknown_offer_and_undetectable_ip() {
        let dir = tmp();
        let ctx = Ctx::for_home(&dir.join("home"));
        let env = env_for(&dir);
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("ltn_cloud_catalog"), "{err}");
        seed_catalog(&ctx);
        let err = provision_with(&ctx, &env, "aws:m7i.large@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("not in the cached"), "{err}");

        std::fs::write(dir.join("curl.cmd"), "@echo off\r\necho curl: (6) Could not resolve host 1>&2\r\nexit /b 6\r\n").unwrap();
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("Refusing to open SSH to 0.0.0.0/0"), "{err}");
        assert!(tofu_log(&dir).is_empty(), "tofu must not run without an SSH CIDR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)] // fake binaries are .cmd scripts
    fn full_sequence_with_fake_binaries_then_deprovision() {
        let dir = tmp();
        let home = dir.join("home");
        let ctx = Ctx::for_home(&home);
        seed_catalog(&ctx);
        let env = env_for(&dir);
        let t = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap();
        assert_eq!(t.id, "web");
        assert_eq!(t.docker_host.as_deref(), Some("ssh://ltn-web"));
        assert_eq!(t.address, "198.51.100.20");
        assert_eq!(t.cost_cents_per_hour, 3);
        assert_eq!(t.provider, "aws");
        assert_eq!(t.labels["instance_id"], "i-0fake1234");
        assert_eq!(t.labels["region"], "us-east-1");
        assert_eq!(t.labels["instance_type"], "t3.small");
        assert!(t.labels["module_dir"].ends_with("web"));
        assert_eq!(tofu_log(&dir), vec!["init", "apply", "output"]);

        let main_tf = std::fs::read_to_string(home.join("cloud/web/main.tf")).unwrap();
        assert!(main_tf.contains("203.0.113.7/32") && main_tf.contains("AAAAIFakeKey"));
        let docker_log = std::fs::read_to_string(dir.join("docker.log")).unwrap();
        assert!(docker_log.contains("-H ssh://ltn-web info"), "{docker_log}");

        let managed = std::fs::read_to_string(home.join("ssh_config")).unwrap();
        assert!(managed.contains("Host ltn-web") && managed.contains("HostName 198.51.100.20"));
        // ltn's ssh wrapper reads ltn's own config; the user's is never written.
        assert!(!env.user_ssh_config.exists(), "the user's ssh config must not be touched");

        let recs = infra_records(&ctx.store().load().unwrap());
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, "ready");
        assert_eq!(recs[0].public_ip.as_deref(), Some("198.51.100.20"));

        // Same name again is refused.
        assert!(provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").is_err());

        deprovision_with(&ctx, &env, &t).unwrap();
        assert_eq!(tofu_log(&dir).last().map(String::as_str), Some("destroy"));
        assert!(infra_records(&ctx.store().load().unwrap()).is_empty());
        assert!(!std::fs::read_to_string(home.join("ssh_config")).unwrap().contains("ltn-web"));
        assert!(!home.join("cloud/web").exists(), "module dir and private key are removed after destroy");
        // The name is reusable after deprovision.
        let again = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap();
        assert_eq!(again.id, "web");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)] // fake binaries are .cmd scripts
    fn failed_apply_runs_destroy_and_reports_cleanup() {
        let dir = tmp();
        let ctx = Ctx::for_home(&dir.join("home"));
        seed_catalog(&ctx);
        std::fs::write(dir.join("fail_apply"), "").unwrap();
        let env = env_for(&dir);
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("`tofu apply` failed") && err.contains("apply exploded"), "{err}");
        assert!(err.contains("cleanup succeeded"), "{err}");
        assert_eq!(tofu_log(&dir), vec!["init", "apply", "destroy"]);
        assert!(infra_records(&ctx.store().load().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(windows)] // fake binaries are .cmd scripts
    fn docker_timeout_destroys_and_failed_cleanup_is_recorded() {
        let dir = tmp();
        let ctx = Ctx::for_home(&dir.join("home"));
        seed_catalog(&ctx);
        std::fs::write(dir.join("fail_docker"), "").unwrap();
        std::fs::write(dir.join("fail_destroy"), "").unwrap();
        let env = env_for(&dir);
        let err = provision_with(&ctx, &env, "aws:t3.small@us-east-1", "web").unwrap_err().to_string();
        assert!(err.contains("did not become reachable") && err.contains("connect refused"), "{err}");
        assert!(err.contains("CLEANUP FAILED") && err.contains("destroy exploded"), "{err}");
        assert_eq!(tofu_log(&dir), vec!["init", "apply", "output", "destroy"]);
        let recs = infra_records(&ctx.store().load().unwrap());
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, "cleanup_failed");
        assert!(!std::fs::read_to_string(dir.join("home/ssh_config")).unwrap().contains("Host ltn-web"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outputs_parser() {
        let (id, ip) = parse_outputs(
            r#"{"instance_id":{"sensitive":false,"type":"string","value":"i-1"},"public_ip":{"sensitive":false,"type":"string","value":"198.51.100.1"}}"#,
        )
        .unwrap();
        assert_eq!((id.as_str(), ip.as_str()), ("i-1", "198.51.100.1"));
        assert!(parse_outputs(r#"{"instance_id":{"value":"i-1"},"public_ip":{"value":""}}"#).is_err());
        assert!(parse_outputs("{}").is_err());
    }

    #[test]
    fn offers_empty_without_cache() {
        let home = tmp();
        let ctx = Ctx::for_home(&home);
        assert!(AwsProvisioner.offers(&ctx).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }
}
