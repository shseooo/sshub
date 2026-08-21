//! 세션 실행 계획 (DESIGN-terminal.md §6) — "무엇을 어떤 cwd로 띄울지"를
//! 결정하는 순수 로직. 실제 PTY spawn은 sshub-terminal이 한다.
//!
//! Electron 판 `startSession`의 결정 규칙을 그대로 옮겼다. 이 부분을 순수
//! 함수로 떼어 둔 이유는 cwd 상속 규칙이 회귀하기 쉬운 지점이기 때문이다
//! (분할할 때 현재 디렉터리를 물려받지 못하는 버그가 실제로 있었다).

use std::collections::HashMap;
use std::path::PathBuf;

use sshub_core::model::{AuthType, Server};
use sshub_core::{build_connect_banner, build_ssh_args, key_files, SshPaths};

/// PTY에 넘길 실행 계획.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// 로컬로 주입할 안내 배너 (PTY에 쓰지 않는다).
    pub banner: Option<String>,
}

/// cwd 결정에 필요한 주변 정보.
pub struct SessionEnv<'a> {
    /// 살아 있는 로컬 세션의 현재 디렉터리 (분할 시 상속원).
    pub live_local_cwd: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// 이전 실행에서 저장해 둔 세션별 cwd.
    pub saved_cwd: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub home: PathBuf,
    pub shell: String,
}

/// 로컬 셸 세션의 시작 디렉터리:
/// ① 분할 원본(포커스된 로컬 세션)의 **현재** cwd → ② 저장된 cwd → ③ 홈.
///
/// ①은 "분할하면 지금 있던 디렉터리에서 열린다"는 기대를 만든다. ②는 앱을
/// 껐다 켰을 때 탭이 원래 자리로 돌아오게 한다. `cwd_from`이 지정된 분할
/// 상황에서는 ②로 내려가지 않는다 — 원본이 이미 죽었다면 저장된 값은 다른
/// 세션의 것이라 엉뚱한 곳을 열게 된다.
pub fn resolve_local_cwd(
    session_id: &str,
    cwd_from: Option<&str>,
    env: &SessionEnv<'_>,
) -> PathBuf {
    if let Some(source) = cwd_from {
        if let Some(cwd) = (env.live_local_cwd)(source) {
            return cwd;
        }
        return env.home.clone();
    }
    (env.saved_cwd)(session_id).unwrap_or_else(|| env.home.clone())
}

/// 로컬 셸 실행 계획. 로그인 셸(`-l`)로 띄워 사용자의 프로필이 적용되게 한다.
pub fn plan_local(session_id: &str, cwd_from: Option<&str>, env: &SessionEnv<'_>) -> SpawnPlan {
    SpawnPlan {
        program: env.shell.clone(),
        args: vec!["-l".to_string()],
        cwd: resolve_local_cwd(session_id, cwd_from, env),
        env: base_env(),
        banner: None,
    }
}

/// SSH 실행 계획. 원격 cwd는 복원할 수 없으므로 로컬 cwd는 홈으로 둔다.
pub fn plan_ssh(server: &Server, keys_dir: &std::path::Path, key_name: Option<&str>, home: PathBuf) -> SpawnPlan {
    let paths = resolve_ssh_paths(server, keys_dir, key_name);
    SpawnPlan {
        program: "ssh".to_string(),
        args: build_ssh_args(server, &paths),
        cwd: home,
        env: base_env(),
        banner: Some(build_connect_banner(server)),
    }
}

/// 인증 방식에 맞는 키/PEM 경로를 해석한다. **파일이 실제로 있을 때만**
/// 넘긴다 — 없는 경로로 `-i`를 주면 ssh가 즉시 실패해, 에이전트나 기본 키로
/// 붙을 수 있었을 기회까지 잃는다.
pub fn resolve_ssh_paths(
    server: &Server,
    keys_dir: &std::path::Path,
    key_name: Option<&str>,
) -> SshPaths {
    let mut paths = SshPaths::default();
    match server.auth_type {
        AuthType::Pem => {
            let pem = keys_dir.join(key_files::server_pem_file_name(server.id));
            if pem.exists() {
                paths.pem_path = Some(pem.to_string_lossy().into_owned());
            }
        }
        AuthType::Key => {
            if let Some(name) = key_name {
                let key = keys_dir.join(key_files::key_file_name(name));
                if key.exists() {
                    paths.key_path = Some(key.to_string_lossy().into_owned());
                }
            }
        }
        AuthType::Password | AuthType::Agent => {}
    }
    paths
}

fn base_env() -> Vec<(String, String)> {
    vec![("TERM".to_string(), "xterm-256color".to_string())]
}

/// pane 라벨: 로컬은 지역화된 "로컬", 서버는 `이름 - 사용자@호스트`.
/// 탭 제목이 이 라벨을 물려받으므로 규칙을 Electron 판과 맞춘다.
pub fn pane_label(server: Option<&Server>, local_label: &str, unknown_label: &str) -> String {
    match server {
        Some(s) => format!("{} - {}@{}", s.name, s.username, s.host),
        None if local_label.is_empty() => unknown_label.to_string(),
        None => local_label.to_string(),
    }
}

/// 세션 id → 살아 있는 터미널 여부를 묻기 위한 최소 인터페이스.
/// (레지스트리 구현은 터미널 계층이 소유한다.)
pub type LiveCwdMap = HashMap<String, PathBuf>;

#[cfg(test)]
mod tests {
    use super::*;
    use sshub_core::model::AuthType;

