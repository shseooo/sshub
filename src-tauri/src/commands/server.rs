use crate::models::{CreateServerDto, Server, UpdateServerDto};
use crate::AppState;
use tauri::State;

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
    state: State<'_, AppState>,
    server: CreateServerDto,
) -> Result<Server, String> {
    state.store.insert_server(&server).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_server(
    state: State<'_, AppState>,
    server: UpdateServerDto,
) -> Result<Server, String> {
    state.store.update_server(&server).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_server(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.store.delete_server(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, id: i64) -> Result<Server, String> {
    state.store.toggle_favorite(id).map_err(|e| e.to_string())
}
