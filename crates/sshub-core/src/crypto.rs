//! 백업 export의 passphrase 암호화 — scrypt 유도 키 + AES-256-GCM.
//! envelope는 JSON이라 import 시 감지가 쉽다. Node 구현과 상호 복호화
//! 가능해야 한다: scrypt N=2^14/r=8/p=1/dk=32 (Node scryptSync 기본값),
//! salt 16B, IV 12B, tag 16B, base64 STANDARD(패딩), 키 순서
//! magic,salt,iv,ct,tag, compact JSON.

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

const MAGIC: &str = "sshub-enc-v1";
const TAG_LEN: usize = 16;
const IV_LEN: usize = 12;

#[derive(Serialize, Deserialize)]
struct Envelope {
    magic: String,
    salt: String,
    iv: String,
    ct: String,
    tag: String,
}

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let params = scrypt::Params::new(14, 8, 1, 32).expect("scrypt 14/8/1/32 are valid params");
    let mut key = [0u8; 32];
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut key)
        .expect("output length 32 is valid");
    key
}

pub fn encrypt_bundle(plaintext: &str, passphrase: &str) -> Result<String, CoreError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut iv = [0u8; IV_LEN];
    OsRng.fill_bytes(&mut iv);
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
    // RustCrypto는 ct 뒤에 tag 16B를 붙여 반환 — Node envelope의 ct/tag 분리
    // 필드에 맞춰 잘라낸다.
    let mut ct = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_bytes())
        .expect("AES-GCM encryption cannot fail with a valid key/nonce");
    let tag = ct.split_off(ct.len() - TAG_LEN);
    let env = Envelope {
        magic: MAGIC.into(),
        salt: B64.encode(salt),
        iv: B64.encode(iv),
        ct: B64.encode(&ct),
        tag: B64.encode(&tag),
    };
    Ok(serde_json::to_string(&env)?)
}

pub fn decrypt_bundle(envelope: &str, passphrase: &str) -> Result<String, CoreError> {
    // JS는 Envelope 형태를 강제하지 않고 magic부터 검사한다 — 필드 누락/깨진
    // base64는 전부 "복호화 실패"로 수렴시킨다.
    let v: serde_json::Value = serde_json::from_str(envelope)?;
    if v.get("magic").and_then(|m| m.as_str()) != Some(MAGIC) {
        return Err(CoreError::NotEncryptedEnvelope);
    }
    let field = |k: &str| -> Result<Vec<u8>, CoreError> {
        let s = v.get(k).and_then(|x| x.as_str()).ok_or(CoreError::DecryptFailed)?;
        B64.decode(s).map_err(|_| CoreError::DecryptFailed)
    };
    let salt = field("salt")?;
    let iv = field("iv")?;
    let ct = field("ct")?;
    let tag = field("tag")?;
    if iv.len() != IV_LEN || tag.len() != TAG_LEN {
        return Err(CoreError::DecryptFailed);
    }
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
    let mut data = ct;
    data.extend_from_slice(&tag);
    let pt = cipher
        .decrypt(Nonce::from_slice(&iv), data.as_ref())
        .map_err(|_| CoreError::DecryptFailed)?;
    String::from_utf8(pt).map_err(|_| CoreError::DecryptFailed)
}

pub fn is_encrypted_envelope(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("magic").and_then(|m| m.as_str()).map(|m| m == MAGIC))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_plaintext_with_the_right_passphrase() {
        let env = encrypt_bundle("{\"hello\":\"세계\"}", "hunter2").unwrap();
        assert_eq!(decrypt_bundle(&env, "hunter2").unwrap(), "{\"hello\":\"세계\"}");
    }

    #[test]
    fn produces_a_recognizable_encrypted_envelope_not_the_plaintext() {
        let env = encrypt_bundle("plaintext-secret", "pw").unwrap();
        assert!(!env.contains("plaintext-secret"));
        assert!(is_encrypted_envelope(&env));
    }

    #[test]
    fn uses_a_fresh_salt_iv_each_time() {
        assert_ne!(encrypt_bundle("same", "pw").unwrap(), encrypt_bundle("same", "pw").unwrap());
    }

    #[test]
    fn throws_on_the_wrong_passphrase() {
        let env = encrypt_bundle("secret", "right").unwrap();
        let err = decrypt_bundle(&env, "wrong").unwrap_err();
        assert_eq!(err.to_string(), "복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.");
    }

    #[test]
    fn is_encrypted_envelope_is_false_for_plain_json_or_garbage() {
        assert!(!is_encrypted_envelope("{\"servers\":[],\"keys\":[]}"));
        assert!(!is_encrypted_envelope("not json"));
    }

    #[test]
    fn wrong_magic_is_rejected_with_the_exact_korean_message() {
        let err = decrypt_bundle("{\"magic\":\"nope\"}", "pw").unwrap_err();
        assert_eq!(err.to_string(), "암호화된 sshub 백업 파일이 아닙니다.");
    }

    #[test]
    fn envelope_key_order_and_compactness_match_node() {
        let env = encrypt_bundle("x", "pw").unwrap();
        assert!(env.starts_with("{\"magic\":\"sshub-enc-v1\",\"salt\":\""));
        let order: Vec<usize> = ["\"magic\"", "\"salt\"", "\"iv\"", "\"ct\"", "\"tag\""]
            .iter()
            .map(|k| env.find(*k).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        assert!(!env.contains('\n'));
    }
}
