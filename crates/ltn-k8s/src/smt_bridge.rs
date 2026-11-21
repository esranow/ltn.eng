use anyhow::Result;
use z3::ast::{Bool, Int};
use z3::{Config, Context, SatResult, Solver};

#[derive(Debug, Clone)]
pub struct K8sNodeCapacity {
    pub name: String,
    pub allocatable_cpu_milli: i64,
    pub allocatable_ram_mb: i64,
    pub gpu_count: i64,
}

pub struct K8sPlacementSolver;

impl K8sPlacementSolver {
    pub fn verify_node_fits(
        node: &K8sNodeCapacity,
        required_cpu_milli: i64,
        required_ram_mb: i64,
        required_gpu: i64,
    ) -> Result<bool> {
        let cfg = Config::new();
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);

        let node_cpu = Int::from_i64(&ctx, node.allocatable_cpu_milli);
        let node_ram = Int::from_i64(&ctx, node.allocatable_ram_mb);
        let node_gpu = Int::from_i64(&ctx, node.gpu_count);

        let req_cpu = Int::from_i64(&ctx, required_cpu_milli);
        let req_ram = Int::from_i64(&ctx, required_ram_mb);
        let req_gpu = Int::from_i64(&ctx, required_gpu);

        let cpu_fits = node_cpu.ge(&req_cpu);
        let ram_fits = node_ram.ge(&req_ram);
        let gpu_fits = node_gpu.ge(&req_gpu);

        let all_fit = Bool::and(&ctx, &[&cpu_fits, &ram_fits, &gpu_fits]);
        solver.assert(&all_fit);

        Ok(solver.check() == SatResult::Sat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_smt_capacity_check() {
        let node = K8sNodeCapacity {
            name: "eks-node-pool-1".to_string(),
            allocatable_cpu_milli: 4000,
            allocatable_ram_mb: 16384,
            gpu_count: 1,
        };

        // should fit
        assert!(K8sPlacementSolver::verify_node_fits(&node, 2000, 8192, 1).unwrap());

        // excessive RAM fails
        assert!(!K8sPlacementSolver::verify_node_fits(&node, 2000, 32768, 1).unwrap());

        // excessive GPU fails
        assert!(!K8sPlacementSolver::verify_node_fits(&node, 1000, 2048, 2).unwrap());
    }
}
