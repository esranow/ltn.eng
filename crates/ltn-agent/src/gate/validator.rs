use ltn_core::ir::{ClusterState, Intent};
use ltn_core::solver::{ExecManifest, SmtSolver, SolverError};

pub struct DeterministicGate;

impl DeterministicGate {
    pub fn validate_mutation_plan(
        intent: &Intent,
        cluster: &ClusterState,
    ) -> Result<ExecManifest, SolverError> {
        // Enforce strict Z3 SMT solving validation
        SmtSolver::solve(intent, cluster)
    }

    pub fn is_mutation_admissible(intent: &Intent, cluster: &ClusterState) -> bool {
        Self::validate_mutation_plan(intent, cluster).is_ok()
    }
}
