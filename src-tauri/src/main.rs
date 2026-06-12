// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sqlx::{SqlitePool, Row};
use std::sync::Mutex;
use sha2::Digest;
use tauri::Manager;

struct AppState {
    db: Mutex<SqlitePool>,
}

#[tokio::main]
async fn main() {
    let _app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = app.path().data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("sshub.db");
            let db_url = format!("sqlite://{}", db_path.display());
            
            let pool = futures::executor::block_on(async {
                SqlitePool::connect(&db_url).await
            })?;
            
            // Initialize database tables
            futures::executor::block_on(async {
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
                ).execute(&pool).await?;

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
                ).execute(&pool).await?;

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
                ).execute(&pool).await?;

                Ok::<_, sqlx::Error>(())
            })?;
            
            app.manage(AppState {
                db: Mutex::new(pool),
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Server commands
            get_servers,
            get_server,
            create_server,
            update_server,
            delete_server,
            toggle_favorite,
            // SSH Config commands
            sync_servers_to_config,
            sync_config_to_servers,
            // SSH Key commands
            get_ssh_keys,
            create_ssh_key,
            import_ssh_key,
            delete_ssh_key,
            // Terminal commands
            start_ssh_session,
            close_ssh_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ==================== Helper Functions ====================

fn get_pool(app_handle: &tauri::AppHandle) -> Result<sqlx::SqlitePool, String> {
    let state = app_handle.state::<AppState>();
    let pool = state.db.lock().map_err(|e| e.to_string())?;
    Ok(pool.clone())
}

fn hash_password(password: &str) -> String {
    use sha2::Sha256;
    use base64::{Engine as _, engine::general_purpose};
    let hash = Sha256::digest(password);
    format!("$sha256${}", general_purpose::STANDARD.encode(hash))
}

// Helper to convert Option<Value> to Value
fn option_to_value(opt: Option<serde_json::Value>) -> serde_json::Value {
    match opt {
        Some(v) => v,
        None => serde_json::json!({"error": "Server not found"}),
    }
}

// ==================== Server Commands ====================

#[tauri::command]
async fn get_servers(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let pool = get_pool(&app)?;
    
    let rows = sqlx::query("SELECT * FROM servers ORDER BY is_favorite DESC, name ASC")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let mut servers = Vec::new();
    for row in rows {
        let server = serde_json::json!({
            "id": row.try_get::<Option<i64>, _>("id").unwrap_or(None),
            "name": row.try_get::<String, _>("name").unwrap_or_default(),
            "host": row.try_get::<String, _>("host").unwrap_or_default(),
            "port": row.try_get::<i32, _>("port").unwrap_or(22),
            "username": row.try_get::<String, _>("username").unwrap_or_default(),
            "auth_type": row.try_get::<String, _>("auth_type").unwrap_or("key".to_string()),
            "key_id": row.try_get::<Option<i64>, _>("key_id").unwrap_or(None),
            "pem_data": row.try_get::<Option<String>, _>("pem_data").unwrap_or(None),
            "password_saved": row.try_get::<bool, _>("password_saved").unwrap_or(false),
            "group_name": row.try_get::<Option<String>, _>("group_name").unwrap_or(None),
            "tags": row.try_get::<Option<String>, _>("tags").unwrap_or(None),
            "is_favorite": row.try_get::<bool, _>("is_favorite").unwrap_or(false),
            "notes": row.try_get::<Option<String>, _>("notes").unwrap_or(None),
            "created_at": row.try_get::<Option<String>, _>("created_at").unwrap_or(None),
            "updated_at": row.try_get::<Option<String>, _>("updated_at").unwrap_or(None),
        });
        servers.push(server);
    }
    
    Ok(servers)
}

#[tauri::command]
async fn get_server(app: tauri::AppHandle, id: i64) -> Result<Option<serde_json::Value>, String> {
    let pool = get_pool(&app)?;
    
    let row = sqlx::query("SELECT * FROM servers WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let server = serde_json::json!({
        "id": row.try_get::<Option<i64>, _>("id").unwrap_or(None),
        "name": row.try_get::<String, _>("name").unwrap_or_default(),
        "host": row.try_get::<String, _>("host").unwrap_or_default(),
        "port": row.try_get::<i32, _>("port").unwrap_or(22),
        "username": row.try_get::<String, _>("username").unwrap_or_default(),
        "auth_type": row.try_get::<String, _>("auth_type").unwrap_or("key".to_string()),
        "key_id": row.try_get::<Option<i64>, _>("key_id").unwrap_or(None),
        "pem_data": row.try_get::<Option<String>, _>("pem_data").unwrap_or(None),
        "password_saved": row.try_get::<bool, _>("password_saved").unwrap_or(false),
        "group_name": row.try_get::<Option<String>, _>("group_name").unwrap_or(None),
        "tags": row.try_get::<Option<String>, _>("tags").unwrap_or(None),
        "is_favorite": row.try_get::<bool, _>("is_favorite").unwrap_or(false),
        "notes": row.try_get::<Option<String>, _>("notes").unwrap_or(None),
        "created_at": row.try_get::<Option<String>, _>("created_at").unwrap_or(None),
        "updated_at": row.try_get::<Option<String>, _>("updated_at").unwrap_or(None),
    });
    
    Ok(Some(server))
}

#[tauri::command]
async fn create_server(app: tauri::AppHandle, server: serde_json::Value) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    let name: String = server["name"].as_str().unwrap_or("").to_string();
    let host: String = server["host"].as_str().unwrap_or("").to_string();
    let port: i32 = server["port"].as_i64().unwrap_or(22) as i32;
    let username: String = server["username"].as_str().unwrap_or("").to_string();
    let auth_type: String = server["auth_type"].as_str().unwrap_or("key").to_string();
    let key_id: Option<i64> = server["key_id"].as_i64();
    let pem_data: Option<String> = server["pem_data"].as_str().map(|s| s.to_string());
    let password: Option<String> = server["password"].as_str().map(|s| s.to_string());
    let group_name: Option<String> = server["group_name"].as_str().map(|s| s.to_string());
    let tags: Option<String> = server["tags"].as_str().map(|s| s.to_string());
    let notes: Option<String> = server["notes"].as_str().map(|s| s.to_string());
    
    let password_hash = if let Some(pass) = &password {
        Some(hash_password(pass))
    } else {
        None
    };
    
    sqlx::query(
        "INSERT INTO servers (name, host, port, username, auth_type, key_id, pem_data, password_hash, password_saved, group_name, tags, is_favorite, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&name)
    .bind(&host)
    .bind(port)
    .bind(&username)
    .bind(&auth_type)
    .bind(key_id)
    .bind(&pem_data)
    .bind(&password_hash)
    .bind(0)
    .bind(&group_name)
    .bind(&tags)
    .bind(0)
    .bind(&notes)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    
    let id = sqlx::query("SELECT last_insert_rowid() as id")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let last_id: i64 = id.try_get::<i64, _>("id").unwrap_or(0);
    
    // Return the created server directly instead of calling get_server
    Ok(serde_json::json!({
        "id": last_id,
        "name": name,
        "host": host,
        "port": port,
        "username": username,
        "auth_type": auth_type,
        "key_id": key_id,
        "pem_data": pem_data,
        "password_saved": false,
        "group_name": group_name,
        "tags": tags,
        "is_favorite": false,
        "notes": notes,
        "created_at": None::<String>,
        "updated_at": None::<String>,
    }))
}

#[tauri::command]
async fn update_server(app: tauri::AppHandle, server: serde_json::Value) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    let id: i64 = server["id"].as_i64().unwrap_or(0);
    let name: Option<String> = server["name"].as_str().map(|s| s.to_string());
    let host: Option<String> = server["host"].as_str().map(|s| s.to_string());
    let port: Option<i32> = server["port"].as_i64().map(|p| p as i32);
    let username: Option<String> = server["username"].as_str().map(|s| s.to_string());
    let auth_type: Option<String> = server["auth_type"].as_str().map(|s| s.to_string());
    let key_id: Option<i64> = server["key_id"].as_i64();
    let pem_data: Option<String> = server["pem_data"].as_str().map(|s| s.to_string());
    let password: Option<String> = server["password"].as_str().map(|s| s.to_string());
    let group_name: Option<String> = server["group_name"].as_str().map(|s| s.to_string());
    let tags: Option<String> = server["tags"].as_str().map(|s| s.to_string());
    let notes: Option<String> = server["notes"].as_str().map(|s| s.to_string());
    
    let password_hash = if let Some(pass) = &password {
        Some(hash_password(pass))
    } else {
        None
    };
    
    sqlx::query(
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
            notes = COALESCE(?, notes)
         WHERE id = ?"
    )
    .bind(&name)
    .bind(&host)
    .bind(port)
    .bind(&username)
    .bind(&auth_type)
    .bind(key_id)
    .bind(&pem_data)
    .bind(&password_hash)
    .bind(&password)
    .bind(&group_name)
    .bind(&tags)
    .bind(&notes)
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    
    // Return updated server directly
    Ok(serde_json::json!({
        "id": id,
        "name": server["name"].clone(),
        "host": server["host"].clone(),
        "port": server["port"].clone(),
        "username": server["username"].clone(),
        "auth_type": server["auth_type"].clone(),
        "key_id": key_id,
        "pem_data": pem_data,
        "password_saved": password.is_some(),
        "group_name": group_name,
        "tags": tags,
        "is_favorite": server["is_favorite"].clone(),
        "notes": notes,
        "created_at": server["created_at"].clone(),
        "updated_at": None::<String>,
    }))
}

