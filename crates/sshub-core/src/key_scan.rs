//! `~/.ssh` 디스크 스캔 — **키 목록의 원본은 사이드카가 아니라 파일 시스템**이다.
//!
//! 호스트에 대해 Phase 2~3이 내린 결론(`~/.ssh/config`가 원본, `sshub.json`은
//! 앱 전용 메타데이터)을 키에도 그대로 적용한다. 사이드카가 원본이던 시절에는
//! 사용자의 `~/.ssh`에 키가 셋 있어도 앱은 자기가 만든 하나만 보여줬다.
//!
//! 비용 규칙: 목록 한 번에 `ssh-keygen -y`를 키 개수만큼 돌리지 않는다
//! (키 하나당 수십~수백 ms + 암호 걸린 키는 실패). 공개 키는 `<name>.pub`을
//! 읽고, 암호화 여부는 파일 **머리 4 KiB**만 보고 판별한다.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine as _;

use crate::key_files::safe_file_component;

/// 판별에 필요한 만큼만 읽는다 (OpenSSH 헤더는 첫 줄 + base64 한 줄이면 끝난다).
const HEAD_BYTES: usize = 4096;

/// 디스크에서 찾은 키 하나. 앱 메타데이터(id·생성 시각)는 여기 없다 —
/// 그건 사이드카가 들고 조인 단계에서 붙는다.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredKey {
    /// `~/.ssh` 안의 파일 이름. 이것이 곧 키의 이름이다.
    pub file_name: String,
    /// `<name>.pub`의 내용. 없으면 빈 문자열 (유도하지 않는다).
    pub public_key: String,
    /// 개인 키 파일이 실제로 있는가. `.pub`만 남은 키(하드웨어 토큰·다른 기기)
    /// 도 목록에는 보여야 하므로 `false`인 항목이 존재한다.
    pub has_private_file: bool,
    /// 파일 내용으로 판별한 암호화 여부. 형식을 못 알아보면 `None`.
    pub encrypted: Option<bool>,
}

/// `~/.ssh`에서 키가 **아닌** 것들. ssh가 쓰는 파일과 앱이 그 옆에 만드는
/// 파일을 모두 걸러낸다 — 하나라도 새어 들어오면 UI에 유령 키가 뜨고,
/// 최악의 경우 사용자가 `config`를 "키 삭제"로 지울 수 있다.
pub fn is_reserved_ssh_file(name: &str) -> bool {
    name == "config"
        || name == "config.tmp"
        || name.starts_with("config.bak.")
        || name.starts_with("known_hosts")
        || name.starts_with("authorized_keys")
        || name == "authorized_principals"
        || name == "environment"
        || name == "rc"
        || name.starts_with("agent.")
        || name.starts_with("sshub.json")
        // 서버별 PEM은 앱 데이터 디렉터리 소관이다(키 목록에 뜨면 안 된다).
        || name.starts_with("pem_server_")
}

/// 스캔에서 아예 건너뛸 이름인가 (숨김 파일·`.pub`·예약어·비정상 이름).
fn is_ignored(name: &str) -> bool {
    name.starts_with('.')
        || name.ends_with(".pub")
        || is_reserved_ssh_file(name)
        || safe_file_component(name).is_none()
}

/// 파일 앞부분만 읽는다. 바이너리는 lossy로 접어도 헤더 판별에는 지장이 없다.
fn read_head(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD_BYTES];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn first_line(head: &str) -> &str {
    head.lines().next().unwrap_or("").trim()
}

/// 첫 줄이 `-----BEGIN ... PRIVATE KEY-----`인가.
pub fn looks_like_private_key(head: &str) -> bool {
    let line = first_line(head);
    line.starts_with("-----BEGIN") && line.ends_with("PRIVATE KEY-----")
}

