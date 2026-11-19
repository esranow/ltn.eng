//! Z3-backed placement of one workload onto one of several capacity targets.
//!
//! Ported from beam-core's `Z3PlacementSolver`: the target is chosen by a Z3
//! `Optimize` query (minimise cost, then maximise RAM left free), and when no
//! target fits, the conflicting constraints are reported from a real Z3 unsat
//! core rather than synthesised from a list of failures.

use serde::{Deserialize, Serialize};
use z3::ast::{Ast, Bool, Int};
use z3::{Config, Context, Optimize, SatResult, Solver};

/// Capacity still available on one deployment target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCapacity {
    pub target_id: String,
    pub free_cpu_millis: u64,
    pub free_ram_mb: u64,
    pub free_gpu_count: u64,
    pub free_vram_mb: u64,
    pub cost_cents_per_hour: u64,
}

/// What one workload needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementRequest {
    pub cpu_millis: u64,
    pub ram_mb: u64,
    pub gpu_count: u64,
    pub vram_mb: u64,
    pub max_cost_cents_per_hour: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementDecision {
    pub target_id: String,
    pub cost_cents_per_hour: u64,
    pub ram_mb_left_free: u64,
    /// The SMT-LIB2 query Z3 solved, kept so a plan can be audited or replayed.
    pub smt2: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementError {
    NoTargets,
    /// Z3 proved no target satisfies the request. `unsat_core` holds the
    /// labels of the constraints that conflict (e.g. `min_ram`, `budget`).
    Unsatisfiable { unsat_core: Vec<String> },
    SolverUnknown(String),
}

impl std::fmt::Display for PlacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlacementError::NoTargets => write!(f, "no deployment targets are configured"),
            PlacementError::Unsatisfiable { unsat_core } => write!(
                f,
                "no target can fit this workload; conflicting constraints: {}",
                unsat_core.join(", ")
            ),
            PlacementError::SolverUnknown(reason) => write!(f, "Z3 returned unknown: {reason}"),
        }
    }
}

impl std::error::Error for PlacementError {}

