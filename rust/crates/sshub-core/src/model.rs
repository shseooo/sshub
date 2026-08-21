//! 데이터 모델. serde 필드 선언 순서 = JS 객체 삽입 순서 — `sshub.json`과
//! export 파일을 바이트 단위로 재현하기 위한 절대 조건이다 (DESIGN-core.md §3).
//! `Option::None`은 JSON `null`로 방출한다 (skip 금지 — JS는 키를 항상 쓴다).
//!
//! 역직렬화는 struct 단위 `#[serde(default)]`로 관대하게: 구버전/부분 파일의
//! 누락 필드는 기본값으로 채워진다 (JS의 Partial<StoreData> 읽기와 동등).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    #[default]
    Key,
    Password,
    Pem,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeyType {
    #[default]
    Ed25519,
    Rsa,
    Ecdsa,
    Dsa,
}

impl KeyType {
    /// ssh-keygen `-t` 인자 / JSON 표기와 동일한 소문자 라벨.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyType::Ed25519 => "ed25519",
            KeyType::Rsa => "rsa",
            KeyType::Ecdsa => "ecdsa",
            KeyType::Dsa => "dsa",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub host: String,
    /// u16이 아닌 i64: JS normalizeData가 범위를 검증하지 않으므로 그대로 보존.
    pub port: i64,
    pub username: String,
    pub auth_type: AuthType,
    pub key_id: Option<i64>,
    /// normalize 후 항상 None — 비밀은 ssh_keys/ 0600 파일에만 존재.
    pub pem_data: Option<String>,
    pub proxy_jump: Option<String>,
    pub group_name: Option<String>,
    /// JSON 인코딩된 문자열 배열 — 파싱하지 않고 String 그대로 유지.
    pub tags: Option<String>,
    pub is_favorite: bool,
    pub notes: Option<String>,
    pub last_connected_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SshKey {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    /// normalize 후 항상 None (Server::pem_data와 동일한 이유).
    pub pem_data: Option<String>,
    pub key_type: KeyType,
    pub key_size: i64,
    pub passphrase_protected: bool,
    pub created_at: Option<String>,
}

/// 런타임 뷰 — `hasPrivateFile`은 절대 영속화하지 않는다 (get_ssh_keys 전용).
#[derive(Debug, Clone, Serialize)]
pub struct SshKeyView {
    #[serde(flatten)]
    pub key: SshKey,
    #[serde(rename = "hasPrivateFile")]
    pub has_private_file: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StoreData {
    pub next_server_id: i64,
    pub next_key_id: i64,
    pub servers: Vec<Server>,
    pub keys: Vec<SshKey>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateServerDto {
    pub name: String,
    pub host: String,
    pub port: Option<i64>,
    pub username: String,
    pub auth_type: AuthType,
    pub key_id: Option<i64>,
    /// insert 시 무시된다 (저장 전 항상 null) — PEM 파일 기록은 호출자 몫.
    pub pem_data: Option<String>,
    pub proxy_jump: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}

/// update 병합 3규칙 (serverOps.ts와 정확히 동일):
/// - `name/host/port/username/auth_type`: JS `??` — `None`이면 기존 값 유지
/// - `key_id/group_name/tags/notes`: JS `!== undefined` — 바깥 `Some`이면 그
///   값(내부 `None` = 클리어)으로 교체, 바깥 `None`이면 유지
/// - `proxy_jump`: authoritative — `dto.proxyJump ?? null`, 부재 시 클리어
#[derive(Debug, Clone, Default)]
pub struct UpdateServerDto {
    pub id: i64,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub auth_type: Option<AuthType>,
    pub key_id: Option<Option<i64>>,
    pub group_name: Option<Option<String>>,
    pub tags: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub proxy_jump: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportBundle {
    pub version: i64,
    pub servers: Vec<Server>,
    pub keys: Vec<SshKey>,
    /// BTreeMap이라 키가 정렬됨 — merge는 키 기준이므로 시맨틱 호환 (§3 주의).
    pub shortcuts: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportSummary {
    pub servers_added: u32,
    pub servers_skipped: u32,
    pub keys_added: u32,
    pub keys_skipped: u32,
    pub shortcuts: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportFilter {
    /// JS truthiness와 동일: `Some(vec![])`는 "모두 필터링"이지 "필터 없음"이 아니다.
    pub server_ids: Option<Vec<i64>>,
    pub key_ids: Option<Vec<i64>>,
    pub shortcuts: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecureBundle {
    pub bundle: ExportBundle,
    #[serde(rename = "privateKeys", default)]
    pub private_keys: Vec<PrivateKeyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateKeyEntry {
    pub name: String,
    pub pem: String,
}

#[derive(Debug, Clone, Default)]
pub struct CreateKeyDto {
    pub name: String,
    /// 대소문자 무관 입력("RSA") — normalize_creatable_key_type이 검증한다.
    pub key_type: String,
    pub key_size: Option<i64>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportKeyDto {
    pub name: String,
    pub public_key: String,
    pub pem_data: Option<String>,
    /// 공개 키에서 타입을 감지하지 못할 때의 폴백 라벨 (접속에는 영향 없음).
    pub key_type: KeyType,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateKeyDto {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    pub key_type: KeyType,
    /// Some(비어있지 않음)일 때만 저장된 개인 키를 교체한다.
    pub pem_data: Option<String>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedKeyFile {
    pub file_name: String,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_serializes_in_js_insertion_order_with_nulls() {
        let s = Server {
            id: 1,
            name: "n".into(),
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"id":1,"name":"n","host":"h","port":22,"username":"u","authType":"key","keyId":null,"pemData":null,"proxyJump":null,"groupName":null,"tags":null,"isFavorite":false,"notes":null,"lastConnectedAt":null,"createdAt":null,"updatedAt":null}"#
        );
    }

    #[test]
    fn ssh_key_serializes_in_js_insertion_order() {
        let k = SshKey {
            id: 2,
            name: "k".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            key_size: 256,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            r#"{"id":2,"name":"k","publicKey":"ssh-ed25519 AAAA","pemData":null,"keyType":"ed25519","keySize":256,"passphraseProtected":false,"createdAt":null}"#
        );
    }

    #[test]
    fn ssh_key_view_appends_has_private_file_and_is_never_deserialized() {
        let v = SshKeyView {
            key: SshKey { id: 1, ..Default::default() },
            has_private_file: true,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.ends_with(r#""hasPrivateFile":true}"#));
    }

    #[test]
    fn lenient_deserialization_fills_missing_fields() {
        let d: StoreData = serde_json::from_str(r#"{"keys":[{"id":1,"pemData":"LEAK"}]}"#).unwrap();
        assert_eq!(d.next_server_id, 0);
        assert_eq!(d.keys[0].pem_data.as_deref(), Some("LEAK"));
        assert_eq!(d.keys[0].name, "");
    }
}
