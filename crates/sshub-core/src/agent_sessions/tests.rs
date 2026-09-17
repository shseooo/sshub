//! 실제 `~/.claude`·`~/.pi`는 절대 읽지 않는다 — 전부 `tempfile` 안의 가짜 홈.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;

fn env_in(home: &Path) -> Env {
    Env { home: home.to_path_buf(), vars: HashMap::new() }
}

fn write(path: &Path, lines: &[&str]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn set_mtime(path: &Path, secs_ago: u64) {
    let t = SystemTime::now() - Duration::from_secs(secs_ago);
    let f = fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(t).unwrap();
}

// ---- 공통 --------------------------------------------------------------

#[test]
fn safe_ids_are_alphanumeric_dash_underscore_only() {
    assert!(is_safe_id("1e3d852f-314b-400e-80c6-7c467940ecb3"));
    assert!(is_safe_id("abc_DEF-9"));
    assert!(!is_safe_id(""));
    assert!(!is_safe_id("../etc/passwd"));
    assert!(!is_safe_id("a b"));
    assert!(!is_safe_id("x; rm -rf ~"));
    assert!(!is_safe_id("$(whoami)"));
    assert!(!is_safe_id(&"a".repeat(65)));
}

#[test]
fn resume_command_refuses_unknown_agents_and_unsafe_ids() {
    assert_eq!(resume_command("claude", "abc-123").as_deref(), Some("claude --resume abc-123"));
    assert_eq!(resume_command("pi", "abc-123").as_deref(), Some("pi --session abc-123"));
    assert_eq!(resume_command("codex", "abc-123"), None);
    assert_eq!(resume_command("claude", "abc;rm"), None);
}

#[test]
fn title_line_takes_the_first_non_empty_line_and_truncates() {
    assert_eq!(title_line("\n\n  hello   world  \nsecond", 48), "hello world");
    assert_eq!(title_line("가나다라마바사", 4), "가나다…");
    assert_eq!(title_line("", 10), "");
}

#[test]
fn nothing_is_listed_when_no_agent_is_installed() {
    let home = tempfile::tempdir().unwrap();
    let groups = list_all(&env_in(home.path()), Path::new("/work/x"), 5);
    assert!(groups.is_empty());
}

#[test]
fn registry_ids_are_unique() {
    let ids: Vec<&str> = registry().iter().map(|a| a.id()).collect();
    let mut dedup = ids.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(ids.len(), dedup.len());
}

// ---- Claude Code -------------------------------------------------------

const CWD: &str = "/Users/me/work/sub_projects/app";

fn claude_dir(home: &Path) -> PathBuf {
    home.join(".claude/projects").join(ClaudeCode::encode_cwd(Path::new(CWD)))
}

fn claude_user(cwd: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":{}}},"timestamp":"2026-09-17T05:16:41.463Z"}}"#,
        serde_json::to_string(text).unwrap()
    )
}

#[test]
fn claude_encodes_slashes_and_underscores_as_dashes() {
    assert_eq!(
        ClaudeCode::encode_cwd(Path::new(CWD)),
        "-Users-me-work-sub-projects-app"
    );
}

#[test]
fn claude_prefers_the_last_ai_title_over_the_first_prompt() {
    let home = tempfile::tempdir().unwrap();
    let file = claude_dir(home.path()).join("aaaa-1111.jsonl");
    write(
        &file,
        &[
            r#"{"type":"mode","sessionId":"aaaa-1111"}"#,
            &claude_user(CWD, "첫 프롬프트\n둘째 줄"),
            r#"{"type":"ai-title","aiTitle":"옛 제목","sessionId":"aaaa-1111"}"#,
            r#"{"type":"assistant","cwd":"/Users/me/work/sub_projects/app","message":{"role":"assistant","content":[]}}"#,
            r#"{"type":"ai-title","aiTitle":"새 제목","sessionId":"aaaa-1111"}"#,
        ],
    );
    let list = list_sessions(&ClaudeCode, &env_in(home.path()), Path::new(CWD), 10);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "aaaa-1111");
    assert_eq!(list[0].title, "새 제목");
    assert_eq!(list[0].agent, "claude");
}

