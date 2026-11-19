//! Free capacity of a target: what `docker info` reports on that host, minus
//! what the current releases of deployments living there reserve.

use anyhow::{anyhow, Result};
use ltn_core::topology::HostHardwareInventory;
use ltn_state::docker::DockerInfo;
use ltn_state::store::{State, Target};
use ltn_state::{Ctx, TargetCapacity};
use serde::Serialize;
use serde_json::{json, Value};

/// Resources held by current releases on one target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Reservation {
    pub cpu_millis: u64,
    pub ram_mb: u64,
    pub gpu_count: u64,
    pub vram_mb: u64,
}

/// Sum of the spec reservations of every deployment on `target_id` that has a
/// current release. `except_app` is left out: when an app is re-planned its
/// new release replaces the old one, so its reservation is added back.
pub fn reserved_on(state: &State, target_id: &str, except_app: Option<&str>) -> Reservation {
    let mut r = Reservation::default();
    for d in state.deployments.values() {
        if d.target_id != target_id || d.current.is_none() || Some(d.app.as_str()) == except_app {
            continue;
        }
        r.cpu_millis += d.spec.cpu_millis;
        r.ram_mb += d.spec.ram_mb;
        r.gpu_count += d.spec.gpu_count;
        r.vram_mb += d.spec.vram_mb;
    }
    r
}

/// Total capacity of a target from its docker info and GPU figures.
pub fn total_capacity(target: &Target, info: &DockerInfo, gpu: (u64, u64)) -> TargetCapacity {
    TargetCapacity {
        target_id: target.id.clone(),
        free_cpu_millis: info.cpus * 1000,
        free_ram_mb: info.memory_mb,
        free_gpu_count: gpu.0,
        free_vram_mb: gpu.1,
        cost_cents_per_hour: target.cost_cents_per_hour,
    }
}

pub fn free_capacity(total: &TargetCapacity, r: Reservation) -> TargetCapacity {
    TargetCapacity {
        target_id: total.target_id.clone(),
        free_cpu_millis: total.free_cpu_millis.saturating_sub(r.cpu_millis),
        free_ram_mb: total.free_ram_mb.saturating_sub(r.ram_mb),
        free_gpu_count: total.free_gpu_count.saturating_sub(r.gpu_count),
        free_vram_mb: total.free_vram_mb.saturating_sub(r.vram_mb),
        cost_cents_per_hour: total.cost_cents_per_hour,
    }
}

/// GPUs usable by containers on a target. The local daemon is probed with
/// nvidia-smi, but only counts when Docker has the nvidia runtime; remote
/// targets use the figures registered with the target.
pub fn target_gpus(target: &Target, info: &DockerInfo) -> (u64, u64) {
    if target.docker_host.is_none() {
        if !info.has_nvidia_runtime {
            return (0, 0);
        }
        let inv = HostHardwareInventory::probe_local_system();
        (inv.gpu.count as u64, inv.gpu.total_vram_mb)
    } else {
        (target.gpu_count, target.vram_mb)
    }
}

/// Talks to the target's Docker daemon; Err carries why it is unreachable.
pub fn probe_target(ctx: &Ctx, target: &Target) -> std::result::Result<(DockerInfo, TargetCapacity), String> {
    let info = ctx.docker(target).info().map_err(|e| e.to_string())?;
    let gpus = target_gpus(target, &info);
    let total = total_capacity(target, &info, gpus);
    Ok((info, total))
}

/// The JSON capacity block reported by the targets tools.
pub fn capacity_json(state: &State, target: &Target, info: &DockerInfo, total: &TargetCapacity) -> Value {
    let reserved = reserved_on(state, &target.id, None);
    json!({
        "free": free_capacity(total, reserved),
        "total": total,
        "reserved": reserved,
        "docker": info,
    })
}

/// Budget for the Z3 query: the spec's cap and the `LTN_MAX_HOURLY_CENTS`
/// env cap (passed in as its raw value), whichever is lower.
pub fn budget_cap(spec_max: u64, env_value: Option<&str>) -> Result<u64> {
    match env_value.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(spec_max),
        Some(raw) => {
            let cap: u64 = raw.parse().map_err(|_| {
                anyhow!("LTN_MAX_HOURLY_CENTS must be a whole number of cents per hour, got `{raw}`")
            })?;
            Ok(spec_max.min(cap))
        }
    }
}

pub fn env_budget() -> Option<String> {
    std::env::var("LTN_MAX_HOURLY_CENTS").ok()
}
