//! `sshub.json` v2 — **앱 전용 메타데이터 사이드카**.
//!
//! Phase 2에서 접속 정보(HostName/Port/User/ProxyJump/IdentityFile)의 원본은
//! `~/.ssh/config`로 옮겨갔다. 여기 남는 것은 config로 표현할 수 없는 것뿐이다:
//! 안정적 숫자 id, 즐겨찾기, 그룹, 태그, 메모, 마지막 접속 시각, 그리고
//! config만으로는 되살릴 수 없는 인증 방식.
//!
//! `id`는 절대 바뀌면 안 된다 — 저장된 터미널 레이아웃이 `serverId`를 들고
//! 있고, 서버별 PEM 파일 이름이 `pem_server_<id>`다. 별칭(Host)이 바뀌어도
//! id는 따라 이동한다.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::{AuthType, Server, SshKey, StoreData};
use crate::ssh_config::{alias_for_v1, host_spec, is_writable_alias, Document, HostSpec};

pub const SIDECAR_VERSION: i64 = 2;

/// 별칭 하나에 붙는 앱 전용 메타데이터.
///
/// 직렬화에서 `None`은 아예 생략한다 (v1 모델과 다른 점 — v1은 Electron
/// 앱과 바이트 호환을 위해 `null`을 반드시 써야 했지만, v2는 Rust 앱 전용
/// 포맷이라 파일이 짧고 읽기 쉬운 쪽이 낫다).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HostMeta {
    pub id: i64,
    #[serde(skip_serializing_if = "is_false")]
    pub favorite: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// 기존 `Server::tags`와 같은 JSON 인코딩 문자열 그대로 (파싱하지 않는다).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<String>,
    /// `None`이면 config의 `IdentityFile`로 유추한다 (손으로 쓴 호스트가
    /// 그 경로를 탄다). 앱이 직접 쓴 호스트는 항상 값을 채운다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// `sshub.json` v2 전체. `hosts`가 `BTreeMap`인 이유는 저장 순서를 별칭
/// 사전순으로 고정하기 위해서다 — 같은 상태면 같은 바이트가 나와야 쓸데없는
/// 디스크 쓰기와 diff 노이즈가 없다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarData {
    pub version: i64,
    pub next_host_id: i64,
    pub next_key_id: i64,
    pub hosts: BTreeMap<String, HostMeta>,
    pub keys: Vec<SshKey>,
}

impl Default for SidecarData {
    fn default() -> Self {
        SidecarData {
            version: SIDECAR_VERSION,
            next_host_id: 1,
            next_key_id: 1,
            hosts: BTreeMap::new(),
            keys: Vec::new(),
        }
    }
}

impl SidecarData {
    /// 카운터가 레코드보다 앞서 있도록 보정 + 비밀 스크럽. (v1
    /// `normalize_data`와 같은 역할 — id 재사용은 저장된 레이아웃과 PEM 파일
    /// 이름을 엉뚱한 서버에 붙여 버린다.)
    pub fn normalize(mut self) -> SidecarData {
        for k in self.keys.iter_mut() {
            k.pem_data = None;
        }
        let max_host = self.hosts.values().fold(0, |m, h| m.max(h.id));
        let max_key = self.keys.iter().fold(0, |m, k| m.max(k.id));
        self.version = SIDECAR_VERSION;
        self.next_host_id = self.next_host_id.max(max_host + 1).max(1);
        self.next_key_id = self.next_key_id.max(max_key + 1).max(1);
        self
    }

    pub fn take_id(&mut self) -> i64 {
        let id = self.next_host_id;
        self.next_host_id += 1;
        id
    }
}

// -- 인증 방식 유추 ---------------------------------------------------------

/// `IdentityFile` → (auth_type, key_id). 사이드카에 `auth`가 없을 때만 쓰인다
/// (= 앱이 처음 보는, 손으로 쓴 호스트).
///
/// 규칙 네 가지:
/// 1. 앱 키 파일(`<keys_dir>/id_<name>`)과 일치 → `Key` + 그 키의 id
/// 2. `<keys_dir>/pem_server_<id>`와 일치 → `Pem`
/// 3. 그 외 `IdentityFile` → `Key` + `key_id: None` (사용자가 직접 관리하는 키)
/// 4. `IdentityFile` 없음 → `Agent`
pub fn derive_auth(
    identity_file: Option<&str>,
    keys_dir: &Path,
    keys: &[SshKey],
    host_id: i64,
) -> (AuthType, Option<i64>) {
    let Some(raw) = identity_file else { return (AuthType::Agent, None) };
    let path = Path::new(raw);
    if let Some(key) = keys
        .iter()
        .find(|k| keys_dir.join(crate::key_files::key_file_name(&k.name)) == path)
    {
        return (AuthType::Key, Some(key.id));
    }
    if keys_dir.join(crate::key_files::server_pem_file_name(host_id)) == path {
        return (AuthType::Pem, None);
    }
    (AuthType::Key, None)
}

