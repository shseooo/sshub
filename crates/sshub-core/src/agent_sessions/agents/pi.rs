//! pi coding agent (`pi`) — `~/.pi/agent/sessions/--<인코딩된 cwd>--/<시각>_<uuid>.jsonl`.
//!
//! 인코딩은 `/`만 `-`로 바꾸고(`_`는 유지) 양끝을 `--`로 감싼다. 첫 줄이
//! `{"type":"session","id":…,"cwd":…}` 헤더라 첫 줄만 읽어도 id·cwd를 안다.
//!
//! 제목은 `--name`으로 붙인 이름(`session_info` 레코드의 `name`)이 있으면 그것,
//! 없으면 첫 사용자 메시지. 저장 위치는 `PI_CODING_AGENT_SESSION_DIR`(세션만)
//! 또는 `PI_CODING_AGENT_DIR`(설정 디렉터리 전체)로 바꿀 수 있다.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::super::{jsonl, title_line, AgentSession, Env, SessionAgent};

pub struct Pi;

const TITLE_CHARS: usize = 48;

impl Pi {
    fn agent_dir(env: &Env) -> PathBuf {
        match env.var("PI_CODING_AGENT_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => env.home.join(".pi").join("agent"),
        }
    }

    fn sessions_root(env: &Env) -> PathBuf {
        match env.var("PI_CODING_AGENT_SESSION_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => Self::agent_dir(env).join("sessions"),
        }
    }

    /// `/Users/me/sub_projects/x` → `--Users-me-sub_projects-x--`
    pub fn encode_cwd(cwd: &Path) -> String {
        let body: String = cwd
            .to_string_lossy()
            .chars()
            .map(|c| if c == '/' { '-' } else { c })
            .collect();
        format!("-{body}--")
    }
}

impl SessionAgent for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "pi"
    }

    fn new_session_command(&self) -> String {
        "pi".to_string()
    }

    fn is_available(&self, env: &Env) -> bool {
        Self::agent_dir(env).is_dir()
    }

    fn project_dir(&self, env: &Env, cwd: &Path) -> Option<PathBuf> {
        let dir = Self::sessions_root(env).join(Self::encode_cwd(cwd));
        dir.is_dir().then_some(dir)
    }

    fn parse_session(&self, path: &Path, cwd: &Path) -> Option<AgentSession> {
        let header = jsonl::first_line(path)?;
        if header.get("type").and_then(Value::as_str) != Some("session") {
            return None;
        }
        if header.get("cwd").and_then(Value::as_str) != Some(cwd.to_string_lossy().as_ref()) {
            return None;
        }
        let id = header.get("id").and_then(Value::as_str)?.to_string();

        let mut name: Option<String> = None;
        let mut first_prompt: Option<String> = None;
        jsonl::scan(
            path,
            &["\"type\":\"session_info\"", "\"role\":\"user\""],
            |v| {
                match v.get("type").and_then(Value::as_str) {
                    Some("session_info") => {
                        if let Some(n) = v.get("name").and_then(Value::as_str) {
                            name = Some(n.to_string());
                        }
                    }
                    Some("message") if first_prompt.is_none() => {
                        let msg = v.get("message");
                        if msg.and_then(|m| m.get("role")).and_then(Value::as_str) == Some("user") {
                            first_prompt = msg
                                .and_then(|m| m.get("content"))
                                .and_then(jsonl::content_text);
                        }
                    }
                    _ => {}
                }
                // 이름은 나중에 바뀔 수 있어 끝까지 본다. 이름 없는 세션이 대부분
                // 이고 파일도 작아(pi는 도구 결과를 덜 남긴다) 비용은 미미하다.
                true
            },
        );

        let title = name.or(first_prompt)?;
        Some(AgentSession {
            agent: self.id(),
            id,
            title: title_line(&title, TITLE_CHARS),
            updated_at: SystemTime::UNIX_EPOCH,
        })
    }

    fn resume_command(&self, session_id: &str) -> String {
        format!("pi --session {session_id}")
    }
}
