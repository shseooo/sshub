//! Phase 2 — `~/.ssh/config`가 접속 정보의 원본이고 `sshub.json` v2는 앱 전용
//! 메타데이터 사이드카라는 계약을 고정한다.
//!
//! 이 파일의 모든 테스트는 `tempfile` 안에서만 논다. 진짜 `~/.ssh/config`나
//! 진짜 `~/Library/Application Support/sshub.json`은 절대 열지 않는다 —
//! `Store::new`가 경로 3종을 강제로 받는 이유가 그것이다.

use std::fs;
use std::path::PathBuf;

use sshub_core::model::{AuthType, CreateServerDto, KeyType, UpdateServerDto};
use sshub_core::ops::key_ops::NewKey;
use sshub_core::{CoreError, Store};

struct Ctx {
    dir: tempfile::TempDir,
}

impl Ctx {
    fn new() -> Ctx {
        let ctx = Ctx { dir: tempfile::tempdir().unwrap() };
        fs::create_dir_all(ctx.keys_dir()).unwrap();
        ctx
    }
    fn store_path(&self) -> PathBuf {
        self.dir.path().join("sshub.json")
    }
    fn config_path(&self) -> PathBuf {
        self.dir.path().join(".ssh").join("config")
    }
    fn keys_dir(&self) -> PathBuf {
        self.dir.path().join("ssh_keys")
    }
    fn open(&self) -> Store {
        let mut s = Store::new(self.store_path(), self.config_path(), self.keys_dir());
        s.load();
        s
    }
    fn write_config(&self, text: &str) {
        fs::create_dir_all(self.config_path().parent().unwrap()).unwrap();
        fs::write(self.config_path(), text).unwrap();
    }
    fn config(&self) -> String {
        fs::read_to_string(self.config_path()).unwrap()
    }
    fn sidecar(&self) -> String {
        fs::read_to_string(self.store_path()).unwrap()
    }
    fn files_matching(&self, needle: &str) -> Vec<String> {
        fs::read_dir(self.dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(needle))
            .collect()
    }
}

fn dto(name: &str) -> CreateServerDto {
    CreateServerDto {
        name: name.into(),
        host: "h".into(),
        username: "u".into(),
        auth_type: AuthType::Agent,
        ..Default::default()
    }
}

/// 빈 줄을 뺀 줄 목록 — "이 호스트의 블록 말고는 바이트 동일"을 재는 자.
/// (블록을 넣고 지우면 앞뒤 빈 줄 하나가 남을 수 있다.)
fn nonblank(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.trim().is_empty()).collect()
}

// ==================== A. v1 → v2 마이그레이션 ====================

const V1_STORE: &str = r#"{
  "nextServerId": 12,
  "nextKeyId": 3,
  "servers": [
    {
      "id": 3,
      "name": "web",
      "host": "10.0.0.1",
      "port": 22,
      "username": "deploy",
      "authType": "key",
      "keyId": 2,
      "pemData": null,
      "proxyJump": null,
      "groupName": "production",
      "tags": "[\"web\"]",
      "isFavorite": true,
      "notes": "주 웹서버",
      "lastConnectedAt": "2026-08-20T01:02:03.000Z",
      "createdAt": "2026-01-01T00:00:00.000Z",
      "updatedAt": "2026-08-01T00:00:00.000Z"
    },
    {
      "id": 5,
      "name": "db",
      "host": "10.0.0.2",
      "port": 2222,
      "username": "root",
      "authType": "password",
      "proxyJump": "bastion",
      "groupName": "",
      "isFavorite": false
    },
    {
      "id": 11,
      "name": "legacy",
      "host": "old.example.com",
      "port": 22,
      "username": "root",
      "authType": "agent",
      "notes": "곧 폐기"
    }
  ],
  "keys": [
    {
      "id": 2,
      "name": "work key",
      "publicKey": "ssh-ed25519 AAAA",
      "keyType": "ed25519",
      "keySize": 256,
      "passphraseProtected": false
    }
  ]
}"#;

