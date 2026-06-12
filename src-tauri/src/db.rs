use sqlx::{SqlitePool, Row};
use chrono::Utc;
use crate::models::*;
use std::path::PathBuf;
use tauri::AppHandle;

pub async fn get_database(app: &AppHandle) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let db_path = get_db_path(app)?;
    let pool = SqlitePool::connect(&format!("sqlite://{}", db_path.display())).await?;
    init_database(&pool).await?;
    Ok(pool)
}

fn get_db_path(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let data_dir = app.path().data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    Ok(data_dir.join("sshub.db"))
}

async fn init_database(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS servers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            host TEXT NOT NULL,
            port INTEGER NOT NULL DEFAULT 22,
            username TEXT NOT NULL,
            auth_type TEXT NOT NULL DEFAULT 'key',
            key_id INTEGER,
            pem_data TEXT,
            password_hash TEXT,
            password_saved INTEGER NOT NULL DEFAULT 0,
            group_name TEXT,
            tags TEXT,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_keys (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            public_key TEXT NOT NULL,
            pem_data TEXT,
            key_type TEXT NOT NULL DEFAULT 'ed25519',
            key_size INTEGER NOT NULL DEFAULT 256,
            passphrase_protected INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ssh_config_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host_pattern TEXT NOT NULL,
            hostname TEXT,
            user TEXT,
            port INTEGER,
            identity_file TEXT,
            forward_agent TEXT,
            local_forward TEXT,
            remote_forward TEXT,
            other_options TEXT,
            synced INTEGER NOT NULL DEFAULT 0,
            server_id INTEGER
        )"
    ).execute(pool).await?;

    Ok(())
}

// ==================== Server CRUD ====================

