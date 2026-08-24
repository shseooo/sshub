//! 옛 앱 키 디렉터리(`<app_data>/ssh_keys`) → `~/.ssh` 이관.
//!
//! 앱이 만든 키가 `~/Library/Application Support/ssh_keys/`에 갇혀 있으면
//! `ssh`도, 다른 도구도 그 키를 모른다. 키의 원본이 `~/.ssh`가 된 이상
//! 옛 키들을 그리로 옮겨줘야 한다.
//!
//! 이 이관의 유일한 계약은 **`~/.ssh`에서 아무것도 잃지 않는다**이다:
//! 덮어쓰기 없음, 삭제 없음. 이름이 같은데 내용이 다르면 둘 다 그대로 두고
//! 충돌로 보고한다 (사용자가 판단할 일이지 앱이 고를 일이 아니다).
//! 옛 디렉터리도 지우지 않는다 — 구버전으로 되돌아가도 아무것도 사라지지
//! 않아야 한다.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::fsutil::secure_write;
use crate::key_scan::discover_keys;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyMigration {
    /// `~/.ssh`에 없던 키 — 0600으로 복사했다(`.pub`도 함께).
    Copied(String),
    /// 바이트까지 같은 키가 이미 있다 — 할 일 없음.
    AlreadyPresent(String),
    /// 이름은 같은데 내용이 다르다 — 양쪽 다 건드리지 않았다.
    Conflict(String),
}

impl KeyMigration {
    pub fn file_name(&self) -> &str {
        match self {
            KeyMigration::Copied(n) | KeyMigration::AlreadyPresent(n) | KeyMigration::Conflict(n) => n,
        }
    }
}

