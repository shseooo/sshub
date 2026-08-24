//! 실제 `ssh-keygen`을 호출하는 통합 테스트 — 서브프로세스 인자·권한·rename
//! 시맨틱은 목으로는 검증되지 않는다.
//!
//! ssh-keygen이 없는 환경에서는 조용히 통과한다(CI 이식성).
//!
//! 키 이름은 곧 `~/.ssh` 안의 파일명이다 — 그래서 이름을 `id_rsa` 꼴로 쓴다.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sshub_core::keys_io;
use sshub_core::model::{CreateKeyDto, ImportKeyDto, KeyType, UpdateKeyDto};
use sshub_core::store::Store;

fn have_ssh_keygen() -> bool {
    Command::new("ssh-keygen").arg("-?").output().is_ok()
}

struct Env {
    _dir: tempfile::TempDir,
    store: Store,
    keys_dir: PathBuf,
}

fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let keys_dir = dir.path().join("ssh_keys");
    std::fs::create_dir_all(&keys_dir).unwrap();
    let store = Store::new(
        dir.path().join("sshub.json"),
        dir.path().join(".ssh").join("config"),
        dir.path().join("ssh_keys"),
        dir.path().join("ssh_keys"),
    );
    Env { _dir: dir, store, keys_dir }
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn generates_ed25519_key_with_0600_files() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let dto = CreateKeyDto {
        name: "id_test_ed25519".into(),
        key_type: "ed25519".into(),
        key_size: None,
        passphrase: None,
    };
    let key = keys_io::create_ssh_key(&mut env.store, &env.keys_dir, &dto).unwrap();

    assert_eq!(key.key_type.as_str(), "ed25519");
    assert!(key.public_key.starts_with("ssh-ed25519 "));
    assert!(!key.passphrase_protected);
    assert!(key.pem_data.is_none(), "개인 키 평문이 스토어에 들어가면 안 된다");

    // 이름의 공백은 새니타이즈되어 파일명이 된다.
    let priv_path = env.keys_dir.join("id_test_ed25519");
    assert!(priv_path.exists(), "개인 키 파일");
    assert!(priv_path.with_extension("pub").exists(), "공개 키 파일");
    assert_eq!(mode_of(&priv_path), 0o600);

    // 목록 조회는 hasPrivateFile 뷰 필드를 채운다.
    let views = keys_io::get_ssh_keys(&env.store, &env.keys_dir);
    assert_eq!(views.len(), 1);
    assert!(views[0].has_private_file);
}

#[test]
fn refuses_to_overwrite_an_existing_key_file() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let dto = CreateKeyDto {
        name: "id_dup".into(),
        key_type: "ed25519".into(),
        key_size: None,
        passphrase: None,
    };
    keys_io::create_ssh_key(&mut env.store, &env.keys_dir, &dto).unwrap();
    let err = keys_io::create_ssh_key(&mut env.store, &env.keys_dir, &dto).unwrap_err();
    assert!(err.to_string().starts_with("Key file already exists:"), "{err}");
}

#[test]
fn rejects_dsa_generation() {
    let mut env = setup();
    let dto = CreateKeyDto {
        name: "id_nope".into(),
        key_type: "dsa".into(),
        key_size: None,
        passphrase: None,
    };
    let err = keys_io::create_ssh_key(&mut env.store, &env.keys_dir, &dto).unwrap_err();
    assert_eq!(err.to_string(), "Unsupported key type: dsa");
}

#[test]
fn rename_moves_private_and_public_files() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let key = keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_before".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();

    let updated = keys_io::update_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &UpdateKeyDto {
            id: key.id,
            name: "id_after".into(),
            public_key: key.public_key.clone(),
            key_type: KeyType::Ed25519,
            pem_data: None,
            passphrase: None,
        },
    )
    .unwrap();

    assert_eq!(updated.name, "id_after");
    assert!(!env.keys_dir.join("id_before").exists());
    assert!(!env.keys_dir.join("id_before.pub").exists());
    assert!(env.keys_dir.join("id_after").exists(), "개인 키가 함께 이동해야 접속이 유지된다");
    assert!(env.keys_dir.join("id_after.pub").exists());
    assert_eq!(mode_of(&env.keys_dir.join("id_after")), 0o600);
}

#[test]
fn rename_onto_an_existing_file_is_refused() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let a = keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_alpha".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();
    keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_beta".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();

    let err = keys_io::update_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &UpdateKeyDto {
            id: a.id,
            name: "id_beta".into(),
            public_key: a.public_key.clone(),
            key_type: KeyType::Ed25519,
            pem_data: None,
            passphrase: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "같은 이름의 키 파일이 이미 있습니다.");
}

