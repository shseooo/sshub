use crate::store::{ExportBundle, ImportSummary};
use crate::AppState;
use tauri::State;

/// Write the (secret-free) server/key list to a JSON file the user picked.
#[tauri::command]
pub fn export_data(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let bundle = state.store.export_bundle().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Read an export file and merge it into the store (skips existing names).
#[tauri::command]
pub fn import_data(state: State<'_, AppState>, path: String) -> Result<ImportSummary, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let bundle: ExportBundle = serde_json::from_str(&content)
        .map_err(|e| format!("올바른 sshub 내보내기 파일이 아닙니다: {}", e))?;
    state.store.import_bundle(bundle).map_err(|e| e.to_string())
}
