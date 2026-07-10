use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupInfo {
    pub path: String,
    pub device_name: Option<String>,
    pub product_version: Option<String>,
    pub encrypted: Option<bool>,
}
