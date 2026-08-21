//! 로컬 터미널의 세션별 마지막 작업 디렉터리 (terminalCwd.ts 직역).
//! 라이브 PTY는 재시작 후 되살아나지 않으므로, 종료 시 각 로컬 세션의 cwd를
//! 스냅샷해 다음 실행에서 셸을 그 위치에 다시 띄운다. SSH 세션은 제외 —
//! 원격 cwd는 우리가 복원할 대상이 아니다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fsutil::write_0600;

/// pid로 실행 중 프로세스의 cwd를 얻는다. 실패(프로세스 종료, 도구 없음,
/// 미지원 플랫폼)는 None. PTY의 pid는 로그인 셸 자신이므로 사용자의 `cd`를
/// 따라간다. 코어는 동기 — macOS lsof는 수십~수백 ms 걸릴 수 있으니 UI는
/// 반드시 백그라운드 executor에서 호출할 것 (DESIGN-core.md §4).
pub fn read_pid_cwd(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        return fs::read_link(format!("/proc/{pid}/cwd"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
    }
    #[cfg(target_os = "macos")]
    {
        // `lsof -Fn -d cwd`는 cwd 경로를 'n' 접두 라인으로 출력한다. 절대
        // 경로를 쓰는 이유: Finder/dock에서 실행된 GUI 앱의 PATH에는
        // /usr/sbin이 없을 수 있다.
        let out = Command::new("/usr/sbin/lsof")
            .args(["-a", "-d", "cwd", "-Fn", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        return stdout.lines().find(|l| l.starts_with('n')).map(|l| l[1..].to_string());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// sessionId → 마지막 로컬 cwd의 JSON 맵 (compact, 0600, best-effort).
pub struct TerminalCwdStore {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

impl TerminalCwdStore {
    pub fn new(path: PathBuf) -> TerminalCwdStore {
        TerminalCwdStore { path, map: BTreeMap::new() }
    }

    pub fn load(&mut self) {
        self.map = fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
    }

    /// 저장된 cwd — 단, 디스크에 아직 존재할 때만.
    pub fn get(&self, session_id: &str) -> Option<String> {
        self.map
            .get(session_id)
            .filter(|cwd| !cwd.is_empty() && Path::new(cwd).exists())
            .cloned()
    }

    pub fn set(&mut self, session_id: &str, cwd: &str) {
        if self.map.get(session_id).map(String::as_str) == Some(cwd) {
            return;
        }
        self.map.insert(session_id.to_string(), cwd.to_string());
        self.persist();
    }

    pub fn delete(&mut self, session_id: &str) {
        if self.map.remove(session_id).is_some() {
            self.persist();
        }
    }

    /// 레이아웃에서 사라진 세션 엔트리를 제거한다 (scrollback prune과 대칭).
    pub fn prune(&mut self, live_ids: &[String]) {
        let keep: std::collections::HashSet<&str> =
            live_ids.iter().map(String::as_str).collect();
        let before = self.map.len();
        self.map.retain(|id, _| keep.contains(id.as_str()));
        if self.map.len() != before {
            self.persist();
        }
    }

    fn persist(&self) {
        // best-effort: cwd 스냅샷 실패는 다음 실행이 홈에서 열린다는 뜻일 뿐.
        if let Ok(json) = serde_json::to_string(&self.map) {
            let _ = write_0600(&self.path, json.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cwd.json");
        (dir, path)
    }

    #[test]
    fn returns_none_for_an_unknown_session() {
        let (_d, path) = setup();
        let s = TerminalCwdStore::new(path);
        assert_eq!(s.get("nope"), None);
    }

    #[test]
    fn persists_a_cwd_across_reloads() {
        let (dir, path) = setup();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let mut a = TerminalCwdStore::new(path.clone());
        a.set("s1", &dir_str); // 실제 존재하는 디렉터리라 get()이 반환한다
        let mut b = TerminalCwdStore::new(path);
        b.load();
        assert_eq!(b.get("s1").as_deref(), Some(dir_str.as_str()));
    }

    #[test]
    fn does_not_return_a_saved_cwd_that_no_longer_exists_on_disk() {
        let (dir, path) = setup();
        let mut s = TerminalCwdStore::new(path);
        s.set("s1", &dir.path().join("deleted-subdir").to_string_lossy());
        assert_eq!(s.get("s1"), None);
    }

    #[test]
    fn delete_removes_an_entry() {
        let (dir, path) = setup();
        let mut s = TerminalCwdStore::new(path);
        s.set("s1", &dir.path().to_string_lossy());
        s.delete("s1");
        assert_eq!(s.get("s1"), None);
    }

    #[test]
    fn prune_keeps_only_live_sessions() {
        let (dir, path) = setup();
        let d = dir.path().to_string_lossy().into_owned();
        let mut s = TerminalCwdStore::new(path);
        s.set("keep", &d);
        s.set("drop", &d);
        s.prune(&["keep".to_string()]);
        assert_eq!(s.get("keep").as_deref(), Some(d.as_str()));
        assert_eq!(s.get("drop"), None);
    }

    #[test]
    fn writes_the_backing_file_with_0600_permissions() {
        let (dir, path) = setup();
        let mut s = TerminalCwdStore::new(path.clone());
        s.set("s1", &dir.path().to_string_lossy());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn starts_empty_when_the_file_is_absent_or_corrupt() {
        // `_dir`은 TempDir 가드 — 이름을 붙여 두지 않으면 즉시 drop되어 디렉터리가 사라진다.
        let (_dir, path) = setup();
        let mut s = TerminalCwdStore::new(path.clone());
        s.load(); // 파일이 아직 없다
        assert_eq!(s.get("s1"), None);
        fs::write(&path, "{ not json").unwrap();
        let mut c = TerminalCwdStore::new(path);
        c.load();
        assert_eq!(c.get("s1"), None);
    }

    #[test]
    fn read_pid_cwd_returns_a_directory_for_our_own_process() {
        // macOS는 lsof, linux는 /proc — 우리 자신의 pid로 스모크 테스트.
        let cwd = read_pid_cwd(std::process::id());
        let cwd = cwd.expect("own process cwd should be readable");
        assert!(Path::new(&cwd).is_dir());
    }
}