#[tauri::command]
async fn delete_server(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let pool = get_pool(&app)?;
    
    sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
async fn toggle_favorite(app: tauri::AppHandle, id: i64) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    // Get current value
    let current = sqlx::query("SELECT is_favorite FROM servers WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let is_fav: bool = current.try_get::<bool, _>("is_favorite").unwrap_or(false);
    let new_value = !is_fav;
    
    sqlx::query("UPDATE servers SET is_favorite = ? WHERE id = ?")
        .bind(new_value)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    // Return updated server directly
    Ok(serde_json::json!({
        "id": id,
        "is_favorite": new_value,
    }))
}

// ==================== SSH Config Commands ====================

#[tauri::command]
async fn sync_servers_to_config(app: tauri::AppHandle) -> Result<(), String> {
    let pool = get_pool(&app)?;
    
    // Get all servers
    let rows = sqlx::query("SELECT * FROM servers ORDER BY is_favorite DESC, name ASC")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let mut servers = Vec::new();
    for row in rows {
        servers.push((
            row.try_get::<String, _>("name").unwrap_or_default(),
            row.try_get::<String, _>("host").unwrap_or_default(),
            row.try_get::<i32, _>("port").unwrap_or(22),
            row.try_get::<String, _>("username").unwrap_or_default(),
            row.try_get::<Option<String>, _>("group_name").unwrap_or(None),
        ));
    }
    
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let config_dir = format!("{}/.ssh", home);
    let config_path = format!("{}/.ssh/config", home);
    
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    
    let mut config = String::from("# Connectunnel managed SSH config\n");
    config.push_str("# Do not edit manually - changes will be overwritten\n\n");
    
    for (name, host, port, user, group) in &servers {
        let display_name = if let Some(g) = group {
            format!("{}-{}", g, name)
        } else {
            name.clone()
        };
        
        config.push_str(&format!("Host {}\n", display_name));
        config.push_str(&format!("    HostName {}\n", host));
        config.push_str(&format!("    Port {}\n", port));
        config.push_str(&format!("    User {}\n", user));
        config.push('\n');
    }
    
    std::fs::write(&config_path, config).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
fn sync_config_to_servers(_app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let config_path = format!("{}/.ssh/config", home);
    
    if !std::path::PathBuf::from(&config_path).exists() {
        return Ok(vec![]);
    }
    
    let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let mut servers = Vec::new();
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port = 22;
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        if trimmed.starts_with("#") || trimmed.is_empty() {
            continue;
        }
        
        if trimmed.starts_with("Host ") {
            if let Some(host) = current_host.take() {
                servers.push(serde_json::json!({
                    "name": host,
                    "host": current_hostname.unwrap_or(host.clone()),
                    "port": current_port,
                    "username": current_user.unwrap_or("user".to_string()),
                    "auth_type": "key",
                    "is_favorite": false,
                }));
            }
            current_host = Some(trimmed[5..].trim().to_string());
            current_hostname = None;
            current_user = None;
            current_port = 22;
            continue;
        }
        
        if let Some(key_value) = trimmed.split_once(|c: char| if c == '=' { true } else { c.is_whitespace() }) {
            let key = key_value.0.trim().to_lowercase();
            let value = key_value.1.trim().to_string();
            
            match key.as_str() {
                "hostname" => current_hostname = Some(value),
                "user" => current_user = Some(value),
                "port" => current_port = value.parse().unwrap_or(22),
                _ => {}
            }
        }
    }
    
    if let Some(host) = current_host.take() {
        servers.push(serde_json::json!({
            "name": host,
            "host": current_hostname.unwrap_or(host),
            "port": current_port,
            "username": current_user.unwrap_or("user".to_string()),
            "auth_type": "key",
            "is_favorite": false,
        }));
    }
    
    Ok(servers)
}

// ==================== SSH Key Commands ====================

#[tauri::command]
async fn get_ssh_keys(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let pool = get_pool(&app)?;
    
    let rows = sqlx::query("SELECT * FROM ssh_keys ORDER BY name ASC")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let mut keys = Vec::new();
    for row in rows {
        let key = serde_json::json!({
            "id": row.try_get::<Option<i64>, _>("id").unwrap_or(None),
            "name": row.try_get::<String, _>("name").unwrap_or_default(),
            "public_key": row.try_get::<String, _>("public_key").unwrap_or_default(),
            "pem_data": row.try_get::<Option<String>, _>("pem_data").unwrap_or(None),
            "key_type": row.try_get::<String, _>("key_type").unwrap_or("ed25519".to_string()),
            "key_size": row.try_get::<i32, _>("key_size").unwrap_or(256),
            "passphrase_protected": row.try_get::<bool, _>("passphrase_protected").unwrap_or(false),
            "created_at": row.try_get::<Option<String>, _>("created_at").unwrap_or(None),
        });
        keys.push(key);
    }
    
    Ok(keys)
}

#[tauri::command]
async fn create_ssh_key(app: tauri::AppHandle, key_data: serde_json::Value) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    let name: String = key_data["name"].as_str().unwrap_or("").to_string();
    let key_type: String = key_data["key_type"].as_str().unwrap_or("ed25519").to_string();
    let key_size: i32 = key_data["key_size"].as_i64().unwrap_or(256) as i32;
    
    // Generate key using ssh-keygen
    let keys_dir = app.path().data_dir().map_err(|e| e.to_string())?.join("ssh_keys");
    std::fs::create_dir_all(&keys_dir).map_err(|e| e.to_string())?;
    
    let key_name = format!("id_{}", name.replace(' ', "_"));
    let key_path = keys_dir.join(&key_name);
    let pub_key_path = key_path.with_extension("pub");
    
    let output = std::process::Command::new("ssh-keygen")
        .args(&["-t", "ed25519"])
        .args(&["-f", &key_path.to_string_lossy()])
        .args(&["-C", "connectunnel-generated"])
        .args(&["-N", ""])
        .output()
        .map_err(|e| format!("ssh-keygen failed: {}", e))?;
    
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ssh-keygen failed: {}", error).to_string());
    }
    
    let public_key = std::fs::read_to_string(&pub_key_path).map_err(|e| e.to_string())?;
    let pem_data = std::fs::read_to_string(&key_path).ok();
    
    let _result = sqlx::query(
        "INSERT INTO ssh_keys (name, public_key, pem_data, key_type, key_size, passphrase_protected)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&name)
    .bind(public_key.trim())
    .bind(&pem_data)
    .bind(&key_type)
    .bind(key_size)
    .bind(0)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    
    let id = sqlx::query("SELECT last_insert_rowid() as id")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let last_id: i64 = id.try_get::<i64, _>("id").unwrap_or(0);
    
    let key = serde_json::json!({
        "id": last_id,
        "name": name,
        "public_key": public_key.trim(),
        "pem_data": pem_data,
        "key_type": key_type,
        "key_size": key_size,
        "passphrase_protected": false,
        "created_at": None::<String>,
    });
    
    Ok(key)
}