    fn env_with(live: LiveCwdMap, saved: LiveCwdMap) -> (LiveCwdMap, LiveCwdMap) {
        (live, saved)
    }

    fn make_env<'a>(
        live: &'a LiveCwdMap,
        saved: &'a LiveCwdMap,
        live_fn: &'a dyn Fn(&str) -> Option<PathBuf>,
        saved_fn: &'a dyn Fn(&str) -> Option<PathBuf>,
    ) -> SessionEnv<'a> {
        let _ = (live, saved);
        SessionEnv {
            live_local_cwd: live_fn,
            saved_cwd: saved_fn,
            home: PathBuf::from("/Users/tester"),
            shell: "/bin/zsh".to_string(),
        }
    }

    #[test]
    fn split_inherits_the_live_cwd_of_the_source_pane() {
        let (live, saved) = env_with(
            [("src".to_string(), PathBuf::from("/work/project"))].into_iter().collect(),
            [("new".to_string(), PathBuf::from("/old/saved"))].into_iter().collect(),
        );
        let live_fn = |id: &str| live.get(id).cloned();
        let saved_fn = |id: &str| saved.get(id).cloned();
        let env = make_env(&live, &saved, &live_fn, &saved_fn);

        let plan = plan_local("new", Some("src"), &env);
        assert_eq!(plan.cwd, PathBuf::from("/work/project"));
        assert_eq!(plan.program, "/bin/zsh");
        assert_eq!(plan.args, ["-l"]);
        assert!(plan.banner.is_none());
    }

    #[test]
    fn split_from_a_dead_source_falls_back_to_home_not_saved_cwd() {
        // 저장된 값은 '이 세션'의 과거 위치가 아니라 남의 것일 수 있다.
        let (live, saved) = env_with(
            LiveCwdMap::new(),
            [("new".to_string(), PathBuf::from("/old/saved"))].into_iter().collect(),
        );
        let live_fn = |id: &str| live.get(id).cloned();
        let saved_fn = |id: &str| saved.get(id).cloned();
        let env = make_env(&live, &saved, &live_fn, &saved_fn);

        assert_eq!(resolve_local_cwd("new", Some("gone"), &env), PathBuf::from("/Users/tester"));
    }

    #[test]
    fn restored_session_uses_its_saved_cwd() {
        let (live, saved) = env_with(
            LiveCwdMap::new(),
            [("s1".to_string(), PathBuf::from("/restored/path"))].into_iter().collect(),
        );
        let live_fn = |id: &str| live.get(id).cloned();
        let saved_fn = |id: &str| saved.get(id).cloned();
        let env = make_env(&live, &saved, &live_fn, &saved_fn);

        assert_eq!(resolve_local_cwd("s1", None, &env), PathBuf::from("/restored/path"));
        assert_eq!(resolve_local_cwd("unknown", None, &env), PathBuf::from("/Users/tester"));
    }

    fn server(auth: AuthType) -> Server {
        Server {
            id: 7,
            name: "prod".into(),
            host: "example.com".into(),
            port: 2200,
            username: "deploy".into(),
            auth_type: auth,
            ..Server::default()
        }
    }

    #[test]
    fn ssh_plan_carries_args_and_banner() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan_ssh(&server(AuthType::Agent), dir.path(), None, PathBuf::from("/home/u"));
        assert_eq!(plan.program, "ssh");
        assert!(plan.args.contains(&"deploy@example.com".to_string()));
        assert!(plan.args.contains(&"-p".to_string()), "22가 아니면 포트 인자");
        let banner = plan.banner.unwrap();
        assert!(banner.contains("deploy@example.com:2200"));
        assert!(banner.starts_with("\u{1b}[90m"), "회색 SGR로 시작");
        assert_eq!(plan.cwd, PathBuf::from("/home/u"), "SSH는 로컬 홈에서 실행");
    }

    #[test]
    fn key_auth_only_passes_identity_when_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(AuthType::Key);

        // 파일이 없으면 -i 를 넘기지 않는다.
        let paths = resolve_ssh_paths(&srv, dir.path(), Some("work key"));
        assert_eq!(paths.key_path, None);

        std::fs::write(dir.path().join("id_work_key"), b"x").unwrap();
        let paths = resolve_ssh_paths(&srv, dir.path(), Some("work key"));
        assert!(paths.key_path.unwrap().ends_with("id_work_key"), "새니타이즈된 파일명 사용");
    }

    #[test]
    fn pem_auth_resolves_the_per_server_pem_file() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(AuthType::Pem);
        assert_eq!(resolve_ssh_paths(&srv, dir.path(), None).pem_path, None);

        std::fs::write(dir.path().join("pem_server_7"), b"x").unwrap();
        let paths = resolve_ssh_paths(&srv, dir.path(), None);
        assert!(paths.pem_path.unwrap().ends_with("pem_server_7"));
    }

    #[test]
    fn password_and_agent_auth_never_pass_identity_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pem_server_7"), b"x").unwrap();
        for auth in [AuthType::Password, AuthType::Agent] {
            let paths = resolve_ssh_paths(&server(auth), dir.path(), Some("k"));
            assert_eq!(paths.key_path, None);
            assert_eq!(paths.pem_path, None);
        }
    }

    #[test]
    fn pane_labels_follow_the_electron_rule() {
        let srv = server(AuthType::Key);
        assert_eq!(pane_label(Some(&srv), "로컬", "서버"), "prod - deploy@example.com");
        assert_eq!(pane_label(None, "로컬", "서버"), "로컬");
    }
}