#[test]
fn claude_falls_back_to_the_first_real_prompt() {
    let home = tempfile::tempdir().unwrap();
    let file = claude_dir(home.path()).join("bbbb-2222.jsonl");
    let meta = format!(
        r#"{{"type":"user","isMeta":true,"cwd":"{CWD}","message":{{"role":"user","content":"caveat"}}}}"#
    );
    write(
        &file,
        &[
            &meta,
            &claude_user(CWD, "<local-command-stdout>ls</local-command-stdout>"),
            &claude_user(CWD, "탭 이름 고쳐 줘"),
            &claude_user(CWD, "두 번째 질문"),
        ],
    );
    let list = list_sessions(&ClaudeCode, &env_in(home.path()), Path::new(CWD), 10);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "탭 이름 고쳐 줘");
}

#[test]
fn claude_skips_sessions_from_a_colliding_cwd_and_empty_sessions() {
    // `sub-projects`와 `sub_projects`는 같은 디렉터리로 인코딩된다.
    let home = tempfile::tempdir().unwrap();
    let dir = claude_dir(home.path());
    write(
        &dir.join("other.jsonl"),
        &[&claude_user("/Users/me/work/sub-projects/app", "남의 세션")],
    );
    write(&dir.join("empty.jsonl"), &[r#"{"type":"mode","sessionId":"empty"}"#]);
    write(&dir.join("mine.jsonl"), &[&claude_user(CWD, "내 세션")]);
    let list = list_sessions(&ClaudeCode, &env_in(home.path()), Path::new(CWD), 10);
    let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["mine"]);
}

#[test]
fn claude_lists_newest_first_and_honors_the_limit() {
    let home = tempfile::tempdir().unwrap();
    let dir = claude_dir(home.path());
    for (name, age) in [("old", 300u64), ("new", 10), ("mid", 100)] {
        let f = dir.join(format!("{name}.jsonl"));
        write(&f, &[&claude_user(CWD, name)]);
        set_mtime(&f, age);
    }
    let list = list_sessions(&ClaudeCode, &env_in(home.path()), Path::new(CWD), 2);
    let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["new", "mid"]);
}

#[test]
fn claude_ignores_files_with_unsafe_names() {
    let home = tempfile::tempdir().unwrap();
    let dir = claude_dir(home.path());
    write(&dir.join("a b.jsonl"), &[&claude_user(CWD, "x")]);
    write(&dir.join("ok.jsonl"), &[&claude_user(CWD, "y")]);
    let list = list_sessions(&ClaudeCode, &env_in(home.path()), Path::new(CWD), 10);
    let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["ok"]);
}

#[test]
fn claude_config_dir_can_be_overridden() {
    let home = tempfile::tempdir().unwrap();
    let alt = tempfile::tempdir().unwrap();
    let config = alt.path().join("claude-config"); // 아직 없는 디렉터리
    let mut env = env_in(home.path());
    env.vars.insert("CLAUDE_CONFIG_DIR".into(), config.to_string_lossy().into_owned());
    assert!(!ClaudeCode.is_available(&env));
    let dir = config.join("projects").join(ClaudeCode::encode_cwd(Path::new(CWD)));
    write(&dir.join("s.jsonl"), &[&claude_user(CWD, "hi")]);
    assert!(ClaudeCode.is_available(&env));
    assert_eq!(list_sessions(&ClaudeCode, &env, Path::new(CWD), 10).len(), 1);
}

// ---- pi ----------------------------------------------------------------

fn pi_dir(home: &Path) -> PathBuf {
    home.join(".pi/agent/sessions").join(Pi::encode_cwd(Path::new(CWD)))
}

