//! ~/.ssh/config 파일 I/O + 스토어 동기화.
//! 순수 파싱/문서 모델은 형제 모듈에 있고 여기는 읽기/병합/쓰기/백업만 다룬다.
//! `_in` 변형은 디렉터리를 주입받아 테스트 가능하게 한 것 — 공개 함수는
//! `~/.ssh`를 쓴다.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::CoreError;
use crate::fsutil::{atomic_write_0600, rm_force};
use crate::key_files::{key_file_name, server_pem_file_name};
use crate::model::{AuthType, Server};
use crate::ssh_config::document::{Document, HostSpec};
use crate::ssh_config::{backups_to_prune, parse_ssh_config};
use crate::store::Store;
use crate::time::now_stamp;

const MAX_CONFIG_BACKUPS: usize = 10;

fn home_ssh_dir() -> Result<PathBuf, CoreError> {
    Ok(dirs::home_dir().ok_or(CoreError::NoHomeDir)?.join(".ssh"))
}

/// 최신 MAX_CONFIG_BACKUPS개의 `config.bak.*`만 남긴다 (best-effort).
fn prune_backups(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for f in backups_to_prune(&names, MAX_CONFIG_BACKUPS) {
        let _ = rm_force(&dir.join(f));
    }
}

/// config에 쓸 `Host` 별칭 — 그룹이 있으면 `group-name`. 기존 규칙 그대로
/// 유지해야 이미 동기화한 사용자에게 중복 블록이 생기지 않는다.
fn alias_for(server: &Server) -> String {
    let group = server.group_name.as_deref().unwrap_or("").trim();
    if group.is_empty() {
        server.name.clone()
    } else {
        format!("{group}-{}", server.name)
    }
}

/// 이 서버로 접속할 때 실제로 쓰는 개인 키 경로. 키 레코드가 없거나 PEM
/// 파일이 아직 없으면 `None` — 존재하지 않는 파일을 `IdentityFile`로 박아
/// 두면 ssh가 그 키만 시도하다 실패한다.
fn identity_file_for(server: &Server, keys_dir: &Path, store: &Store) -> Option<String> {
    match server.auth_type {
        AuthType::Key => {
            let key = store.find_key(server.key_id?)?;
            Some(keys_dir.join(key_file_name(&key.name)).to_string_lossy().into_owned())
        }
        AuthType::Pem => {
            let path = keys_dir.join(server_pem_file_name(server.id));
            path.exists().then(|| path.to_string_lossy().into_owned())
        }
        AuthType::Password | AuthType::Agent => None,
    }
}

fn spec_for(server: &Server, keys_dir: &Path, store: &Store) -> HostSpec {
    HostSpec {
        host_name: Some(server.host.clone()),
        // i64 → u16 범위를 벗어난 값은 ssh가 거부하므로 줄을 쓰지 않는다.
        port: u16::try_from(server.port).ok(),
        user: Some(server.username.clone()),
        proxy_jump: server
            .proxy_jump
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        identity_file: identity_file_for(server, keys_dir, store),
    }
}

