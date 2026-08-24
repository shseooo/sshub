//! 백업 export/import I/O (backup.ts 직역). 평문 export는 비밀 없는 pretty-2
//! JSON, passphrase export는 개인 키 파일까지 묶어 통째로 암호화한다.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::crypto::{decrypt_bundle, encrypt_bundle, is_encrypted_envelope};
use crate::error::CoreError;
use crate::fsutil::secure_write;
use crate::key_files::key_file_name;
use crate::model::{ExportFilter, ImportSummary, PrivateKeyEntry, SecureBundle};
use crate::store::Store;

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub passphrase: Option<String>,
    pub shortcuts: Option<BTreeMap<String, String>>,
    pub server_ids: Option<Vec<i64>>,
    pub key_ids: Option<Vec<i64>>,
}

pub fn export_data(
    store: &Store,
    keys_dir: &Path,
    path: &Path,
    opts: &ExportOptions,
) -> Result<(), CoreError> {
    let bundle = store.export_bundle(&ExportFilter {
        server_ids: opts.server_ids.clone(),
        key_ids: opts.key_ids.clone(),
        shortcuts: opts.shortcuts.clone(),
    });

    match opts.passphrase.as_deref() {
        Some(pass) if !pass.is_empty() => {
            let mut private_keys = Vec::new();
            for key in &bundle.keys {
                let key_path = keys_dir.join(key_file_name(&key.name));
                if key_path.exists() {
                    private_keys.push(PrivateKeyEntry {
                        name: key.name.clone(),
                        pem: fs::read_to_string(&key_path)?,
                    });
                }
            }
            let secure = SecureBundle { bundle, private_keys };
            fs::write(path, encrypt_bundle(&serde_json::to_string(&secure)?, pass)?)?;
        }
        _ => {
            fs::write(path, serde_json::to_string_pretty(&bundle)?)?;
        }
    }
    Ok(())
}

