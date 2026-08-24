//! 키 타입 헬퍼 — OpenSSH 공개 키 접두사 → 저장 라벨 매핑과 생성 기본값.

use crate::error::CoreError;
use crate::model::KeyType;

/// OpenSSH 공개 키의 첫 토큰으로 키 타입 라벨을 판별한다.
pub fn detect_key_type(public_key: &str) -> Option<KeyType> {
    let prefix = public_key.trim().split_whitespace().next()?;
    if prefix.is_empty() {
        return None;
    }
    if prefix == "ssh-ed25519" || prefix == "sk-ssh-ed25519@openssh.com" {
        return Some(KeyType::Ed25519);
    }
    if prefix == "ssh-rsa" {
        return Some(KeyType::Rsa);
    }
    if prefix == "ssh-dss" {
        return Some(KeyType::Dsa);
    }
    if prefix.starts_with("ecdsa-") || prefix.starts_with("sk-ecdsa-") {
        return Some(KeyType::Ecdsa);
    }
    None
}

pub fn default_key_size(key_type: &str) -> i64 {
    if key_type.to_lowercase() == "rsa" { 3072 } else { 256 }
}

/// 소문자 정규화 + ssh-keygen 생성 허용 타입 검증 (dsa는 생성 불가).
pub fn normalize_creatable_key_type(key_type: &str) -> Result<KeyType, CoreError> {
    let t = key_type.to_lowercase();
    match t.as_str() {
        "ed25519" => Ok(KeyType::Ed25519),
        "rsa" => Ok(KeyType::Rsa),
        "ecdsa" => Ok(KeyType::Ecdsa),
        _ => Err(CoreError::UnsupportedKeyType(t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_prefixes() {
        assert_eq!(detect_key_type("ssh-ed25519 AAAA x"), Some(KeyType::Ed25519));
        assert_eq!(detect_key_type("ssh-rsa AAAA x"), Some(KeyType::Rsa));
        assert_eq!(detect_key_type("ssh-dss AAAA x"), Some(KeyType::Dsa));
        assert_eq!(detect_key_type("ecdsa-sha2-nistp256 AAAA x"), Some(KeyType::Ecdsa));
    }

    #[test]
    fn handles_fido2_security_key_prefixes() {
        assert_eq!(detect_key_type("sk-ssh-ed25519@openssh.com AAAA x"), Some(KeyType::Ed25519));
        assert_eq!(detect_key_type("sk-ecdsa-sha2-nistp256@openssh.com AAAA x"), Some(KeyType::Ecdsa));
    }

    #[test]
    fn returns_none_for_unknown_or_empty() {
        assert_eq!(detect_key_type("not-a-key"), None);
        assert_eq!(detect_key_type(""), None);
        assert_eq!(detect_key_type("   "), None);
    }

    #[test]
    fn default_size_is_3072_for_rsa_else_256() {
        assert_eq!(default_key_size("rsa"), 3072);
        assert_eq!(default_key_size("RSA"), 3072);
        assert_eq!(default_key_size("ed25519"), 256);
        assert_eq!(default_key_size("ecdsa"), 256);
    }

    #[test]
    fn normalize_lowercases_and_allows_ed25519_rsa_ecdsa() {
        assert_eq!(normalize_creatable_key_type("RSA").unwrap(), KeyType::Rsa);
        assert_eq!(normalize_creatable_key_type("ed25519").unwrap(), KeyType::Ed25519);
        assert_eq!(normalize_creatable_key_type("ECDSA").unwrap(), KeyType::Ecdsa);
    }

    #[test]
    fn normalize_rejects_unsupported_types() {
        let err = normalize_creatable_key_type("dsa").unwrap_err();
        assert_eq!(err.to_string(), "Unsupported key type: dsa");
    }
}
