//! 키 관리 I/O (keys.ts 직역). 개인 키 재료는 ssh_keys/ 아래 0600 파일에만
//! 존재하며 JSON 스토어에는 절대 넣지 않는다. 생성/패스프레이즈 변경/공개 키
//! 유도는 ssh-keygen 서브프로세스로 수행한다.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CoreError;
use crate::fsutil::{rm_force, secure_write};
use crate::key_scan;
use crate::model::KeyType;
use crate::key_files::{key_file_name, server_pem_file_name};
use crate::key_type::{default_key_size, detect_key_type, normalize_creatable_key_type};
use crate::model::{CreateKeyDto, ImportKeyDto, LoadedKeyFile, SshKey, SshKeyView, UpdateKeyDto};
use crate::ops::key_ops::{KeyMetaUpdate, NewKey};
use crate::store::Store;

/// ssh-keygen 실행, stdout 반환. 실패 시 stderr trim을 에러로 노출 (JS와 동일).
fn keygen<I, S>(args: I) -> Result<String, CoreError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let out = Command::new("ssh-keygen")
        .args(args.into_iter().map(Into::into))
        .output()
        .map_err(|e| CoreError::Keygen(e.to_string()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(CoreError::Keygen(if stderr.is_empty() {
            format!("ssh-keygen failed: {}", out.status)
        } else {
            stderr
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 개인 키에서 공개 키 추출 (`ssh-keygen -y`).
fn derive_public_key(key_path: &Path, passphrase: Option<&str>) -> Result<String, CoreError> {
    let args: Vec<OsString> = vec![
        "-y".into(),
        "-f".into(),
        key_path.into(),
        "-P".into(),
        passphrase.unwrap_or("").into(),
    ];
    match keygen(args) {
        Ok(out) => Ok(out.trim().to_string()),
        Err(e) => Err(CoreError::DerivePublicKey(e.to_string())),
    }
}

fn key_path_for(keys_dir: &Path, name: &str) -> PathBuf {
    keys_dir.join(key_file_name(name))
}

fn pub_path(priv_path: &Path) -> PathBuf {
    crate::fsutil::path_with_suffix(priv_path, ".pub")
}

/// `~/.ssh`에 실제로 있는 키를 보여준다 — config를 원본으로 삼은 것과 같은
/// 원칙이다. 파일 시스템이 진실이고 사이드카는 id·패스프레이즈 여부 같은
/// 앱 메타데이터만 갖는다. 그래서 손으로 만든 `id_rsa`도 목록에 뜬다.
pub fn get_ssh_keys(store: &Store, keys_dir: &Path) -> Vec<SshKeyView> {
    let records = store.list_keys();
    let mut out: Vec<SshKeyView> = key_scan::discover_keys(keys_dir)
        .into_iter()
        .map(|d| {
            // 사이드카에 같은 파일명 기록이 있으면 id·타입·패스프레이즈를 잇는다.
            let known = records.iter().find(|k| k.name == d.file_name);
            let key = SshKey {
                id: known.map(|k| k.id).unwrap_or(0),
                name: d.file_name.clone(),
                public_key: if d.public_key.is_empty() {
                    known.map(|k| k.public_key.clone()).unwrap_or_default()
                } else {
                    d.public_key.clone()
                },
                pem_data: None,
                key_type: detect_key_type(&d.public_key)
                    .or_else(|| known.map(|k| k.key_type))
                    .unwrap_or(KeyType::Ed25519),
                key_size: known.map(|k| k.key_size).unwrap_or(0),
                // 파일에서 못 알아보면 사이드카 값을 믿는다.
                passphrase_protected: d
                    .encrypted
                    .unwrap_or_else(|| known.map(|k| k.passphrase_protected).unwrap_or(false)),
                created_at: known.and_then(|k| k.created_at.clone()),
            };
            SshKeyView { key, has_private_file: d.has_private_file }
        })
        .collect();
    // 파일이 없는 사이드카 기록도 남긴다 — 백업에서 가져왔지만 이 기기엔
    // 개인 키가 없는 경우다. 목록에서 지워 버리면 "이 기기에 개인 키 파일이
    // 없습니다" 안내가 통째로 사라진다.
    for k in records {
        if !out.iter().any(|v| v.key.name == k.name) {
            // 스캔이 형식을 못 알아본 파일일 수도 있으니 존재 여부는 직접 본다.
            let has = key_path_for(keys_dir, &k.name).exists();
            out.push(SshKeyView { key: k, has_private_file: has });
        }
    }
    out.sort_by(|a, b| a.key.name.to_lowercase().cmp(&b.key.name.to_lowercase()));
    out
}

pub fn create_ssh_key(
    store: &mut Store,
    keys_dir: &Path,
    dto: &CreateKeyDto,
) -> Result<SshKey, CoreError> {
    let key_type = normalize_creatable_key_type(&dto.key_type)?;
    let key_size = dto.key_size.unwrap_or_else(|| default_key_size(key_type.as_str()));
    let key_path = key_path_for(keys_dir, &dto.name);
    if key_path.exists() {
        return Err(CoreError::KeyFileExists(key_path.display().to_string()));
    }
    let passphrase = dto.passphrase.clone().unwrap_or_default();

    let mut args: Vec<OsString> = vec![
        "-t".into(),
        key_type.as_str().into(),
        "-f".into(),
        key_path.clone().into(),
        "-C".into(),
        "connectunnel-generated".into(),
        "-N".into(),
        passphrase.clone().into(),
    ];
    if key_type == crate::model::KeyType::Rsa {
        args.push("-b".into());
        args.push(key_size.to_string().into());
    }
    keygen(args)?;

    let public_key = fs::read_to_string(pub_path(&key_path))?.trim().to_string();
    store.insert_key(&NewKey {
        name: key_file_name(&dto.name),
        public_key,
        key_type,
        key_size,
        passphrase_protected: !passphrase.is_empty(),
    })
}

pub fn import_ssh_key(
    store: &mut Store,
    keys_dir: &Path,
    dto: &ImportKeyDto,
) -> Result<SshKey, CoreError> {
    let mut public_key = dto.public_key.trim().to_string();

    if let Some(pem) = &dto.pem_data {
        let key_path = key_path_for(keys_dir, &dto.name);
        secure_write(&key_path, pem.as_bytes())?;
        if public_key.is_empty() {
            // 암호로 보호된 키인데 passphrase가 없으면 그냥 비워둔다 (JS와 동일).
            if let Ok(derived) = derive_public_key(&key_path, dto.passphrase.as_deref()) {
                public_key = derived;
            }
        }
    }

    if public_key.is_empty() && dto.pem_data.is_none() {
        return Err(CoreError::PublicOrPemRequired);
    }

    let key_type = if !public_key.is_empty() {
        detect_key_type(&public_key).unwrap_or(dto.key_type)
    } else {
        dto.key_type
    };
    store.insert_key(&NewKey {
        name: key_file_name(&dto.name),
        public_key,
        key_type,
        key_size: 256,
        passphrase_protected: dto.passphrase.as_deref().is_some_and(|p| !p.is_empty()),
    })
}

pub fn update_ssh_key(
    store: &mut Store,
    keys_dir: &Path,
    dto: &UpdateKeyDto,
) -> Result<SshKey, CoreError> {
    let old = store.get_key(dto.id)?;
    let old_priv = key_path_for(keys_dir, &old.name);
    let new_priv = key_path_for(keys_dir, &dto.name);

    // 이름이 바뀌면 디스크의 키(및 .pub)도 함께 이동 — 접속 경로 정합성 유지.
    if key_file_name(&old.name) != key_file_name(&dto.name) {
        if new_priv.exists() {
            return Err(CoreError::KeyFileNameTaken);
        }
        if old_priv.exists() {
            fs::rename(&old_priv, &new_priv)?;
        }
        if pub_path(&old_priv).exists() {
            fs::rename(pub_path(&old_priv), pub_path(&new_priv))?;
        }
        // 접속이 `ssh <alias>`로 바뀐 뒤로 config의 `IdentityFile`이 키를
        // 지정하는 유일한 경로다 — 옛 파일을 가리키는 블록을 전부 따라 옮기지
        // 않으면 그 호스트들이 그대로 접속 불가가 된다.
        store.rename_identity_file(&old_priv, &new_priv)?;
    }

    let mut passphrase_protected = old.passphrase_protected;
    if let Some(pem) = &dto.pem_data {
        if !pem.trim().is_empty() {
            secure_write(&new_priv, pem.as_bytes())?;
            passphrase_protected = dto.passphrase.as_deref().is_some_and(|p| !p.is_empty());
        }
    }

    let public_key = dto.public_key.trim().to_string();
    let key_type = if !public_key.is_empty() {
        detect_key_type(&public_key).unwrap_or(dto.key_type)
    } else {
        dto.key_type
    };
    store.update_key_meta(&KeyMetaUpdate {
        id: dto.id,
        name: key_file_name(&dto.name),
        public_key,
        key_type,
        passphrase_protected,
    })
}

pub fn change_key_passphrase(
    store: &mut Store,
    keys_dir: &Path,
    id: i64,
    current_passphrase: Option<&str>,
    new_passphrase: Option<&str>,
) -> Result<(), CoreError> {
    let key = store.get_key(id)?;
    let path = key_path_for(keys_dir, &key.name);
    if !path.exists() {
        return Err(CoreError::PrivateFileMissing);
    }
    let next = new_passphrase.unwrap_or("");
    let args: Vec<OsString> = vec![
        "-p".into(),
        "-f".into(),
        path.into(),
        "-P".into(),
        current_passphrase.unwrap_or("").into(),
        "-N".into(),
        next.into(),
    ];
    if let Err(e) = keygen(args) {
        return Err(CoreError::ChangePassphrase(e.to_string()));
    }
    store.set_key_passphrase_protected(id, !next.is_empty())
}

pub fn delete_ssh_key(store: &mut Store, keys_dir: &Path, id: i64) -> Result<(), CoreError> {
    if let Some(key) = store.find_key(id) {
        let priv_path = key_path_for(keys_dir, &key.name);
        rm_force(&priv_path)?;
        rm_force(&pub_path(&priv_path))?;
    }
    store.delete_key(id)
}

pub fn load_key_file(path: &Path) -> Result<LoadedKeyFile, CoreError> {
    let content = fs::read_to_string(path)?;
    // node `basename(path, extname(path))` — 마지막 확장자만 제거.
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    if content.trim_start().starts_with("-----BEGIN") {
        let mut public_key = None;
        let pub_p = pub_path(path);
        if pub_p.exists() {
            public_key = Some(fs::read_to_string(&pub_p)?.trim().to_string());
        } else if let Ok(derived) = derive_public_key(path, None) {
            // 암호화된 bare 키 — import 시 passphrase와 함께 다시 유도한다.
            if !derived.is_empty() {
                public_key = Some(derived);
            }
        }
        return Ok(LoadedKeyFile { file_name, public_key, private_key: Some(content) });
    }
    Ok(LoadedKeyFile {
        file_name,
        public_key: Some(content.trim().to_string()),
        private_key: None,
    })
}

pub fn derive_public_key_from_pem(
    keys_dir: &Path,
    pem: &str,
    passphrase: Option<&str>,
) -> Result<String, CoreError> {
    if pem.trim().is_empty() {
        return Err(CoreError::EmptyPem);
    }
    let tmp = keys_dir.join(".derive.tmp");
    secure_write(&tmp, pem.as_bytes())?;
    let result = derive_public_key(&tmp, passphrase);
    let _ = rm_force(&tmp); // finally — 유도 성공/실패와 무관하게 흔적 제거
    result
}

/// `pem` 인증 서버의 PEM — 서버 id 기준 0600 파일, 스토어에는 절대 없음.
pub fn write_server_pem(keys_dir: &Path, id: i64, pem: &str) -> Result<(), CoreError> {
    secure_write(&keys_dir.join(server_pem_file_name(id)), pem.as_bytes())?;
    Ok(())
}

pub fn delete_server_pem(keys_dir: &Path, id: i64) -> Result<(), CoreError> {
    rm_force(&keys_dir.join(server_pem_file_name(id)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyType;
    use std::os::unix::fs::MetadataExt;

    struct Ctx {
        _dir: tempfile::TempDir,
        store: Store,
        keys_dir: PathBuf,
        root: PathBuf,
    }

    fn make_ctx() -> Ctx {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(
            dir.path().join("sshub.json"),
            dir.path().join(".ssh").join("config"),
            dir.path().join("ssh_keys"),
            dir.path().join("ssh_keys"),
        );
        store.load();
        let keys_dir = dir.path().join("ssh_keys");
        fs::create_dir_all(&keys_dir).unwrap();
        let root = dir.path().to_path_buf();
        Ctx { _dir: dir, store, keys_dir, root }
    }

    fn import_dto(name: &str, public_key: &str, pem: Option<&str>) -> ImportKeyDto {
        ImportKeyDto {
            name: name.into(),
            public_key: public_key.into(),
            pem_data: pem.map(Into::into),
            key_type: KeyType::Ed25519,
            passphrase: None,
        }
    }

    #[test]
    fn import_writes_the_private_key_as_a_0600_file_and_never_stores_the_secret() {
        let mut c = make_ctx();
        let k = import_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &import_dto("mykey", "ssh-ed25519 AAAA", Some("PRIVATE-KEY-MATERIAL")),
        )
        .unwrap();
        let p = key_path_for(&c.keys_dir, "mykey");
        assert_eq!(fs::read_to_string(&p).unwrap(), "PRIVATE-KEY-MATERIAL");
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert_eq!(c.store.find_key(k.id).unwrap().pem_data, None);
    }

    #[test]
    fn import_detects_key_type_from_the_public_key() {
        let mut c = make_ctx();
        let k = import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("r", "ssh-rsa AAAA", None))
            .unwrap();
        assert_eq!(k.key_type, KeyType::Rsa);
    }

    #[test]
    fn import_requires_at_least_a_public_or_private_key() {
        let mut c = make_ctx();
        let err = import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("x", "", None)).unwrap_err();
        assert_eq!(err.to_string(), "공개 키 또는 개인 키(PEM) 중 하나는 필요합니다.");
    }

    #[test]
    fn update_rename_moves_the_private_key_and_its_pub_when_the_name_changes() {
        let mut c = make_ctx();
        let k = import_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &import_dto("old", "ssh-ed25519 A", Some("PRIV")),
        )
        .unwrap();
        fs::write(pub_path(&key_path_for(&c.keys_dir, "old")), "ssh-ed25519 A").unwrap();
        update_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &UpdateKeyDto {
                id: k.id,
                name: "new".into(),
                public_key: "ssh-ed25519 A".into(),
                key_type: KeyType::Ed25519,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!key_path_for(&c.keys_dir, "old").exists());
        assert!(key_path_for(&c.keys_dir, "new").exists());
        assert!(pub_path(&key_path_for(&c.keys_dir, "new")).exists());
        assert_eq!(c.store.find_key(k.id).unwrap().name, "new");
    }

    #[test]
    fn update_rename_moves_the_identity_file_of_every_host_that_used_the_key() {
        // 별칭 접속(`ssh <alias>`)에는 `-i` 안전망이 없다 — config의
        // `IdentityFile`이 옛 경로로 남으면 그 호스트들은 접속 불가가 된다.
        let mut c = make_ctx();
        let config_path = c.root.join(".ssh").join("config");
        let key = import_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &import_dto("old name", "ssh-ed25519 AAAA", Some("PEM")),
        )
        .unwrap();
        let old_path = key_path_for(&c.keys_dir, "old name");
        let new_path = key_path_for(&c.keys_dir, "new name");

        let original = format!(
            concat!(
                "# 손으로 관리하는 파일\n",
                "Host alpha\n",
                "  HostName 1.1.1.1\n",
                "  IdentityFile {old}\n",
                "  ControlMaster auto\n",
                "\n",
                "Host beta\n",
                "  HostName 2.2.2.2\n",
                "  IdentityFile {old}\n",
                "\n",
                "Host gamma\n",
                "  IdentityFile ~/.ssh/id_rsa\n",
            ),
            old = old_path.display()
        );
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, &original).unwrap();
        c.store.load();

        update_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &UpdateKeyDto {
                id: key.id,
                name: "new name".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                key_type: KeyType::Ed25519,
                ..Default::default()
            },
        )
        .unwrap();

        let out = fs::read_to_string(&config_path).unwrap();
        // 두 블록 모두 새 경로로 옮겨졌고, 그 밖의 바이트는 전부 그대로다.
        assert_eq!(
            out,
            original.replace(&old_path.display().to_string(), &new_path.display().to_string())
        );
        assert_eq!(out.matches(&new_path.display().to_string()).count(), 2);
        assert!(new_path.exists() && !old_path.exists());
    }

    #[test]
    fn update_rename_refuses_to_overwrite_an_existing_key_file() {
        let mut c = make_ctx();
        let a = import_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &import_dto("a", "ssh-ed25519 A", Some("P")),
        )
        .unwrap();
        import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("b", "ssh-ed25519 B", Some("P")))
            .unwrap();
        let err = update_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &UpdateKeyDto {
                id: a.id,
                name: "b".into(),
                public_key: "ssh-ed25519 A".into(),
                key_type: KeyType::Ed25519,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "같은 이름의 키 파일이 이미 있습니다.");
    }

    #[test]
    fn delete_removes_the_key_files_and_the_record() {
        let mut c = make_ctx();
        let k = import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("k", "ssh-ed25519 A", Some("P")))
            .unwrap();
        delete_ssh_key(&mut c.store, &c.keys_dir, k.id).unwrap();
        assert!(!key_path_for(&c.keys_dir, "k").exists());
        assert!(c.store.find_key(k.id).is_none());
    }

    #[test]
    fn get_ssh_keys_reports_whether_each_key_has_a_private_file() {
        let mut c = make_ctx();
        import_ssh_key(
            &mut c.store,
            &c.keys_dir,
            &import_dto("withfile", "ssh-ed25519 A", Some("P")),
        )
        .unwrap();
        import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("nofile", "ssh-ed25519 B", None))
            .unwrap();
        let list = get_ssh_keys(&c.store, &c.keys_dir);
        let by_name = |n: &str| list.iter().find(|k| k.key.name == n).unwrap().has_private_file;
        assert!(by_name("withfile"));
        assert!(!by_name("nofile"));
    }

    #[test]
    fn change_passphrase_requires_the_private_file_on_this_machine() {
        let mut c = make_ctx();
        let k = import_ssh_key(&mut c.store, &c.keys_dir, &import_dto("nofile", "ssh-ed25519 A", None))
            .unwrap();
        let err = change_key_passphrase(&mut c.store, &c.keys_dir, k.id, None, Some("x"))
            .unwrap_err();
        assert_eq!(err.to_string(), "이 기기에 개인 키 파일이 없습니다.");
    }

    #[test]
    fn load_key_file_detects_a_public_key_file() {
        let c = make_ctx();
        let p = c.root.join("k.pub");
        fs::write(&p, "ssh-ed25519 AAAA comment\n").unwrap();
        let r = load_key_file(&p).unwrap();
        assert_eq!(r.public_key.as_deref(), Some("ssh-ed25519 AAAA comment"));
        assert_eq!(r.private_key, None);
        assert_eq!(r.file_name, "k"); // 마지막 확장자만 제거
    }

    #[test]
    fn load_key_file_detects_a_private_key_file_by_begin_marker() {
        let c = make_ctx();
        let p = c.root.join("id");
        fs::write(&p, "-----BEGIN OPENSSH PRIVATE KEY-----\nx\n-----END OPENSSH PRIVATE KEY-----\n")
            .unwrap();
        let r = load_key_file(&p).unwrap();
        assert!(r.private_key.unwrap().contains("BEGIN"));
    }

    #[test]
    fn derive_from_pem_rejects_an_empty_pem_and_cleans_up_its_temp_file() {
        let c = make_ctx();
        let err = derive_public_key_from_pem(&c.keys_dir, "   ", None).unwrap_err();
        assert_eq!(err.to_string(), "개인 키(PEM)가 비어 있습니다.");
        // 깨진 PEM: 에러가 나도 .derive.tmp는 남지 않는다
        let _ = derive_public_key_from_pem(&c.keys_dir, "not a key", None);
        assert!(!c.keys_dir.join(".derive.tmp").exists());
    }

    #[test]
    fn server_pem_round_trip_writes_0600_and_deletes_forcefully() {
        let c = make_ctx();
        write_server_pem(&c.keys_dir, 7, "PEM").unwrap();
        let p = c.keys_dir.join("pem_server_7");
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        delete_server_pem(&c.keys_dir, 7).unwrap();
        assert!(!p.exists());
        delete_server_pem(&c.keys_dir, 7).unwrap(); // 없어도 에러 없음
    }
}
