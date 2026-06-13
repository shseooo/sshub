use crate::commands::key::{secure_private_file, server_pem_path};
use crate::models::{CreateServerDto, Server, UpdateServerDto};
use crate::store::Store;
use crate::AppState;
use tauri::{AppHandle, State};

fn write_server_pem(app: &AppHandle, id: i64, pem: &str) -> Result<(), String> {
    let path = server_pem_path(app, id)?;
    std::fs::write(&path, pem).map_err(|e| e.to_string())?;
    secure_private_file(&path)
}

fn remove_server_pem(app: &AppHandle, id: i64) {
    if let Ok(path) = server_pem_path(app, id) {
        let _ = std::fs::remove_file(path);
    }
}

#[tauri::command]
pub fn get_servers(state: State<'_, AppState>) -> Result<Vec<Server>, String> {
    state.store.list_servers().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_server(state: State<'_, AppState>, id: i64) -> Result<Option<Server>, String> {
    state.store.find_server(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_server(
    app: AppHandle,
    state: State<'_, AppState>,
    mut server: CreateServerDto,
) -> Result<Server, String> {
    // The private key never goes to the JSON store — pull it out and persist it
    // to a 0600 file keyed by the new server id.
    let pem = if server.auth_type == "pem" {
        server.pem_data.take()
    } else {
        None
    };
    server.pem_data = None;
    let created = state.store.insert_server(&server).map_err(|e| e.to_string())?;
    if let Some(pem) = pem.filter(|p| !p.trim().is_empty()) {
        write_server_pem(&app, created.id, &pem)?;
    }
    Ok(created)
}

#[tauri::command]
pub fn update_server(
    app: AppHandle,
    state: State<'_, AppState>,
    mut server: UpdateServerDto,
) -> Result<Server, String> {
    let pem = server.pem_data.take();
    server.pem_data = None;
    let updated = state.store.update_server(&server).map_err(|e| e.to_string())?;
    if updated.auth_type != "pem" {
        // Switched away from PEM auth → drop the stored key.
        remove_server_pem(&app, updated.id);
    } else if let Some(pem) = pem.filter(|p| !p.trim().is_empty()) {
        // New PEM provided → replace it. An empty value keeps the existing file.
        write_server_pem(&app, updated.id, &pem)?;
    }
    Ok(updated)
}

#[tauri::command]
pub fn delete_server(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    remove_server_pem(&app, id);
    state.store.delete_server(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, id: i64) -> Result<Server, String> {
    state.store.toggle_favorite(id).map_err(|e| e.to_string())
}

/// One-time migration: move any legacy plaintext server PEMs out of the JSON
/// store into 0600 files. Safe to run on every startup.
pub fn migrate_server_pems(app: &AppHandle, store: &Store) {
    if let Ok(pems) = store.take_server_pems() {
        for (id, pem) in pems {
            let _ = write_server_pem(app, id, &pem);
        }
    }
}