const HAND_WRITTEN: &str = "\
# 손으로 관리하는 파일
Include ~/.ssh/conf.d/*.conf

Host legacy
  HostName really-old.example.com
  User admin
  ControlMaster auto

Host bastion jump
  HostName 1.2.3.4

Host *
  ServerAliveInterval 60
";

fn migrated() -> Ctx {
    let ctx = Ctx::new();
    fs::write(ctx.store_path(), V1_STORE).unwrap();
    ctx.write_config(HAND_WRITTEN);
    ctx
}

#[test]
fn migration_keeps_every_v1_server_reachable_with_its_original_id() {
    let ctx = migrated();
    let store = ctx.open();

    let by_id = |id: i64| store.find_server(id).unwrap_or_else(|| panic!("id {id} 사라짐"));
    // 별칭은 옛 렌더러 규칙 그대로 — 이미 동기화해 둔 사용자에게 중복이 없다.
    assert_eq!(by_id(3).name, "production-web");
    assert_eq!(by_id(5).name, "db");
    assert_eq!(by_id(11).name, "legacy");
    assert_eq!(store.list_servers().len(), 3);
}

#[test]
fn migration_carries_app_only_metadata_into_the_sidecar() {
    let ctx = migrated();
    let store = ctx.open();
    let web = store.find_server(3).unwrap();

    assert!(web.is_favorite);
    assert_eq!(web.group_name.as_deref(), Some("production"));
    assert_eq!(web.tags.as_deref(), Some(r#"["web"]"#));
    assert_eq!(web.notes.as_deref(), Some("주 웹서버"));
    assert_eq!(web.last_connected_at.as_deref(), Some("2026-08-20T01:02:03.000Z"));
    assert_eq!(web.created_at.as_deref(), Some("2026-01-01T00:00:00.000Z"));
    // config가 표현할 수 없는 정보는 사이드카가 들고 있어야 한다.
    assert_eq!(store.find_server(5).unwrap().auth_type, AuthType::Password);
    assert_eq!(web.auth_type, AuthType::Key);
    assert_eq!(web.key_id, Some(2));
    assert_eq!(store.list_keys().len(), 1);
}

#[test]
fn migration_lets_the_config_win_for_connection_fields_of_a_pre_existing_block() {
    let ctx = migrated();
    let store = ctx.open();
    let legacy = store.find_server(11).unwrap();

    // v1은 old.example.com / root였지만 실제로 접속을 책임지던 건 config다.
    assert_eq!(legacy.host, "really-old.example.com");
    assert_eq!(legacy.username, "admin");
    assert!(!ctx.config().contains("HostName old.example.com"), "{}", ctx.config());
    // 반대로 config에 없던 값은 v1에서 채워진다.
    assert_eq!(store.find_server(5).unwrap().proxy_jump.as_deref(), Some("bastion"));
    assert_eq!(store.find_server(5).unwrap().port, 2222);
}

#[test]
fn migration_preserves_every_hand_written_line_and_backs_up_the_v1_store() {
    let ctx = migrated();
    let _ = ctx.open();

    let out = ctx.config();
    for line in HAND_WRITTEN.lines() {
        assert!(out.lines().any(|m| m == line), "사라진 줄: {line:?}\n결과:\n{out}");
    }
    // 새 블록은 `Host *`보다 앞이라야 가려지지 않는다.
    assert!(out.find("Host production-web").unwrap() < out.find("Host *").unwrap());

    // 되돌릴 길: v1 원본 + config 백업.
    assert_eq!(ctx.files_matching("sshub.json.v1.").len(), 1);
    let baks = fs::read_dir(ctx.config_path().parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref().unwrap().file_name().to_string_lossy().starts_with("config.bak.")
        })
        .count();
    assert_eq!(baks, 1);
}

#[test]
fn migration_is_idempotent_a_second_load_changes_nothing() {
    let ctx = migrated();
    let _ = ctx.open();
    let config_once = ctx.config();
    let sidecar_once = ctx.sidecar();

    let store = ctx.open();
    assert_eq!(ctx.config(), config_once, "두 번째 load가 config를 바꿨다");
    assert_eq!(ctx.sidecar(), sidecar_once, "두 번째 load가 사이드카를 바꿨다");
    assert_eq!(store.list_servers().len(), 3);
    // 마이그레이션은 한 번만 — v1 백업이 늘어나지 않는다.
    assert_eq!(ctx.files_matching("sshub.json.v1.").len(), 1);
}

#[test]
fn migration_sets_the_id_counter_past_every_v1_id() {
    let ctx = migrated();
    let mut store = ctx.open();
    let fresh = store.insert_server(&dto("fresh")).unwrap();
    assert_eq!(fresh.id, 12, "v1의 nextServerId(12)를 이어받아야 한다");
}

// ==================== B. 손으로 쓴 호스트가 그대로 뜬다 ====================

#[test]
fn a_hand_written_host_shows_up_with_an_id_that_is_stable_across_loads() {
    let ctx = Ctx::new();
    ctx.write_config("Host manual\n  HostName 10.9.9.9\n  User me\n  Port 2200\n");

    let first = ctx.open();
    let manual = first.list_servers().into_iter().next().unwrap();
    assert_eq!(manual.name, "manual");
    assert_eq!(manual.host, "10.9.9.9");
    assert_eq!(manual.username, "me");
    assert_eq!(manual.port, 2200);

    let second = ctx.open();
    assert_eq!(second.list_servers()[0].id, manual.id, "id가 load마다 바뀐다");
}

#[test]
fn a_hand_written_host_falls_back_to_the_alias_and_default_user_and_port() {
    let ctx = Ctx::new();
    ctx.write_config("Host bare\n");
    let store = ctx.open();
    let bare = &store.list_servers()[0];
    assert_eq!(bare.host, "bare");
    assert_eq!(bare.username, "user");
    assert_eq!(bare.port, 22);
    assert_eq!(bare.auth_type, AuthType::Agent);
}

#[test]
fn registering_a_hand_written_host_does_not_rewrite_the_config() {
    let ctx = Ctx::new();
    let original = "Host manual\n  HostName 10.9.9.9\n";
    ctx.write_config(original);
    let _ = ctx.open();
    // id 배정은 사이드카에만 남는다 — config는 한 바이트도 안 바뀐다.
    assert_eq!(ctx.config(), original);
    assert!(ctx.sidecar().contains("manual"));
}

// ==================== C. 왕복 (insert → update → delete) ====================

const SURROUNDING: &str = "\
# 맨 위 주석
Include ~/.ssh/conf.d/*.conf

Host keep
  HostName 1.1.1.1
  ControlMaster auto
  # 블록 안 주석

Host *
  ServerAliveInterval 60
";

#[test]
fn insert_update_rename_delete_leaves_the_surrounding_config_intact() {
    let ctx = Ctx::new();
    ctx.write_config(SURROUNDING);
    let mut store = ctx.open();

    let mut d = dto("tmp");
    d.host = "10.0.0.5".into();
    d.port = Some(2222);
    d.proxy_jump = Some("bastion".into());
    let created = store.insert_server(&d).unwrap();
    assert!(ctx.config().contains("Host tmp"));

    let renamed = store
        .update_server(&UpdateServerDto {
            id: created.id,
            name: Some("tmp2".into()),
            host: Some("10.0.0.6".into()),
            proxy_jump: Some("bastion".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(renamed.id, created.id, "이름이 바뀌어도 id는 그대로여야 한다");
    assert!(ctx.config().contains("Host tmp2"));
    assert!(ctx.config().contains("HostName 10.0.0.6"));

    store.delete_server(created.id).unwrap();
    let after = ctx.config();
    assert!(!after.contains("tmp"), "{after}");
    // 남은 파일은 빈 줄을 빼면 원본과 완전히 같다.
    assert_eq!(nonblank(&after), nonblank(SURROUNDING), "{after}");
    // 사용자 블록은 바이트 그대로 살아 있다.
    assert!(after.contains("Host keep\n  HostName 1.1.1.1\n  ControlMaster auto\n  # 블록 안 주석"));
    // 사용자의 `Host keep`만 남는다.
    let names: Vec<String> = ctx.open().list_servers().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["keep"]);
}

#[test]
fn a_rename_moves_the_metadata_under_the_new_alias() {
    let ctx = Ctx::new();
    let mut store = ctx.open();
    let mut d = dto("old");
    d.notes = Some("소중한 메모".into());
    let created = store.insert_server(&d).unwrap();
    store.toggle_favorite(created.id).unwrap();

    store
        .update_server(&UpdateServerDto {
            id: created.id,
            name: Some("new".into()),
            ..Default::default()
        })
        .unwrap();

    let reloaded = ctx.open();
    let moved = reloaded.find_server(created.id).unwrap();
    assert_eq!(moved.name, "new");
    assert_eq!(moved.notes.as_deref(), Some("소중한 메모"));
    assert!(moved.is_favorite);
    assert!(!ctx.sidecar().contains("\"old\""), "{}", ctx.sidecar());
}

// ==================== D. 메타데이터 조작은 config를 건드리지 않는다 ====================

#[test]
fn toggle_favorite_and_touch_last_connected_never_touch_the_config() {
    let ctx = Ctx::new();
    ctx.write_config(SURROUNDING);
    let mut store = ctx.open();
    let keep = store.list_servers().into_iter().find(|s| s.name == "keep").unwrap();

    let before = fs::read(ctx.config_path()).unwrap();
    store.toggle_favorite(keep.id).unwrap();
    store.touch_last_connected(keep.id).unwrap();
    assert_eq!(fs::read(ctx.config_path()).unwrap(), before, "config가 바뀌었다");

    let reloaded = ctx.open();
    let after = reloaded.find_server(keep.id).unwrap();
    assert!(after.is_favorite);
    assert!(after.last_connected_at.is_some());
}

// ==================== E. IdentityFile → 인증 방식 유추 ====================

#[test]
fn derives_auth_type_and_key_id_from_the_identity_file() {
    let ctx = Ctx::new();
    let mut store = ctx.open();
    let key = store
        .insert_key(&NewKey {
            name: "work key".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            key_type: KeyType::Ed25519,
            key_size: 256,
            passphrase_protected: false,
        })
        .unwrap();
    drop(store);

    // 파일 순서 = id 배정 순서. k-pem이 첫 블록이므로 id 1을 받는다.
    let keys = ctx.keys_dir();
    ctx.write_config(&format!(
        "Host k-pem\n  IdentityFile {pem}\n\n\
         Host k-app\n  IdentityFile {app}\n\n\
         Host k-other\n  IdentityFile ~/.ssh/id_rsa\n\n\
         Host k-none\n  HostName x\n",
        pem = keys.join("pem_server_1").display(),
        app = keys.join("id_work_key").display(),
    ));

    let store = ctx.open();
    let get = |name: &str| store.list_servers().into_iter().find(|s| s.name == name).unwrap();

    // 1) 앱 키 파일과 일치 → Key + 그 키의 id
    assert_eq!(get("k-app").auth_type, AuthType::Key);
    assert_eq!(get("k-app").key_id, Some(key.id));
    // 2) 이 서버의 PEM 경로와 일치 → Pem
    assert_eq!(get("k-pem").id, 1);
    assert_eq!(get("k-pem").auth_type, AuthType::Pem);
    assert_eq!(get("k-pem").key_id, None);
    // 3) 그 밖의 IdentityFile → Key + key_id 없음 (사용자가 직접 관리하는 키)
    assert_eq!(get("k-other").auth_type, AuthType::Key);
    assert_eq!(get("k-other").key_id, None);
    // 4) IdentityFile 없음 → Agent
    assert_eq!(get("k-none").auth_type, AuthType::Agent);
    assert_eq!(get("k-none").key_id, None);
}

#[test]
fn the_sidecar_auth_wins_over_derivation_because_config_cannot_express_it() {
    let ctx = Ctx::new();
    let mut store = ctx.open();
    let mut d = dto("pw");
    d.auth_type = AuthType::Password;
    let created = store.insert_server(&d).unwrap();

    // config에는 IdentityFile이 없으니 유추라면 Agent가 나왔을 자리다.
    assert!(!ctx.config().contains("IdentityFile"));
    assert_eq!(ctx.open().find_server(created.id).unwrap().auth_type, AuthType::Password);
}

// ==================== F. 읽기 전용 블록 ====================

#[test]
fn read_only_blocks_are_not_listed_and_cannot_be_edited_or_deleted() {
    let ctx = Ctx::new();
    let original = "\
Host a b c
  User multi

Host *.dev
  User devs

Host *
  ServerAliveInterval 60
";
    ctx.write_config(original);
    let mut store = ctx.open();

    // 하나도 목록에 뜨지 않는다.
    assert!(store.list_servers().is_empty());

    // 그 별칭으로 새 서버를 만들 수도 없다 (읽기 전용 블록이 이미 소유).
    for taken in ["a", "b", "c", "*", "*.dev"] {
        let err = store.insert_server(&dto(taken)).unwrap_err();
        assert!(matches!(err, CoreError::ServerNotFound), "{taken}: {err}");
    }
    // id가 없으니 수정/삭제 대상도 아니다.
    assert!(matches!(
        store.update_server(&UpdateServerDto { id: 1, ..Default::default() }),
        Err(CoreError::ServerNotFound)
    ));
    store.delete_server(1).unwrap(); // 없는 id는 조용히 성공 (기존 동작)

    assert_eq!(ctx.config(), original, "읽기 전용 블록이 건드려졌다");
}

#[test]
fn a_sidecar_entry_survives_a_rename_made_outside_the_app() {
    let ctx = Ctx::new();
    let mut store = ctx.open();
    let mut d = dto("notes-here");
    d.notes = Some("잃으면 안 되는 메모".into());
    store.insert_server(&d).unwrap();

    // 앱 밖에서 별칭을 바꿔 버린다.
    ctx.write_config("Host renamed-by-hand\n  HostName h\n");
    let store = ctx.open();

    // 새 별칭은 새 서버로 보이지만, 옛 메타데이터는 지워지지 않고 남는다.
    assert_eq!(store.list_servers().len(), 1);
    assert_eq!(store.list_servers()[0].name, "renamed-by-hand");
    assert!(ctx.sidecar().contains("잃으면 안 되는 메모"), "{}", ctx.sidecar());
}

#[test]
fn list_servers_keeps_favorites_first_then_lowercased_name() {
    let ctx = Ctx::new();
    ctx.write_config("Host Beta\n\nHost alpha\n\nHost zulu\n");
    let mut store = ctx.open();
    let zulu = store.list_servers().into_iter().find(|s| s.name == "zulu").unwrap();
    store.toggle_favorite(zulu.id).unwrap();

    let names: Vec<String> = store.list_servers().into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["zulu", "alpha", "Beta"]);
}
