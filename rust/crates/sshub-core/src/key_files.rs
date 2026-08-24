//! 키/PEM 파일명. 보안 경계: 사용자 입력 이름이 그대로 `~/.ssh` 안의 파일명이
//! 되므로, 경로 요소 **하나**로 접히는 것만 허용한다.
//!
//! Phase 4에서 `id_` 강제 접두사를 뺐다 — 키의 이름이 곧 파일 이름이다
//! (`id_rsa`, `id_rsa_seod`). 접두사를 계속 붙이면 사용자의 `~/.ssh/id_rsa`를
//! 앱은 `id_id_rsa`라는 있지도 않은 파일로 찾아 영영 만나지 못한다.

/// 사용자 입력 → 안전한 파일명.
///
/// `[A-Za-z0-9._-]` 밖의 모든 문자(특히 `/`)를 `_`로 중화하고, 앞의 `.`은
/// 전부 벗긴다 — `.`·`..`(경로 traversal)과 숨김 파일을 한 규칙으로 막는다.
/// 코드포인트 단위 순회라 멀티유닛 문자도 `_` 하나로 접힌다.
/// 남는 게 없으면 `key` (빈 파일명은 만들 수 없다).
pub fn key_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = safe.trim_start_matches('.');
    if trimmed.is_empty() {
        "key".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 이미 디스크에 있는 파일 이름은 **새니타이즈하지 않는다** —
/// `id_ed25519@work` 같은 실제 파일명을 고쳐 쓰면 엉뚱한 파일을 가리키거나
/// 없는 파일을 만든다. 대신 "경로 요소 하나인가"만 확인한다.
pub fn safe_file_component(name: &str) -> Option<&str> {
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    if name.chars().any(char::is_control) {
        return None;
    }
    Some(name)
}

/// v1·v2 사이드카의 "이름 X ↔ 파일 `id_X`" 규칙. 사이드카를 v3로 올릴 때
/// 옛 이름을 **그 시절의 파일명**으로 되돌리는 데만 쓴다 (새 이름을 만드는
/// 데는 절대 쓰지 않는다).
pub fn legacy_key_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!("id_{safe}")
}

pub fn server_pem_file_name(id: i64) -> String {
    format!("pem_server_{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_unsafe_chars_with_underscore_without_forcing_a_prefix() {
        assert_eq!(key_file_name("my key!"), "my_key_");
        assert_eq!(key_file_name("ok-name_1"), "ok-name_1");
        // 실제 키 파일명에 흔한 점은 살린다 (`id_rsa.pem`).
        assert_eq!(key_file_name("id_rsa.pem"), "id_rsa.pem");
    }

    #[test]
    fn neutralizes_path_traversal_characters() {
        assert_eq!(key_file_name("../etc/passwd"), "_etc_passwd");
        assert_eq!(key_file_name("a/../b"), "a_.._b");
        assert_eq!(key_file_name(".."), "key");
        assert_eq!(key_file_name("."), "key");
        // 앞의 구분자도 다른 것들과 같게 `_`가 된다 (경로 요소는 언제나 하나).
        assert_eq!(key_file_name("/etc/shadow"), "_etc_shadow");
        // 결과는 언제나 경로 요소 하나다.
        for probe in ["../../x", "..", "/", "a/b/c", "\u{0}x"] {
            let out = key_file_name(probe);
            assert!(!out.contains('/'), "{probe} → {out}");
            assert!(out != "." && out != "..", "{probe} → {out}");
        }
    }

    #[test]
    fn never_produces_a_dotfile_or_an_empty_name() {
        assert_eq!(key_file_name(".DS_Store"), "DS_Store");
        assert_eq!(key_file_name(""), "key");
        assert_eq!(key_file_name("   "), "___");
    }

    #[test]
    fn keeps_only_ascii_alphanumerics_dot_dash_underscore() {
        assert_eq!(key_file_name("aZ09-_."), "aZ09-_.");
        // non-ASCII → 코드포인트당 밑줄 하나
        assert_eq!(key_file_name("é한"), "__");
    }

    #[test]
    fn safe_component_accepts_real_file_names_and_refuses_traversal() {
        assert_eq!(safe_file_component("id_ed25519@work"), Some("id_ed25519@work"));
        assert_eq!(safe_file_component("id_rsa"), Some("id_rsa"));
        assert_eq!(safe_file_component(".."), None);
        assert_eq!(safe_file_component("."), None);
        assert_eq!(safe_file_component(""), None);
        assert_eq!(safe_file_component("../etc/passwd"), None);
        assert_eq!(safe_file_component("a/b"), None);
        assert_eq!(safe_file_component("a\nb"), None);
    }

    #[test]
    fn legacy_rule_reproduces_the_v2_file_name() {
        assert_eq!(legacy_key_file_name("work key"), "id_work_key");
        assert_eq!(legacy_key_file_name("HIS-CodeCommit-User"), "id_HIS-CodeCommit-User");
    }

    #[test]
    fn builds_pem_server_id() {
        assert_eq!(server_pem_file_name(7), "pem_server_7");
    }
}
