use crate::models::{CreateKeyDto, ImportKeyDto, SshKey};
use crate::store::NewSshKey;
use crate::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

pub fn keys_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .data_dir()
        .map_err(|e| e.to_string())?
        .join("ssh_keys");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Restrict private key files to owner-only access (0600).
/// ssh refuses to use a private key whose file is group/world-readable, and
/// it keeps secret material from other local users.
pub fn secure_private_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

// Key names become file names; restrict to a safe character set.
pub fn key_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("id_{}", safe)
}

/// A key plus whether its private key file actually exists on this machine.
/// After an import, key metadata can be present without the private file.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyView {
    #[serde(flatten)]
    key: SshKey,
    has_private_file: bool,
}

#[tauri::command]
pub fn get_ssh_keys(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<SshKeyView>, String> {
    let dir = keys_dir(&app)?;
    let keys = state.store.list_ssh_keys().map_err(|e| e.to_string())?;
    Ok(keys
        .into_iter()
        .map(|k| {
            let has_private_file = dir.join(key_file_name(&k.name)).exists();
            SshKeyView {
                key: k,
                has_private_file,
            }
        })
        .collect())
}

#[tauri::command]
pub fn create_ssh_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key_data: CreateKeyDto,
) -> Result<SshKey, String> {
    let key_type = key_data.key_type.to_lowercase();
    if !matches!(key_type.as_str(), "ed25519" | "rsa" | "ecdsa") {
        return Err(format!("Unsupported key type: {}", key_type));
    }

    let key_size = key_data.key_size.unwrap_or(match key_type.as_str() {
        "rsa" => 3072,
        _ => 256,
    });

    let key_path = keys_dir(&app)?.join(key_file_name(&key_data.name));
    if key_path.exists() {
        return Err(format!("Key file already exists: {}", key_path.display()));
    }
    let pub_key_path = key_path.with_extension("pub");

    let passphrase = key_data.passphrase.unwrap_or_default();

    let mut cmd = std::process::Command::new("ssh-keygen");
    cmd.args(["-t", &key_type])
        .arg("-f")
        .arg(&key_path)
        .args(["-C", "connectunnel-generated"])
        .args(["-N", &passphrase]);
    if key_type == "rsa" {
        cmd.args(["-b", &key_size.to_string()]);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("ssh-keygen failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // ssh-keygen already writes the private key 0600; the secret stays on disk
    // only — it is never copied into the (world-readable) JSON store.
    let public_key = std::fs::read_to_string(&pub_key_path).map_err(|e| e.to_string())?;

    state
        .store
        .insert_ssh_key(&NewSshKey {
            name: key_data.name,
            public_key: public_key.trim().to_string(),
            key_type,
            key_size,
            passphrase_protected: !passphrase.is_empty(),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_ssh_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key_data: ImportKeyDto,
) -> Result<SshKey, String> {
    // Save private key to a 0600 file; never store the secret in the JSON store.
    if let Some(ref pem) = key_data.pem_data {
        let key_path = keys_dir(&app)?.join(key_file_name(&key_data.name));
        std::fs::write(&key_path, pem).map_err(|e| e.to_string())?;
        secure_private_file(&key_path)?;
    }

    state
        .store
        .insert_ssh_key(&NewSshKey {
            name: key_data.name,
            public_key: key_data.public_key,
            key_type: key_data.key_type,
            key_size: 256,
            passphrase_protected: key_data.passphrase.is_some(),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_ssh_key(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.store.delete_ssh_key(id).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedKeyFile {
    pub file_name: String,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
}

/// Read a key file picked via the file dialog. Detects whether it is a
/// private or public key; for private keys the sibling `.pub` is loaded too.
#[tauri::command]
pub fn load_key_file(path: String) -> Result<LoadedKeyFile, String> {
    let path = PathBuf::from(path);
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if content.trim_start().starts_with("-----BEGIN") {
        let mut pub_path = path.into_os_string();
        pub_path.push(".pub");
        let public_key = std::fs::read_to_string(PathBuf::from(pub_path))
            .ok()
            .map(|s| s.trim().to_string());

        Ok(LoadedKeyFile {
            file_name,
            public_key,
            private_key: Some(content),
        })
    } else {
        Ok(LoadedKeyFile {
            file_name,
            public_key: Some(content.trim().to_string()),
            private_key: None,
        })
    }
}
