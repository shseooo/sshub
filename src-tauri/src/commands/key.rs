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

/// 0600 file path holding a `pem`-auth server's private key (kept out of JSON).
pub fn server_pem_path(app: &AppHandle, id: i64) -> Result<PathBuf, String> {
    Ok(keys_dir(app)?.join(format!("pem_server_{}", id)))
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

/// Extract the public key from a private key with `ssh-keygen -y`.
/// Encrypted keys need the passphrase; without it ssh-keygen fails.
fn derive_public_key(key_path: &std::path::Path, passphrase: Option<&str>) -> Result<String, String> {
    let output = std::process::Command::new("ssh-keygen")
        .arg("-y")
        .arg("-f")
        .arg(key_path)
        .arg("-P")
        .arg(passphrase.unwrap_or(""))
        .output()
        .map_err(|e| format!("ssh-keygen -y failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "개인 키에서 공개 키를 추출하지 못했습니다. 암호로 보호된 키라면 passphrase를 입력하세요. ({})",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Map an OpenSSH public-key prefix to our stored key-type label.
fn detect_key_type(public_key: &str) -> Option<String> {
    let prefix = public_key.split_whitespace().next()?;
    let t = match prefix {
        "ssh-ed25519" | "sk-ssh-ed25519@openssh.com" => "ed25519",
        "ssh-rsa" => "rsa",
        "ssh-dss" => "dsa",
        s if s.starts_with("ecdsa-") || s.starts_with("sk-ecdsa-") => "ecdsa",
        _ => return None,
    };
    Some(t.to_string())
}

#[tauri::command]
pub fn import_ssh_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key_data: ImportKeyDto,
) -> Result<SshKey, String> {
    let mut public_key = key_data.public_key.trim().to_string();

    // Save private key to a 0600 file; never store the secret in the JSON store.
    if let Some(ref pem) = key_data.pem_data {
        let key_path = keys_dir(&app)?.join(key_file_name(&key_data.name));
        std::fs::write(&key_path, pem).map_err(|e| e.to_string())?;
        secure_private_file(&key_path)?;

        // Auto-extract the public key from the private key when not provided
        // (best-effort: an encrypted key with no passphrase just stays empty).
        if public_key.is_empty() {
            if let Ok(derived) = derive_public_key(&key_path, key_data.passphrase.as_deref()) {
                public_key = derived;
            }
        }
    }

    // May still be empty (encrypted PEM, no passphrase). Require at least one of
    // public key / private key so the entry isn't empty.
    if public_key.is_empty() && key_data.pem_data.is_none() {
        return Err("공개 키 또는 개인 키(PEM) 중 하나는 필요합니다.".to_string());
    }

    // When a public key is present, trust it over the UI's guessed type.
    let key_type = if public_key.is_empty() {
        key_data.key_type
    } else {
        detect_key_type(&public_key).unwrap_or(key_data.key_type)
    };

    state
        .store
        .insert_ssh_key(&NewSshKey {
            name: key_data.name,
            public_key,
            key_type,
            key_size: 256,
            passphrase_protected: key_data
                .passphrase
                .as_deref()
                .map(|p| !p.is_empty())
                .unwrap_or(false),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_ssh_key(
    app: AppHandle,
    state: State<'_, AppState>,
    key_data: crate::models::UpdateKeyDto,
) -> Result<SshKey, String> {
    let old = state
        .store
        .get_ssh_key(key_data.id)
        .map_err(|e| e.to_string())?;
    let dir = keys_dir(&app)?;
    let old_priv = dir.join(key_file_name(&old.name));
    let new_priv = dir.join(key_file_name(&key_data.name));

    // Rename the on-disk private key (and its .pub) when the name changes, so
    // the path terminal.rs derives from the new name still resolves.
    if key_file_name(&old.name) != key_file_name(&key_data.name) {
        if new_priv.exists() {
            return Err("같은 이름의 키 파일이 이미 있습니다.".to_string());
        }
        if old_priv.exists() {
            std::fs::rename(&old_priv, &new_priv).map_err(|e| e.to_string())?;
        }
        let old_pub = old_priv.with_extension("pub");
        if old_pub.exists() {
            let _ = std::fs::rename(&old_pub, new_priv.with_extension("pub"));
        }
    }

    // Optionally replace the private key material. (Changing the passphrase of
    // an existing key is a separate operation — see change_key_passphrase.)
    let mut passphrase_protected = old.passphrase_protected;
    if let Some(pem) = key_data.pem_data.as_ref().filter(|p| !p.trim().is_empty()) {
        std::fs::write(&new_priv, pem).map_err(|e| e.to_string())?;
        secure_private_file(&new_priv)?;
        // New key → protected iff a passphrase was given.
        passphrase_protected = key_data
            .passphrase
            .as_deref()
            .map(|p| !p.is_empty())
            .unwrap_or(false);
    }

    let public_key = key_data.public_key.trim().to_string();
    let key_type = if public_key.is_empty() {
        key_data.key_type
    } else {
        detect_key_type(&public_key).unwrap_or(key_data.key_type)
    };

    state
        .store
        .update_ssh_key(
            key_data.id,
            &key_data.name,
            &public_key,
            &key_type,
            passphrase_protected,
        )
        .map_err(|e| e.to_string())
}

/// Re-encrypt the stored private key with a new passphrase via `ssh-keygen -p`.
/// `current_passphrase` must match (empty if the key is currently unprotected);
/// an empty `new_passphrase` removes protection.
#[tauri::command]
pub fn change_key_passphrase(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    current_passphrase: Option<String>,
    new_passphrase: Option<String>,
) -> Result<(), String> {
    let key = state.store.get_ssh_key(id).map_err(|e| e.to_string())?;
    let path = keys_dir(&app)?.join(key_file_name(&key.name));
    if !path.exists() {
        return Err("이 기기에 개인 키 파일이 없습니다.".to_string());
    }
    let cur = current_passphrase.unwrap_or_default();
    let new = new_passphrase.unwrap_or_default();
    let output = std::process::Command::new("ssh-keygen")
        .arg("-p")
        .arg("-f")
        .arg(&path)
        .arg("-P")
        .arg(&cur)
        .arg("-N")
        .arg(&new)
        .output()
        .map_err(|e| format!("ssh-keygen -p failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "패스프레이즈 변경 실패 — 현재 패스프레이즈가 맞는지 확인하세요. ({})",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    state
        .store
        .set_key_passphrase_protected(id, !new.is_empty())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_ssh_key(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    // Remove the on-disk key files too, so the same name can be reused later.
    if let Ok(key) = state.store.get_ssh_key(id) {
        if let Ok(dir) = keys_dir(&app) {
            let priv_path = dir.join(key_file_name(&key.name));
            let _ = std::fs::remove_file(&priv_path);
            let _ = std::fs::remove_file(priv_path.with_extension("pub"));
        }
    }
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
        let derive_from = path.clone();
        let mut pub_path = path.into_os_string();
        pub_path.push(".pub");
        let mut public_key = std::fs::read_to_string(PathBuf::from(pub_path))
            .ok()
            .map(|s| s.trim().to_string());
        // No sibling .pub (e.g. a bare AWS .pem) → auto-extract it. Works for
        // unencrypted keys; encrypted ones derive at import with a passphrase.
        if public_key.is_none() {
            if let Ok(derived) = derive_public_key(&derive_from, None) {
                if !derived.is_empty() {
                    public_key = Some(derived);
                }
            }
        }

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

/// Manually derive the public key from a pasted/loaded private key (PEM).
/// The secret is written to a 0600 temp file inside the app's private data dir
/// (never world-writable /tmp) for the `ssh-keygen -y` call, then removed.
#[tauri::command]
pub fn derive_public_key_from_pem(
    app: AppHandle,
    pem: String,
    passphrase: Option<String>,
) -> Result<String, String> {
    if pem.trim().is_empty() {
        return Err("개인 키(PEM)가 비어 있습니다.".to_string());
    }
    let tmp = keys_dir(&app)?.join(".derive.tmp");
    std::fs::write(&tmp, pem).map_err(|e| e.to_string())?;
    secure_private_file(&tmp)?;
    let result = derive_public_key(&tmp, passphrase.as_deref());
    let _ = std::fs::remove_file(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::{detect_key_type, key_file_name};

    #[test]
    fn key_file_name_sanitizes_unsafe_chars() {
        assert_eq!(key_file_name("my key!"), "id_my_key_");
        // Path-traversal chars (. and /) are neutralized to underscores.
        assert_eq!(key_file_name("../etc/passwd"), "id____etc_passwd");
        assert_eq!(key_file_name("ok-name_1"), "id_ok-name_1");
    }

    #[test]
    fn detect_key_type_maps_known_prefixes() {
        assert_eq!(detect_key_type("ssh-ed25519 AAAA x").as_deref(), Some("ed25519"));
        assert_eq!(detect_key_type("ssh-rsa AAAA x").as_deref(), Some("rsa"));
        assert_eq!(detect_key_type("ssh-dss AAAA x").as_deref(), Some("dsa"));
        assert_eq!(
            detect_key_type("ecdsa-sha2-nistp256 AAAA x").as_deref(),
            Some("ecdsa")
        );
    }

    #[test]
    fn detect_key_type_handles_fido2_security_keys() {
        assert_eq!(
            detect_key_type("sk-ssh-ed25519@openssh.com AAAA x").as_deref(),
            Some("ed25519")
        );
        assert_eq!(
            detect_key_type("sk-ecdsa-sha2-nistp256@openssh.com AAAA x").as_deref(),
            Some("ecdsa")
        );
    }

    #[test]
    fn detect_key_type_returns_none_for_unknown() {
        assert!(detect_key_type("not-a-key").is_none());
        assert!(detect_key_type("").is_none());
    }
}
