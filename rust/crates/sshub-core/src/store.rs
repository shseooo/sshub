//! JSON 파일 기반 스토어 (store.ts 직역). 모든 데이터가 단일 파일에 있고
//! (`~/Library/Application Support/sshub.json`), 모든 변경은 원자적으로
//! (tmp + rename) 0600 권한으로 영속화된다. 직렬화는 pretty indent-2 —
//! `JSON.stringify(data, null, 2)`와 바이트 동일.

use std::fs;
use std::path::PathBuf;

use crate::error::CoreError;
use crate::fsutil::{atomic_write_0600, path_with_suffix};
use crate::model::{
    CreateServerDto, ExportBundle, ExportFilter, ImportSummary, Server, SshKey, StoreData,
    UpdateServerDto,
};
use crate::ops::bundle_ops::{build_export_bundle, merge_bundle};
use crate::ops::key_ops::{self, KeyMetaUpdate, KeyStore, NewKey};
use crate::ops::server_ops::{self, ServerStore};
use crate::time::{now_iso, now_stamp};

/// 기본값 채움 + id 카운터를 레코드보다 앞서게 유지 + 비밀 스크럽.
pub fn normalize_data(raw: Option<StoreData>) -> StoreData {
    let raw = raw.unwrap_or_default();
    let servers = raw.servers;
    let keys: Vec<SshKey> = raw
        .keys
        .into_iter()
        .map(|mut k| {
            k.pem_data = None;
            k
        })
        .collect();
    let max_server_id = servers.iter().fold(0, |m, s| m.max(s.id));
    let max_key_id = keys.iter().fold(0, |m, k| m.max(k.id));
    StoreData {
        next_server_id: raw.next_server_id.max(max_server_id + 1),
        next_key_id: raw.next_key_id.max(max_key_id + 1),
        servers,
        keys,
    }
}

pub struct Store {
    path: PathBuf,
    data: StoreData,
}

impl Store {
    pub fn new(path: PathBuf) -> Store {
        Store { path, data: normalize_data(None) }
    }

    /// 실패하지 않는다: 손상/읽기 불가 파일은 `.corrupt.<ts>`로 보존하고 빈
    /// 상태로 부팅한다 (앱이 창을 못 여는 것보다 낫다). 비밀을 스크럽했거나
    /// 손상 파일을 교체한 경우에만 재저장한다.
    pub fn load(&mut self) {
        let mut raw: Option<StoreData> = None;
        let mut recovered = false;
        if self.path.exists() {
            match fs::read_to_string(&self.path)
                .map_err(CoreError::from)
                .and_then(|s| serde_json::from_str::<StoreData>(&s).map_err(CoreError::from))
            {
                Ok(parsed) => raw = Some(parsed),
                Err(_) => {
                    self.backup_corrupt();
                    recovered = true;
                }
            }
        }
        let had_secret = raw
            .as_ref()
            .map(|r| r.keys.iter().any(|k| k.pem_data.is_some()))
            .unwrap_or(false);
        self.data = normalize_data(raw);
        if had_secret || recovered {
            // load는 실패하지 않는 계약 — 재저장은 best-effort.
            let _ = self.save();
        }
    }

    /// 손상 스토어를 `sshub.json.corrupt.<timestamp>`로 비켜 놓는다 (best-effort).
    fn backup_corrupt(&self) {
        let dest = path_with_suffix(&self.path, &format!(".corrupt.{}", now_stamp()));
        let _ = fs::copy(&self.path, dest);
    }

    fn save(&self) -> Result<(), CoreError> {
        let json = serde_json::to_string_pretty(&self.data)?;
        atomic_write_0600(&self.path, json.as_bytes())?;
        Ok(())
    }

    fn server_slice(&self) -> ServerStore {
        ServerStore {
            servers: self.data.servers.clone(),
            next_server_id: self.data.next_server_id,
        }
    }

    fn key_slice(&self) -> KeyStore {
        KeyStore { keys: self.data.keys.clone(), next_key_id: self.data.next_key_id }
    }

    // ==================== Servers ====================

