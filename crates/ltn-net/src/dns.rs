use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DnsRecordType {
    A,
    AAAA,
    CNAME,
    TXT,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    pub record_type: DnsRecordType,
    pub name: String,
    pub content: String,
    pub ttl: u32,
    pub proxied: bool,
}

impl DnsRecord {
    pub fn a(name: &str, ip: &str) -> Self {
        Self {
            record_type: DnsRecordType::A,
            name: name.to_string(),
            content: ip.to_string(),
            ttl: 300,
            proxied: true,
        }
    }

    pub fn cname(name: &str, target: &str) -> Self {
        Self {
            record_type: DnsRecordType::CNAME,
            name: name.to_string(),
            content: target.to_string(),
            ttl: 300,
            proxied: true,
        }
    }

    pub fn to_cloudflare_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": format!("{:?}", self.record_type),
            "name": self.name,
            "content": self.content,
            "ttl": self.ttl,
            "proxied": self.proxied
        })
    }

    pub fn to_hosts_entry(&self) -> Option<String> {
        if self.record_type == DnsRecordType::A {
            Some(format!("{}\t{}", self.content, self.name))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_records() {
        let rec = DnsRecord::a("api.test.com", "192.168.1.50");
        let json = rec.to_cloudflare_json();
        assert_eq!(json["name"], "api.test.com");
        assert_eq!(json["type"], "A");
        assert_eq!(json["content"], "192.168.1.50");

        let entry = rec.to_hosts_entry().unwrap();
        assert_eq!(entry, "192.168.1.50\tapi.test.com");
    }
}