pub async fn insert_server(pool: &SqlitePool, server: &CreateServerDto) -> Result<Server, Box<dyn std::error::Error>> {
    let now = Utc::now().to_rfc3339();
    let port = server.port.unwrap_or(22);
    
    let password_hash = if let Some(ref pass) = server.password {
        Some(hash_password(pass))
    } else {
        None
    };

    let result = sqlx::query(
        "INSERT INTO servers (name, host, port, username, auth_type, key_id, pem_data, password_hash, password_saved, group_name, tags, is_favorite, notes, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&server.name)
    .bind(&server.host)
    .bind(port)
    .bind(&server.username)
    .bind(&server.auth_type)
    .bind(server.key_id)
    .bind(&server.pem_data)
    .bind(&password_hash)
    .bind(0)
    .bind(&server.group_name)
    .bind(&server.tags)
    .bind(0)
    .bind(&server.notes)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    let id = result.last_insert_row_id();
    get_server_by_id(pool, id).await
}

pub async fn get_server_by_id(pool: &SqlitePool, id: i64) -> Result<Server, Box<dyn std::error::Error>> {
    let row = sqlx::query(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(Server {
        id: Some(row.try_get("id")?),
        name: row.try_get("name")?,
        host: row.try_get("host")?,
        port: row.try_get("port")?,
        username: row.try_get("username")?,
        auth_type: row.try_get("auth_type")?,
        key_id: row.try_get("key_id")?,
        pem_data: row.try_get("pem_data")?,
        password_hash: row.try_get("password_hash")?,
        password_saved: row.try_get("password_saved")?,
        group_name: row.try_get("group_name")?,
        tags: row.try_get("tags")?,
        is_favorite: row.try_get("is_favorite")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn get_all_servers(pool: &SqlitePool) -> Result<Vec<Server>, Box<dyn std::error::Error>> {
    let rows = sqlx::query(
        "SELECT * FROM servers ORDER BY is_favorite DESC, name ASC"
    )
    .fetch_all(pool)
    .await?;

    let servers: Vec<Server> = rows.into_iter().map(|row| {
        Ok(Server {
            id: Some(row.try_get("id")?),
            name: row.try_get("name")?,
            host: row.try_get("host")?,
            port: row.try_get("port")?,
            username: row.try_get("username")?,
            auth_type: row.try_get("auth_type")?,
            key_id: row.try_get("key_id")?,
            pem_data: row.try_get("pem_data")?,
            password_hash: row.try_get("password_hash")?,
            password_saved: row.try_get("password_saved")?,
            group_name: row.try_get("group_name")?,
            tags: row.try_get("tags")?,
            is_favorite: row.try_get("is_favorite")?,
            notes: row.try_get("notes")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }).collect::<Result<_, _>>()?;

    Ok(servers)
}

pub async fn update_server(pool: &SqlitePool, dto: &UpdateServerDto) -> Result<Server, Box<dyn std::error::Error>> {
    let now = Utc::now().to_rfc3339();
    
    let password_hash = if let Some(ref pass) = dto.password {
        Some(hash_password(pass))
    } else {
        None
    };

    let result = sqlx::query(
        "UPDATE servers SET
            name = COALESCE(?, name),
            host = COALESCE(?, host),
            port = COALESCE(?, port),
            username = COALESCE(?, username),
            auth_type = COALESCE(?, auth_type),
            key_id = COALESCE(?, key_id),
            pem_data = COALESCE(?, pem_data),
            password_hash = COALESCE(?, password_hash),
            password_saved = CASE WHEN ? IS NOT NULL THEN 1 ELSE password_saved END,
            group_name = COALESCE(?, group_name),
            tags = COALESCE(?, tags),
            notes = COALESCE(?, notes),
            updated_at = ?
         WHERE id = ?"
    )
    .bind(&dto.name)
    .bind(&dto.host)
    .bind(dto.port)
    .bind(&dto.username)
    .bind(&dto.auth_type)
    .bind(dto.key_id)
    .bind(&dto.pem_data)
    .bind(&password_hash)
    .bind(&dto.password)
    .bind(&dto.group_name)
    .bind(&dto.tags)
    .bind(&dto.notes)
    .bind(&now)
    .bind(dto.id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err("Server not found".into());
    }

    get_server_by_id(pool, dto.id).await
}

pub async fn delete_server(pool: &SqlitePool, id: i64) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn toggle_favorite(pool: &SqlitePool, id: i64) -> Result<Server, Box<dyn std::error::Error>> {
    let current = get_server_by_id(pool, id).await?;
    let new_value = !current.is_favorite;
    
    sqlx::query("UPDATE servers SET is_favorite = ? WHERE id = ?")
        .bind(new_value as i32)
        .bind(id)
        .execute(pool)
        .await?;

    get_server_by_id(pool, id).await
}

// ==================== SSH Key CRUD ====================

pub async fn insert_ssh_key(pool: &SqlitePool, key: &SshKey) -> Result<SshKey, Box<dyn std::error::Error>> {
    let now = Utc::now().to_rfc3339();
    
    let result = sqlx::query(
        "INSERT INTO ssh_keys (name, public_key, pem_data, key_type, key_size, passphrase_protected, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&key.name)
    .bind(&key.public_key)
    .bind(&key.pem_data)
    .bind(&key.key_type)
    .bind(key.key_size)
    .bind(key.passphrase_protected as i32)
    .bind(&now)
    .execute(pool)
    .await?;

    let id = result.last_insert_row_id();
    get_ssh_key_by_id(pool, id).await
}

pub async fn get_ssh_key_by_id(pool: &SqlitePool, id: i64) -> Result<SshKey, Box<dyn std::error::Error>> {
    let row = sqlx::query(
        "SELECT * FROM ssh_keys WHERE id = ?"
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(SshKey {
        id: Some(row.try_get("id")?),
        name: row.try_get("name")?,
        public_key: row.try_get("public_key")?,
        pem_data: row.try_get("pem_data")?,
        key_type: row.try_get("key_type")?,
        key_size: row.try_get("key_size")?,
        passphrase_protected: row.try_get("passphrase_protected")?,
        created_at: row.try_get("created_at")?,
    })
}

pub async fn get_all_ssh_keys(pool: &SqlitePool) -> Result<Vec<SshKey>, Box<dyn std::error::Error>> {
    let rows = sqlx::query(
        "SELECT * FROM ssh_keys ORDER BY name ASC"
    )
    .fetch_all(pool)
    .await?;

    let keys: Vec<SshKey> = rows.into_iter().map(|row| {
        Ok(SshKey {
            id: Some(row.try_get("id")?),
            name: row.try_get("name")?,
            public_key: row.try_get("public_key")?,
            pem_data: row.try_get("pem_data")?,
            key_type: row.try_get("key_type")?,
            key_size: row.try_get("key_size")?,
            passphrase_protected: row.try_get("passphrase_protected")?,
            created_at: row.try_get("created_at")?,
        })
    }).collect::<Result<_, _>>()?;

    Ok(keys)
}

pub async fn delete_ssh_key(pool: &SqlitePool, id: i64) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("DELETE FROM ssh_keys WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ==================== Helper Functions ====================

fn hash_password(password: &str) -> String {
    use sha2::Sha256;
    use base64::{Engine as _, engine::general_purpose};
    let hash = Sha256::digest(password);
    format!("$sha256${}", general_purpose::STANDARD.encode(hash))
}