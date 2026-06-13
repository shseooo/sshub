use crate::commands::key::{key_file_name, keys_dir, secure_private_file};
use crate::store::{ExportBundle, ImportSummary};
use crate::AppState;
use cocoon::Cocoon;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

/// Marker returned to the frontend when an import file is encrypted but no
/// passphrase was supplied — the UI then prompts for one and retries.
const NEEDS_PASSPHRASE: &str = "ENCRYPTED";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrivateKeyEntry {
    name: String,
    pem: String,
}

/// What an encrypted export contains: the (secret-free) bundle plus the actual
/// private key files. Serialized to JSON then encrypted with the passphrase.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecureBundle {
    bundle: ExportBundle,
    private_keys: Vec<PrivateKeyEntry>,
}

/// Export the server/key list. With a passphrase, private key files are bundled
/// in and the whole thing is encrypted (safe to sync anywhere). Without one,
/// a plain secret-free JSON is written.
#[tauri::command]
pub fn export_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
    shortcuts: Option<serde_json::Value>,
    server_ids: Option<Vec<i64>>,
    key_ids: Option<Vec<i64>>,
) -> Result<(), String> {
    let mut bundle = state.store.export_bundle().map_err(|e| e.to_string())?;
    // None = export everything; Some(ids) = only the selected entries.
    if let Some(ids) = server_ids {
        bundle.servers.retain(|s| ids.contains(&s.id));
    }
    if let Some(ids) = key_ids {
        bundle.keys.retain(|k| ids.contains(&k.id));
    }
    bundle.shortcuts = shortcuts;

    match passphrase {
        Some(pass) if !pass.is_empty() => {
            let dir = keys_dir(&app)?;
            let mut private_keys = Vec::new();
            for key in &bundle.keys {
                let key_path = dir.join(key_file_name(&key.name));
                if let Ok(pem) = std::fs::read_to_string(&key_path) {
                    private_keys.push(PrivateKeyEntry {
                        name: key.name.clone(),
                        pem,
                    });
                }
            }
            let secure = SecureBundle {
                bundle,
                private_keys,
            };
            let json = serde_json::to_vec(&secure).map_err(|e| e.to_string())?;
            let mut cocoon = Cocoon::new(pass.as_bytes());
            let encrypted = cocoon.wrap(&json).map_err(|e| format!("암호화 실패: {:?}", e))?;
            std::fs::write(&path, encrypted).map_err(|e| e.to_string())
        }
        _ => {
            let json = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| e.to_string())
        }
    }
}

/// Import an export file. Plain JSON merges metadata only. An encrypted file
/// needs the passphrase: it merges metadata AND restores private key files (0600).
#[tauri::command]
pub fn import_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
) -> Result<ImportSummary, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;

    // A plain export is valid UTF-8 JSON; anything else is treated as encrypted.
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if let Ok(bundle) = serde_json::from_str::<ExportBundle>(text) {
            return state.store.import_bundle(bundle).map_err(|e| e.to_string());
        }
    }

    let pass = match passphrase {
        Some(p) if !p.is_empty() => p,
        _ => return Err(NEEDS_PASSPHRASE.to_string()),
    };

    let cocoon = Cocoon::new(pass.as_bytes());
    let json = cocoon
        .unwrap(&bytes)
        .map_err(|_| "복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.".to_string())?;
    let secure: SecureBundle =
        serde_json::from_slice(&json).map_err(|e| format!("잘못된 백업 파일입니다: {}", e))?;

    let summary = state
        .store
        .import_bundle(secure.bundle)
        .map_err(|e| e.to_string())?;

    // Restore private key files that don't already exist on this machine.
    let dir = keys_dir(&app)?;
    for entry in secure.private_keys {
        let key_path = dir.join(key_file_name(&entry.name));
        if !key_path.exists() {
            std::fs::write(&key_path, entry.pem).map_err(|e| e.to_string())?;
            secure_private_file(&key_path)?;
        }
    }

    Ok(summary)
}