fn pi_header(id: &str, cwd: &str) -> String {
    format!(r#"{{"type":"session","version":3,"id":"{id}","timestamp":"2026-09-16T05:28:47.740Z","cwd":"{cwd}"}}"#)
}

fn pi_user(text: &str) -> String {
    format!(
        r#"{{"type":"message","id":"96eac820","parentId":null,"timestamp":"2026-09-16T05:29:04.015Z","message":{{"role":"user","content":[{{"type":"text","text":{}}}]}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

#[test]
fn pi_encodes_slashes_only_and_wraps_in_double_dashes() {
    assert_eq!(
        Pi::encode_cwd(Path::new(CWD)),
        "--Users-me-work-sub_projects-app--"
    );
}

#[test]
fn pi_reads_the_id_from_the_header_and_titles_by_first_prompt() {
    let home = tempfile::tempdir().unwrap();
    let file = pi_dir(home.path()).join("2026-09-16T05-28-47-740Z_01a0a8b0-6579-725d-b624-08f4e5bd2a90.jsonl");
    write(
        &file,
        &[
            &pi_header("01a0a8b0-6579-725d-b624-08f4e5bd2a90", CWD),
            r#"{"type":"model_change","provider":"omlx","modelId":"Qwen"}"#,
            &pi_user("# Role\n야트지 게임 만들기"),
            &pi_user("다음 단계"),
        ],
    );
    let list = list_sessions(&Pi, &env_in(home.path()), Path::new(CWD), 10);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "01a0a8b0-6579-725d-b624-08f4e5bd2a90");
    assert_eq!(list[0].title, "# Role");
    assert_eq!(list[0].agent, "pi");
}

#[test]
fn pi_prefers_a_session_name_when_present() {
    let home = tempfile::tempdir().unwrap();
    let file = pi_dir(home.path()).join("x_deadbeef.jsonl");
    write(
        &file,
        &[
            &pi_header("deadbeef", CWD),
            &pi_user("첫 질문"),
            r#"{"type":"session_info","name":"인증 리팩터","id":"1"}"#,
        ],
    );
    let list = list_sessions(&Pi, &env_in(home.path()), Path::new(CWD), 10);
    assert_eq!(list[0].title, "인증 리팩터");
}

#[test]
fn pi_skips_other_cwds_and_non_session_files() {
    let home = tempfile::tempdir().unwrap();
    let dir = pi_dir(home.path());
    write(&dir.join("a.jsonl"), &[&pi_header("aaaa", "/somewhere/else"), &pi_user("x")]);
    write(&dir.join("b.jsonl"), &[r#"{"type":"message"}"#, &pi_user("x")]);
    write(&dir.join("c.jsonl"), &[&pi_header("cccc", CWD), &pi_user("x")]);
    write(&dir.join("d.jsonl"), &[&pi_header("dddd", CWD)]); // 빈 세션
    let list = list_sessions(&Pi, &env_in(home.path()), Path::new(CWD), 10);
    let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["cccc"]);
}

#[test]
fn pi_session_dir_can_be_overridden() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".pi/agent")).unwrap();
    let alt = tempfile::tempdir().unwrap();
    let mut env = env_in(home.path());
    env.vars.insert(
        "PI_CODING_AGENT_SESSION_DIR".into(),
        alt.path().to_string_lossy().into_owned(),
    );
    let dir = alt.path().join(Pi::encode_cwd(Path::new(CWD)));
    write(&dir.join("s.jsonl"), &[&pi_header("s1", CWD), &pi_user("hi")]);
    assert!(Pi.is_available(&env));
    assert_eq!(list_sessions(&Pi, &env, Path::new(CWD), 10).len(), 1);
}

// ---- 합성 --------------------------------------------------------------

#[test]
fn list_all_groups_only_installed_agents_in_registry_order() {
    let home = tempfile::tempdir().unwrap();
    // pi만 설치된 홈.
    let dir = pi_dir(home.path());
    write(&dir.join("s.jsonl"), &[&pi_header("s1", CWD), &pi_user("hi")]);
    let groups = list_all(&env_in(home.path()), Path::new(CWD), 5);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].agent, "pi");
    assert_eq!(groups[0].new_session_command, "pi");
    assert_eq!(groups[0].sessions.len(), 1);

    // 둘 다 설치됐지만 이 cwd의 세션은 없는 경우 — 그룹은 있고 목록은 빈다.
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    let groups = list_all(&env_in(home.path()), Path::new("/no/such/dir"), 5);
    let names: Vec<&str> = groups.iter().map(|g| g.agent).collect();
    assert_eq!(names, ["claude", "pi"]);
    assert!(groups.iter().all(|g| g.sessions.is_empty()));
}
