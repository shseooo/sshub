use crate::models::{CreateServerDto, Server};
use crate::AppState;
use std::collections::HashSet;
use std::fmt::Write as _;
use tauri::State;

fn ssh_config_path() -> Result<(String, String), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    Ok((format!("{}/.ssh", home), format!("{}/.ssh/config", home)))
}

/// Write all stored servers to ~/.ssh/config.
/// The existing file is backed up to `config.bak.<timestamp>` before being replaced.
#[tauri::command]
pub fn sync_servers_to_config(state: State<'_, AppState>) -> Result<(), String> {
    let servers = state.store.list_servers().map_err(|e| e.to_string())?;

    if servers.is_empty() {
        return Err("등록된 서버가 없어 ~/.ssh/config를 덮어쓰지 않았습니다.".to_string());
    }

    let (config_dir, config_path) = ssh_config_path()?;
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;

    if std::path::Path::new(&config_path).exists() {
        let backup_path = format!(
            "{}.bak.{}",
            config_path,
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        std::fs::copy(&config_path, &backup_path).map_err(|e| e.to_string())?;
    }

    let mut config = String::from("# Connectunnel managed SSH config\n");
    config.push_str("# Do not edit manually - changes will be overwritten\n\n");

    for server in &servers {
        let display_name = match &server.group_name {
            Some(group) if !group.is_empty() => format!("{}-{}", group, server.name),
            _ => server.name.clone(),
        };

        let _ = writeln!(config, "Host {}", display_name);
        let _ = writeln!(config, "    HostName {}", server.host);
        let _ = writeln!(config, "    Port {}", server.port);
        let _ = writeln!(config, "    User {}", server.username);
        config.push('\n');
    }

    std::fs::write(&config_path, config).map_err(|e| e.to_string())
}

/// Import hosts from ~/.ssh/config into the store.
/// Hosts whose name already exists are skipped; returns the newly added servers.
#[tauri::command]
pub fn sync_config_to_servers(state: State<'_, AppState>) -> Result<Vec<Server>, String> {
    let (_, config_path) = ssh_config_path()?;

    if !std::path::Path::new(&config_path).exists() {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let entries = parse_ssh_config(&content);

    let existing: HashSet<String> = state
        .store
        .list_servers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| s.name)
        .collect();

    let mut imported = Vec::new();
    for entry in entries {
        if existing.contains(&entry.name) {
            continue;
        }
        let server = state.store.insert_server(&entry).map_err(|e| e.to_string())?;
        imported.push(server);
    }

    Ok(imported)
}

fn parse_ssh_config(content: &str) -> Vec<CreateServerDto> {
    let mut entries = Vec::new();
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port = 22;

    let flush = |host: Option<String>,
                 hostname: Option<String>,
                 user: Option<String>,
                 port: i32,
                 entries: &mut Vec<CreateServerDto>| {
        if let Some(host) = host {
            // Skip wildcard patterns like "Host *"
            if host.contains('*') || host.contains('?') {
                return;
            }
            entries.push(CreateServerDto {
                name: host.clone(),
                host: hostname.unwrap_or(host),
                port: Some(port),
                username: user.unwrap_or_else(|| "user".to_string()),
                auth_type: "key".to_string(),
                key_id: None,
                pem_data: None,
                group_name: None,
                tags: None,
                notes: None,
            });
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(host) = trimmed.strip_prefix("Host ") {
            flush(
                current_host.take(),
                current_hostname.take(),
                current_user.take(),
                current_port,
                &mut entries,
            );
            current_host = Some(host.trim().to_string());
            current_port = 22;
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(|c: char| c == '=' || c.is_whitespace()) {
            let value = value.trim().to_string();
            match key.trim().to_lowercase().as_str() {
                "hostname" => current_hostname = Some(value),
                "user" => current_user = Some(value),
                "port" => current_port = value.parse().unwrap_or(22),
                _ => {}
            }
        }
    }

    flush(
        current_host,
        current_hostname,
        current_user,
        current_port,
        &mut entries,
    );

    entries
}
