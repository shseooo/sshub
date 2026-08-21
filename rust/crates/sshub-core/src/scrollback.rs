//! 세션별 터미널 스크롤백 영속화 (scrollback.ts + scrollbackStore.ts 직역).
//! 라이브 PTY는 재시작 후 되살아나지 않지만 출력 히스토리는 복원된다.
//! 세션 id가 파일명이 되므로 `[A-Za-z0-9_-]` 밖의 문자는 중화한다.

use std::fs::{self, DirBuilder, Permissions};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::CoreError;
use crate::fsutil::{rm_force, write_0600};

/// 터미널당 영속 스크롤백 최대 라인 수 (라이브 버퍼 20000과는 별개).
pub const SCROLLBACK_LINES: usize = 1000;

pub fn scrollback_file_name(session_id: &str) -> String {
    let safe: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!("{safe}.txt")
}

pub struct ScrollbackStore {
    dir: PathBuf,
}

impl ScrollbackStore {
    /// 0700: 스크롤백은 화면 출력 캡처라 사용자가 찍은 비밀(토큰,
    /// `cat id_rsa`, env 덤프)이 들어갈 수 있다. 소유자 전용으로 유지.
    pub fn new(dir: PathBuf) -> Result<ScrollbackStore, CoreError> {
        DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        // mkdir mode는 생성 시에만 적용 — 이전에 만들어진 디렉터리도 조인다.
        let _ = fs::set_permissions(&dir, Permissions::from_mode(0o700));
        Ok(ScrollbackStore { dir })
    }

    fn path_for(&self, session_id: &str) -> PathBuf {
        self.dir.join(scrollback_file_name(session_id))
    }

    pub fn save(&self, session_id: &str, data: &str) -> Result<(), CoreError> {
        // 디렉터리와 같은 이유로 0600 — 절대 world-readable이 되면 안 된다.
        write_0600(&self.path_for(session_id), data.as_bytes())?;
        Ok(())
    }

    pub fn load(&self, session_id: &str) -> Option<String> {
        let p = self.path_for(session_id);
        if p.exists() { fs::read_to_string(p).ok() } else { None }
    }

    pub fn delete(&self, session_id: &str) {
        let _ = rm_force(&self.path_for(session_id));
    }

    /// 레이아웃에서 사라진 세션의 스크롤백 파일(고아)을 제거한다.
    pub fn prune(&self, live_ids: &[String]) {
        let keep: std::collections::HashSet<String> =
            live_ids.iter().map(|id| scrollback_file_name(id)).collect();
        let Ok(rd) = fs::read_dir(&self.dir) else { return };
        for entry in rd.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".txt") && !keep.contains(&name) {
                let _ = rm_force(&entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn keeps_a_normal_uuid_session_id_and_adds_txt() {
        assert_eq!(
            scrollback_file_name("3f2504e0-4f89-41d3-9a0c-0305e82c3301"),
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301.txt"
        );
    }

    #[test]
    fn neutralizes_path_traversal_unsafe_characters() {
        assert_eq!(scrollback_file_name("../../etc/passwd"), "______etc_passwd.txt");
        assert_eq!(scrollback_file_name("a/b"), "a_b.txt");
        assert_eq!(scrollback_file_name("x.y"), "x_y.txt");
    }

    #[test]
    fn handles_empty_id() {
        assert_eq!(scrollback_file_name(""), ".txt");
    }

    fn store() -> (tempfile::TempDir, ScrollbackStore) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("scrollback");
        let s = ScrollbackStore::new(dir).unwrap();
        (root, s)
    }

    #[test]
    fn round_trips_save_load_per_session_and_returns_none_for_unknown_ids() {
        let (_root, s) = store();
        assert_eq!(s.load("a"), None);
        s.save("a", "line1\nline2").unwrap();
        assert_eq!(s.load("a").as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn creates_the_directory_0700_and_scrollback_files_0600() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("scrollback");
        let s = ScrollbackStore::new(dir.clone()).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        s.save("sess", "export TOKEN=secret").unwrap();
        let file = dir.join(scrollback_file_name("sess"));
        assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn deletes_a_single_session_file() {
        let (_root, s) = store();
        s.save("a", "x").unwrap();
        s.delete("a");
        assert_eq!(s.load("a"), None);
    }

    #[test]
    fn prunes_files_for_sessions_no_longer_in_the_layout() {
        let (_root, s) = store();
        s.save("keep", "x").unwrap();
        s.save("drop", "y").unwrap();
        s.prune(&["keep".to_string()]);
        assert_eq!(s.load("keep").as_deref(), Some("x"));
        assert_eq!(s.load("drop"), None);
    }
}