    pub fn list_servers(&self) -> Vec<Server> {
        server_ops::list_servers(&self.data.servers)
    }

    pub fn find_server(&self, id: i64) -> Option<Server> {
        server_ops::find_server(&self.data.servers, id).cloned()
    }

    pub fn insert_server(&mut self, dto: &CreateServerDto) -> Result<Server, CoreError> {
        let (store, server) = server_ops::insert_server(&self.server_slice(), dto, &now_iso());
        self.data.servers = store.servers;
        self.data.next_server_id = store.next_server_id;
        self.save()?;
        Ok(server)
    }

    pub fn update_server(&mut self, dto: &UpdateServerDto) -> Result<Server, CoreError> {
        let (store, server) = server_ops::update_server(&self.server_slice(), dto, &now_iso())?;
        self.data.servers = store.servers;
        self.save()?;
        Ok(server)
    }

    pub fn delete_server(&mut self, id: i64) -> Result<(), CoreError> {
        self.data.servers = server_ops::delete_server(&self.server_slice(), id).servers;
        self.save()
    }

    pub fn toggle_favorite(&mut self, id: i64) -> Result<Server, CoreError> {
        let (store, server) = server_ops::toggle_favorite(&self.server_slice(), id)?;
        self.data.servers = store.servers;
        self.save()?;
        Ok(server)
    }

    pub fn touch_last_connected(&mut self, id: i64) -> Result<(), CoreError> {
        if let Some(s) = self.data.servers.iter_mut().find(|s| s.id == id) {
            s.last_connected_at = Some(now_iso());
            self.save()?;
        }
        Ok(())
    }

    // ==================== SSH Keys ====================

    pub fn list_keys(&self) -> Vec<SshKey> {
        key_ops::list_keys(&self.data.keys)
    }

    pub fn find_key(&self, id: i64) -> Option<SshKey> {
        key_ops::find_key(&self.data.keys, id).cloned()
    }

    pub fn get_key(&self, id: i64) -> Result<SshKey, CoreError> {
        self.find_key(id).ok_or(CoreError::KeyNotFoundId(id))
    }

    pub fn insert_key(&mut self, nk: &NewKey) -> Result<SshKey, CoreError> {
        let (store, key) = key_ops::insert_key(&self.key_slice(), nk, &now_iso());
        self.data.keys = store.keys;
        self.data.next_key_id = store.next_key_id;
        self.save()?;
        Ok(key)
    }

    pub fn update_key_meta(&mut self, u: &KeyMetaUpdate) -> Result<SshKey, CoreError> {
        let (store, key) = key_ops::update_key_meta(&self.key_slice(), u)?;
        self.data.keys = store.keys;
        self.save()?;
        Ok(key)
    }

    pub fn set_key_passphrase_protected(&mut self, id: i64, protected: bool) -> Result<(), CoreError> {
        self.data.keys = key_ops::set_passphrase_protected(&self.key_slice(), id, protected).keys;
        self.save()
    }

    pub fn delete_key(&mut self, id: i64) -> Result<(), CoreError> {
        self.data.keys = key_ops::delete_key(&self.key_slice(), id).keys;
        self.save()
    }

    // ==================== Export / Import ====================

    pub fn export_bundle(&self, filter: &ExportFilter) -> ExportBundle {
        build_export_bundle(&self.data, filter)
    }

    pub fn import_bundle(&mut self, bundle: &ExportBundle) -> Result<ImportSummary, CoreError> {
        let (data, summary) = merge_bundle(&self.data, bundle);
        self.data = data;
        self.save()?;
        Ok(summary)
    }

    /// 테스트/점검용 — 정렬 없는 원본 키 목록 (비밀은 이미 스크럽됨).
    pub fn list_keys_raw(&self) -> &[SshKey] {
        &self.data.keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthType, KeyType};
    use std::os::unix::fs::MetadataExt;

