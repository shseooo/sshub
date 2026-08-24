//! ssh 명령 인자 구성.
//!
//! Phase 3부터 접속 대상은 **별칭 하나**다: `ssh [정책 -o …] <alias>`.
//! HostName/Port/User/IdentityFile/ProxyJump는 전부 `~/.ssh/config`의 Host
//! 블록이 갖고 있고 그게 원본이므로, 같은 값을 커맨드라인에 한 번 더 실어
//! 보내면 두 곳이 어긋나는 순간 조용히 잘못된 곳으로 붙는다.
//!
//! 그래도 `-o`로 남기는 것들이 있다:
//! - 앱 정책(`StrictHostKeyChecking`·`ConnectTimeout`·`ServerAlive*`) — 사용자의
//!   파일에 일부러 쓰지 않는다. 앱이 띄운 세션에만 적용되어야 하는 값이다.
//! - 인증 방식 힌트 — config는 "비밀번호로 붙는다"를 표현할 방법이 없다.
//!   password → keyboard-interactive,password + PubkeyAuthentication=no
//!   agent    → PreferredAuthentications=publickey
//!   key/pem  → 추가 없음 (블록의 `IdentityFile`이 알아서 한다)

use crate::model::{AuthType, Server};

pub fn build_ssh_args(server: &Server) -> Vec<String> {
    let mut args: Vec<String> = [
        "-o", "StrictHostKeyChecking=accept-new",
        "-o", "ConnectTimeout=15",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    match server.auth_type {
        AuthType::Password => {
            // 곧장 비밀번호 프롬프트로 — 안 그러면 ssh가 agent/기본 키를 뿌리다
            // 비밀번호를 묻기도 전에 MaxAuthTries에 걸릴 수 있다.
            args.push("-o".into());
            args.push("PreferredAuthentications=keyboard-interactive,password".into());
            args.push("-o".into());
            args.push("PubkeyAuthentication=no".into());
        }
        AuthType::Agent => {
            args.push("-o".into());
            args.push("PreferredAuthentications=publickey".into());
        }
        // config 블록의 `IdentityFile`이 키를 지정한다 — `-i`로 덮어쓰면
        // 사용자가 손으로 고친 키 경로를 앱이 이긴다.
        AuthType::Key | AuthType::Pem => {}
    }

    // `Server::name`이 곧 Host 별칭이다 (Phase 2 이후 목록의 모든 서버가
    // config 블록에서 온다). 여러 패턴 블록의 개별 패턴도 그대로 별칭이다.
    args.push(server.name.clone());
    args
}

/// ssh가 출력을 내기 전에 먼저 찍는 연결 배너.
pub fn build_connect_banner(server: &Server) -> String {
    let pj = server.proxy_jump.as_deref().map(str::trim).unwrap_or("");
    let jump_note = if pj.is_empty() { String::new() } else { format!(" -J {pj}") };
    let port_suffix = if server.port != 22 { format!(":{}", server.port) } else { String::new() };
    format!(
        "\x1b[90m── sshub ──▶ ssh{jump_note} {}@{}{port_suffix} \x1b[0m(연결 중, 15초 내 응답 없으면 시간 초과)\r\n",
        server.username, server.host
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv() -> Server {
        Server {
            id: 1,
            name: "prod-web".into(),
            host: "example.com".into(),
            port: 22,
            username: "root".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    const POLICY: [&str; 8] = [
        "-o", "StrictHostKeyChecking=accept-new",
        "-o", "ConnectTimeout=15",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=3",
    ];

    /// 정책 인자 + 인증 인자 + 별칭. 벡터 전체를 통째로 고정한다 — 예전처럼
    /// `contains`로만 보면 `-i`/`-p`/`-J`가 슬그머니 되살아나도 통과한다.
    fn expect(auth_args: &[&str], alias: &str) -> Vec<String> {
        POLICY
            .iter()
            .chain(auth_args.iter())
            .chain(std::iter::once(&alias))
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn key_auth_is_policy_flags_plus_the_alias() {
        assert_eq!(build_ssh_args(&srv()), expect(&[], "prod-web"));
    }

    #[test]
    fn pem_auth_is_policy_flags_plus_the_alias() {
        let mut s = srv();
        s.auth_type = AuthType::Pem;
        assert_eq!(build_ssh_args(&s), expect(&[], "prod-web"));
    }

    #[test]
    fn password_auth_adds_keyboard_interactive_and_disables_pubkey() {
        let mut s = srv();
        s.auth_type = AuthType::Password;
        assert_eq!(
            build_ssh_args(&s),
            expect(
                &[
                    "-o",
                    "PreferredAuthentications=keyboard-interactive,password",
                    "-o",
                    "PubkeyAuthentication=no",
                ],
                "prod-web",
            )
        );
    }

    #[test]
    fn agent_auth_prefers_publickey() {
        let mut s = srv();
        s.auth_type = AuthType::Agent;
        assert_eq!(
            build_ssh_args(&s),
            expect(&["-o", "PreferredAuthentications=publickey"], "prod-web")
        );
    }

    #[test]
    fn connection_fields_never_reach_the_command_line() {
        // 전부 config 블록이 갖는 값이다 — 하나라도 여기로 새면 두 원본이 된다.
        let mut s = srv();
        s.port = 2222;
        s.proxy_jump = Some("user@bastion".into());
        s.username = "deploy".into();
        s.host = "10.0.0.1".into();
        let a = build_ssh_args(&s);
        for flag in ["-p", "-i", "-J", "IdentitiesOnly=yes"] {
            assert!(!a.iter().any(|x| x == flag), "{flag} 가 남아 있다: {a:?}");
        }
        assert!(!a.iter().any(|x| x.contains('@')), "user@host 가 남아 있다: {a:?}");
        assert_eq!(a.last().unwrap(), "prod-web");
    }

    #[test]
    fn read_only_hosts_connect_through_their_pattern() {
        // `Host a b c`의 패턴 하나하나가 그대로 접속 대상이 된다.
        let mut s = srv();
        s.name = "b".into();
        s.read_only = true;
        assert_eq!(build_ssh_args(&s).last().unwrap(), "b");
    }

    #[test]
    fn banner_includes_destination_and_jump_host() {
        let mut s = srv();
        s.proxy_jump = Some("jh".into());
        s.port = 2222;
        let b = build_connect_banner(&s);
        assert!(b.contains("root@example.com"));
        assert!(b.contains("-J jh"));
        assert!(b.contains(":2222"));
    }

    #[test]
    fn banner_omits_port_suffix_for_default_port() {
        assert!(!build_connect_banner(&srv()).contains(":22"));
    }

    #[test]
    fn banner_bytes_match_the_electron_string_exactly() {
        assert_eq!(
            build_connect_banner(&srv()),
            "\u{1b}[90m── sshub ──▶ ssh root@example.com \u{1b}[0m(연결 중, 15초 내 응답 없으면 시간 초과)\r\n"
        );
    }
}
