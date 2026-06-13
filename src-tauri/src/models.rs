use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub username: String,
    pub auth_type: String, // "key", "password", "pem", "agent"
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    /// `-J` jump host(s), e.g. "user@bastion" or "host1,host2". Optional.
    #[serde(default)]
    pub proxy_jump: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>, // JSON-encoded string array
    pub is_favorite: bool,
    pub notes: Option<String>,
    pub last_connected_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServerDto {
    pub name: String,
    pub host: String,
    pub port: Option<i32>,
    pub username: String,
    pub auth_type: String,
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    #[serde(default)]
    pub proxy_jump: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServerDto {
    pub id: i64,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub username: Option<String>,
    pub auth_type: Option<String>,
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    #[serde(default)]
    pub proxy_jump: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKey {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    pub pem_data: Option<String>,
    pub key_type: String, // "ed25519", "rsa", "ecdsa", "dsa"
    pub key_size: i32,
    pub passphrase_protected: bool,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateKeyDto {
    pub name: String,
    pub key_type: String,
    pub key_size: Option<i32>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportKeyDto {
    pub name: String,
    pub public_key: String,
    pub private_key: Option<String>,
    pub pem_data: Option<String>,
    pub key_type: String,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateKeyDto {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    pub key_type: String,
    /// When present, replaces the stored private key file.
    pub pem_data: Option<String>,
    pub passphrase: Option<String>,
}
