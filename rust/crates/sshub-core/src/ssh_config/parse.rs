//! ssh config → 서버 DTO 파서 (sshConfig.ts 직역).
//! `hostname|user|port|proxyjump`(대소문자 무관)만 읽고, 첫 `=`/공백에서
//! 분리하며, `*`/`?` 와일드카드 Host는 스킵한다.

use crate::model::{AuthType, CreateServerDto};

struct Block {
    host: String,
    hostname: Option<String>,
    user: Option<String>,
    port: i64,
    proxy_jump: Option<String>,
}

fn flush(block: Option<Block>, entries: &mut Vec<CreateServerDto>) {
    let Some(b) = block else { return };
    if b.host.contains('*') || b.host.contains('?') {
        return; // 와일드카드 패턴
    }
    entries.push(CreateServerDto {
        name: b.host.clone(),
        host: b.hostname.unwrap_or(b.host),
        port: Some(b.port),
        username: b.user.unwrap_or_else(|| "user".into()),
        auth_type: AuthType::Key,
        proxy_jump: b.proxy_jump,
        ..Default::default()
    });
}

/// JS `parseInt(value, 10)`: 선행 부호 + 십진 숫자 접두부만 읽는다
/// ("2200x" → 2200, "nope" → None).
fn js_parse_int(s: &str) -> Option<i64> {
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
    let mut entries = Vec::new();
    let mut cur: Option<Block> = None;

    for line in content.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Host ") {
            flush(cur.take(), &mut entries);
            cur = Some(Block {
                host: rest.trim().to_string(),
                hostname: None,
                user: None,
                port: 22,
                proxy_jump: None,
            });
            continue;
        }

        // 첫 '=' 또는 공백에서 분리.
        let Some(i) = trimmed.find(|c: char| c == '=' || c.is_whitespace()) else {
            continue;
        };
        let key = trimmed[..i].trim().to_lowercase();
        let value = trimmed[i + 1..].trim();
        let Some(b) = cur.as_mut() else { continue };
        match key.as_str() {
            "hostname" => b.hostname = Some(value.to_string()),
            "user" => b.user = Some(value.to_string()),
            "port" => b.port = js_parse_int(value).unwrap_or(22),
            "proxyjump" => b.proxy_jump = Some(value.to_string()),
            _ => {}
        }
    }
    flush(cur, &mut entries);
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
}
