//! Claude Code (`claude`) — `~/.claude/projects/<인코딩된 cwd>/<uuid>.jsonl`.
//!
//! 인코딩은 경로의 `/`와 `_`를 **모두** `-`로 바꾼다(`sub_projects` →
//! `sub-projects`). 그래서 디렉터리 이름만으로는 cwd를 되찾을 수 없고, 서로 다른
//! cwd가 한 디렉터리를 공유할 수도 있다 — 파일 안의 `cwd` 필드로 다시 확인한다.
//!
//! 제목은 `ai-title` 레코드(`aiTitle`)가 있으면 그것(여러 번 다시 쓰이므로
//! **마지막** 것), 없으면 첫 사용자 프롬프트. `isMeta` 메시지와 `<…>` 태그로
//! 시작하는 시스템성 메시지는 프롬프트로 치지 않는다.
//!
//! 설정 디렉터리는 `CLAUDE_CONFIG_DIR`로 바꿀 수 있다.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::super::{jsonl, title_line, AgentSession, Env, SessionAgent};

pub struct ClaudeCode;

const TITLE_CHARS: usize = 48;

impl ClaudeCode {
    fn config_dir(env: &Env) -> PathBuf {
        match env.var("CLAUDE_CONFIG_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => env.home.join(".claude"),
        }
    }

    /// `/Users/me/sub_projects/x` → `-Users-me-sub-projects-x`
    pub fn encode_cwd(cwd: &Path) -> String {
        cwd.to_string_lossy()
            .chars()
            .map(|c| if c == '/' || c == '_' { '-' } else { c })
            .collect()
    }
}

impl SessionAgent for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn new_session_command(&self) -> String {
        "claude".to_string()
    }

    fn is_available(&self, env: &Env) -> bool {
        Self::config_dir(env).is_dir()
    }

    fn project_dir(&self, env: &Env, cwd: &Path) -> Option<PathBuf> {
        let dir = Self::config_dir(env).join("projects").join(Self::encode_cwd(cwd));
        dir.is_dir().then_some(dir)
    }

    fn parse_session(&self, path: &Path, cwd: &Path) -> Option<AgentSession> {
        let id = path.file_stem()?.to_str()?.to_string();
        let mut title: Option<String> = None;
        let mut first_prompt: Option<String> = None;
        let mut cwd_matches: Option<bool> = None;
        let want = cwd.to_string_lossy();

        jsonl::scan(
            path,
            &["\"type\":\"ai-title\"", "\"type\":\"user\"", "\"cwd\":"],
            |v| {
                if cwd_matches.is_none() {
                    if let Some(c) = v.get("cwd").and_then(Value::as_str) {
                        cwd_matches = Some(c == want);
                        if c != want {
                            return false;
                        }
                    }
                }
                match v.get("type").and_then(Value::as_str) {
                    Some("ai-title") => {
                        if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                            title = Some(t.to_string());
                        }
                    }
                    Some("user") if first_prompt.is_none() => {
                        let is_meta = v.get("isMeta").and_then(Value::as_bool).unwrap_or(false);
                        let is_side = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
                        if !is_meta && !is_side {
                            if let Some(text) = v
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(jsonl::content_text)
                            {
                                if !text.trim_start().starts_with('<') {
                                    first_prompt = Some(text);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                true
            },
        );

        // cwd가 확인되지 않은 파일(레코드에 cwd가 전혀 없음)은 남의 것일 수
        // 있으니 뺀다. 프롬프트도 제목도 없는 빈 세션은 재개할 가치가 없다.
        if cwd_matches != Some(true) {
            return None;
        }
        let title = title.or(first_prompt)?;
        Some(AgentSession {
            agent: self.id(),
            id,
            title: title_line(&title, TITLE_CHARS),
            updated_at: SystemTime::UNIX_EPOCH,
        })
    }

    fn resume_command(&self, session_id: &str) -> String {
        format!("claude --resume {session_id}")
    }
}
