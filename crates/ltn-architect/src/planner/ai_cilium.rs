use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct L7SecurityRule {
    pub source_service: String,
    pub target_service: String,
    pub target_port: u16,
    pub protocol: String, // "TCP" | "UDP"
    pub allowed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZeroTrustNetworkContract {
    pub ingress_rules: Vec<L7SecurityRule>,
    pub egress_rules: Vec<L7SecurityRule>,
    pub wireguard_mesh_peer: String,
    pub wireguard_overlay_cidr: String,
    pub ebpf_filter_bytecode_spec: String,
    pub nftables_ruleset: String,
}

pub struct AiCiliumPlanner;

impl AiCiliumPlanner {
    /// Synthesizes zero-trust L7 microsegmentation and eBPF/WireGuard rules directly from code AST
    pub fn synthesize_network_contract(
        service_name: &str,
        exposed_port: u16,
        detected_egress_dependencies: &[String],
    ) -> ZeroTrustNetworkContract {
        let mut egress_rules = Vec::new();

        for dep in detected_egress_dependencies {
            let port = if dep.contains("postgres") || dep.contains("5432") {
                5432
            } else if dep.contains("redis") || dep.contains("6379") {
                6379
            } else if dep.contains("vllm") || dep.contains("8000") {
                8000
            } else {
                8080
            };

            egress_rules.push(L7SecurityRule {
                source_service: service_name.to_string(),
                target_service: dep.clone(),
                target_port: port,
                protocol: "TCP".to_string(),
                allowed_paths: vec!["/*".to_string()],
            });
        }

        let ingress_rules = vec![L7SecurityRule {
            source_service: "external_gateway".to_string(),
            target_service: service_name.to_string(),
            target_port: exposed_port,
            protocol: "TCP".to_string(),
            allowed_paths: vec![
                "/health".to_string(),
                "/api/*".to_string(),
                "/metrics".to_string(),
            ],
        }];

        // Deterministic WireGuard overlay CIDR allocation (e.g. 10.42.0.X/32)
        let hash = service_name
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let host_id = (hash % 254) + 1;
        let wireguard_overlay_cidr = format!("10.42.0.{}/32", host_id);

        let nftables = format!(
            "table inet ltn_filter {{\n  chain input {{\n    type filter hook input priority 0; policy drop;\n    tcp dport {} accept\n    iif lo accept\n  }}\n}}",
            exposed_port
        );

        let ebpf_spec = format!(
            "SEC(\"sockops\") int ltn_sock_filter(struct bpf_sock_ops *skops) {{\n    if (skops->remote_port == {0} || skops->local_port == {0}) {{\n        return 1; // redirect/accept socket traffic on port {0}\n    }}\n    return 0; // filter/drop unauthorized traffic\n}}",
            exposed_port
        );

        ZeroTrustNetworkContract {
            ingress_rules,
            egress_rules,
            wireguard_mesh_peer: format!("wg-{}.mesh.ltn.internal", service_name),
            wireguard_overlay_cidr,
            ebpf_filter_bytecode_spec: ebpf_spec,
            nftables_ruleset: nftables,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthesize_network_contract_wireguard_overlay_cidr() {
        let contract1 = AiCiliumPlanner::synthesize_network_contract("auth-service", 8080, &[]);
        let contract2 = AiCiliumPlanner::synthesize_network_contract("auth-service", 8080, &[]);
        let contract3 = AiCiliumPlanner::synthesize_network_contract("payment-service", 8080, &[]);

        // Determinism: same service name must yield identical overlay CIDR
        assert_eq!(
            contract1.wireguard_overlay_cidr,
            contract2.wireguard_overlay_cidr
        );

        // CIDR format check (10.42.0.X/32)
        assert!(
            contract1.wireguard_overlay_cidr.starts_with("10.42.0."),
            "Expected 10.42.0.X/32, got: {}",
            contract1.wireguard_overlay_cidr
        );
        assert!(
            contract1.wireguard_overlay_cidr.ends_with("/32"),
            "Expected /32 suffix, got: {}",
            contract1.wireguard_overlay_cidr
        );

        // Extract host octet and check valid IPv4 host range (1..=254)
        let octet_str = contract1
            .wireguard_overlay_cidr
            .strip_prefix("10.42.0.")
            .unwrap()
            .strip_suffix("/32")
            .unwrap();
        let octet: u32 = octet_str.parse().expect("valid host octet number");
        assert!(
            (1..=254).contains(&octet),
            "Host octet {} out of range",
            octet
        );

        // Peer mesh hostname check
        assert_eq!(
            contract1.wireguard_mesh_peer,
            "wg-auth-service.mesh.ltn.internal"
        );
        assert_eq!(
            contract3.wireguard_mesh_peer,
            "wg-payment-service.mesh.ltn.internal"
        );
    }

    #[test]
    fn test_synthesize_network_contract_ebpf_spec() {
        let exposed_port = 8443;
        let contract = AiCiliumPlanner::synthesize_network_contract("api-gw", exposed_port, &[]);

        assert!(
            contract
                .ebpf_filter_bytecode_spec
                .contains("SEC(\"sockops\")"),
            "eBPF spec should declare sockops section"
        );
        assert!(
            contract
                .ebpf_filter_bytecode_spec
                .contains(&format!("remote_port == {}", exposed_port)),
            "eBPF spec should filter on exposed remote port {}",
            exposed_port
        );
        assert!(
            contract
                .ebpf_filter_bytecode_spec
                .contains(&format!("local_port == {}", exposed_port)),
            "eBPF spec should filter on exposed local port {}",
            exposed_port
        );
        assert!(
            contract.ebpf_filter_bytecode_spec.contains("return 1"),
            "eBPF spec should redirect/accept matching traffic"
        );
        assert!(
            contract.ebpf_filter_bytecode_spec.contains("return 0"),
            "eBPF spec should drop non-matching traffic"
        );
    }

    #[test]
    fn test_synthesize_network_contract_nftables() {
        let exposed_port = 9090;
        let contract =
            AiCiliumPlanner::synthesize_network_contract("metrics-srv", exposed_port, &[]);

        assert!(
            contract.nftables_ruleset.contains("table inet ltn_filter"),
            "nftables ruleset should declare ltn_filter table"
        );
        assert!(
            contract.nftables_ruleset.contains("policy drop;"),
            "nftables ruleset should default to policy drop (zero-trust)"
        );
        assert!(
            contract.nftables_ruleset.contains("iif lo accept"),
            "nftables ruleset should accept loopback interface traffic"
        );
        assert!(
            contract
                .nftables_ruleset
                .contains(&format!("tcp dport {} accept", exposed_port)),
            "nftables ruleset should accept exposed port {}",
            exposed_port
        );
    }

    #[test]
    fn test_synthesize_network_contract_dependencies_and_rules() {
        let deps = vec![
            "postgres-db".to_string(),
            "redis-cache".to_string(),
            "vllm-inference".to_string(),
            "custom-worker".to_string(),
        ];
        let contract = AiCiliumPlanner::synthesize_network_contract("orchestrator", 3000, &deps);

        assert_eq!(contract.ingress_rules.len(), 1);
        assert_eq!(contract.ingress_rules[0].target_port, 3000);
        assert_eq!(contract.ingress_rules[0].source_service, "external_gateway");

        assert_eq!(contract.egress_rules.len(), 4);
        assert_eq!(contract.egress_rules[0].target_port, 5432); // postgres
        assert_eq!(contract.egress_rules[1].target_port, 6379); // redis
        assert_eq!(contract.egress_rules[2].target_port, 8000); // vllm
        assert_eq!(contract.egress_rules[3].target_port, 8080); // custom
    }

    #[test]
    fn test_zero_trust_network_contract_partial_eq_and_serde() {
        let deps = vec!["redis".to_string()];
        let contract = AiCiliumPlanner::synthesize_network_contract("srv", 8080, &deps);
        let serialized = serde_json::to_string(&contract).expect("must serialize");
        let deserialized: ZeroTrustNetworkContract =
            serde_json::from_str(&serialized).expect("must deserialize");

        assert_eq!(contract, deserialized);
        assert_eq!(
            deserialized.wireguard_overlay_cidr,
            contract.wireguard_overlay_cidr
        );
        assert_eq!(
            deserialized.wireguard_mesh_peer,
            contract.wireguard_mesh_peer
        );
        assert_eq!(
            deserialized.ebpf_filter_bytecode_spec,
            contract.ebpf_filter_bytecode_spec
        );
        assert_eq!(deserialized.nftables_ruleset, contract.nftables_ruleset);
    }
}
