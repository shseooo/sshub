//! ssh 명령 인자 구성 (ssh.ts 직역). 순수: 이미 존재가 확인된 키/PEM 경로를
//! 받아 인자만 만든다. 인증 옵션은 프로젝트 규칙 그대로:
//!   password → keyboard-interactive,password + PubkeyAuthentication=no
//!   key/pem  → -i <path> + IdentitiesOnly=yes (경로가 있을 때만)
//!   agent    → PreferredAuthentications=publickey

use crate::model::{AuthType, Server};

#[derive(Debug, Clone, Default)]
pub struct SshPaths {
    /// `key` 인증용으로 해석된 개인 키 경로 (없으면 None).
    pub key_path: Option<String>,
    /// `pem` 인증용으로 해석된 PEM 경로 (없으면 None).
    pub pem_path: Option<String>,
}

pub fn build_ssh_args(server: &Server, paths: &SshPaths) -> Vec<String> {
    let mut args: Vec<String> = [
        "-o", "StrictHostKeyChecking=accept-new",
        "-o", "ConnectTimeout=15",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=3",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    if server.port != 22 {
        args.push("-p".into());
        args.push(server.port.to_string());
    }

    match server.auth_type {
        AuthType::Password => {
            // 곧장 비밀번호 프롬프트로 — 안 그러면 ssh가 agent/기본 키를 뿌리다
            // 비밀번호를 묻기도 전에 MaxAuthTries에 걸릴 수 있다.
            args.push("-o".into());
            args.push("PreferredAuthentications=keyboard-interactive,password".into());
            args.push("-o".into());
            args.push("PubkeyAuthentication=no".into());
        }
        AuthType::Pem => {
            if let Some(p) = &paths.pem_path {
                args.push("-i".into());
                args.push(p.clone());
                args.push("-o".into());
                args.push("IdentitiesOnly=yes".into());
            }
        }
        AuthType::Agent => {
            args.push("-o".into());
            args.push("PreferredAuthentications=publickey".into());
        }
        AuthType::Key => {
            // 선택한 키만 사용 (agent 키 난사 방지).
            if let Some(p) = &paths.key_path {
                args.push("-i".into());
                args.push(p.clone());
                args.push("-o".into());
                args.push("IdentitiesOnly=yes".into());
            }
        }
    }

    if let Some(pj) = server.proxy_jump.as_deref().map(str::trim) {
        if !pj.is_empty() {
            args.push("-J".into());
            args.push(pj.to_string());
        }
    }

    args.push(format!("{}@{}", server.username, server.host));
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
            name: "s".into(),
            host: "example.com".into(),
            port: 22,
            username: "root".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    const BASE: [&str; 8] = [
        "-o", "StrictHostKeyChecking=accept-new",
        "-o", "ConnectTimeout=15",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=3",
    ];

    fn contains(args: &[String], s: &str) -> bool {
        args.iter().any(|a| a == s)
    }

    fn index_of(args: &[String], s: &str) -> usize {
        args.iter().position(|a| a == s).unwrap()
    }

    #[test]
    fn starts_with_the_standard_o_options_and_ends_with_user_at_host() {
        let a = build_ssh_args(&srv(), &SshPaths::default());
        assert_eq!(a[..BASE.len()], BASE.map(String::from));
        assert_eq!(a.last().unwrap(), "root@example.com");
    }

    #[test]
    fn adds_p_only_for_non_default_ports() {
        assert!(!contains(&build_ssh_args(&srv(), &SshPaths::default()), "-p"));
        let mut s = srv();
        s.port = 2222;
        let a = build_ssh_args(&s, &SshPaths::default());
        assert!(contains(&a, "-p"));
        assert_eq!(a[index_of(&a, "-p") + 1], "2222");
    }

    #[test]
    fn password_auth_uses_keyboard_interactive_and_disables_pubkey_no_i() {
        let mut s = srv();
        s.auth_type = AuthType::Password;
        let a = build_ssh_args(&s, &SshPaths::default());
        assert!(contains(&a, "PreferredAuthentications=keyboard-interactive,password"));
        assert!(contains(&a, "PubkeyAuthentication=no"));
        assert!(!contains(&a, "-i"));
    }

    #[test]
    fn key_auth_with_a_resolved_key_path_adds_i_and_identities_only() {
        let mut s = srv();
        s.key_id = Some(3);
        let paths = SshPaths { key_path: Some("/keys/id_mykey".into()), pem_path: None };
        let a = build_ssh_args(&s, &paths);
        assert!(contains(&a, "-i"));
        assert_eq!(a[index_of(&a, "-i") + 1], "/keys/id_mykey");
        assert!(contains(&a, "IdentitiesOnly=yes"));
    }

    #[test]
    fn key_auth_with_no_resolved_path_adds_no_i() {
        let mut s = srv();
        s.key_id = Some(3);
        assert!(!contains(&build_ssh_args(&s, &SshPaths::default()), "-i"));
    }

    #[test]
    fn pem_auth_with_a_resolved_pem_path_adds_i_and_identities_only() {
        let mut s = srv();
        s.auth_type = AuthType::Pem;
        let paths = SshPaths { key_path: None, pem_path: Some("/keys/pem_server_1".into()) };
        let a = build_ssh_args(&s, &paths);
        assert_eq!(a[index_of(&a, "-i") + 1], "/keys/pem_server_1");
        assert!(contains(&a, "IdentitiesOnly=yes"));
    }

    #[test]
    fn agent_auth_prefers_publickey_no_i() {
        let mut s = srv();
        s.auth_type = AuthType::Agent;
        let a = build_ssh_args(&s, &SshPaths::default());
        assert!(contains(&a, "PreferredAuthentications=publickey"));
        assert!(!contains(&a, "-i"));
    }

    #[test]
    fn proxy_jump_adds_j_trimmed_and_ignores_blank() {
        let mut s = srv();
        s.proxy_jump = Some("  user@bastion  ".into());
        let a = build_ssh_args(&s, &SshPaths::default());
        assert!(contains(&a, "-J"));
        assert_eq!(a[index_of(&a, "-J") + 1], "user@bastion");

        let mut blank = srv();
        blank.proxy_jump = Some("   ".into());
        assert!(!contains(&build_ssh_args(&blank, &SshPaths::default()), "-J"));
    }

    #[test]
    fn orders_j_before_the_destination() {
        let mut s = srv();
        s.proxy_jump = Some("b".into());
        let a = build_ssh_args(&s, &SshPaths::default());
        assert!(index_of(&a, "-J") < index_of(&a, "root@example.com"));
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
