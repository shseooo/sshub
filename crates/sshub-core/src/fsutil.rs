//! 파일 쓰기 프리미티브 — Electron 구현의 시퀀스를 그대로 따른다.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// `path` 뒤에 접미사를 붙인 형제 경로 (`sshub.json` → `sshub.json.tmp`).
pub fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(suffix);
    PathBuf::from(os)
}

/// JS `Store.save()` 시퀀스 그대로: `<path>.tmp`를 0600으로 열어 쓰고
/// fsync → rename → chmod 0600. rename 전에 fsync해야 크래시가 파일을
/// 잘라먹지 못한다.
pub fn atomic_write_0600(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path_with_suffix(path, ".tmp");
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // 기존 파일 위로 rename됐을 때도 0600을 보장 (JS chmodSync와 동일).
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(())
}

/// JS `secureWrite`: `writeFileSync(path, data, {mode:0o600})` + 무조건 chmod.
/// (writeFileSync의 mode는 생성 시에만 적용되므로 chmod가 기존 파일을 조인다.)
pub fn secure_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_0600(path, bytes)?;
    fs::set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(())
}

/// `writeFileSync(path, data, {mode:0o600})` — 생성 시에만 0600, chmod 없음.
pub fn write_0600(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

/// JS `rmSync(path, { force: true })` — 없는 파일은 무시, 그 외 에러는 전파.
pub fn rm_force(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn atomic_write_leaves_a_0600_file_and_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.json");
        atomic_write_0600(&p, b"{}").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"{}");
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert!(!path_with_suffix(&p, ".tmp").exists());
    }

    #[test]
    fn secure_write_tightens_an_existing_looser_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("k");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&p, Permissions::from_mode(0o644)).unwrap();
        secure_write(&p, b"new").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn rm_force_ignores_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rm_force(&dir.path().join("nope")).is_ok());
    }
}
