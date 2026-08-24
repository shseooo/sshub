//! 세션 실행 계획 (DESIGN-terminal.md §6) — "무엇을 어떤 cwd로 띄울지"를
//! 결정하는 순수 로직. 실제 PTY spawn은 sshub-terminal이 한다.
//!
//! Electron 판 `startSession`의 결정 규칙을 그대로 옮겼다. 이 부분을 순수
//! 함수로 떼어 둔 이유는 cwd 상속 규칙이 회귀하기 쉬운 지점이기 때문이다
//! (분할할 때 현재 디렉터리를 물려받지 못하는 버그가 실제로 있었다).

use std::collections::HashMap;
use std::path::PathBuf;

use sshub_core::model::Server;
use sshub_core::{build_connect_banner, build_ssh_args};

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
///
/// Phase 3부터 접속 대상은 별칭 하나이고 키/포트/유저/점프 호스트는 전부
/// `~/.ssh/config`의 Host 블록이 갖는다 — 그래서 여기서 키 경로를 해석할 일이
/// 없어졌다(`resolve_ssh_paths` 삭제).
pub fn plan_ssh(server: &Server, home: PathBuf) -> SpawnPlan {
    SpawnPlan {
        program: "ssh".to_string(),
        args: build_ssh_args(server),
        cwd: home,
        env: base_env(),
        banner: Some(build_connect_banner(server)),
    }
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
    fn ssh_plan_connects_through_the_alias_and_keeps_the_banner() {
        let plan = plan_ssh(&server(AuthType::Agent), PathBuf::from("/home/u"));
        assert_eq!(plan.program, "ssh");
        assert_eq!(plan.args.last().unwrap(), "prod", "별칭 하나가 접속 대상");
        // 포트·유저·호스트는 config 블록의 몫이다 — 커맨드라인에 없어야 한다.
        assert!(!plan.args.iter().any(|a| a == "-p" || a.contains('@')), "{:?}", plan.args);
        let banner = plan.banner.unwrap();
        assert!(banner.contains("deploy@example.com:2200"));
        assert!(banner.starts_with("\u{1b}[90m"), "회색 SGR로 시작");
        assert_eq!(plan.cwd, PathBuf::from("/home/u"), "SSH는 로컬 홈에서 실행");
    }

    #[test]
    fn pane_labels_follow_the_electron_rule() {
        let srv = server(AuthType::Key);
        assert_eq!(pane_label(Some(&srv), "로컬", "서버"), "prod - deploy@example.com");
        assert_eq!(pane_label(None, "로컬", "서버"), "로컬");
    }
}
