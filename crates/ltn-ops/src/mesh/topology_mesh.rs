use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshNode {
    pub node_id: String,
    pub node_type: String, // "BARE_METAL", "AWS_VPC", "GCP_VPC", "AZURE_VNET"
    pub virtual_ip: String,
    pub endpoint: String,
    pub public_key: String,
    pub is_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridMeshTopology {
    pub mesh_network_cidr: String,
    pub nodes: Vec<MeshNode>,
    pub wireguard_config_snippet: String,
}

pub struct MeshTopologyCoordinator;

impl MeshTopologyCoordinator {
    /// Federates bare-metal on-prem nodes and cloud VPCs into a unified private overlay mesh.
    pub fn coordinate_mesh(
        bare_metal_host: &str,
        has_gpu: bool,
        cloud_provider: &str,
    ) -> HybridMeshTopology {
        let mut nodes = Vec::new();

        // 1. On-Prem Bare Metal Node (High-Throughput / GPU Inference)
        nodes.push(MeshNode {
            node_id: format!("node-metal-{}", bare_metal_host),
            node_type: if has_gpu {
                "BARE_METAL_GPU".to_string()
            } else {
                "BARE_METAL_CPU".to_string()
            },
            virtual_ip: "10.42.0.1".to_string(),
            endpoint: "on-prem.mesh.internal:51820".to_string(),
            public_key: "kMetalMeshPubkeyX8921aZ=".to_string(),
            is_connected: true,
        });

        // 2. Cloud Ingress / Edge Node
        let cloud_ip = match cloud_provider.to_uppercase().as_str() {
            "AWS" => "10.42.1.1",
            "GCP" => "10.42.2.1",
            "AZURE" => "10.42.3.1",
            _ => "10.42.0.2",
        };

        nodes.push(MeshNode {
            node_id: format!("node-cloud-{}", cloud_provider.to_lowercase()),
            node_type: format!("{}_VPC", cloud_provider.to_uppercase()),
            virtual_ip: cloud_ip.to_string(),
            endpoint: format!(
                "{}.cloud.mesh.internal:51820",
                cloud_provider.to_lowercase()
            ),
            public_key: "kCloudMeshPubkeyY4102bW=".to_string(),
            is_connected: true,
        });

        let wg_config = format!(
            r#"[Interface]
PrivateKey = <EPHEMERAL_SECRET_KEY>
Address = 10.42.0.1/16
ListenPort = 51820

[Peer]
# {} Edge Node
PublicKey = kCloudMeshPubkeyY4102bW=
Endpoint = {}.cloud.mesh.internal:51820
AllowedIPs = 10.42.0.0/16
PersistentKeepalive = 25
"#,
            cloud_provider,
            cloud_provider.to_lowercase()
        );

        HybridMeshTopology {
            mesh_network_cidr: "10.42.0.0/16".to_string(),
            nodes,
            wireguard_config_snippet: wg_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_mesh_generation() {
        let mesh = MeshTopologyCoordinator::coordinate_mesh("workstation-gpu", true, "AWS");
        assert_eq!(mesh.mesh_network_cidr, "10.42.0.0/16");
        assert_eq!(mesh.nodes.len(), 2);
        assert_eq!(mesh.nodes[0].virtual_ip, "10.42.0.1");
        assert_eq!(mesh.nodes[0].node_type, "BARE_METAL_GPU");
        assert_eq!(mesh.nodes[1].virtual_ip, "10.42.1.1");
        assert!(mesh
            .wireguard_config_snippet
            .contains("AllowedIPs = 10.42.0.0/16"));
    }
}