// -- v1 → v2 마이그레이션 ---------------------------------------------------

/// 별칭에서 config에 쓸 수 없는 문자를 중화한다 (와일드카드·쉼표·따옴표).
/// 이런 이름은 v1에서는 그냥 문자열이었지만 v2에서는 파일에 박히는 별칭이라
/// 반드시 되읽을 수 있어야 한다.
fn sanitize_alias(alias: &str) -> String {
    let safe: String = alias
        .chars()
        .map(|c| if matches!(c, '*' | '?' | '!' | ',' | '"') { '_' } else { c })
        .collect();
    let trimmed = safe.trim().to_string();
    if trimmed.is_empty() {
        "host".to_string()
    } else {
        trimmed
    }
}

/// v1 서버가 실제로 쓸 수 있는 별칭을 고른다. 마이그레이션은 **서버를 하나도
/// 잃어선 안 되므로**, 원하는 별칭을 읽기 전용 블록(`Host a b`, `Host *`)이
/// 이미 차지하고 있거나 이미 배정된 이름이면 `-<id>` 접미사로 비켜간다.
fn resolve_alias(doc: &Document, taken: &HashSet<String>, desired: &str, id: i64) -> String {
    let base = sanitize_alias(desired);
    let usable = |c: &str| -> bool { !taken.contains(c) && is_writable_alias(c) && doc.can_write(c) };
    if usable(&base) {
        return base;
    }
    let with_id = format!("{base}-{id}");
    if usable(&with_id) {
        return with_id;
    }
    for n in 2.. {
        let candidate = format!("{base}-{id}-{n}");
        if usable(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

/// 이미 있는 블록에는 **없는 지시어만** 채운다 — config가 실제로 접속을
/// 책임지고 있으므로 HostName/Port/User는 config가 이긴다.
fn fill_missing_only(doc: &Document, alias: &str, mut spec: HostSpec) -> HostSpec {
    let Some(block) = doc.host(alias) else { return spec };
    if block.get("hostname").is_some() {
        spec.host_name = None;
    }
    if block.get("port").is_some() {
        spec.port = None;
    }
    if block.get("user").is_some() {
        spec.user = None;
    }
    if block.get("proxyjump").is_some() {
        spec.proxy_jump = None;
    }
    if block.get("identityfile").is_some() {
        spec.identity_file = None;
    }
    spec
}

/// v1 스토어를 config + 사이드카로 흩어 놓는다.
///
/// - 별칭은 옛 렌더러 규칙(`{group}-{name}`) 그대로라 이미 동기화해 둔
///   사용자에게 중복 블록이 생기지 않는다.
/// - 이미 있는 블록의 접속 필드는 건드리지 않고 빠진 줄만 채운다.
/// - `auth`/`keyId`는 항상 기록한다 — config가 표현할 수 없는 정보다.
pub fn migrate_v1(v1: &StoreData, doc: &mut Document, keys_dir: &Path) -> SidecarData {
    let mut sidecar = SidecarData {
        version: SIDECAR_VERSION,
        next_host_id: v1.next_server_id.max(1),
        next_key_id: v1.next_key_id.max(1),
        hosts: BTreeMap::new(),
        keys: v1.keys.clone(),
    };

    // id 순서로 처리해야 별칭 충돌 해소가 결정적이다 (같은 입력 → 같은 결과).
    let mut servers = v1.servers.clone();
    servers.sort_by_key(|s| s.id);

    let mut taken: HashSet<String> = HashSet::new();
    for server in &servers {
        let alias = resolve_alias(doc, &taken, &alias_for_v1(server), server.id);
        let spec = fill_missing_only(doc, &alias, host_spec(server, keys_dir, &v1.keys));
        doc.upsert_host(&alias, &spec);
        sidecar.hosts.insert(alias.clone(), meta_from_server(server));
        taken.insert(alias);
    }
    sidecar.normalize()
}

pub fn meta_from_server(server: &Server) -> HostMeta {
    HostMeta {
        id: server.id,
        favorite: server.is_favorite,
        group: server.group_name.clone(),
        tags: server.tags.clone(),
        notes: server.notes.clone(),
        last_connected_at: server.last_connected_at.clone(),
        auth: Some(server.auth_type),
        key_id: server.key_id,
        created_at: server.created_at.clone(),
        updated_at: server.updated_at.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyType;

    fn srv(id: i64, name: &str) -> Server {
        Server {
            id,
            name: name.into(),
            host: "10.0.0.1".into(),
            port: 22,
            username: "deploy".into(),
            auth_type: AuthType::Password,
            ..Default::default()
        }
    }

    fn v1(servers: Vec<Server>) -> StoreData {
        StoreData {
            next_server_id: servers.iter().fold(0, |m, s| m.max(s.id)) + 1,
            next_key_id: 1,
            servers,
            keys: vec![],
        }
    }

    #[test]
    fn keeps_the_group_prefixed_alias_of_the_old_renderer() {
        let mut s = srv(1, "web");
        s.group_name = Some("prod".into());
        let mut doc = Document::parse("");
        let side = migrate_v1(&v1(vec![s]), &mut doc, Path::new("/keys"));
        assert!(side.hosts.contains_key("prod-web"), "{:?}", side.hosts.keys());
        assert!(doc.to_string().contains("Host prod-web"));
    }

    #[test]
    fn preserves_v1_ids_and_advances_the_counter_past_them() {
        let side = migrate_v1(&v1(vec![srv(3, "a"), srv(11, "b")]), &mut Document::parse(""), Path::new("/k"));
        assert_eq!(side.hosts["a"].id, 3);
        assert_eq!(side.hosts["b"].id, 11);
        assert_eq!(side.next_host_id, 12);
    }

    #[test]
    fn config_wins_for_connection_fields_of_a_pre_existing_block() {
        let mut doc = Document::parse("Host web\n  HostName 192.168.0.9\n");
        let mut s = srv(1, "web");
        s.host = "10.0.0.1".into();
        s.port = 2222;
        migrate_v1(&v1(vec![s]), &mut doc, Path::new("/k"));
        let out = doc.to_string();
        // HostName은 config 값 그대로, 빠져 있던 User/Port만 채워진다.
        assert!(out.contains("HostName 192.168.0.9"), "{out}");
        assert!(!out.contains("10.0.0.1"), "{out}");
        assert!(out.contains("User deploy"), "{out}");
        assert!(out.contains("Port 2222"), "{out}");
    }

    #[test]
    fn sidesteps_an_alias_owned_by_a_read_only_block_instead_of_dropping_the_server() {
        // `Host a b`는 앱이 편집할 수 없다 — 그래도 v1 서버 "a"는 살아남아야 한다.
        let mut doc = Document::parse("Host a b\n  User multi\n");
        let side = migrate_v1(&v1(vec![srv(7, "a")]), &mut doc, Path::new("/k"));
        assert!(!side.hosts.contains_key("a"));
        assert_eq!(side.hosts["a-7"].id, 7);
        assert!(doc.to_string().starts_with("Host a b\n  User multi\n"));
    }

    #[test]
    fn neutralizes_wildcards_in_a_v1_server_name() {
        let side = migrate_v1(&v1(vec![srv(1, "*")]), &mut Document::parse(""), Path::new("/k"));
        assert_eq!(side.hosts.keys().collect::<Vec<_>>(), vec!["_"]);
    }

    #[test]
    fn records_auth_and_key_id_because_config_cannot_express_them() {
        let mut s = srv(1, "web");
        s.auth_type = AuthType::Password;
        let side = migrate_v1(&v1(vec![s]), &mut Document::parse(""), Path::new("/k"));
        assert_eq!(side.hosts["web"].auth, Some(AuthType::Password));
    }

    #[test]
    fn derives_agent_when_there_is_no_identity_file() {
        assert_eq!(derive_auth(None, Path::new("/k"), &[], 1), (AuthType::Agent, None));
    }

    #[test]
    fn derives_key_with_id_for_an_app_managed_key_file() {
        let key = SshKey {
            id: 4,
            name: "work key".into(),
            key_type: KeyType::Ed25519,
            ..Default::default()
        };
        let got = derive_auth(Some("/k/id_work_key"), Path::new("/k"), &[key], 1);
        assert_eq!(got, (AuthType::Key, Some(4)));
    }

    #[test]
    fn derives_pem_for_the_per_server_pem_path() {
        let got = derive_auth(Some("/k/pem_server_9"), Path::new("/k"), &[], 9);
        assert_eq!(got, (AuthType::Pem, None));
        // 다른 서버의 PEM 경로는 이 서버의 것이 아니다 → 일반 키로 본다.
        assert_eq!(derive_auth(Some("/k/pem_server_9"), Path::new("/k"), &[], 3).0, AuthType::Key);
    }

    #[test]
    fn derives_key_without_id_for_a_hand_managed_identity_file() {
        let got = derive_auth(Some("/home/me/.ssh/id_rsa"), Path::new("/k"), &[], 1);
        assert_eq!(got, (AuthType::Key, None));
    }

    #[test]
    fn serializes_without_null_noise() {
        let json = serde_json::to_string(&HostMeta { id: 3, favorite: true, ..Default::default() })
            .unwrap();
        assert_eq!(json, r#"{"id":3,"favorite":true}"#);
    }
}
