//! ~/.ssh/config 파일 쓰기 + 백업.
//! 순수 파싱/문서 모델은 형제 모듈에 있고 여기는 쓰기/백업/경로 헬퍼만 다룬다.
//!
//! Phase 3에서 "서버 → config"·"config → 서버" 일괄 동기화 두 함수를 걷어냈다.
//! config가 곧 스토어가 된 이상 동기화할 상대가 없다 — 정상 상태에서 완전한
//! no-op이면서 실패했을 때만 사용자의 손글씨에 값을 역주입할 수 있는,
//! 위험만 남은 코드였다. 외부 편집은 `Store::reload_if_changed`가 받는다.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::CoreError;
use crate::fsutil::{atomic_write_0600, rm_force};
use crate::key_files::{safe_file_component, server_pem_file_name};
use crate::model::{AuthType, Server, SshKey};
use crate::ssh_config::document::{Document, HostSpec};
use crate::ssh_config::backups_to_prune;
use crate::time::now_stamp;

const MAX_CONFIG_BACKUPS: usize = 10;

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

/// `IdentityFile` 값을 비교 가능한 절대 경로로 편다.
///
/// 사람이 쓴 config는 거의 항상 `~/.ssh/id_rsa` 꼴이고, 앱은 절대 경로를 쓴다.
/// 두 표기를 같은 것으로 보지 못하면 "이 키를 쓰는 호스트"를 영영 못 찾는다
/// (이름 변경 시 접속 경로 추적, 삭제 시 영향 호스트 수 모두 여기에 걸린다).
///
/// `~`는 진짜 홈 디렉터리를 읽는 대신 **`keys_dir`의 부모**로 편다 —
/// `keys_dir`가 `<home>/.ssh`라는 사실을 이용한 것으로, 테스트가 임시
/// 디렉터리 안에 완전히 갇힌 채로도 같은 규칙이 성립한다.
pub fn resolve_identity_path(raw: &str, keys_dir: &Path) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"');
    match trimmed.strip_prefix("~/") {
        Some(rest) => match keys_dir.parent() {
            Some(home) => home.join(rest),
            None => PathBuf::from(trimmed),
        },
        None => PathBuf::from(trimmed),
    }
}

/// 이 서버로 접속할 때 실제로 쓰는 개인 키 경로. 키 레코드가 없거나 PEM
/// 파일이 아직 없으면 `None` — 존재하지 않는 파일을 `IdentityFile`로 박아
/// 두면 ssh가 그 키만 시도하다 실패한다.
///
/// `keys_dir`는 `~/.ssh`(키의 원본), `pem_dir`는 서버별 PEM이 있는 앱 데이터
/// 디렉터리다 — 둘은 더 이상 같은 곳이 아니다.
pub(crate) fn identity_file_for(
    server: &Server,
    keys_dir: &Path,
    pem_dir: &Path,
    keys: &[SshKey],
) -> Option<String> {
    match server.auth_type {
        AuthType::Key => {
            let id = server.key_id?;
            let key = keys.iter().find(|k| k.id == id)?;
            // 키 이름은 이제 디스크의 파일명 그대로다 — 새니타이즈하면
            // `id_ed25519@work` 같은 실제 파일을 놓친다.
            let file = safe_file_component(&key.name)?;
            Some(keys_dir.join(file).to_string_lossy().into_owned())
        }
        AuthType::Pem => {
            let path = pem_dir.join(server_pem_file_name(server.id));
            path.exists().then(|| path.to_string_lossy().into_owned())
        }
        AuthType::Password | AuthType::Agent => None,
    }
}

pub(crate) fn host_spec(
    server: &Server,
    keys_dir: &Path,
    pem_dir: &Path,
    keys: &[SshKey],
) -> HostSpec {
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
        identity_file: identity_file_for(server, keys_dir, pem_dir, keys),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CreateServerDto, KeyType, UpdateServerDto};
    use crate::ops::key_ops::NewKey;
    use crate::store::Store;
    use std::path::PathBuf;

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
        // 키 이름이 곧 파일명이다 — `id_` 접두를 붙이지 않는다.
        let expected = ctx.keys().join("work_key");
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
    fn never_edits_a_multi_pattern_block_even_though_its_patterns_are_listed() {
        let ctx = Ctx::new();
        let original = "Host a b\n  User multi\n\nMatch all\n  User m\n";
        ctx.write_config(original);
        let mut store = ctx.store();
        // Phase 3: `Host a b`의 패턴 둘 다 읽기 전용 항목으로 목록에 뜬다.
        let listed: Vec<String> = store.list_servers().into_iter().map(|s| s.name).collect();
        assert_eq!(listed, ["a", "b"]);
        assert!(store.list_servers().iter().all(|s| s.read_only));

        // 이름이 "a b"인 서버는 그와 별개다 — 따옴표로 감싼 새 블록이 된다.
        store.insert_server(&dto("a b")).unwrap();
        let out = ctx.config();
        assert!(out.starts_with(original), "{out}");
        assert!(out.contains("Host \"a b\"\n"), "{out}");
        assert_eq!(out.matches("Host \"a b\"").count(), 1);
        // 읽기 전용 블록은 한 바이트도 바뀌지 않는다.
        assert!(out.contains("Host a b\n  User multi\n"), "{out}");
    }

    #[test]
    fn rename_identity_file_follows_the_key_in_every_block_it_appears() {
        // 별칭 접속에는 `-i` 안전망이 없다 — config의 경로가 유일한 키 지정이다.
        let ctx = Ctx::new();
        let old_path = ctx.keys().join("id_old");
        let new_path = ctx.keys().join("id_new");
        let original = format!(
            concat!(
                "# 머리말\n",
                "Host one\n",
                "  HostName 1.1.1.1\n",
                "  IdentityFile {old}\n",
                "\n",
                "Host two\n",
                "  HostName 2.2.2.2\n",
                "  IdentityFile {old}\n",
                "  IdentityFile ~/.ssh/id_rsa\n",
                "\n",
                "Host untouched\n",
                "  IdentityFile ~/.ssh/other\n",
            ),
            old = old_path.display()
        );
        ctx.write_config(&original);
        let mut store = ctx.store();

        assert!(store.rename_identity_file(&old_path, &new_path).unwrap());

        let out = ctx.config();
        assert_eq!(out, original.replace(&old_path.display().to_string(), &new_path.display().to_string()));
        assert_eq!(out.matches(&new_path.display().to_string()).count(), 2);
        assert!(!out.contains(&format!("{}\n", old_path.display())));
        // 그 외 줄은 바이트 그대로다.
        assert!(out.contains("# 머리말\n"));
        assert!(out.contains("  IdentityFile ~/.ssh/id_rsa\n"));
        assert!(out.contains("  IdentityFile ~/.ssh/other\n"));

        // 가리키는 줄이 없으면 파일을 건드리지 않는다.
        assert!(!store.rename_identity_file(&old_path, &new_path).unwrap());
    }
}