/// `~/.ssh` 자체는 0700이어야 한다(느슨하면 ssh가 키 사용을 거부한다).
fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// 옛 디렉터리의 개인 키를 `keys_dir`로 복사한다. 결과는 파일명 사전순.
///
/// 서버별 PEM(`pem_server_<id>`)은 대상이 아니다 — 아직 앱 데이터 디렉터리에
/// 남는다(`key_scan::is_reserved_ssh_file`이 걸러낸다).
pub fn migrate_legacy_keys(legacy_dir: &Path, keys_dir: &Path) -> Vec<KeyMigration> {
    // 같은 디렉터리면 이관할 것이 없다 (자기 자신을 덮어쓰는 사고 방지).
    if legacy_dir == keys_dir || !legacy_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for found in discover_keys(legacy_dir) {
        if !found.has_private_file {
            continue;
        }
        let name = found.file_name;
        let src = legacy_dir.join(&name);
        let dst = keys_dir.join(&name);
        let Ok(bytes) = fs::read(&src) else { continue };

        match fs::read(&dst) {
            Ok(existing) if existing == bytes => {
                out.push(KeyMigration::AlreadyPresent(name));
                continue;
            }
            Ok(_) => {
                out.push(KeyMigration::Conflict(name));
                continue;
            }
            Err(_) => {}
        }

        if ensure_dir(keys_dir).is_err() || secure_write(&dst, &bytes).is_err() {
            continue;
        }
        // `.pub`은 비밀이 아니라 0644가 관례다. 이미 있으면 손대지 않는다.
        let src_pub = crate::fsutil::path_with_suffix(&src, ".pub");
        let dst_pub = crate::fsutil::path_with_suffix(&dst, ".pub");
        if src_pub.is_file() && !dst_pub.exists() && fs::copy(&src_pub, &dst_pub).is_ok() {
            let _ = fs::set_permissions(&dst_pub, fs::Permissions::from_mode(0o644));
        }
        out.push(KeyMigration::Copied(name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    const PEM: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n";

    struct Ctx {
        _dir: tempfile::TempDir,
        legacy: std::path::PathBuf,
        ssh: std::path::PathBuf,
    }

    fn ctx() -> Ctx {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("ssh_keys");
        let ssh = dir.path().join(".ssh");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&ssh).unwrap();
        Ctx { _dir: dir, legacy, ssh }
    }

    #[test]
    fn copies_a_key_that_only_exists_in_the_legacy_directory() {
        let c = ctx();
        fs::write(c.legacy.join("id_only"), PEM).unwrap();
        fs::write(c.legacy.join("id_only.pub"), "ssh-ed25519 AAAA\n").unwrap();

        let out = migrate_legacy_keys(&c.legacy, &c.ssh);
        assert_eq!(out, vec![KeyMigration::Copied("id_only".into())]);
        assert_eq!(fs::read_to_string(c.ssh.join("id_only")).unwrap(), PEM);
        assert_eq!(fs::metadata(c.ssh.join("id_only")).unwrap().mode() & 0o777, 0o600);
        assert!(c.ssh.join("id_only.pub").exists());
        // 원본은 그대로 남는다 (구버전으로 되돌아가도 잃는 것이 없도록).
        assert!(c.legacy.join("id_only").exists());
    }

    #[test]
    fn an_identical_key_is_a_no_op() {
        let c = ctx();
        fs::write(c.legacy.join("id_same"), PEM).unwrap();
        fs::write(c.ssh.join("id_same"), PEM).unwrap();
        let before = fs::metadata(c.ssh.join("id_same")).unwrap().modified().unwrap();

        let out = migrate_legacy_keys(&c.legacy, &c.ssh);
        assert_eq!(out, vec![KeyMigration::AlreadyPresent("id_same".into())]);
        assert_eq!(fs::metadata(c.ssh.join("id_same")).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn a_conflicting_key_leaves_both_files_untouched() {
        let c = ctx();
        let theirs = "-----BEGIN OPENSSH PRIVATE KEY-----\nTHEIRS\n-----END OPENSSH PRIVATE KEY-----\n";
        fs::write(c.legacy.join("id_rsa"), PEM).unwrap();
        fs::write(c.ssh.join("id_rsa"), theirs).unwrap();

        let out = migrate_legacy_keys(&c.legacy, &c.ssh);
        assert_eq!(out, vec![KeyMigration::Conflict("id_rsa".into())]);
        assert_eq!(fs::read_to_string(c.ssh.join("id_rsa")).unwrap(), theirs, "~/.ssh는 이긴다");
        assert_eq!(fs::read_to_string(c.legacy.join("id_rsa")).unwrap(), PEM);
    }

    #[test]
    fn never_removes_anything_from_the_target_directory() {
        let c = ctx();
        fs::write(c.legacy.join("id_new"), PEM).unwrap();
        fs::write(c.ssh.join("id_keep"), PEM).unwrap();
        fs::write(c.ssh.join("config"), "Host x\n").unwrap();

        migrate_legacy_keys(&c.legacy, &c.ssh);
        assert!(c.ssh.join("id_keep").exists());
        assert_eq!(fs::read_to_string(c.ssh.join("config")).unwrap(), "Host x\n");
    }

    #[test]
    fn server_pem_files_stay_in_the_app_data_directory() {
        let c = ctx();
        fs::write(c.legacy.join("pem_server_4"), PEM).unwrap();
        assert!(migrate_legacy_keys(&c.legacy, &c.ssh).is_empty());
        assert!(!c.ssh.join("pem_server_4").exists());
    }

    #[test]
    fn is_idempotent() {
        let c = ctx();
        fs::write(c.legacy.join("id_x"), PEM).unwrap();
        assert_eq!(migrate_legacy_keys(&c.legacy, &c.ssh), vec![KeyMigration::Copied("id_x".into())]);
        assert_eq!(
            migrate_legacy_keys(&c.legacy, &c.ssh),
            vec![KeyMigration::AlreadyPresent("id_x".into())]
        );
    }

    #[test]
    fn a_missing_legacy_directory_is_not_an_error() {
        let c = ctx();
        assert!(migrate_legacy_keys(&c.legacy.join("nope"), &c.ssh).is_empty());
        assert!(migrate_legacy_keys(&c.ssh, &c.ssh).is_empty());
    }
}
