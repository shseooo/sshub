//! ~/.ssh/config 파일 I/O + 스토어 동기화 (sshConfigFile.ts 직역).
//! 순수 parse/render는 형제 모듈에 있고 여기는 읽기/쓰기/백업만 다룬다.
//! `_in` 변형은 디렉터리를 주입받아 테스트 가능하게 한 것 — 공개 함수는
//! `~/.ssh`를 쓴다.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CoreError;
use crate::fsutil::rm_force;
use crate::model::Server;
use crate::ssh_config::{backups_to_prune, parse_ssh_config, render_ssh_config};
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

/// 저장된 서버들로 `<dir>/config`를 덮어쓴다 (기존 파일은 백업).
pub fn sync_servers_to_config_in(dir: &Path, store: &Store) -> Result<(), CoreError> {
    let servers = store.list_servers();
    if servers.is_empty() {
        return Err(CoreError::NoServersForConfig);
    }
    fs::create_dir_all(dir)?;
    let path = dir.join("config");
    if path.exists() {
        let bak = dir.join(format!("config.bak.{}", now_stamp()));
        fs::copy(&path, bak)?;
        prune_backups(dir);
    }
    // 원자적 쓰기: temp에 렌더한 뒤 rename — 쓰기 도중 크래시가
    // ~/.ssh/config를 잘라먹지 못하게 (외부 도구들이 이 파일에 의존한다).
    let tmp = dir.join("config.tmp");
    fs::write(&tmp, render_ssh_config(&servers))?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn sync_servers_to_config(store: &Store) -> Result<(), CoreError> {
    sync_servers_to_config_in(&home_ssh_dir()?, store)
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
    use crate::model::{AuthType, CreateServerDto};

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

    #[test]
    fn refuses_to_overwrite_config_when_no_servers_are_registered() {
        let tmp = tempfile::tempdir().unwrap();
        let store = make_store(tmp.path());
        let err = sync_servers_to_config_in(&tmp.path().join(".ssh"), &store).unwrap_err();
        assert_eq!(err.to_string(), "등록된 서버가 없어 ~/.ssh/config를 덮어쓰지 않았습니다.");
    }

    #[test]
    fn writes_config_and_backs_up_the_previous_one_on_resync() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        let mut store = make_store(tmp.path());
        store.insert_server(&dto("web")).unwrap();

        sync_servers_to_config_in(&ssh_dir, &store).unwrap();
        let config = fs::read_to_string(ssh_dir.join("config")).unwrap();
        assert!(config.contains("Host web"));
        assert!(!ssh_dir.join("config.tmp").exists());

        sync_servers_to_config_in(&ssh_dir, &store).unwrap();
        let baks = fs::read_dir(&ssh_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("config.bak."))
            .count();
        assert_eq!(baks, 1);
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
}
