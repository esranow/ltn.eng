use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardPeer {
    pub name: String,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardInterface {
    pub private_key: String,
    pub address: String,
    pub listen_port: u16,
    pub peers: Vec<WireGuardPeer>,
}

impl WireGuardInterface {
    pub fn new(address: &str, listen_port: u16, private_key: &str) -> Self {
        Self {
            address: address.to_string(),
            listen_port,
            private_key: private_key.to_string(),
            peers: Vec::new(),
        }
    }

    pub fn add_peer(&mut self, peer: WireGuardPeer) {
        self.peers.push(peer);
    }

    pub fn to_wg_conf(&self) -> String {
        let mut conf = String::new();
        conf.push_str("[Interface]\n");
        conf.push_str(&format!("Address = {}\n", self.address));
        conf.push_str(&format!("ListenPort = {}\n", self.listen_port));
        conf.push_str(&format!("PrivateKey = {}\n\n", self.private_key));

        for p in &self.peers {
            conf.push_str(&format!("# Peer: {}\n", p.name));
            conf.push_str("[Peer]\n");
            conf.push_str(&format!("PublicKey = {}\n", p.public_key));
            conf.push_str(&format!("AllowedIPs = {}\n", p.allowed_ips));
            if let Some(ref ep) = p.endpoint {
                conf.push_str(&format!("Endpoint = {}\n", ep));
            }
            conf.push_str("PersistentKeepalive = 25\n\n");
        }
        conf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wireguard_conf_generation() {
        let mut iface = WireGuardInterface::new("10.42.0.1/24", 51820, "PRIVATE_KEY_123=");
        iface.add_peer(WireGuardPeer {
            name: "worker-node-1".to_string(),
            public_key: "PUBKEY_WORKER_1=".to_string(),
            endpoint: Some("203.0.113.10:51820".to_string()),
            allowed_ips: "10.42.0.2/32".to_string(),
        });

        let conf = iface.to_wg_conf();
        assert!(conf.contains("[Interface]"));
        assert!(conf.contains("Address = 10.42.0.1/24"));
        assert!(conf.contains("PublicKey = PUBKEY_WORKER_1="));
        assert!(conf.contains("Endpoint = 203.0.113.10:51820"));
    }
}
