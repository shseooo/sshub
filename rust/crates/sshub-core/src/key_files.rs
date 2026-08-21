//! 키/PEM 파일명. 보안 경계: 사용자 입력 이름이 파일명이 되므로
//! `[A-Za-z0-9_-]` 밖의 모든 문자(특히 `.`과 `/`)를 `_`로 중화해 경로
//! traversal을 차단한다. 코드포인트 단위 순회라 멀티유닛 문자도 `_` 하나로
//! 접힌다 (JS `Array.from`과 동일).

pub fn key_file_name(name: &str) -> String {
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
    fn replaces_unsafe_chars_with_underscore_and_prefixes_id() {
        assert_eq!(key_file_name("my key!"), "id_my_key_");
        assert_eq!(key_file_name("ok-name_1"), "id_ok-name_1");
    }

    #[test]
    fn neutralizes_path_traversal_characters() {
        assert_eq!(key_file_name("../etc/passwd"), "id____etc_passwd");
        assert_eq!(key_file_name("a/../b"), "id_a____b");
    }

    #[test]
    fn keeps_only_ascii_alphanumerics_dash_underscore() {
        assert_eq!(key_file_name("aZ09-_"), "id_aZ09-_");
        // non-ASCII → 코드포인트당 밑줄 하나
        assert_eq!(key_file_name("é한"), "id___");
    }

    #[test]
    fn handles_empty_name() {
        assert_eq!(key_file_name(""), "id_");
    }

    #[test]
    fn builds_pem_server_id() {
        assert_eq!(server_pem_file_name(7), "pem_server_7");
    }
}