pub fn place(
    request: &PlacementRequest,
    targets: &[TargetCapacity],
) -> Result<PlacementDecision, PlacementError> {
    if targets.is_empty() {
        return Err(PlacementError::NoTargets);
    }

    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let selected = Int::new_const(&ctx, "selected_target");
    let zero = Int::from_u64(&ctx, 0);
    let len = Int::from_u64(&ctx, targets.len() as u64);
    let in_range = Bool::and(&ctx, &[&selected.ge(&zero), &selected.lt(&len)]);

    // Select each target's attributes through an if-then-else chain on `selected`.
    let pick = |f: &dyn Fn(&TargetCapacity) -> u64| {
        let mut ast = Int::from_u64(&ctx, 0);
        for (i, t) in targets.iter().enumerate() {
            let is_this = selected._eq(&Int::from_u64(&ctx, i as u64));
            ast = is_this.ite(&Int::from_u64(&ctx, f(t)), &ast);
        }
        ast
    };
    let cpu = pick(&|t| t.free_cpu_millis);
    let ram = pick(&|t| t.free_ram_mb);
    let gpu = pick(&|t| t.free_gpu_count);
    let vram = pick(&|t| t.free_vram_mb);
    let cost = pick(&|t| t.cost_cents_per_hour);

    let constraints: [(&str, Bool); 5] = [
        ("min_cpu", cpu.ge(&Int::from_u64(&ctx, request.cpu_millis))),
        ("min_ram", ram.ge(&Int::from_u64(&ctx, request.ram_mb))),
        ("min_gpu", gpu.ge(&Int::from_u64(&ctx, request.gpu_count))),
        ("min_vram", vram.ge(&Int::from_u64(&ctx, request.vram_mb))),
        ("budget", cost.le(&Int::from_u64(&ctx, request.max_cost_cents_per_hour))),
    ];

    let opt = Optimize::new(&ctx);
    opt.assert(&in_range);
    for (_, c) in &constraints {
        opt.assert(c);
    }
    opt.minimize(&cost);
    opt.maximize(&ram);
    let smt2 = opt.to_string();

    match opt.check(&[]) {
        SatResult::Sat => {
            let model = opt
                .get_model()
                .ok_or_else(|| PlacementError::SolverUnknown("SAT without a model".into()))?;
            let idx = model
                .eval(&selected, true)
                .and_then(|v| v.as_u64())
                .ok_or_else(|| PlacementError::SolverUnknown("unreadable model".into()))?
                as usize;
            let chosen = &targets[idx];
            Ok(PlacementDecision {
                target_id: chosen.target_id.clone(),
                cost_cents_per_hour: chosen.cost_cents_per_hour,
                ram_mb_left_free: chosen.free_ram_mb - request.ram_mb,
                smt2,
            })
        }
        SatResult::Unsat => {
            // Optimize has no unsat-core API in these bindings, so re-check the
            // same constraints with tracked labels on a plain Solver.
            let solver = Solver::new(&ctx);
            solver.assert(&in_range);
            for (label, c) in &constraints {
                solver.assert_and_track(c, &Bool::new_const(&ctx, *label));
            }
            let _ = solver.check();
            let unsat_core = solver
                .get_unsat_core()
                .iter()
                .map(|b| b.to_string().trim_matches('|').to_string())
                .collect();
            Err(PlacementError::Unsatisfiable { unsat_core })
        }
        SatResult::Unknown => Err(PlacementError::SolverUnknown(
            opt.get_reason_unknown().unwrap_or_else(|| "no reason given".into()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, cpu: u64, ram: u64, cost: u64) -> TargetCapacity {
        TargetCapacity {
            target_id: id.into(),
            free_cpu_millis: cpu,
            free_ram_mb: ram,
            free_gpu_count: 0,
            free_vram_mb: 0,
            cost_cents_per_hour: cost,
        }
    }

    fn request(cpu: u64, ram: u64) -> PlacementRequest {
        PlacementRequest {
            cpu_millis: cpu,
            ram_mb: ram,
            gpu_count: 0,
            vram_mb: 0,
            max_cost_cents_per_hour: u64::MAX,
        }
    }

    #[test]
    fn picks_cheapest_target_that_fits() {
        let targets = [
            target("big-expensive", 8000, 32000, 90),
            target("small-cheap", 1000, 2000, 10),
            target("too-small", 100, 128, 0),
        ];
        let d = place(&request(500, 1024), &targets).unwrap();
        assert_eq!(d.target_id, "small-cheap");
        assert_eq!(d.ram_mb_left_free, 976);
        assert!(d.smt2.contains("selected_target"));
    }

    #[test]
    fn breaks_cost_ties_by_most_free_ram() {
        let targets = [target("a", 4000, 4000, 0), target("b", 4000, 16000, 0)];
        assert_eq!(place(&request(500, 1024), &targets).unwrap().target_id, "b");
    }

    #[test]
    fn unsat_core_names_the_constraint_that_does_not_fit() {
        let targets = [target("a", 8000, 2048, 0), target("b", 8000, 1024, 0)];
        let err = place(&request(500, 4096), &targets).unwrap_err();
        match err {
            PlacementError::Unsatisfiable { unsat_core } => {
                assert!(unsat_core.contains(&"min_ram".to_string()), "{unsat_core:?}");
                assert!(!unsat_core.contains(&"min_cpu".to_string()), "{unsat_core:?}");
            }
            other => panic!("expected unsat, got {other:?}"),
        }
    }

    #[test]
    fn budget_cap_is_enforced() {
        let targets = [target("pricey", 8000, 8000, 500)];
        let mut req = request(100, 100);
        req.max_cost_cents_per_hour = 100;
        match place(&req, &targets).unwrap_err() {
            PlacementError::Unsatisfiable { unsat_core } => {
                assert_eq!(unsat_core, vec!["budget".to_string()])
            }
            other => panic!("expected unsat, got {other:?}"),
        }
    }

    #[test]
    fn empty_target_list_is_an_error() {
        assert_eq!(place(&request(1, 1), &[]).unwrap_err(), PlacementError::NoTargets);
    }
}