#[tauri::command]
async fn import_ssh_key(app: tauri::AppHandle, key_data: serde_json::Value) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    let name: String = key_data["name"].as_str().unwrap_or("").to_string();
    let public_key: String = key_data["public_key"].as_str().unwrap_or("").to_string();
    let _private_key: Option<String> = key_data["private_key"].as_str().map(|s| s.to_string());
    let pem_data: Option<String> = key_data["pem_data"].as_str().map(|s| s.to_string());
    let key_type: String = key_data["key_type"].as_str().unwrap_or("ed25519").to_string();
    
    // Save private key to file if provided
    if let Some(ref pem) = pem_data {
        let keys_dir = app.path().data_dir().map_err(|e| e.to_string())?.join("ssh_keys");
        std::fs::create_dir_all(&keys_dir).map_err(|e| e.to_string())?;
        
        let key_name = format!("id_{}", name.replace(' ', "_"));
        let key_path = keys_dir.join(&key_name);
        std::fs::write(&key_path, pem).map_err(|e| e.to_string())?;
    }
    
    let _result = sqlx::query(
        "INSERT INTO ssh_keys (name, public_key, pem_data, key_type, key_size, passphrase_protected)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&name)
    .bind(&public_key)
    .bind(&pem_data)
    .bind(&key_type)
    .bind(256)
    .bind(0)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    
    let id = sqlx::query("SELECT last_insert_rowid() as id")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let last_id: i64 = id.try_get::<i64, _>("id").unwrap_or(0);
    
    let key = serde_json::json!({
        "id": last_id,
        "name": name,
        "public_key": public_key,
        "pem_data": pem_data,
        "key_type": key_type,
        "key_size": 256,
        "passphrase_protected": false,
        "created_at": None::<String>,
    });
    
    Ok(key)
}