/// 저장된 서버들을 `<dir>/config`에 **병합**한다 (덮어쓰지 않는다).
/// 앱이 소유한 다섯 지시어만 갱신되고 나머지 지시어·주석·`Include`·`Match`는
/// 바이트 그대로 남는다.
pub fn sync_servers_to_config_in(
    dir: &Path,
    keys_dir: &Path,
    store: &Store,
) -> Result<(), CoreError> {
    let servers = store.list_servers();
    // 서버가 없으면 할 일이 없다. (예전에는 파일을 통째로 덮어썼기 때문에
    // "빈 목록으로 config를 날리는" 사고를 막는 에러가 필요했지만, 이제는
    // 어떤 경우에도 기존 내용을 지우지 않으므로 조용한 no-op이면 충분하다.)
    if servers.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    let path = dir.join("config");

    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    // OpenSSH는 그룹/전체 쓰기 가능한 config를 거부한다. 사용자가 정해둔
    // 모드를 임의로 풀거나 조이지 않고 그대로 물려준다 (새 파일만 0600).
    let mode = match fs::metadata(&path) {
        Ok(m) => m.permissions().mode() & 0o7777,
        Err(_) => 0o600,
    };

    let mut doc = Document::parse(existing.as_deref().unwrap_or(""));
    for server in &servers {
        doc.upsert_host(&alias_for(server), &spec_for(server, keys_dir, store));
    }
    let rendered = doc.to_string();

    if existing.is_some() {
        let bak = dir.join(format!("config.bak.{}", now_stamp()));
        fs::copy(&path, bak)?;
        prune_backups(dir);
    }
    // 원자적 쓰기: temp에 쓴 뒤 fsync → rename — 쓰기 도중 크래시가
    // ~/.ssh/config를 잘라먹지 못하게 (외부 도구들이 이 파일에 의존한다).
    // 항상 0600으로 만든 뒤 원래 모드로 되돌린다(느슨해지는 창을 안 만든다).
    atomic_write_0600(&path, rendered.as_bytes())?;
    if mode != 0o600 {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

pub fn sync_servers_to_config(store: &Store, keys_dir: &Path) -> Result<(), CoreError> {
    sync_servers_to_config_in(&home_ssh_dir()?, keys_dir, store)
}

/// `<dir>/config`의 호스트를 import; 이미 있는 이름은 스킵.
pub fn sync_config_to_servers_in(dir: &Path, store: &mut Store) -> Result<Vec<Server>, CoreError> {
    let path = dir.join("config");
    if !path.exists() {
        return Ok(vec![]);
    }
    let entries = parse_ssh_config(&fs::read_to_string(&path)?);
    let existing: std::collections::HashSet<String> =
        store.list_servers().into_iter().map(|s| s.name).collect();
    let mut imported = Vec::new();
    for entry in entries {
        if existing.contains(&entry.name) {
            continue;
        }
        imported.push(store.insert_server(&entry)?);
    }
    Ok(imported)
}

pub fn sync_config_to_servers(store: &mut Store) -> Result<Vec<Server>, CoreError> {
    sync_config_to_servers_in(&home_ssh_dir()?, store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CreateServerDto, KeyType};
    use crate::ops::key_ops::NewKey;

    fn make_store(dir: &Path) -> Store {
        let mut s = Store::new(dir.join("sshub.json"));
        s.load();
        s
    }

    fn dto(name: &str) -> CreateServerDto {
        CreateServerDto {
            name: name.into(),
            host: "h".into(),
            username: "u".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn does_nothing_when_no_servers_are_registered() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        let original = "Host manual\n  HostName 1.1.1.1\n";
        fs::write(ssh_dir.join("config"), original).unwrap();
        let store = make_store(tmp.path());

        sync_servers_to_config_in(&ssh_dir, &tmp.path().join("keys"), &store).unwrap();
        assert_eq!(fs::read_to_string(ssh_dir.join("config")).unwrap(), original);
    }

    #[test]
    fn writes_config_and_backs_up_the_previous_one_on_resync() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let keys = tmp.path().join("keys");
        let mut store = make_store(tmp.path());
        store.insert_server(&dto("web")).unwrap();

        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        let config = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(config.contains("Host web"));
        assert!(!ssh_dir.join("config.tmp").exists());

        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        let baks = fs::read_dir(&ssh_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("config.bak."))
            .count();
        assert_eq!(baks, 1);
        // 두 번째 동기화가 블록을 복제하지 않는다.
        let again = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert_eq!(again.matches("Host web").count(), 1);
        assert_eq!(again, config);
    }

    #[test]
    fn preserves_hand_written_directives_comments_and_wildcards() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        let original = "\
# 손으로 관리하는 설정
Include ~/.ssh/conf.d/*.conf

Host web
  HostName 10.0.0.1
  ControlMaster auto
  # 이 주석은 살아 있어야 한다

Host *
  ServerAliveInterval 60
";
        fs::write(ssh_dir.join("config"), original).unwrap();
        let mut store = make_store(tmp.path());
        let mut d = dto("web");
        d.host = "10.0.0.2".into();
        store.insert_server(&d).unwrap();
        store.insert_server(&dto("brand-new")).unwrap();

        sync_servers_to_config_in(&ssh_dir, &tmp.path().join("keys"), &store).unwrap();
        let out = fs::read_to_string(ssh_dir.join("config")).unwrap();

        assert!(out.contains("# 손으로 관리하는 설정"));
        assert!(out.contains("Include ~/.ssh/conf.d/*.conf"));
        assert!(out.contains("  ControlMaster auto"));
        assert!(out.contains("  # 이 주석은 살아 있어야 한다"));
        assert!(out.contains("Host *\n  ServerAliveInterval 60"));
        assert!(out.contains("  HostName 10.0.0.2"));
        // 새 블록은 `Host *`보다 앞에 와야 가려지지 않는다.
        assert!(out.find("Host brand-new").unwrap() < out.find("Host *").unwrap());
    }

    #[test]
    fn writes_proxy_jump_and_identity_file_from_the_server() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let keys = tmp.path().join("keys");
        fs::create_dir_all(&keys).unwrap();
        let mut store = make_store(tmp.path());
        let key = store
            .insert_key(&NewKey {
                name: "work key".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                key_type: KeyType::Ed25519,
                key_size: 256,
                passphrase_protected: false,
            })
            .unwrap();
        let mut d = dto("jumped");
        d.proxy_jump = Some("  bastion  ".into());
        d.key_id = Some(key.id);
        store.insert_server(&d).unwrap();

        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        let out = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(out.contains("    ProxyJump bastion"), "{out}");
        let expected = keys.join("id_work_key");
        assert!(out.contains(&format!("    IdentityFile {}", expected.display())), "{out}");
    }

    #[test]
    fn omits_identity_file_when_the_pem_is_not_on_this_machine() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let keys = tmp.path().join("keys");
        fs::create_dir_all(&keys).unwrap();
        let mut store = make_store(tmp.path());
        let mut d = dto("pemmed");
        d.auth_type = AuthType::Pem;
        let server = store.insert_server(&d).unwrap();

        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        assert!(!fs::read_to_string(ssh_dir.join("config")).unwrap().contains("IdentityFile"));

        fs::write(keys.join(server_pem_file_name(server.id)), "PEM").unwrap();
        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        let out = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(out.contains(&format!(
            "IdentityFile {}",
            keys.join(server_pem_file_name(server.id)).display()
        )));
    }

    #[test]
    fn prefixes_the_alias_with_the_group_when_set() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let mut store = make_store(tmp.path());
        let mut d = dto("web");
        d.group_name = Some("prod".into());
        store.insert_server(&d).unwrap();
        let mut e = dto("solo");
        e.group_name = Some(String::new());
        store.insert_server(&e).unwrap();

        sync_servers_to_config_in(&ssh_dir, &tmp.path().join("keys"), &store).unwrap();
        let out = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(out.contains("Host prod-web"));
        assert!(out.contains("Host solo"));
    }

    #[test]
    fn keeps_the_existing_permission_mode_and_uses_0600_for_new_files() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let keys = tmp.path().join("keys");
        let mut store = make_store(tmp.path());
        store.insert_server(&dto("web")).unwrap();

        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        assert_eq!(mode_of(&ssh_dir.join("config")), 0o600);

        fs::set_permissions(&ssh_dir.join("config"), fs::Permissions::from_mode(0o644)).unwrap();
        sync_servers_to_config_in(&ssh_dir, &keys, &store).unwrap();
        assert_eq!(mode_of(&ssh_dir.join("config")), 0o644);
    }

    #[test]
    fn sync_is_purely_additive_for_hand_written_config() {
        // 사용자의 실제 config로 확인한 성질을 고정한다 — 동기화는 줄을 더할 뿐
        // 어떤 줄도 없애지 않는다. (비밀번호 인증 서버 때문에 사용자의
        // IdentityFile이 지워지던 회귀가 여기서 잡힌다.)
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let original = concat!(
            "# 개인 설정\n",
            "Host *\n",
            "  AddKeysToAgent yes\n",
            "  IdentityFile ~/.ssh/id_rsa\n",
            "\n",
            "Host legacy\n",
            "  HostName old.example.com\n",
            "  User root\n",
            "  IdentityFile ~/.ssh/id_rsa\n",
            "  IdentityFile ~/.ssh/id_backup\n",
            "  ControlMaster auto\n",
        );
        std::fs::write(ssh.join("config"), original).unwrap();

        let mut store = Store::new(dir.path().join("sshub.json"));
        // 비밀번호 인증 — 앱에는 키가 없다.
        let mut d = dto("legacy");
        d.host = "old.example.com".into();
        d.username = "root".into();
        d.auth_type = AuthType::Password;
        store.insert_server(&d).unwrap();
        store.insert_server(&dto("brand-new")).unwrap();

        sync_servers_to_config_in(&ssh, &dir.path().join("keys"), &store).unwrap();
        let merged = std::fs::read_to_string(ssh.join("config")).unwrap();

        for line in original.lines() {
            assert!(
                merged.lines().any(|m| m == line),
                "사라진 줄: {line:?}\n결과:\n{merged}"
            );
        }
        assert!(merged.contains("Host brand-new"), "새 서버가 추가되지 않았다");
    }

    #[test]
    fn imports_hosts_skipping_names_that_already_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        fs::write(
            ssh_dir.join("config"),
            "Host dup\n  HostName x\n\nHost fresh\n  HostName y\n",
        )
        .unwrap();
        let mut store = make_store(tmp.path());
        store.insert_server(&dto("dup")).unwrap();

        let imported = sync_config_to_servers_in(&ssh_dir, &mut store).unwrap();
        let names: Vec<String> = imported.into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["fresh"]);
    }

    #[test]
    fn import_returns_empty_when_config_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = make_store(tmp.path());
        let imported = sync_config_to_servers_in(&tmp.path().join(".ssh"), &mut store).unwrap();
        assert!(imported.is_empty());
    }

    #[test]
    fn leaves_multi_pattern_blocks_alone_and_stays_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        fs::create_dir_all(&ssh_dir).unwrap();
        let original = "Host a b\n  User multi\n\nMatch all\n  User m\n";
        fs::write(ssh_dir.join("config"), original).unwrap();
        let mut store = make_store(tmp.path());
        store.insert_server(&dto("a b")).unwrap();

        // `Host a b`(패턴 두 개)는 이름이 "a b"인 서버와 다른 것이므로 손대지
        // 않는다. 대신 따옴표로 감싼 별도 블록을 만들고, 다시 읽을 수 있으므로
        // 재동기화해도 중복되지 않는다.
        sync_servers_to_config_in(&ssh_dir, &tmp.path().join("keys"), &store).unwrap();
        let once = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(once.starts_with(original), "{once}");
        assert!(once.contains("Host \"a b\"\n"), "{once}");

        sync_servers_to_config_in(&ssh_dir, &tmp.path().join("keys"), &store).unwrap();
        let twice = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert_eq!(once, twice);
        assert_eq!(twice.matches("Host \"a b\"").count(), 1);
    }
}
