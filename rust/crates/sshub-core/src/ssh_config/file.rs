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
use crate::model::{AuthType, Server, SshKey};
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

/// **v1 마이그레이션 전용** 별칭 규칙 — 그룹이 있으면 `group-name`.
/// 옛 렌더러가 쓰던 규칙 그대로여야 이미 동기화해 둔 사용자에게 중복
/// 블록이 생기지 않는다. v2 이후로는 `Server::name`이 곧 별칭이다.
pub(crate) fn alias_for_v1(server: &Server) -> String {
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
pub(crate) fn identity_file_for(
    server: &Server,
    keys_dir: &Path,
    keys: &[SshKey],
) -> Option<String> {
    match server.auth_type {
        AuthType::Key => {
            let id = server.key_id?;
            let key = keys.iter().find(|k| k.id == id)?;
            Some(keys_dir.join(key_file_name(&key.name)).to_string_lossy().into_owned())
        }
        AuthType::Pem => {
            let path = keys_dir.join(server_pem_file_name(server.id));
            path.exists().then(|| path.to_string_lossy().into_owned())
        }
        AuthType::Password | AuthType::Agent => None,
    }
}

pub(crate) fn host_spec(server: &Server, keys_dir: &Path, keys: &[SshKey]) -> HostSpec {
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
        identity_file: identity_file_for(server, keys_dir, keys),
    }
}

/// config 쓰기 결과 — 호출자가 "실제로 파일이 바뀌었는가"를 구분할 수 있게.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigWrite {
    /// 렌더 결과가 디스크와 바이트 동일 — 아무것도 하지 않았다(백업도 없다).
    Unchanged,
    Written,
}