/// OpenSSH 컨테이너의 첫 base64 줄에서 cipher 이름을 꺼낸다.
///
/// 레이아웃: `"openssh-key-v1\0"`(15바이트) + `u32 len` + ciphername.
/// 암호가 없으면 ciphername이 `none`이다. 한 줄(≈70자 → 51바이트)이면
/// 충분하므로 파일 전체를 디코드하지 않는다.
fn openssh_cipher(head: &str) -> Option<String> {
    // base64 알파벳만 남긴다 — 깨진 파일에서 슬라이스 경계가 튀지 않게.
    let body: Vec<u8> = head
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with("-----END"))
        .flat_map(|l| l.trim().bytes())
        .filter(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/' || *b == b'=')
        .take(64)
        .collect();
    let usable = body.len() - body.len() % 4;
    if usable == 0 {
        return None;
    }
    let chunk = &body[..usable];
    let bytes = STANDARD_NO_PAD
        .decode(chunk)
        .or_else(|_| STANDARD.decode(chunk))
        .ok()?;
    let rest = bytes.strip_prefix(b"openssh-key-v1\0")?;
    let len_bytes: [u8; 4] = rest.get(..4)?.try_into().ok()?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let name = rest.get(4..4 + len)?;
    String::from_utf8(name.to_vec()).ok()
}

/// 개인 키가 패스프레이즈로 보호돼 있는가. 형식을 못 알아보면 `None`.
///
/// - OpenSSH 컨테이너: 헤더의 cipher가 `none`이면 평문.
/// - 옛 PEM(PKCS#1/SEC1): `Proc-Type: 4,ENCRYPTED` 줄이 있으면 암호화.
/// - PKCS#8: `BEGIN ENCRYPTED PRIVATE KEY`면 암호화, `BEGIN PRIVATE KEY`면 평문.
pub fn detect_encrypted(head: &str) -> Option<bool> {
    let line = first_line(head);
    if line == "-----BEGIN OPENSSH PRIVATE KEY-----" {
        return openssh_cipher(head).map(|c| c != "none");
    }
    if line == "-----BEGIN ENCRYPTED PRIVATE KEY-----" {
        return Some(true);
    }
    if !looks_like_private_key(head) {
        return None;
    }
    // 옛 PEM은 `Proc-Type`/`DEK-Info` 헤더가 곧 암호화 표시다.
    if head.contains("Proc-Type: 4,ENCRYPTED") || head.contains("DEK-Info:") {
        return Some(true);
    }
    Some(false)
}