#[tauri::command]
async fn delete_ssh_key(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let pool = get_pool(&app)?;
    
    sqlx::query("DELETE FROM ssh_keys WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    Ok(())
}

// ==================== Terminal Commands ====================

#[tauri::command]
async fn start_ssh_session(app: tauri::AppHandle, server_id: i64, password: Option<String>) -> Result<serde_json::Value, String> {
    let pool = get_pool(&app)?;
    
    let row = sqlx::query("SELECT * FROM servers WHERE id = ?")
        .bind(server_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    
    let host: String = row.try_get::<String, _>("host").map_err(|e| e.to_string())?;
    let port: i32 = row.try_get::<i32, _>("port").map_err(|e| e.to_string())?;
    let username: String = row.try_get::<String, _>("username").map_err(|e| e.to_string())?;
    let auth_type: String = row.try_get::<String, _>("auth_type").map_err(|e| e.to_string())?;
    
    // Check if password is needed
    if auth_type == "password" && password.is_none() {
        return Ok(serde_json::json!({
            "success": false,
            "message": "Password authentication required.",
            "needs_password": true,
        }));
    }
    
    // Build SSH command
    let mut ssh_cmd = format!("ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null ");
    
    if port != 22 {
        ssh_cmd.push_str(&format!("-p {} ", port));
    }
    
    ssh_cmd.push_str(&format!("{}@{}", username, host));
    
    // Open in terminal
    open_ssh_terminal(&ssh_cmd)?;
    
    Ok(serde_json::json!({
        "success": true,
        "message": "SSH session started.",
        "needs_password": false,
    }))
}

fn open_ssh_terminal(ssh_cmd: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"
            tell application "Terminal"
                do script "{}"
                activate
            end tell
            "#,
            ssh_cmd
        );
        
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {}", e))?
            .wait()
            .map_err(|e| format!("Failed to wait: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        let terminals = ["gnome-terminal", "konsole", "xterm"];
        for term in &terminals {
            if std::process::Command::new(term)
                .arg("-e")
                .arg("bash")
                .arg("-c")
                .arg(&format!("\"{}\"; exec bash", ssh_cmd))
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .arg("/c")
            .arg(&format!("start \"SSH\" cmd /k {}", ssh_cmd))
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {}", e))?
            .wait()
            .map_err(|e| format!("Failed to wait: {}", e))?;
    }
    
    Ok(())
}

#[tauri::command]
fn close_ssh_session(_session_id: String) -> Result<(), String> {
    // Session management would be implemented here
    Ok(())
}