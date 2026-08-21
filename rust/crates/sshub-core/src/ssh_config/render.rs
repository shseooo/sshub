//! 서버 목록 → ssh config 텍스트 렌더러 (sshConfig.ts 직역).

use crate::model::Server;

// ssh_config는 라인 지향이라 필드 값에 개행(또는 다른 제어 문자)이 섞이면
// 조작된 서버 이름/호스트/유저가 임의 지시어를 주입할 수 있다 — 예: 다음
// `ssh`에서 실행되는 `ProxyCommand`. 모든 값에서 제어 문자를 제거한다.
// 정상적인 이름/호스트/유저에는 제어 문자가 없다. 신뢰 불가 값은 config
// import 왕복과 공유/편집된 서버 항목으로 유입된다.
fn sanitize_config_value(value: &str) -> String {
    // C0 제어 문자(<0x20, CR/LF/TAB 포함)와 DEL(0x7f)만 제거하고 나머지는
    // 유지 — 비ASCII 이름 같은 인쇄 가능한 유니코드는 보존된다.
    value
        .chars()
        .filter(|&c| {
            let code = c as u32;
            code >= 0x20 && code != 0x7f
        })
        .collect()
}

/// 모든 서버를 ssh config 텍스트로 렌더 (~/.ssh/config를 덮어쓴다).
pub fn render_ssh_config(servers: &[Server]) -> String {
    let mut out = String::new();
    for s in servers {
        let name = sanitize_config_value(&s.name);
        let group = s.group_name.as_deref().map(sanitize_config_value).unwrap_or_default();
        let display_name = if group.is_empty() { name } else { format!("{group}-{name}") };
        out.push_str(&format!("Host {display_name}\n"));
        out.push_str(&format!("    HostName {}\n", sanitize_config_value(&s.host)));
        out.push_str(&format!("    Port {}\n", s.port));
        out.push_str(&format!("    User {}\n", sanitize_config_value(&s.username)));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::parse_ssh_config;

    fn srv() -> Server {
        Server {
            id: 1,
            name: "web".into(),
            host: "10.0.0.1".into(),
            port: 2222,
            username: "deploy".into(),
            ..Default::default()
        }
    }

    #[test]
    fn writes_a_host_block_with_hostname_port_user() {
        let out = render_ssh_config(&[srv()]);
        assert!(out.contains("Host web"));
        assert!(out.contains("    HostName 10.0.0.1"));
        assert!(out.contains("    Port 2222"));
        assert!(out.contains("    User deploy"));
    }

    #[test]
    fn prefixes_the_display_name_with_the_group_when_set() {
        let mut grouped = srv();
        grouped.group_name = Some("prod".into());
        assert!(render_ssh_config(&[grouped]).contains("Host prod-web"));
        let mut empty_group = srv();
        empty_group.group_name = Some(String::new());
        assert!(render_ssh_config(&[empty_group]).contains("Host web"));
    }

    #[test]
    fn round_trips_through_the_parser() {
        let mut s = srv();
        s.name = "r".into();
        s.host = "h".into();
        s.port = 2200;
        s.username = "u".into();
        let parsed = parse_ssh_config(&render_ssh_config(&[s]));
        assert_eq!(parsed[0].name, "r");
        assert_eq!(parsed[0].host, "h");
        assert_eq!(parsed[0].port, Some(2200));
        assert_eq!(parsed[0].username, "u");
    }

    #[test]
    fn collapses_newlines_so_a_crafted_field_cannot_inject_a_directive() {
        let mut s = srv();
        s.host = "10.0.0.1\n    ProxyCommand touch /tmp/pwned".into();
        let out = render_ssh_config(&[s]);
        // 개행이 사라졌으므로 독립된 ProxyCommand 지시어 라인이 없다 —
        // 페이로드는 HostName 라인 위의 무해한 텍스트로 남는다.
        assert!(!out.lines().any(|l| l.trim_start().starts_with("ProxyCommand")));
        assert!(out.contains("    HostName 10.0.0.1    ProxyCommand touch /tmp/pwned"));
    }

    #[test]
    fn strips_control_chars_from_every_field_so_no_extra_directive_or_host_block() {
        let mut s = srv();
        s.name = "a\r\nHost evil".into();
        s.host = "h\nProxyCommand x".into();
        s.username = "u\nProxyCommand y".into();
        s.group_name = Some("g\nHost z".into());
        let out = render_ssh_config(&[s]);
        assert!(!out.lines().any(|l| l.trim_start().starts_with("ProxyCommand")));
        let host_blocks = out.lines().filter(|l| l.starts_with("Host ")).count();
        assert_eq!(host_blocks, 1);
    }
}