pub fn import_data(
    store: &mut Store,
    keys_dir: &Path,
    path: &Path,
    passphrase: Option<&str>,
) -> Result<ImportSummary, CoreError> {
    let text = fs::read_to_string(path)?;

    if is_encrypted_envelope(&text) {
        let pass = match passphrase {
            Some(p) if !p.is_empty() => p,
            _ => return Err(CoreError::NeedsPassphrase),
        };
        let secure: SecureBundle = serde_json::from_str(&decrypt_bundle(&text, pass)?)?;
        let summary = store.import_bundle(&secure.bundle)?;
        // 이 기기에 없는 개인 키 파일만 복원한다 (0600) — 기존 키를 덮지 않는다.
        for entry in &secure.private_keys {
            let key_path = keys_dir.join(key_file_name(&entry.name));
            if !key_path.exists() {
                secure_write(&key_path, entry.pem.as_bytes())?;
            }
        }
        return Ok(summary);
    }

    store.import_bundle(&serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthType, CreateServerDto, KeyType};
    use crate::ops::key_ops::NewKey;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    struct Ctx {
        _dir: tempfile::TempDir,
        store: Store,
        keys_dir: PathBuf,
        out: PathBuf,
    }

    fn make_ctx() -> Ctx {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(
            dir.path().join("sshub.json"),
            dir.path().join(".ssh").join("config"),
            dir.path().join("ssh_keys"),
        );
        store.load();
        let keys_dir = dir.path().join("ssh_keys");
        fs::create_dir_all(&keys_dir).unwrap();
        let out = dir.path().join("export");
        Ctx { _dir: dir, store, keys_dir, out }
    }

    fn srv_dto(name: &str) -> CreateServerDto {
        CreateServerDto {
            name: name.into(),
            host: "h".into(),
            username: "u".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    fn key_nk(name: &str, public_key: &str) -> NewKey {
        NewKey {
            name: name.into(),
            public_key: public_key.into(),
            key_type: KeyType::Ed25519,
            key_size: 256,
            passphrase_protected: true,
        }
    }

    #[test]
    fn writes_a_secret_free_json_export() {
        let mut a = make_ctx();
        a.store.insert_server(&srv_dto("s")).unwrap();
        a.store
            .insert_key(&NewKey { passphrase_protected: false, ..key_nk("k", "p") })
            .unwrap();
        export_data(&a.store, &a.keys_dir, &a.out, &ExportOptions::default()).unwrap();
        let txt = fs::read_to_string(&a.out).unwrap();
        assert!(!is_encrypted_envelope(&txt));
        let j: serde_json::Value = serde_json::from_str(&txt).unwrap();
        assert_eq!(j["servers"][0]["name"], "s");
        assert_eq!(j["servers"][0]["pemData"], serde_json::Value::Null);
        assert_eq!(j["keys"][0]["name"], "k");
        assert_eq!(j["keys"][0]["pemData"], serde_json::Value::Null);
    }

    #[test]
    fn merges_a_plain_export_skipping_names_that_already_exist() {
        let mut a = make_ctx();
        a.store.insert_server(&srv_dto("s1")).unwrap();
        export_data(&a.store, &a.keys_dir, &a.out, &ExportOptions::default()).unwrap();

        let mut b = make_ctx();
        b.store.insert_server(&srv_dto("s1")).unwrap(); // 중복 이름
        let sum = import_data(&mut b.store, &b.keys_dir, &a.out, None).unwrap();
        assert_eq!(sum.servers_added, 0);
        assert_eq!(sum.servers_skipped, 1);
    }

    #[test]
    fn encrypts_then_restores_metadata_and_the_0600_private_key_file() {
        let mut a = make_ctx();
        a.store.insert_key(&key_nk("mk", "ssh-ed25519 A")).unwrap();
        secure_write(&a.keys_dir.join(key_file_name("mk")), b"PRIVATE-KEY").unwrap();

        let opts = ExportOptions { passphrase: Some("pw".into()), ..Default::default() };
        export_data(&a.store, &a.keys_dir, &a.out, &opts).unwrap();
        assert!(is_encrypted_envelope(&fs::read_to_string(&a.out).unwrap()));

        let mut b = make_ctx();
        let sum = import_data(&mut b.store, &b.keys_dir, &a.out, Some("pw")).unwrap();
        assert_eq!(sum.keys_added, 1);
        let restored = b.keys_dir.join(key_file_name("mk"));
        assert_eq!(fs::read_to_string(&restored).unwrap(), "PRIVATE-KEY");
        assert_eq!(fs::metadata(&restored).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn rejects_an_encrypted_file_imported_without_a_passphrase() {
        let mut a = make_ctx();
        a.store.insert_key(&key_nk("mk", "p")).unwrap();
        secure_write(&a.keys_dir.join(key_file_name("mk")), b"P").unwrap();
        let opts = ExportOptions { passphrase: Some("pw".into()), ..Default::default() };
        export_data(&a.store, &a.keys_dir, &a.out, &opts).unwrap();

        let mut b = make_ctx();
        let err = import_data(&mut b.store, &b.keys_dir, &a.out, None).unwrap_err();
        assert_eq!(err.to_string(), "ENCRYPTED");
        // 빈 passphrase도 동일하게 거부
        let err = import_data(&mut b.store, &b.keys_dir, &a.out, Some("")).unwrap_err();
        assert_eq!(err.to_string(), "ENCRYPTED");
    }

    #[test]
    fn rejects_a_wrong_passphrase() {
        let mut a = make_ctx();
        a.store.insert_key(&key_nk("mk", "p")).unwrap();
        secure_write(&a.keys_dir.join(key_file_name("mk")), b"P").unwrap();
        let opts = ExportOptions { passphrase: Some("right".into()), ..Default::default() };
        export_data(&a.store, &a.keys_dir, &a.out, &opts).unwrap();

        let mut b = make_ctx();
        let err = import_data(&mut b.store, &b.keys_dir, &a.out, Some("wrong")).unwrap_err();
        assert_eq!(err.to_string(), "복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.");
    }
}
