use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Server {
    pub id: Option<i64>,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub username: String,
    pub auth_type: String, // "key", "password", "pem"
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    pub password_hash: Option<String>,
    pub password_saved: bool,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub is_favorite: bool,
    pub notes: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServerDto {
    pub name: String,
    pub host: String,
    pub port: Option<i32>,
    pub username: String,
    pub auth_type: String,
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    pub password: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateServerDto {
    pub id: i64,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub username: Option<String>,
    pub auth_type: Option<String>,
    pub key_id: Option<i64>,
    pub pem_data: Option<String>,
    pub password: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SshKey {
    pub id: Option<i64>,
    pub name: String,
    pub public_key: String,
    pub pem_data: Option<String>,
    pub key_type: String, // "ed25519", "rsa", "ecdsa", "dsa"
    pub key_size: i32,
    pub passphrase_protected: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyDto {
    pub name: String,
    pub key_type: String,
    pub key_size: Option<i32>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportKeyDto {
    pub name: String,
    pub public_key: String,
    pub private_key: Option<String>,
    pub pem_data: Option<String>,
    pub key_type: String,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfigEntry {
    pub id: Option<i64>,
    pub host_pattern: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<i32>,
    pub identity_file: Option<String>,
    pub forward_agent: Option<String>,
    pub local_forward: Option<String>,
    pub remote_forward: Option<String>,
    pub other_options: Option<String>,
    pub synced: bool,
    pub server_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSshResult {
    pub success: bool,
    pub message: String,
    pub needs_password: bool,
}