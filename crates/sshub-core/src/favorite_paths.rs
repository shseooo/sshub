//! 즐겨찾기 경로 — pane 메뉴에서 고르면 그 pane의 셸에 `cd <경로>`를 보낸다.
//!
//! 경로는 사용자가 설정 화면에 적은 문자열 그대로 저장한다(`Settings::favorite_paths`).
//! 로컬 pane뿐 아니라 원격 셸에서도 쓸 수 있어야 하므로 여기서 존재 여부를
//! 검사하지 않는다 — 없는 폴더면 셸이 에러를 내고, 그게 맞는 동작이다.
//!
//! 셸에 넣는 문자열은 이 모듈이 전부 인용한다. 설정 파일은 사용자 소유지만
//! 다른 프로그램이 고쳐 놓을 수도 있으니 신뢰하지 않는다.

/// 설정에 저장하기 전 정규화. 앞뒤 공백을 지우고, 빈 문자열·개행/제어 문자가
/// 든 값은 거부한다 — 제어 문자는 어떻게 인용해도 터미널 한 줄에 못 넣는다.
pub fn normalize_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    Some(trimmed.to_string())
}

/// `cd` 명령줄(Enter 제외). `~`/`~/…`만 셸이 확장하도록 인용 밖에 두고, 나머지는
/// 전부 작은따옴표로 감싼다. 공백·`$`·백틱·`;`가 들어 있어도 그대로 경로로 취급된다.
pub fn cd_command(path: &str) -> Option<String> {
    let path = normalize_path(path)?;
    let (prefix, rest) = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => ("~", rest),
        _ => ("", path.as_str()),
    };
    if rest.is_empty() {
        return Some(format!("cd {prefix}"));
    }
    Some(format!("cd {prefix}{}", single_quote(rest)))
}

/// POSIX 작은따옴표 인용. 안의 `'`는 `'\''`로 끊어 잇는다.
fn single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// 설정 목록에 추가할 때의 규칙: 정규화 후 **중복은 넣지 않는다**(같은 경로가
/// 메뉴에 두 번 뜨면 어느 쪽을 지워야 하는지 헷갈린다). 추가됐으면 true.
pub fn add_favorite(list: &mut Vec<String>, raw: &str) -> bool {
    let Some(path) = normalize_path(raw) else { return false };
    if list.iter().any(|p| *p == path) {
        return false;
    }
    list.push(path);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_absolute_path_is_single_quoted() {
        assert_eq!(cd_command("/Users/me/work").unwrap(), "cd '/Users/me/work'");
    }

    #[test]
    fn tilde_prefix_stays_outside_quotes_so_the_shell_expands_it() {
        assert_eq!(cd_command("~").unwrap(), "cd ~");
        assert_eq!(cd_command("~/").unwrap(), "cd ~'/'");
        assert_eq!(cd_command("~/work dir").unwrap(), "cd ~'/work dir'");
    }

    #[test]
    fn tilde_user_form_is_not_expanded() {
        // `~bob/x`는 지원하지 않는다 — 글자 그대로의 폴더명으로 취급.
        assert_eq!(cd_command("~bob/x").unwrap(), "cd '~bob/x'");
    }

    #[test]
    fn shell_metacharacters_are_inert() {
        assert_eq!(
            cd_command("/tmp/a b; rm -rf $HOME `x`").unwrap(),
            "cd '/tmp/a b; rm -rf $HOME `x`'"
        );
    }

    #[test]
    fn embedded_single_quote_is_escaped() {
        assert_eq!(cd_command("/tmp/it's").unwrap(), r"cd '/tmp/it'\''s'");
    }

    #[test]
    fn control_characters_and_empty_input_are_rejected() {
        assert_eq!(cd_command(""), None);
        assert_eq!(cd_command("   "), None);
        assert_eq!(cd_command("/tmp\nrm -rf /"), None);
        assert_eq!(cd_command("/tmp\rls"), None);
        assert_eq!(cd_command("/tmp\x1b[2J"), None);
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_path("  /a/b  ").unwrap(), "/a/b");
    }

    #[test]
    fn add_favorite_dedupes_and_rejects_bad_input() {
        let mut list = vec![];
        assert!(add_favorite(&mut list, " /a "));
        assert!(!add_favorite(&mut list, "/a"));
        assert!(!add_favorite(&mut list, ""));
        assert!(!add_favorite(&mut list, "/b\nx"));
        assert!(add_favorite(&mut list, "~/b"));
        assert_eq!(list, vec!["/a", "~/b"]);
    }
}
