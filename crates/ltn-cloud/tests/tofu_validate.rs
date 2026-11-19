//! Validates the generated module with a real OpenTofu binary. The only
//! network contact is `tofu init` downloading the hashicorp/aws provider;
//! no AWS API is called and nothing is applied.

use ltn_cloud::hcl::{render_module, ModuleParams, DEFAULT_APP_PORTS, DEFAULT_ROOT_VOLUME_GB};
use std::process::Command;

fn run(tofu: &str, dir: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(tofu)
        .args(args)
        .current_dir(dir)
        // Never pick up real credentials or remote state, even by accident.
        .env("AWS_ACCESS_KEY_ID", "")
        .env("AWS_SECRET_ACCESS_KEY", "")
        .env("AWS_PROFILE", "ltn-validate-nonexistent")
        .env("TF_IN_AUTOMATION", "1")
        .output()
        .unwrap_or_else(|e| panic!("could not run {tofu}: {e}"));
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
#[ignore = "requires OpenTofu (set LTN_TOFU_BIN) and network access"]
fn generated_module_passes_tofu_validate() {
    let tofu = std::env::var("LTN_TOFU_BIN").expect("set LTN_TOFU_BIN to an OpenTofu binary");
    let dir = std::env::temp_dir().join(format!("ltn-cloud-validate-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let hcl = render_module(&ModuleParams {
        target_name: "validate-1".into(),
        unique_suffix: "0a1b2c3d".into(),
        region: "us-east-1".into(),
        instance_type: "t3.small".into(),
        public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ0e7k3sZ8t1o5vUu0c2b8Xo8m0h6l4p1q2r3s4t5u6v ltn-validate".into(),
        ssh_cidr: "203.0.113.7/32".into(),
        app_ports: DEFAULT_APP_PORTS.to_vec(),
        root_volume_gb: DEFAULT_ROOT_VOLUME_GB,
    })
    .unwrap();
    std::fs::write(dir.join("main.tf"), &hcl).unwrap();

    let (ok, text) = run(&tofu, &dir, &["init", "-backend=false", "-input=false", "-no-color"]);
    assert!(ok, "tofu init failed:\n{text}");
    let (ok, text) = run(&tofu, &dir, &["validate", "-no-color"]);
    println!("tofu validate:\n{text}");
    assert!(ok, "tofu validate failed:\n{text}\n--- main.tf ---\n{hcl}");
    assert!(text.contains("Success"), "{text}");
    let (ok, text) = run(&tofu, &dir, &["fmt", "-check", "-diff", "-no-color"]);
    assert!(ok, "generated HCL is not canonically formatted:\n{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
