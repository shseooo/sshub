use crate::models::{CreateServerDto, Server, SshKey, UpdateServerDto};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Portable export/import payload for syncing the server list between machines.
/// Carries no secrets — private key material is never included.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportBundle {
    pub version: u32,
    pub servers: Vec<Server>,
    pub keys: Vec<SshKey>,
    /// Opaque frontend prefs (keyboard shortcuts). Set by the export command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub servers_added: usize,
    pub servers_skipped: usize,
    pub keys_added: usize,
    pub keys_skipped: usize,
    /// Shortcuts carried in the imported file, for the frontend to apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<serde_json::Value>,
}

pub type StoreError = Box<dyn std::error::Error + Send + Sync>;
pub type StoreResult<T> = Result<T, StoreError>;

/// Restrict a file to owner-only access (0600) on Unix. Best-effort: a failure
/// here must not block persistence, so the result is intentionally ignored.
fn set_owner_only(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Data {
    next_server_id: i64,
    next_key_id: i64,
    servers: Vec<Server>,
    keys: Vec<SshKey>,
}

/// JSON-file-backed store. All data lives in `<data_dir>/sshub.json`;
/// every mutation is persisted atomically (temp file + rename).
pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Store {
    pub fn load(app: &AppHandle) -> StoreResult<Self> {
        let dir = app.path().data_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("sshub.json");

        let mut data: Data = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            Data::default()
        };
        // Keep id counters ahead of existing records even if the file was hand-edited
        data.next_server_id = data
            .next_server_id
            .max(data.servers.iter().map(|s| s.id).max().unwrap_or(0) + 1);
        data.next_key_id = data
            .next_key_id
            .max(data.keys.iter().map(|k| k.id).max().unwrap_or(0) + 1);

        let store = Self {
            path,
            data: Mutex::new(data),
        };

        // One-time cleanup: scrub any private key material left in the JSON by
        // older versions (the store must never hold secrets) and re-persist.
        {
            let mut data = store.data.lock().map_err(|e| e.to_string())?;
            let had_secret = data.keys.iter().any(|k| k.pem_data.is_some());
            if had_secret {
                for k in &mut data.keys {
                    k.pem_data = None;
                }
                store.save(&data)?;
            }
        }

        Ok(store)
    }

    fn save(&self, data: &Data) -> StoreResult<()> {
        let tmp = self.path.with_extension("json.tmp");
        let file = std::fs::File::create(&tmp)?;
        serde_json::to_writer_pretty(&file, data)?;
        file.sync_all()?; // flush to disk before rename so a crash can't truncate
        drop(file);
        set_owner_only(&tmp);
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    // ==================== Servers ====================

    pub fn list_servers(&self) -> StoreResult<Vec<Server>> {
        let data = self.data.lock().map_err(|e| e.to_string())?;
        let mut servers = data.servers.clone();
        servers.sort_by(|a, b| {
            b.is_favorite
                .cmp(&a.is_favorite)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(servers)
    }

    pub fn find_server(&self, id: i64) -> StoreResult<Option<Server>> {
        let data = self.data.lock().map_err(|e| e.to_string())?;
        Ok(data.servers.iter().find(|s| s.id == id).cloned())
    }

    pub fn get_server(&self, id: i64) -> StoreResult<Server> {
        self.find_server(id)?
            .ok_or_else(|| format!("Server not found: {}", id).into())
    }

    pub fn insert_server(&self, dto: &CreateServerDto) -> StoreResult<Server> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let now = Utc::now().to_rfc3339();
        let server = Server {
            id: data.next_server_id,
            name: dto.name.clone(),
            host: dto.host.clone(),
            port: dto.port.unwrap_or(22),
            username: dto.username.clone(),
            auth_type: dto.auth_type.clone(),
            key_id: dto.key_id,
            // Secrets never live in the JSON store; server PEMs go to 0600 files.
            pem_data: None,
            proxy_jump: dto.proxy_jump.clone(),
            group_name: dto.group_name.clone(),
            tags: dto.tags.clone(),
            is_favorite: false,
            notes: dto.notes.clone(),
            last_connected_at: None,
            created_at: Some(now.clone()),
            updated_at: Some(now),
        };
        data.next_server_id += 1;
        data.servers.push(server.clone());
        self.save(&data)?;
        Ok(server)
    }

    pub fn update_server(&self, dto: &UpdateServerDto) -> StoreResult<Server> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let server = data
            .servers
            .iter_mut()
            .find(|s| s.id == dto.id)
            .ok_or("Server not found")?;

        if let Some(v) = &dto.name {
            server.name = v.clone();
        }
        if let Some(v) = &dto.host {
            server.host = v.clone();
        }
        if let Some(v) = dto.port {
            server.port = v;
        }
        if let Some(v) = &dto.username {
            server.username = v.clone();
        }
        if let Some(v) = &dto.auth_type {
            server.auth_type = v.clone();
        }
        if dto.key_id.is_some() {
            server.key_id = dto.key_id;
        }
        // Server PEM is never stored in JSON — the command layer writes it to a
        // 0600 file. Keep it None here regardless of the DTO.
        server.pem_data = None;
        // Always authoritative — the edit form posts the full value, so this
        // also lets a jump host be cleared by submitting it empty.
        server.proxy_jump = dto.proxy_jump.clone();
        if dto.group_name.is_some() {
            server.group_name = dto.group_name.clone();
        }
        if dto.tags.is_some() {
            server.tags = dto.tags.clone();
        }
        if dto.notes.is_some() {
            server.notes = dto.notes.clone();
        }
        server.updated_at = Some(Utc::now().to_rfc3339());

        let updated = server.clone();
        self.save(&data)?;
        Ok(updated)
    }

    pub fn delete_server(&self, id: i64) -> StoreResult<()> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        data.servers.retain(|s| s.id != id);
        self.save(&data)
    }

    /// Pull any legacy plaintext server PEMs out of the JSON (one-time migration
    /// to 0600 files). Returns (server id, pem) and clears them from the store.
    pub fn take_server_pems(&self) -> StoreResult<Vec<(i64, String)>> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let mut changed = false;
        for s in data.servers.iter_mut() {
            if let Some(pem) = s.pem_data.take() {
                changed = true;
                if !pem.trim().is_empty() {
                    out.push((s.id, pem));
                }
            }
        }
        if changed {
            self.save(&data)?;
        }
        Ok(out)
    }

    pub fn toggle_favorite(&self, id: i64) -> StoreResult<Server> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let server = data
            .servers
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or("Server not found")?;
        server.is_favorite = !server.is_favorite;
        let updated = server.clone();
        self.save(&data)?;
        Ok(updated)
    }

    pub fn touch_last_connected(&self, id: i64) -> StoreResult<()> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        if let Some(server) = data.servers.iter_mut().find(|s| s.id == id) {
            server.last_connected_at = Some(Utc::now().to_rfc3339());
            self.save(&data)?;
        }
        Ok(())
    }

    // ==================== SSH Keys ====================

    pub fn list_ssh_keys(&self) -> StoreResult<Vec<SshKey>> {
        let data = self.data.lock().map_err(|e| e.to_string())?;
        let mut keys = data.keys.clone();
        keys.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(keys)
    }

    pub fn get_ssh_key(&self, id: i64) -> StoreResult<SshKey> {
        let data = self.data.lock().map_err(|e| e.to_string())?;
        data.keys
            .iter()
            .find(|k| k.id == id)
            .cloned()
            .ok_or_else(|| format!("SSH key not found: {}", id).into())
    }

    pub fn insert_ssh_key(&self, key: &NewSshKey) -> StoreResult<SshKey> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let ssh_key = SshKey {
            id: data.next_key_id,
            name: key.name.clone(),
            public_key: key.public_key.clone(),
            // Private key material is never persisted to the JSON store;
            // it lives only in the 0600 file under ssh_keys/.
            pem_data: None,
            key_type: key.key_type.clone(),
            key_size: key.key_size,
            passphrase_protected: key.passphrase_protected,
            created_at: Some(Utc::now().to_rfc3339()),
        };
        data.next_key_id += 1;
        data.keys.push(ssh_key.clone());
        self.save(&data)?;
        Ok(ssh_key)
    }

    pub fn update_ssh_key(
        &self,
        id: i64,
        name: &str,
        public_key: &str,
        key_type: &str,
        passphrase_protected: bool,
    ) -> StoreResult<SshKey> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let key = data
            .keys
            .iter_mut()
            .find(|k| k.id == id)
            .ok_or("SSH key not found")?;
        key.name = name.to_string();
        key.public_key = public_key.to_string();
        key.key_type = key_type.to_string();
        key.passphrase_protected = passphrase_protected;
        let updated = key.clone();
        self.save(&data)?;
        Ok(updated)
    }

    pub fn set_key_passphrase_protected(&self, id: i64, protected: bool) -> StoreResult<()> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        if let Some(k) = data.keys.iter_mut().find(|k| k.id == id) {
            k.passphrase_protected = protected;
            self.save(&data)?;
        }
        Ok(())
    }

    pub fn delete_ssh_key(&self, id: i64) -> StoreResult<()> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        data.keys.retain(|k| k.id != id);
        self.save(&data)
    }

    // ==================== Export / Import ====================

    pub fn export_bundle(&self) -> StoreResult<ExportBundle> {
        let data = self.data.lock().map_err(|e| e.to_string())?;
        // Defensively strip any secret material so the export file is safe to
        // sync via git/iCloud/etc.
        let servers = data
            .servers
            .iter()
            .cloned()
            .map(|mut s| {
                s.pem_data = None;
                s
            })
            .collect();
        let keys = data
            .keys
            .iter()
            .cloned()
            .map(|mut k| {
                k.pem_data = None;
                k
            })
            .collect();
        Ok(ExportBundle {
            version: 1,
            servers,
            keys,
            shortcuts: None,
        })
    }

    /// Merge an imported bundle into the store. Entries whose name already
    /// exists are skipped (never overwritten); new entries get fresh ids.
    pub fn import_bundle(&self, bundle: ExportBundle) -> StoreResult<ImportSummary> {
        let mut data = self.data.lock().map_err(|e| e.to_string())?;
        let mut summary = ImportSummary {
            servers_added: 0,
            servers_skipped: 0,
            keys_added: 0,
            keys_skipped: 0,
            shortcuts: bundle.shortcuts.clone(),
        };

        let mut server_names: HashSet<String> =
            data.servers.iter().map(|s| s.name.clone()).collect();
        for mut s in bundle.servers {
            if server_names.contains(&s.name) {
                summary.servers_skipped += 1;
                continue;
            }
            server_names.insert(s.name.clone());
            s.id = data.next_server_id;
            data.next_server_id += 1;
            s.pem_data = None;
            // The referenced key may not exist on this machine; clear the link
            // so connections fall back to default auth instead of a dangling id.
            s.key_id = None;
            data.servers.push(s);
            summary.servers_added += 1;
        }

        let mut key_names: HashSet<String> = data.keys.iter().map(|k| k.name.clone()).collect();
        for mut k in bundle.keys {
            if key_names.contains(&k.name) {
                summary.keys_skipped += 1;
                continue;
            }
            key_names.insert(k.name.clone());
            k.id = data.next_key_id;
            data.next_key_id += 1;
            k.pem_data = None;
            data.keys.push(k);
            summary.keys_added += 1;
        }

        self.save(&data)?;
        Ok(summary)
    }
}

pub struct NewSshKey {
    pub name: String,
    pub public_key: String,
    pub key_type: String,
    pub key_size: i32,
    pub passphrase_protected: bool,
}