#[test]
fn passphrase_lifecycle_round_trip() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let key = keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_phrase".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: Some("first-pass".into()),
        },
    )
    .unwrap();
    assert!(key.passphrase_protected);

    // 잘못된 현재 패스프레이즈는 한국어 안내와 함께 거부된다.
    let err =
        keys_io::change_key_passphrase(&mut env.store, &env.keys_dir, key.id, Some("wrong"), Some("x"))
            .unwrap_err();
    assert!(err.to_string().starts_with("패스프레이즈 변경 실패"), "{err}");

    // 올바른 값으로는 성공하고, 빈 문자열로 바꾸면 보호 해제로 기록된다.
    keys_io::change_key_passphrase(
        &mut env.store,
        &env.keys_dir,
        key.id,
        Some("first-pass"),
        Some(""),
    )
    .unwrap();
    assert!(!env.store.get_key(key.id).unwrap().passphrase_protected);
}

#[test]
fn imports_a_pem_and_derives_its_public_key() {
    if !have_ssh_keygen() {
        return;
    }
    // 먼저 실제 키를 만들어 PEM 원문을 확보한다.
    let mut src = setup();
    let generated = keys_io::create_ssh_key(
        &mut src.store,
        &src.keys_dir,
        &CreateKeyDto {
            name: "id_source".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();
    let pem = std::fs::read_to_string(src.keys_dir.join("id_source")).unwrap();

    // 공개 키 없이 PEM만으로 가져오기 — ssh-keygen -y 로 유도되어야 한다.
    let mut env = setup();
    let imported = keys_io::import_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &ImportKeyDto {
            name: "id_imported".into(),
            public_key: String::new(),
            pem_data: Some(pem.clone()),
            key_type: KeyType::Rsa, // 실제 타입 감지가 이 라벨을 덮어써야 한다
            passphrase: None,
        },
    )
    .unwrap();

    assert_eq!(imported.key_type.as_str(), "ed25519", "공개 키에서 타입을 감지한다");
    assert_eq!(
        imported.public_key.split_whitespace().take(2).collect::<Vec<_>>(),
        generated.public_key.split_whitespace().take(2).collect::<Vec<_>>(),
        "유도된 공개 키가 원본과 같아야 한다"
    );
    assert_eq!(mode_of(&env.keys_dir.join("id_imported")), 0o600);

    // PEM에서 직접 유도하는 커맨드도 같은 값을 낸다.
    let derived = keys_io::derive_public_key_from_pem(&env.keys_dir, &pem, None).unwrap();
    assert!(derived.starts_with("ssh-ed25519 "));
    // 유도용 임시 파일은 남지 않아야 한다.
    assert!(!env.keys_dir.join(".derive.tmp").exists());
}

#[test]
fn loads_key_files_from_disk() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_loadme".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();

    // 개인 키 파일: PEM으로 인식하고 형제 .pub 을 함께 읽는다.
    let loaded = keys_io::load_key_file(&env.keys_dir.join("id_loadme")).unwrap();
    assert_eq!(loaded.file_name, "id_loadme");
    assert!(loaded.private_key.unwrap().contains("BEGIN OPENSSH PRIVATE KEY"));
    assert!(loaded.public_key.unwrap().starts_with("ssh-ed25519 "));

    // 공개 키 파일: 전체 내용이 공개 키다.
    let pub_loaded = keys_io::load_key_file(&env.keys_dir.join("id_loadme.pub")).unwrap();
    assert_eq!(pub_loaded.file_name, "id_loadme");
    assert!(pub_loaded.private_key.is_none());
    assert!(pub_loaded.public_key.unwrap().starts_with("ssh-ed25519 "));
}

#[test]
fn delete_removes_both_key_files_and_the_record() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let key = keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_goner".into(),
            key_type: "ed25519".into(),
            key_size: None,
            passphrase: None,
        },
    )
    .unwrap();

    keys_io::delete_ssh_key(&mut env.store, &env.keys_dir, key.id).unwrap();
    assert!(!env.keys_dir.join("id_goner").exists());
    assert!(!env.keys_dir.join("id_goner.pub").exists());
    assert!(env.store.find_key(key.id).is_none());
}

#[test]
fn rsa_generation_honors_key_size() {
    if !have_ssh_keygen() {
        return;
    }
    let mut env = setup();
    let key = keys_io::create_ssh_key(
        &mut env.store,
        &env.keys_dir,
        &CreateKeyDto {
            name: "id_rsa2048".into(),
            key_type: "rsa".into(),
            key_size: Some(2048),
            passphrase: None,
        },
    )
    .unwrap();
    assert_eq!(key.key_type.as_str(), "rsa");
    assert_eq!(key.key_size, 2048);
    assert!(key.public_key.starts_with("ssh-rsa "));

    // ssh-keygen -l 로 실제 비트수를 교차 확인한다.
    let out = Command::new("ssh-keygen")
        .args(["-l", "-f"])
        .arg(env.keys_dir.join("id_rsa2048.pub"))
        .output()
        .unwrap();
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.starts_with("2048 "), "실제 키 비트수: {line}");
}
