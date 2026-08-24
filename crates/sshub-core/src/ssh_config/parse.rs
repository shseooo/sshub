//! ssh config → 서버 DTO 파서. 파서는 `document.rs` 하나뿐이고 여기서는
//! Host 블록을 앱의 `CreateServerDto`로 투영하기만 한다.
//!
//! 보존한 기존 동작(quirk):
//! - `*`/`?` 와일드카드 Host는 건너뛴다 (`!` 부정 패턴도 같은 이유로 추가).
//! - `HostName`이 없으면 Host 별칭을 그대로 호스트로 쓴다.
//! - `User`가 없으면 `"user"`로 채운다.
//! - 잘못된 `Port`는 22 (JS `parseInt` 접두부 파싱 규칙까지 동일).
//! - `auth_type`은 항상 `Key` (config에는 인증 방식 정보가 없다).
//! - 같은 키가 여러 번 나오면 **마지막 값**이 이긴다 (ssh 본체는 첫 값이
//!   이기지만, 기존 import 동작을 바꾸지 않기 위해 유지).
//! - `Host a b c`처럼 패턴이 여러 개면 공백으로 이어붙인 한 이름이 된다
//!   (기존 동작). 그런 블록은 문서 모델에서 읽기 전용이라 되쓰기는 되지 않는다.

use crate::model::{AuthType, CreateServerDto};
use crate::ssh_config::document::{has_wildcard, Document, Entry};

/// JS `parseInt(value, 10)`: 선행 부호 + 십진 숫자 접두부만 읽는다
/// ("2200x" → 2200, "nope" → None).
pub(crate) fn js_parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let prefix: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if prefix.is_empty() {
        return None;
    }
    prefix.parse::<i64>().ok().map(|n| sign * n)
}

pub fn parse_ssh_config(content: &str) -> Vec<CreateServerDto> {
    let doc = Document::parse(content);
    let mut entries = Vec::new();

    for block in doc.hosts() {
        let name = block.patterns.join(" ");
        if name.is_empty() || has_wildcard(&name) {
            continue;
        }
        let (mut hostname, mut user, mut proxy_jump) = (None, None, None);
        let mut port = 22i64;
        for entry in &block.entries {
            let Entry::Directive { key, value, .. } = entry else { continue };
            match key.as_str() {
                "hostname" => hostname = Some(value.clone()),
                "user" => user = Some(value.clone()),
                "port" => port = js_parse_int(value).unwrap_or(22),
                "proxyjump" => proxy_jump = Some(value.clone()),
                _ => {}
            }
        }
        entries.push(CreateServerDto {
            name: name.clone(),
            host: hostname.unwrap_or(name),
            port: Some(port),
            username: user.unwrap_or_else(|| "user".into()),
            auth_type: AuthType::Key,
            proxy_jump,
            ..Default::default()
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_basic_host_block() {
        let e = parse_ssh_config("Host web\n  HostName 10.0.0.1\n  User deploy\n  Port 2222\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "web");
        assert_eq!(e[0].host, "10.0.0.1");
        assert_eq!(e[0].username, "deploy");
        assert_eq!(e[0].port, Some(2222));
        assert_eq!(e[0].auth_type, AuthType::Key);
    }

    #[test]
    fn maps_proxy_jump() {
        let e = parse_ssh_config("Host internal\n  HostName 10.0.0.9\n  ProxyJump jump@bastion\n");
        assert_eq!(e[0].proxy_jump.as_deref(), Some("jump@bastion"));
    }

    #[test]
    fn skips_wildcard_patterns() {
        let e = parse_ssh_config("Host *\n  User nobody\n\nHost real\n  HostName example.com\n");
        let names: Vec<&str> = e.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn applies_defaults_for_missing_fields() {
        let e = parse_ssh_config("Host bare\n");
        assert_eq!(e[0].name, "bare");
        assert_eq!(e[0].host, "bare");
        assert_eq!(e[0].port, Some(22));
        assert_eq!(e[0].username, "user");
        assert_eq!(e[0].auth_type, AuthType::Key);
    }

    #[test]
    fn supports_key_equals_value_syntax() {
        let e = parse_ssh_config("Host eq\n  HostName=1.2.3.4\n  Port=2200\n");
        assert_eq!(e[0].host, "1.2.3.4");
        assert_eq!(e[0].port, Some(2200));
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let e = parse_ssh_config("# a comment\n\nHost x\n  HostName h\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "x");
    }

    #[test]
    fn falls_back_to_port_22_on_an_invalid_port() {
        assert_eq!(parse_ssh_config("Host x\n  Port nope\n")[0].port, Some(22));
    }

    #[test]
    fn reads_spaced_equals_and_quoted_values() {
        // 옛 파서는 `Key = Value`에서 값이 "= Value"가 되는 버그가 있었다.
        let e = parse_ssh_config("Host s\n  HostName = 1.2.3.4\n  User = \"deploy\"\n");
        assert_eq!(e[0].host, "1.2.3.4");
        assert_eq!(e[0].username, "deploy");
    }

    #[test]
    fn does_not_leak_directives_of_a_match_block_into_the_previous_host() {
        let e = parse_ssh_config("Host a\n  HostName h\n\nMatch host b\n  User leaked\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].username, "user");
    }
}
