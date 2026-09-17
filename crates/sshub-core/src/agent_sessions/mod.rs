//! 코딩 에이전트(Claude Code · pi …) 세션 탐색 + 재개 명령.
//!
//! 에이전트마다 세션을 **cwd 기준으로** 로컬 파일에 남기고, 세션 id로 재개하는
//! CLI 플래그가 있다. 이 모듈은 "이 폴더에서 열었던 세션 목록"을 읽어 주고,
//! 선택한 세션을 되살리는 명령줄을 만든다.
//!
//! 에이전트 하나 = [`SessionAgent`] 구현 하나. 새 에이전트는 `agents/` 아래에
//! 파일을 추가하고 [`registry`]에 넣기만 하면 된다. 저장 형식은 전부
//! **비공식**이라 파싱에 실패한 파일은 조용히 건너뛴다 — 목록이 비는 것은
//! 괜찮지만 앱이 멈추면 안 된다.
//!
//! 홈·환경 변수는 밖에서 주입한다([`Env`]) — 테스트가 실제 `~/.claude`나
//! `~/.pi`를 읽지 않게 하기 위해서다.

mod agents;
mod jsonl;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use agents::{claude_code::ClaudeCode, pi::Pi};

/// 홈 디렉터리와 환경 변수 — 실제 프로세스 값 또는 테스트용 고정값.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub home: PathBuf,
    pub vars: HashMap<String, String>,
}

impl Env {
    /// 실제 프로세스 환경. 홈을 못 찾으면 `None`(이 기능만 꺼진다).
    pub fn from_process() -> Option<Env> {
        Some(Env {
            home: dirs::home_dir()?,
            vars: std::env::vars().collect(),
        })
    }

    pub fn var(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// 에이전트 하나가 남긴 세션 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    /// [`SessionAgent::id`]
    pub agent: &'static str,
    /// 재개 명령에 넣는 id. 새니타이즈 통과분만 여기 온다([`is_safe_id`]).
    pub id: String,
    /// 표시 제목 — 에이전트가 붙인 제목, 없으면 첫 사용자 프롬프트의 첫 줄.
    pub title: String,
    /// 마지막 갱신(파일 mtime). 목록 정렬 기준.
    pub updated_at: SystemTime,
}

/// 에이전트 어댑터. 파일 형식과 CLI 플래그를 아는 유일한 곳.
pub trait SessionAgent: Send + Sync {
    /// 안정적인 식별자 (`"claude"`, `"pi"`). 저장·로그에 쓴다.
    fn id(&self) -> &'static str;
    /// 메뉴에 보이는 이름.
    fn display_name(&self) -> &'static str;
    /// 새 세션을 시작하는 명령줄.
    fn new_session_command(&self) -> String;
    /// 이 에이전트가 설치돼 있는가 — 설정 디렉터리 존재 여부로 본다.
    /// (PATH 조회는 Finder에서 띄운 앱의 PATH가 셸과 달라 믿을 수 없다.)
    fn is_available(&self, env: &Env) -> bool;
    /// `cwd`의 세션들이 들어 있는 디렉터리. 없으면 `None`.
    fn project_dir(&self, env: &Env, cwd: &Path) -> Option<PathBuf>;
    /// 세션 파일 하나 → 요약. 이 에이전트의 세션 파일이 아니거나 다른 cwd의
    /// 것이면 `None`.
    fn parse_session(&self, path: &Path, cwd: &Path) -> Option<AgentSession>;
    /// `session_id`를 재개하는 명령줄. id는 이미 [`is_safe_id`]를 통과했다.
    fn resume_command(&self, session_id: &str) -> String;
}

/// 지원하는 에이전트 전부. 메뉴 순서가 곧 이 순서다.
pub fn registry() -> Vec<Box<dyn SessionAgent>> {
    vec![Box::new(ClaudeCode), Box::new(Pi)]
}

/// 셸에 그대로 넣어도 안전한 세션 id인가 — 영숫자·`-`·`_`만, 1..=64자.
/// 세션 파일은 사용자 홈 아래지만 다른 프로그램이 쓴 것이라 신뢰하지 않는다.
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 에이전트 하나의 `cwd` 세션 목록 — 최근 갱신 순, 최대 `limit`개.
pub fn list_sessions(
    agent: &dyn SessionAgent,
    env: &Env,
    cwd: &Path,
    limit: usize,
) -> Vec<AgentSession> {
    let Some(dir) = agent.project_dir(env, cwd) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    // mtime으로 먼저 정렬해 최근 파일부터 파싱하고, `limit`개가 차면 멈춘다 —
    // 세션 파일은 수 MB까지 커서 전부 읽으면 메뉴가 늦게 뜬다.
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));

    let mut out = Vec::new();
    for (modified, path) in files {
        if out.len() >= limit {
            break;
        }
        if let Some(mut session) = agent.parse_session(&path, cwd) {
            if !is_safe_id(&session.id) {
                continue;
            }
            session.updated_at = modified;
            out.push(session);
        }
    }
    out
}

/// 메뉴 한 묶음 — 에이전트 하나와 그 세션들.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentGroup {
    pub agent: &'static str,
    pub display_name: &'static str,
    pub new_session_command: String,
    pub sessions: Vec<AgentSession>,
}

/// 설치된 에이전트 전부의 `cwd` 세션 목록. 설치 안 된 에이전트는 빠진다.
pub fn list_all(env: &Env, cwd: &Path, per_agent: usize) -> Vec<AgentGroup> {
    registry()
        .iter()
        .filter(|a| a.is_available(env))
        .map(|a| AgentGroup {
            agent: a.id(),
            display_name: a.display_name(),
            new_session_command: a.new_session_command(),
            sessions: list_sessions(a.as_ref(), env, cwd, per_agent),
        })
        .collect()
}

/// `agent`의 `session_id`를 재개하는 명령줄. 모르는 에이전트나 위험한 id면 `None`.
pub fn resume_command(agent: &str, session_id: &str) -> Option<String> {
    if !is_safe_id(session_id) {
        return None;
    }
    registry()
        .into_iter()
        .find(|a| a.id() == agent)
        .map(|a| a.resume_command(session_id))
}

/// 메뉴 힌트용 갱신 시각 — 로컬 시간 `MM/DD HH:MM`.
pub fn format_updated(at: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = at.into();
    local.format("%m/%d %H:%M").to_string()
}

/// 제목용 한 줄 — 첫 줄만, 공백 정리, `max_chars`에서 자른다.
pub fn title_line(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let mut s: String = collapsed.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests;