/// `dir` 안의 키를 전부 찾는다. 이름 사전순 — 같은 디렉터리면 같은 순서라야
/// 새 파일에 배정되는 id가 결정적이다.
///
/// 개인 키 판정: 형제 `<name>.pub`이 있거나, 첫 줄이 PRIVATE KEY 마커다.
/// 여기에 더해 `.pub`만 남은 키도 (개인 키 없음 표시로) 목록에 넣는다 —
/// 옛 앱이 공개 키만 가져오기 한 키가 조용히 사라지지 않도록.
pub fn discover_keys(dir: &Path) -> Vec<DiscoveredKey> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: BTreeSet<String> = BTreeSet::new();
    for e in entries.flatten() {
        // 디렉터리·심볼릭 링크가 가리키는 디렉터리는 키가 아니다.
        if !e.path().is_file() {
            continue;
        }
        files.insert(e.file_name().to_string_lossy().into_owned());
    }

    let read_pub = |stem: &str| -> String {
        std::fs::read_to_string(dir.join(format!("{stem}.pub")))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let mut out: Vec<DiscoveredKey> = Vec::new();
    for name in &files {
        if is_ignored(name) {
            continue;
        }
        let has_pub = files.contains(&format!("{name}.pub"));
        let head = read_head(&dir.join(name));
        let is_private = has_pub || head.as_deref().is_some_and(looks_like_private_key);
        if !is_private {
            continue;
        }
        out.push(DiscoveredKey {
            file_name: name.clone(),
            public_key: if has_pub { read_pub(name) } else { String::new() },
            has_private_file: true,
            encrypted: head.as_deref().and_then(detect_encrypted),
        });
    }

    // 짝 잃은 `<name>.pub` — 개인 키가 이 기기에 없는 키.
    for name in &files {
        let Some(stem) = name.strip_suffix(".pub") else { continue };
        if files.contains(stem) || stem.starts_with('.') || is_reserved_ssh_file(stem) {
            continue;
        }
        if safe_file_component(stem).is_none() {
            continue;
        }
        out.push(DiscoveredKey {
            file_name: stem.to_string(),
            public_key: read_pub(stem),
            has_private_file: false,
            encrypted: None,
        });
    }

    out.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const OPENSSH_PLAIN: &str = concat!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        // "openssh-key-v1\0" + u32(4) + "none" + u32(4) + "none" + ...
        "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gt\n",
        "-----END OPENSSH PRIVATE KEY-----\n",
    );

    const OPENSSH_ENCRYPTED: &str = concat!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        // ciphername = "aes256-ctr"
        "b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABBFcHJv\n",
        "-----END OPENSSH PRIVATE KEY-----\n",
    );

    const PEM_ENCRYPTED: &str = concat!(
        "-----BEGIN RSA PRIVATE KEY-----\n",
        "Proc-Type: 4,ENCRYPTED\n",
        "DEK-Info: AES-128-CBC,0123456789ABCDEF\n",
        "\n",
        "AAAA\n",
        "-----END RSA PRIVATE KEY-----\n",
    );

    const PEM_PLAIN: &str = concat!(
        "-----BEGIN RSA PRIVATE KEY-----\n",
        "MIIEpAIBAAKCAQEA0000\n",
        "-----END RSA PRIVATE KEY-----\n",
    );

    #[test]
    fn detects_openssh_encryption_from_the_container_header() {
        assert_eq!(detect_encrypted(OPENSSH_PLAIN), Some(false));
        assert_eq!(detect_encrypted(OPENSSH_ENCRYPTED), Some(true));
    }

    #[test]
    fn detects_pem_encryption_from_proc_type() {
        assert_eq!(detect_encrypted(PEM_ENCRYPTED), Some(true));
        assert_eq!(detect_encrypted(PEM_PLAIN), Some(false));
        assert_eq!(detect_encrypted("-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n"), Some(true));
        assert_eq!(detect_encrypted("-----BEGIN PRIVATE KEY-----\nAAAA\n"), Some(false));
    }

    #[test]
    fn says_it_does_not_know_for_anything_else() {
        assert_eq!(detect_encrypted("ssh-ed25519 AAAA me@host\n"), None);
        assert_eq!(detect_encrypted(""), None);
        // OpenSSH 마커인데 본문이 깨졌다 → 추측하지 않는다.
        assert_eq!(detect_encrypted("-----BEGIN OPENSSH PRIVATE KEY-----\n!!!!\n"), None);
    }

    #[test]
    fn lists_only_real_keys_and_ignores_everything_ssh_owns() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::write(d.join("id_rsa"), PEM_PLAIN).unwrap();
        fs::write(d.join("id_rsa.pub"), "ssh-rsa AAAA me@host\n").unwrap();
        fs::write(d.join("id_bare"), OPENSSH_ENCRYPTED).unwrap();
        fs::write(d.join("config"), "Host x\n").unwrap();
        fs::write(d.join("config.bak.20260101"), "Host x\n").unwrap();
        fs::write(d.join("known_hosts"), "host ssh-rsa AAAA\n").unwrap();
        fs::write(d.join("known_hosts.old"), "host ssh-rsa AAAA\n").unwrap();
        fs::write(d.join("authorized_keys"), "ssh-rsa AAAA\n").unwrap();
        fs::write(d.join(".DS_Store"), "junk").unwrap();
        fs::write(d.join("pem_server_3"), PEM_PLAIN).unwrap();
        fs::create_dir(d.join("conf.d")).unwrap();

        let found = discover_keys(d);
        let names: Vec<&str> = found.iter().map(|k| k.file_name.as_str()).collect();
        assert_eq!(names, vec!["id_bare", "id_rsa"]);

        let rsa = &found[1];
        assert_eq!(rsa.public_key, "ssh-rsa AAAA me@host");
        assert!(rsa.has_private_file);
        assert_eq!(rsa.encrypted, Some(false));

        let bare = &found[0];
        assert_eq!(bare.public_key, "", "공개 키는 유도하지 않는다");
        assert_eq!(bare.encrypted, Some(true));
    }

    #[test]
    fn lists_an_orphan_pub_as_a_key_without_a_private_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("remote_only.pub"), "ssh-ed25519 AAAA\n").unwrap();
        let found = discover_keys(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name, "remote_only");
        assert!(!found[0].has_private_file);
        assert_eq!(found[0].public_key, "ssh-ed25519 AAAA");
    }

    #[test]
    fn a_missing_directory_is_simply_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_keys(&dir.path().join("nope")).is_empty());
    }
}
