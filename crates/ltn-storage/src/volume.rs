use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VolumeType {
    LocalDirectory,
    BindMount { host_path: String },
    PersistentVolumeClaim { pvc_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeSpec {
    pub name: String,
    pub mount_path: String,
    pub size_gib: u32,
    pub volume_type: VolumeType,
    pub read_only: bool,
}

impl VolumeSpec {
    pub fn new_local_dir(name: &str, mount_path: &str, size_gib: u32) -> Self {
        Self {
            name: name.to_string(),
            mount_path: mount_path.to_string(),
            size_gib,
            volume_type: VolumeType::LocalDirectory,
            read_only: false,
        }
    }

    pub fn new_pvc(name: &str, mount_path: &str, size_gib: u32, pvc_name: &str) -> Self {
        Self {
            name: name.to_string(),
            mount_path: mount_path.to_string(),
            size_gib,
            volume_type: VolumeType::PersistentVolumeClaim {
                pvc_name: pvc_name.to_string(),
            },
            read_only: false,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Volume name cannot be empty".to_string());
        }
        if !self.mount_path.starts_with('/') {
            return Err(format!(
                "Mount path must be absolute (got '{}')",
                self.mount_path
            ));
        }
        if self.size_gib == 0 {
            return Err("Volume size must be greater than 0 GiB".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_validation() {
        let valid = VolumeSpec::new_local_dir("data", "/var/lib/data", 20);
        assert!(valid.validate().is_ok());

        let relative = VolumeSpec::new_local_dir("data", "relative/path", 10);
        assert!(relative.validate().is_err());

        let zero_size = VolumeSpec::new_local_dir("data", "/mnt", 0);
        assert!(zero_size.validate().is_err());
    }
}