    fn dto(name: &str) -> CreateServerDto {
        CreateServerDto {
            name: name.into(),
            host: "h".into(),
            username: "u".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    // ---- normalizeData ----

    #[test]
    fn normalize_fills_defaults_for_an_empty_absent_file() {
        let d = normalize_data(None);
        assert_eq!(d.next_server_id, 1);
        assert_eq!(d.next_key_id, 1);
        assert!(d.servers.is_empty());
        assert!(d.keys.is_empty());
    }

    #[test]
    fn normalize_keeps_id_counters_ahead_of_existing_records() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "nextServerId": 2,
            "servers": [{"id": 3}, {"id": 7}],
            "keys": [{"id": 5}],
        }))
        .unwrap();
        let d = normalize_data(Some(raw));
        assert_eq!(d.next_server_id, 8); // max(2, 7+1)
        assert_eq!(d.next_key_id, 6); // max(0, 5+1)
    }

    #[test]
    fn normalize_does_not_lower_a_counter_that_is_already_ahead() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "nextServerId": 100,
            "servers": [{"id": 3}],
        }))
        .unwrap();
        assert_eq!(normalize_data(Some(raw)).next_server_id, 100);
    }

    #[test]
    fn normalize_scrubs_any_private_key_material() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "keys": [{"id": 1, "pemData": "PRIVATE"}],
        }))
        .unwrap();
        assert_eq!(normalize_data(Some(raw)).keys[0].pem_data, None);
    }

    // ---- Store (file-backed, atomic, 0600) ----

    #[test]
    fn persists_an_inserted_server_and_reloads_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        let mut s = Store::new(path.clone());
        s.load();
        let server = s.insert_server(&dto("web")).unwrap();
        assert_eq!(server.id, 1);

        let mut reloaded = Store::new(path);
        reloaded.load();
        let names: Vec<String> = reloaded.list_servers().into_iter().map(|x| x.name).collect();
        assert_eq!(names, vec!["web"]);
    }

    #[test]
    fn writes_the_file_with_0600_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        let mut s = Store::new(path.clone());
        s.load();
        s.insert_server(&dto("web")).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn scrubs_secrets_present_in_an_existing_file_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        fs::write(&path, r#"{"keys":[{"id":1,"pemData":"LEAK"}]}"#).unwrap();
        let mut s = Store::new(path);
        s.load();
        assert_eq!(s.list_keys_raw()[0].pem_data, None);
    }

    #[test]
    fn recovers_from_a_corrupt_file_boots_empty_preserves_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        fs::write(&path, "{ this is not valid json ").unwrap();
        let mut s = Store::new(path.clone());
        s.load(); // 패닉/에러 없이 부팅해야 한다
        assert!(s.list_servers().is_empty());
        // 복구용 .corrupt.* 백업이 남는다
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(backups.len(), 1);
        // 이제 스토어는 유효하고 재로드 가능한 파일이다
        let mut reloaded = Store::new(path);
        reloaded.load();
        assert!(reloaded.list_servers().is_empty());
    }

    #[test]
    fn toggle_favorite_flips_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        let mut s = Store::new(path.clone());
        s.load();
        let srv = s.insert_server(&dto("web")).unwrap();
        s.toggle_favorite(srv.id).unwrap();
        let mut reloaded = Store::new(path);
        reloaded.load();
        assert_eq!(reloaded.find_server(srv.id).map(|x| x.is_favorite), Some(true));
    }

    #[test]
    fn get_key_error_includes_the_id() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path().join("sshub.json"));
        assert_eq!(s.get_key(42).unwrap_err().to_string(), "SSH key not found: 42");
    }

    #[test]
    fn load_does_not_create_a_file_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        let mut s = Store::new(path.clone());
        s.load();
        assert!(!path.exists());
    }

    #[test]
    fn saved_file_matches_json_stringify_indent2_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub.json");
        let mut s = Store::new(path.clone());
        s.load();
        s.insert_key(&NewKey {
            name: "k".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            key_type: KeyType::Ed25519,
            key_size: 256,
            passphrase_protected: false,
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("{\n  \"nextServerId\": 1,\n  \"nextKeyId\": 2,\n  \"servers\": [],\n  \"keys\": [\n    {\n      \"id\": 1,"));
        assert!(!text.ends_with('\n')); // JSON.stringify는 개행을 붙이지 않는다
    }
}