/// 문서를 `path`에 써넣는다. Phase 1의 보장 4종을 한곳에 모아둔 유일한 출구:
/// 타임스탬프 백업 → 원자적 쓰기 → 권한 보존, 그리고 내용이 같으면 no-op
/// (편집하지 않은 저장이 백업 파일만 쌓지 않게).
pub fn write_document(path: &Path, doc: &Document) -> Result<ConfigWrite, CoreError> {
    let rendered = doc.to_string();
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if existing.as_deref() == Some(rendered.as_str()) {
        return Ok(ConfigWrite::Unchanged);
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    // OpenSSH는 그룹/전체 쓰기 가능한 config를 거부한다. 사용자가 정해둔
    // 모드를 임의로 풀거나 조이지 않고 그대로 물려준다 (새 파일만 0600).
    let mode = match fs::metadata(path) {
        Ok(m) => m.permissions().mode() & 0o7777,
        Err(_) => 0o600,
    };
    if existing.is_some() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let stem = name.unwrap_or_else(|| "config".to_string());
        fs::copy(path, dir.join(format!("{stem}.bak.{}", now_stamp())))?;
        prune_backups(dir);
    }
    // 원자적 쓰기: temp에 쓴 뒤 fsync → rename — 쓰기 도중 크래시가
    // ~/.ssh/config를 잘라먹지 못하게 (외부 도구들이 이 파일에 의존한다).
    // 항상 0600으로 만든 뒤 원래 모드로 되돌린다(느슨해지는 창을 안 만든다).
    atomic_write_0600(path, rendered.as_bytes())?;
    if mode != 0o600 {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(ConfigWrite::Written)
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
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };

    // Phase 2 이후 `Server::name`이 곧 `Host` 별칭이다 (그룹 접두사는 v1
    // 마이그레이션에서만 쓴다 — 다시 붙이면 `prod-prod-web` 유령이 생긴다).
    //
    // 그리고 config가 원본이 된 지금, 이미 블록이 있는 별칭은 **건드리지
    // 않는다**. 목록의 서버는 모두 config에서 온 것이므로 이 동기화는 정상
    // 상태에서 완전한 no-op이고, 남은 쓸모는 "앱 밖에서 지워진 블록 되살리기"
    // 하나뿐이다. (건드렸다간 유추로 채운 `User user` 같은 값이 사용자의
    // 손글씨 블록에 역주입된다.)
    let keys = store.list_keys_raw().to_vec();
    let mut doc = Document::parse(&existing);
    for server in &servers {
        if doc.host(&server.name).is_some() {
            continue;
        }
        doc.upsert_host(&server.name, &host_spec(server, keys_dir, &keys));
    }
    write_document(&path, &doc)?;
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
    use crate::model::{CreateServerDto, KeyType, UpdateServerDto};
    use crate::ops::key_ops::NewKey;

    struct Ctx {
        dir: tempfile::TempDir,
    }

    impl Ctx {
        fn new() -> Ctx {
            Ctx { dir: tempfile::tempdir().unwrap() }
        }
        fn ssh(&self) -> PathBuf {
            self.dir.path().join(".ssh")
        }
        fn config_path(&self) -> PathBuf {
            self.ssh().join("config")
        }
        fn keys(&self) -> PathBuf {
            self.dir.path().join("keys")
        }
        fn write_config(&self, text: &str) {
            fs::create_dir_all(self.ssh()).unwrap();
            fs::write(self.config_path(), text).unwrap();
        }
        fn config(&self) -> String {
            fs::read_to_string(self.config_path()).unwrap()
        }
        /// 테스트는 절대 진짜 `~/.ssh/config`를 열지 않는다 — tempdir 안에 머문다.
        fn store(&self) -> Store {
            let mut s = Store::new(
                self.dir.path().join("sshub.json"),
                self.config_path(),
                self.keys(),
            );
            s.load();
            s
        }
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
        let ctx = Ctx::new();
        fs::create_dir_all(ctx.ssh()).unwrap();
        let store = ctx.store();

        sync_servers_to_config_in(&ctx.ssh(), &ctx.keys(), &store).unwrap();
        assert!(!ctx.config_path().exists());
    }

    #[test]
    fn sync_leaves_blocks_that_already_exist_untouched() {
        // config가 원본이 된 뒤로 동기화는 정상 상태에서 no-op이어야 한다 —
        // 유추로 채운 값이 사용자의 손글씨 블록에 역주입되면 안 된다.
        let ctx = Ctx::new();
        let original = "Host manual\n  HostName 1.1.1.1\n";
        ctx.write_config(original);
        let store = ctx.store();
        assert_eq!(store.list_servers().len(), 1);

        sync_servers_to_config_in(&ctx.ssh(), &ctx.keys(), &store).unwrap();
        assert_eq!(ctx.config(), original);
    }

    #[test]
    fn sync_restores_a_block_that_vanished_behind_the_apps_back() {
        // 남은 유일한 쓸모: 앱 밖에서 블록이 지워졌을 때 되살리기.
        let ctx = Ctx::new();
        let mut store = ctx.store();
        store.insert_server(&dto("web")).unwrap();
        ctx.write_config("# 사용자가 블록만 지웠다\n");

        sync_servers_to_config_in(&ctx.ssh(), &ctx.keys(), &store).unwrap();
        let out = ctx.config();
        assert!(out.contains("# 사용자가 블록만 지웠다"), "{out}");
        assert!(out.contains("Host web"), "{out}");
    }

    #[test]
    fn store_writes_preserve_hand_written_directives_comments_and_wildcards() {
        let ctx = Ctx::new();
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
        ctx.write_config(original);
        let mut store = ctx.store();
        // `Host web`은 이미 서버로 잡혀 있다 — 새로 넣는 게 아니라 고친다.
        let web = store.list_servers().into_iter().find(|s| s.name == "web").unwrap();
        store
            .update_server(&UpdateServerDto {
                id: web.id,
                host: Some("10.0.0.2".into()),
                ..Default::default()
            })
            .unwrap();
        store.insert_server(&dto("brand-new")).unwrap();

        let out = ctx.config();
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
    fn backs_up_the_previous_config_on_every_real_edit() {
        let ctx = Ctx::new();
        let mut store = ctx.store();
        store.insert_server(&dto("web")).unwrap();
        let after_first = ctx.config();
        assert!(after_first.contains("Host web"));
        assert_eq!(baks(&ctx), 0, "새 파일 생성에는 백업할 원본이 없다");

        store.insert_server(&dto("second")).unwrap();
        assert_eq!(baks(&ctx), 1);
        assert_eq!(ctx.config().matches("Host web").count(), 1);
    }

    fn baks(ctx: &Ctx) -> usize {
        fs::read_dir(ctx.ssh())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("config.bak."))
            .count()
    }

    #[test]
    fn writes_proxy_jump_and_identity_file_from_the_server() {
        let ctx = Ctx::new();
        fs::create_dir_all(ctx.keys()).unwrap();
        let mut store = ctx.store();
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

        let out = ctx.config();
        assert!(out.contains("    ProxyJump bastion"), "{out}");
        let expected = ctx.keys().join("id_work_key");
        assert!(out.contains(&format!("    IdentityFile {}", expected.display())), "{out}");
    }

    #[test]
    fn omits_identity_file_when_the_pem_is_not_on_this_machine() {
        let ctx = Ctx::new();
        fs::create_dir_all(ctx.keys()).unwrap();
        let mut store = ctx.store();
        let mut d = dto("pemmed");
        d.auth_type = AuthType::Pem;
        let server = store.insert_server(&d).unwrap();
        assert!(!ctx.config().contains("IdentityFile"));

        fs::write(ctx.keys().join(server_pem_file_name(server.id)), "PEM").unwrap();
        store
            .update_server(&UpdateServerDto { id: server.id, ..Default::default() })
            .unwrap();
        let out = ctx.config();
        assert!(out.contains(&format!(
            "IdentityFile {}",
            ctx.keys().join(server_pem_file_name(server.id)).display()
        )));
    }

    #[test]
    fn uses_the_server_name_verbatim_as_the_alias() {
        // v1의 `{group}-{name}` 규칙은 마이그레이션 전용이다 — 그룹은 이제
        // 순수 메타데이터라 별칭에 섞이면 재동기화마다 접두사가 쌓인다.
        let ctx = Ctx::new();
        let mut store = ctx.store();
        let mut d = dto("web");
        d.group_name = Some("prod".into());
        store.insert_server(&d).unwrap();

        let out = ctx.config();
        assert!(out.contains("Host web"), "{out}");
        assert!(!out.contains("prod-web"), "{out}");
        assert_eq!(store.find_server(1).unwrap().group_name.as_deref(), Some("prod"));
    }

    #[test]
    fn keeps_the_existing_permission_mode_and_uses_0600_for_new_files() {
        let ctx = Ctx::new();
        let mut store = ctx.store();
        store.insert_server(&dto("web")).unwrap();
        assert_eq!(mode_of(&ctx.config_path()), 0o600);

        fs::set_permissions(ctx.config_path(), fs::Permissions::from_mode(0o644)).unwrap();
        store.insert_server(&dto("web2")).unwrap();
        assert_eq!(mode_of(&ctx.config_path()), 0o644);
    }

    #[test]
    fn store_writes_are_purely_additive_for_hand_written_config() {
        // 사용자의 실제 config로 확인한 성질을 고정한다 — 앱이 줄을 더할 뿐
        // 어떤 줄도 없애지 않는다. (비밀번호 인증 서버 때문에 사용자의
        // IdentityFile이 지워지던 회귀가 여기서 잡힌다.)
        let ctx = Ctx::new();
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
        ctx.write_config(original);
        let mut store = ctx.store();

        // 비밀번호 인증으로 바꿔도 사용자의 IdentityFile 줄은 남는다.
        let legacy = store.list_servers().into_iter().find(|s| s.name == "legacy").unwrap();
        store
            .update_server(&UpdateServerDto {
                id: legacy.id,
                auth_type: Some(AuthType::Password),
                ..Default::default()
            })
            .unwrap();
        store.insert_server(&dto("brand-new")).unwrap();
        let merged = ctx.config();

        for line in original.lines() {
            assert!(
                merged.lines().any(|m| m == line),
                "사라진 줄: {line:?}\n결과:\n{merged}"
            );
        }
        assert!(merged.contains("Host brand-new"), "새 서버가 추가되지 않았다");
    }

    #[test]
    fn import_is_a_no_op_because_every_writable_host_is_already_a_server() {
        // config가 원본이 된 뒤 이 함수는 사실상 잉여다 — 쓰기 가능한 블록은
        // load 시점에 이미 서버로 잡혀 있다.
        let ctx = Ctx::new();
        ctx.write_config("Host dup\n  HostName x\n\nHost fresh\n  HostName y\n");
        let mut store = ctx.store();
        assert_eq!(store.list_servers().len(), 2);

        let imported = sync_config_to_servers_in(&ctx.ssh(), &mut store).unwrap();
        assert!(imported.is_empty());
    }

    #[test]
    fn import_returns_empty_when_config_is_missing() {
        let ctx = Ctx::new();
        let mut store = ctx.store();
        let imported = sync_config_to_servers_in(&ctx.ssh(), &mut store).unwrap();
        assert!(imported.is_empty());
    }

    #[test]
    fn leaves_multi_pattern_blocks_alone_and_stays_idempotent() {
        let ctx = Ctx::new();
        let original = "Host a b\n  User multi\n\nMatch all\n  User m\n";
        ctx.write_config(original);
        let mut store = ctx.store();
        // `Host a b`(패턴 두 개)는 읽기 전용이라 서버 목록에 뜨지 않는다.
        assert!(store.list_servers().is_empty());

        // 이름이 "a b"인 서버는 그와 별개다 — 따옴표로 감싼 새 블록이 된다.
        store.insert_server(&dto("a b")).unwrap();
        let once = ctx.config();
        assert!(once.starts_with(original), "{once}");
        assert!(once.contains("Host \"a b\"\n"), "{once}");

        let store = ctx.store();
        sync_servers_to_config_in(&ctx.ssh(), &ctx.keys(), &store).unwrap();
        let twice = ctx.config();
        assert_eq!(once, twice);
        assert_eq!(twice.matches("Host \"a b\"").count(), 1);
    }
}
