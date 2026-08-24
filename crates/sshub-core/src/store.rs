//! 스토어 — **`~/.ssh/config`가 접속 정보의 원본**이고 `sshub.json`(v2)은
//! 앱 전용 메타데이터 사이드카다. 겉으로 드러나는 API는 Phase 1과 같다:
//! 앱 계층은 여전히 `Server` 하나만 보고, 그것이 두 파일의 조인이라는 사실을
//! 몰라도 된다.
//!
//! 조인 규칙
//! - config의 **쓰기 가능한** Host 블록(단일 패턴, 와일드카드 없음)만 서버가 된다.
//! - 별칭(Host)이 사이드카의 키다. 사이드카에 없는 별칭 = 손으로 추가한
//!   호스트 → 새 id를 받아 목록에 나타난다.
//! - 사이드카에만 있고 config에 없는 별칭은 **지우지 않는다** (앱 밖에서 이름을
//!   바꿨을 때 메모를 조용히 잃지 않도록). 다만 목록에는 뜨지 않는다.
//!
//! 쓰기 규칙
//! - config 쓰기는 전부 `ssh_config::write_document` 한 곳을 지난다:
//!   타임스탬프 백업 → 원자적 쓰기 → 권한 보존, 내용이 같으면 no-op.
//! - 메타데이터만 바뀌는 조작(즐겨찾기·마지막 접속 시각)은 config를 건드리지
//!   않는다.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::CoreError;
use crate::fsutil::{atomic_write_0600, path_with_suffix};
use crate::model::{
    CreateServerDto, ExportBundle, ExportFilter, ImportSummary, Server, SshKey, StoreData,
    UpdateServerDto,
};
use crate::ops::bundle_ops::{build_export_bundle, merge_bundle};
use crate::ops::key_ops::{self, KeyMetaUpdate, KeyStore, NewKey};
use crate::ops::server_ops::{self, ServerStore};
use crate::sidecar::{
    derive_auth, meta_from_server, migrate_v1, HostMeta, SidecarData, SIDECAR_VERSION,
};
use crate::ssh_config::{
    host_spec, is_writable_alias, js_parse_int, write_document, Document,
};
use crate::time::{now_iso, now_stamp};

/// 기본값 채움 + id 카운터를 레코드보다 앞서게 유지 + 비밀 스크럽.
/// v1(`sshub.json` version 없음) 파일을 읽을 때만 쓰인다.
pub fn normalize_data(raw: Option<StoreData>) -> StoreData {
    let raw = raw.unwrap_or_default();
    let servers = raw.servers;
    let keys: Vec<SshKey> = raw
        .keys
        .into_iter()
        .map(|mut k| {
            k.pem_data = None;
            k
        })
        .collect();
    let max_server_id = servers.iter().fold(0, |m, s| m.max(s.id));
    let max_key_id = keys.iter().fold(0, |m, k| m.max(k.id));
    StoreData {
        next_server_id: raw.next_server_id.max(max_server_id + 1),
        next_key_id: raw.next_key_id.max(max_key_id + 1),
        servers,
        keys,
    }
}

/// config 블록에서 조인에 필요한 값만 뽑아둔 스냅숏 (문서 빌림을 끊기 위해).
struct ConfigHost {
    alias: String,
    /// 이 별칭을 소유한 블록을 앱이 편집할 수 없다 (다중 패턴 블록의 패턴).
    read_only: bool,
    host_name: Option<String>,
    port: Option<String>,
    user: Option<String>,
    proxy_jump: Option<String>,
    identity_file: Option<String>,
}

/// 파일의 "지문" — 외부 편집 감지에만 쓴다. mtime+길이 조합은 사람이 손으로
/// 고치는 시나리오(에디터 저장)에서 실질적으로 항상 달라진다. 해시를 쓰지
/// 않는 이유는 창 활성화마다 파일 전체를 읽고 싶지 않아서다.
type FileStamp = Option<(SystemTime, u64)>;

fn file_stamp(path: &Path) -> FileStamp {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub struct Store {
    path: PathBuf,
    ssh_config_path: PathBuf,
    keys_dir: PathBuf,
    /// 서버별 PEM(`pem_server_<id>`)은 앱 데이터 디렉터리에 남는다 —
    /// 사용자가 붙여넣은 blob이라 `~/.ssh`에 섞을 이유가 없다.
    pem_dir: PathBuf,
    /// 마지막 로드에서 이관한 레거시 키 결과 (UI 안내용).
    key_migrations: Vec<crate::key_migration::KeyMigration>,
    sidecar: SidecarData,
    doc: Document,
    /// config를 읽지 못했다(권한 등). 이 상태에서 쓰기를 허용하면 빈 문서로
    /// 사용자의 config를 덮어써 버린다 — 그래서 모든 쓰기를 막는다.
    config_error: Option<String>,
    /// 마지막으로 읽어들인 두 파일의 지문 — `reload_if_changed`의 기준점.
    config_stamp: FileStamp,
    sidecar_stamp: FileStamp,
    /// 조인된 런타임 뷰. `servers`는 config ⨝ 사이드카, `keys`는 사이드카 그대로.
    data: StoreData,
}

impl Store {
    /// 경로 3종을 **반드시** 명시한다. 기본값으로 진짜 `~/.ssh/config`를
    /// 집어드는 생성자는 두지 않는다 — 테스트가 사용자의 실제 데이터를
    /// 건드린 적이 있어서, 그 사고를 타입 수준에서 불가능하게 만들었다.
    pub fn new(
        store_path: PathBuf,
        ssh_config_path: PathBuf,
        keys_dir: PathBuf,
        pem_dir: PathBuf,
    ) -> Store {
        Store {
            path: store_path,
            ssh_config_path,
            keys_dir,
            pem_dir,
            key_migrations: Vec::new(),
            sidecar: SidecarData::default(),
            doc: Document::parse(""),
            config_error: None,
            config_stamp: None,
            sidecar_stamp: None,
            data: normalize_data(None),
        }
    }

    pub fn ssh_config_path(&self) -> &Path {
        &self.ssh_config_path
    }

    pub fn keys_dir(&self) -> &Path {
        &self.keys_dir
    }

    pub fn pem_dir(&self) -> &Path {
        &self.pem_dir
    }

    /// 마지막 로드에서 옛 디렉터리로부터 이관한 키들.
    pub fn key_migrations(&self) -> &[crate::key_migration::KeyMigration] {
        &self.key_migrations
    }

    /// 실패하지 않는다: 손상/읽기 불가 파일은 `.corrupt.<ts>`로 보존하고 빈
    /// 상태로 부팅한다 (앱이 창을 못 여는 것보다 낫다).
    pub fn load(&mut self) {
        let (mut sidecar, v1, mut dirty) = self.read_sidecar();

        // config는 원본이므로 실패를 삼키지 않는다 — 못 읽으면 쓰기를 막는다.
        self.config_error = None;
        let text = match fs::read_to_string(&self.ssh_config_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                self.config_error = Some(e.to_string());
                String::new()
            }
        };
        self.doc = Document::parse(&text);

        let mut persist = true;
        if let Some(v1) = v1 {
            // 되돌릴 길을 먼저 만든다 (v2 사이드카는 Electron 앱이 못 읽는다).
            let dest = path_with_suffix(&self.path, &format!(".v1.{}", now_stamp()));
            let _ = fs::copy(&self.path, dest);
            sidecar = migrate_v1(&v1, &mut self.doc, &self.keys_dir, &self.pem_dir);
            dirty = true;
            // config 기록이 실패하면 사이드카도 쓰지 않는다 — v2 사이드카만
            // 남고 config에 블록이 없으면 서버가 통째로 사라져 보인다.
            if let Err(e) = write_document(&self.ssh_config_path, &self.doc) {
                self.config_error = Some(e.to_string());
                persist = false;
            }
        }

        // 옛 앱 키 디렉터리에만 있던 키를 `~/.ssh`로 들여온다. `~/.ssh`의
        // 파일은 절대 덮어쓰지 않고, 이름이 같은데 내용이 다르면 둘 다 둔다.
        self.key_migrations =
            crate::key_migration::migrate_legacy_keys(&self.pem_dir, &self.keys_dir);

        self.sidecar = sidecar;
        let assigned = self.rebuild();
        if persist && (dirty || assigned) {
            // load는 실패하지 않는 계약 — 재저장은 best-effort.
            let _ = self.save_sidecar();
        }
        self.remember_stamps();
    }

    /// 지금 디스크에 있는 두 파일의 지문을 기준점으로 삼는다. 앱이 스스로
    /// 쓴 직후에도 호출해야 자기 쓰기를 "외부 편집"으로 오인하지 않는다.
    fn remember_stamps(&mut self) {
        self.config_stamp = file_stamp(&self.ssh_config_path);
        self.sidecar_stamp = file_stamp(&self.path);
    }

    /// 앱 밖에서 `~/.ssh/config`(또는 사이드카)가 바뀌었으면 다시 읽는다.
    /// **어느 파일도 쓰지 않는다** — 사용자가 편집기로 저장한 직후에 앱이
    /// 되받아쓰기를 하면 그게 곧 손실이다.
    ///
    /// 돌려주는 값은 "조인 결과가 실제로 달라졌는가". 저장만 하고 내용은
    /// 그대로인 경우(mtime만 변경)에 UI를 흔들지 않기 위해서다.
    pub fn reload_if_changed(&mut self) -> bool {
        let config_stamp = file_stamp(&self.ssh_config_path);
        let sidecar_stamp = file_stamp(&self.path);
        if config_stamp == self.config_stamp && sidecar_stamp == self.sidecar_stamp {
            return false;
        }
        // 사이드카가 v1이거나 손상됐으면 마이그레이션/복구가 필요한데 그건
        // 파일을 쓰는 일이다 — 여기서는 하지 않고 현재 상태를 유지한 채
        // 다음 `load()`에 맡긴다.
        let sidecar = match self.peek_sidecar() {
            Some(s) => s,
            None => return false,
        };

        let mut config_error = None;
        let text = match fs::read_to_string(&self.ssh_config_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                config_error = Some(e.to_string());
                String::new()
            }
        };

        let before = self.data.clone();
        self.doc = Document::parse(&text);
        self.config_error = config_error;
        self.sidecar = sidecar;
        // 새 별칭에 배정한 id는 **즉시** 저장한다. id는 저장된 터미널 레이아웃의
        // `serverId`와 `pem_server_<id>` 파일명이 참조하므로, 메모리에만 두면
        // 그 사이 config 앞쪽에 호스트가 추가됐을 때 다음 실행에서 번호가 밀려
        // 복원된 탭이 엉뚱한 서버에 붙을 수 있다.
        // (사용자의 ~/.ssh/config는 여기서 절대 쓰지 않는다 — 사이드카만이다.)
        if self.rebuild() {
            let _ = self.save_sidecar();
        }
        self.config_stamp = config_stamp;
        self.sidecar_stamp = file_stamp(&self.path);
        self.data != before
    }

    /// 부수효과 없는 사이드카 읽기. v2가 아니거나 파싱 불가면 `None`
    /// (`read_sidecar`와 달리 `.corrupt.*` 백업을 만들지 않는다).
    fn peek_sidecar(&self) -> Option<SidecarData> {
        if !self.path.exists() {
            return Some(SidecarData::default());
        }
        let text = fs::read_to_string(&self.path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        if value.get("version").and_then(|v| v.as_i64()) != Some(SIDECAR_VERSION) {
            return None;
        }
        serde_json::from_value::<SidecarData>(value).ok().map(SidecarData::normalize)
    }

    /// `(사이드카, v1 원본, 재저장 필요 여부)`.
    fn read_sidecar(&self) -> (SidecarData, Option<StoreData>, bool) {
        if !self.path.exists() {
            return (SidecarData::default(), None, false);
        }
        let parsed = fs::read_to_string(&self.path)
            .map_err(CoreError::from)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).map_err(CoreError::from));
        let value = match parsed {
            Ok(v) => v,
            Err(_) => {
                self.backup_with(".corrupt.");
                return (SidecarData::default(), None, true);
            }
        };
        if value.get("version").and_then(|v| v.as_i64()) == Some(SIDECAR_VERSION) {
            return match serde_json::from_value::<SidecarData>(value) {
                Ok(s) => {
                    let had_secret = s.keys.iter().any(|k| k.pem_data.is_some());
                    (s.normalize(), None, had_secret)
                }
                Err(_) => {
                    self.backup_with(".corrupt.");
                    (SidecarData::default(), None, true)
                }
            };
        }
        // version이 없거나 2가 아니면 v1으로 읽는다 (StoreData는 전 필드
        // `default`라 부분 파일도 관대하게 파싱된다).
        match serde_json::from_value::<StoreData>(value) {
            Ok(raw) => (SidecarData::default(), Some(normalize_data(Some(raw))), true),
            Err(_) => {
                self.backup_with(".corrupt.");
                (SidecarData::default(), None, true)
            }
        }
    }

    fn backup_with(&self, marker: &str) {
        let dest = path_with_suffix(&self.path, &format!("{marker}{}", now_stamp()));
        let _ = fs::copy(&self.path, dest);
    }

    // ==================== 조인 ====================

    /// config + 사이드카 → `data`. 사이드카에 없던 별칭에 id를 새로 배정했으면
    /// `true` (호출자가 사이드카를 저장해야 한다).
    fn rebuild(&mut self) -> bool {
        let mut hosts: Vec<ConfigHost> = Vec::new();
        for b in self.doc.hosts() {
            // `Host a b c`는 패턴 하나하나가 접속 가능한 별칭이다 — 하나만
            // 고르면 나머지는 앱에서 영영 보이지 않는다. 반대로 와일드카드
            // 패턴(`Host *`, `*.dev`)은 호스트가 아니라 기본값 블록이므로
            // 목록에 넣지 않는다.
            let writable = b.writable_alias().map(str::to_string);
            let read_only = writable.is_none();
            let aliases: Vec<String> = match writable {
                Some(alias) => vec![alias],
                None => b
                    .patterns
                    .iter()
                    .filter(|p| is_writable_alias(p))
                    .cloned()
                    .collect(),
            };
            for alias in aliases {
                hosts.push(ConfigHost {
                    alias,
                    read_only,
                    host_name: b.get("hostname").map(str::to_string),
                    port: b.get("port").map(str::to_string),
                    user: b.get("user").map(str::to_string),
                    proxy_jump: b.get("proxyjump").map(str::to_string),
                    identity_file: b.get("identityfile").map(str::to_string),
                });
            }
        }

        let mut changed = false;
        let mut seen: HashSet<String> = HashSet::new();
        let mut servers = Vec::with_capacity(hosts.len());
        for h in hosts {
            // 같은 별칭이 두 번 나오면 ssh는 먼저 나온 블록을 쓴다.
            if !seen.insert(h.alias.clone()) {
                continue;
            }
            let meta = match self.sidecar.hosts.get(&h.alias) {
                Some(m) => m.clone(),
                None => {
                    // 손으로 추가한 호스트 — 여기서 처음 id를 받는다. 배정한
                    // id는 즉시 영속화되므로 다음 load에서도 같은 id다.
                    let m = HostMeta { id: self.sidecar.take_id(), ..Default::default() };
                    self.sidecar.hosts.insert(h.alias.clone(), m.clone());
                    changed = true;
                    m
                }
            };
            let (auth_type, key_id) = match meta.auth {
                Some(a) => (a, meta.key_id),
                None => {
                    let (a, k) = derive_auth(
                        h.identity_file.as_deref(),
                        &self.keys_dir,
                        &self.pem_dir,
                        &self.sidecar.keys,
                        meta.id,
                    );
                    (a, meta.key_id.or(k))
                }
            };
            servers.push(Server {
                id: meta.id,
                name: h.alias.clone(),
                host: h.host_name.unwrap_or_else(|| h.alias.clone()),
                port: h.port.as_deref().and_then(js_parse_int).unwrap_or(22),
                username: h.user.unwrap_or_else(|| "user".to_string()),
                auth_type,
                key_id,
                pem_data: None,
                proxy_jump: h.proxy_jump,
                group_name: meta.group.clone(),
                tags: meta.tags.clone(),
                is_favorite: meta.favorite,
                notes: meta.notes.clone(),
                last_connected_at: meta.last_connected_at.clone(),
                created_at: meta.created_at.clone(),
                updated_at: meta.updated_at.clone(),
                read_only: h.read_only,
            });
        }

        self.data = StoreData {
            next_server_id: self.sidecar.next_host_id,
            next_key_id: self.sidecar.next_key_id,
            servers,
            keys: self.sidecar.keys.clone(),
        };
        changed
    }

    // ==================== 영속화 ====================

    fn save_sidecar(&mut self) -> Result<(), CoreError> {
        let json = serde_json::to_string_pretty(&self.sidecar)?;
        atomic_write_0600(&self.path, json.as_bytes())?;
        self.sidecar_stamp = file_stamp(&self.path);
        Ok(())
    }

    /// config → 사이드카 순서로 쓴다. config 쓰기가 실패하면 사이드카도 쓰지
    /// 않아 두 파일이 어긋나지 않는다.
    fn persist(&mut self) -> Result<(), CoreError> {
        self.ensure_config_writable()?;
        write_document(&self.ssh_config_path, &self.doc)?;
        self.config_stamp = file_stamp(&self.ssh_config_path);
        self.rebuild();
        self.save_sidecar()
    }

    /// 메타데이터만 바뀐 경우 — config는 바이트 하나도 건드리지 않는다.
    fn persist_meta_only(&mut self) -> Result<(), CoreError> {
        self.rebuild();
        self.save_sidecar()
    }

    fn ensure_config_writable(&self) -> Result<(), CoreError> {
        match &self.config_error {
            Some(msg) => Err(CoreError::ConfigUnreadable(msg.clone())),
            None => Ok(()),
        }
    }

    /// 서버 하나를 config 블록 + 사이드카 항목으로 써넣는다.
    ///
    /// `clear_owned`는 "사용자가 이 서버를 직접 편집했다"는 뜻이다. 그때만
    /// 앱 소유 지시어를 지운다 — `upsert_host`는 절대 줄을 지우지 않으므로,
    /// ProxyJump를 비우거나 포트를 22로 되돌린 편집이 다음 load에서 되살아나는
    /// 것을 막으려면 이 예외가 필요하다. (`IdentityFile`은 예외의 예외로 절대
    /// 지우지 않는다 — 사용자가 손으로 넣은 키 줄을 날린 회귀가 있었다.)
    fn write_host(&mut self, alias: &str, server: &Server, clear_owned: bool) {
        let keys = self.sidecar.keys.clone();
        let spec = host_spec(server, &self.keys_dir, &self.pem_dir, &keys);
        self.doc.upsert_host(alias, &spec);
        if clear_owned {
            if spec.proxy_jump.is_none() {
                self.doc.remove_directive(alias, "proxyjump");
            }
            if server.port == 22 {
                self.doc.remove_directive(alias, "port");
            }
        }
        self.sidecar.hosts.insert(alias.to_string(), meta_from_server(server));
    }

    // ==================== Servers ====================

    pub fn list_servers(&self) -> Vec<Server> {
        server_ops::list_servers(&self.data.servers)
    }

    pub fn find_server(&self, id: i64) -> Option<Server> {
        server_ops::find_server(&self.data.servers, id).cloned()
    }

    /// 이 별칭을 앱이 소유할 수 있는가. 읽기 전용 블록(`Host a b`, `Host *`)이
    /// 이미 갖고 있으면 `ServerNotFound`, 이미 쓰이는 이름이면 `ServerAliasTaken`.
    fn check_free_alias(&self, alias: &str) -> Result<(), CoreError> {
        if !is_writable_alias(alias) {
            return Err(CoreError::ServerNotFound);
        }
        if let Some(_owner) = self.doc.host(alias) {
            if !self.doc.can_write(alias) {
                return Err(CoreError::ServerNotFound);
            }
            return Err(CoreError::ServerAliasTaken(alias.to_string()));
        }
        if self.sidecar.hosts.contains_key(alias) {
            return Err(CoreError::ServerAliasTaken(alias.to_string()));
        }
        Ok(())
    }

    pub fn insert_server(&mut self, dto: &CreateServerDto) -> Result<Server, CoreError> {
        self.ensure_config_writable()?;
        // v2에서 `Server::name`이 곧 `Host` 별칭이다 (그룹 접두사는 v1
        // 마이그레이션에서만 쓰인다 — 그룹은 순수 메타데이터가 되었다).
        let alias = dto.name.clone();
        self.check_free_alias(&alias)?;

        let now = now_iso();
        let id = self.sidecar.take_id();
        let server = Server {
            id,
            name: alias.clone(),
            host: dto.host.clone(),
            port: dto.port.unwrap_or(22),
            username: dto.username.clone(),
            auth_type: dto.auth_type,
            key_id: dto.key_id,
            pem_data: None, // 비밀은 절대 데이터에 넣지 않는다
            proxy_jump: dto.proxy_jump.clone(),
            group_name: dto.group_name.clone(),
            tags: dto.tags.clone(),
            is_favorite: false,
            notes: dto.notes.clone(),
            last_connected_at: None,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            read_only: false,
        };
        self.write_host(&alias, &server, false);
        self.persist()?;
        self.find_server(id).ok_or(CoreError::ServerNotFound)
    }

    pub fn update_server(&mut self, dto: &UpdateServerDto) -> Result<Server, CoreError> {
        self.ensure_config_writable()?;
        let prev = self.find_server(dto.id).ok_or(CoreError::ServerNotFound)?;
        // 읽기 전용 블록(`Host a b c` 등)은 앱의 소유가 아니다. 여기서 막지
        // 않으면 config는 그대로인 채 사이드카에만 값이 들어가 "저장했는데
        // 아무것도 안 바뀌는" 조용한 실패가 된다.
        if prev.read_only {
            return Err(CoreError::ServerNotFound);
        }
        let old_alias = prev.name.clone();
        // 병합 3규칙(`??` / `Option<Option<T>>` / authoritative proxy_jump)은
        // 그대로 재사용한다 — 규칙을 두 벌 두면 반드시 갈라진다.
        let slice = ServerStore { servers: vec![prev], next_server_id: self.sidecar.next_host_id };
        let (_, merged) = server_ops::update_server(&slice, dto, &now_iso())?;
        let new_alias = merged.name.clone();

        if new_alias != old_alias {
            self.check_free_alias(&new_alias)?;
            if !self.doc.rename_host(&old_alias, &new_alias) {
                return Err(CoreError::ServerNotFound);
            }
            // 사이드카 항목은 id를 지고 새 별칭 아래로 옮겨간다.
            if let Some(meta) = self.sidecar.hosts.remove(&old_alias) {
                self.sidecar.hosts.insert(new_alias.clone(), meta);
            }
        }
        self.write_host(&new_alias, &merged, true);
        self.persist()?;
        self.find_server(dto.id).ok_or(CoreError::ServerNotFound)
    }

    pub fn delete_server(&mut self, id: i64) -> Result<(), CoreError> {
        self.ensure_config_writable()?;
        // 없는 id는 조용히 성공 (기존 동작 유지).
        let Some(server) = self.find_server(id) else { return Ok(()) };
        // 읽기 전용 블록은 지우지 않는다 — 패턴 하나를 지우려면 나머지
        // 패턴의 의미까지 바꿔야 한다.
        if server.read_only {
            return Err(CoreError::ServerNotFound);
        }
        self.doc.remove_host(&server.name);
        self.sidecar.hosts.remove(&server.name);
        self.persist()
    }

    pub fn toggle_favorite(&mut self, id: i64) -> Result<Server, CoreError> {
        let server = self.find_server(id).ok_or(CoreError::ServerNotFound)?;
        let meta = self
            .sidecar
            .hosts
            .get_mut(&server.name)
            .ok_or(CoreError::ServerNotFound)?;
        meta.favorite = !meta.favorite;
        self.persist_meta_only()?;
        self.find_server(id).ok_or(CoreError::ServerNotFound)
    }

    pub fn touch_last_connected(&mut self, id: i64) -> Result<(), CoreError> {
        let Some(server) = self.find_server(id) else { return Ok(()) };
        if let Some(meta) = self.sidecar.hosts.get_mut(&server.name) {
            meta.last_connected_at = Some(now_iso());
            self.persist_meta_only()?;
        }
        Ok(())
    }

    /// 키 파일 이름이 바뀌었을 때 config의 `IdentityFile`을 따라 옮긴다.
    ///
    /// 별칭 접속으로 바뀐 뒤(`ssh <alias>`)로는 `-i`라는 안전망이 없다 —
    /// 블록에 남은 옛 경로가 곧 접속 실패다. 값만 갈아끼우므로 사용자가 쓴
    /// 다른 줄은 그대로다. 실제로 바뀐 줄이 있으면 `true`.
    pub fn rename_identity_file(&mut self, from: &Path, to: &Path) -> Result<bool, CoreError> {
        if from == to {
            return Ok(false);
        }
        let from = from.to_string_lossy().into_owned();
        let to = to.to_string_lossy().into_owned();
        self.ensure_config_writable()?;
        // 바꿀 줄이 없으면 파일을 열지 않는다 (쓸데없는 백업이 쌓이지 않게).
        if self.doc.rewrite_identity_file(&from, &to) == 0 {
            return Ok(false);
        }
        self.persist()?;
        Ok(true)
    }

    // ==================== SSH Keys ====================

    fn key_slice(&self) -> KeyStore {
        KeyStore { keys: self.sidecar.keys.clone(), next_key_id: self.sidecar.next_key_id }
    }

    fn commit_keys(&mut self, store: KeyStore) -> Result<(), CoreError> {
        self.sidecar.keys = store.keys;
        self.sidecar.next_key_id = store.next_key_id;
        self.persist_meta_only()
    }

    pub fn list_keys(&self) -> Vec<SshKey> {
        key_ops::list_keys(&self.data.keys)
    }

    pub fn find_key(&self, id: i64) -> Option<SshKey> {
        key_ops::find_key(&self.data.keys, id).cloned()
    }

    pub fn get_key(&self, id: i64) -> Result<SshKey, CoreError> {
        self.find_key(id).ok_or(CoreError::KeyNotFoundId(id))
    }

    /// 키 이름은 곧 `~/.ssh` 안의 파일명이다 — 경계에서 새니타이즈해 이
    /// 불변식을 강제한다. 안 그러면 config의 `IdentityFile`이 존재하지 않는
    /// 경로를 가리키고(공백이 든 이름 등), 백업에서 가져온 이름이 경로로
    /// 새어 들어올 수도 있다.
    pub fn insert_key(&mut self, nk: &NewKey) -> Result<SshKey, CoreError> {
        let nk = &NewKey { name: crate::key_files::key_file_name(&nk.name), ..nk.clone() };
        let (store, key) = key_ops::insert_key(&self.key_slice(), nk, &now_iso());
        self.commit_keys(store)?;
        Ok(key)
    }

    pub fn update_key_meta(&mut self, u: &KeyMetaUpdate) -> Result<SshKey, CoreError> {
        let u = &KeyMetaUpdate { name: crate::key_files::key_file_name(&u.name), ..u.clone() };
        let (store, key) = key_ops::update_key_meta(&self.key_slice(), u)?;
        self.commit_keys(store)?;
        Ok(key)
    }

    pub fn set_key_passphrase_protected(
        &mut self,
        id: i64,
        protected: bool,
    ) -> Result<(), CoreError> {
        let store = key_ops::set_passphrase_protected(&self.key_slice(), id, protected);
        self.commit_keys(store)
    }

    pub fn delete_key(&mut self, id: i64) -> Result<(), CoreError> {
        let store = key_ops::delete_key(&self.key_slice(), id);
        self.commit_keys(store)
    }

    // ==================== Export / Import ====================

    pub fn export_bundle(&self, filter: &ExportFilter) -> ExportBundle {
        build_export_bundle(&self.data, filter)
    }

    pub fn import_bundle(&mut self, bundle: &ExportBundle) -> Result<ImportSummary, CoreError> {
        self.ensure_config_writable()?;
        let (data, summary) = merge_bundle(&self.data, bundle);
        self.sidecar.keys = data.keys;
        self.sidecar.next_key_id = data.next_key_id;
        self.sidecar.next_host_id = self.sidecar.next_host_id.max(data.next_server_id);
        // merge_bundle은 이름이 겹치는 서버를 건너뛰므로 여기서 별칭 충돌은
        // 없다. 쓸 수 없는 별칭(와일드카드 등)만 조용히 버린다.
        for server in &data.servers {
            if is_writable_alias(&server.name) && self.doc.can_write(&server.name) {
                self.write_host(&server.name.clone(), server, false);
            }
        }
        self.persist()?;
        Ok(summary)
    }

    /// 테스트/점검용 — 정렬 없는 원본 키 목록 (비밀은 이미 스크럽됨).
    pub fn list_keys_raw(&self) -> &[SshKey] {
        &self.data.keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthType, KeyType};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    struct Ctx {
        dir: tempfile::TempDir,
    }

    impl Ctx {
        fn new() -> Ctx {
            Ctx { dir: tempfile::tempdir().unwrap() }
        }
        fn store_path(&self) -> PathBuf {
            self.dir.path().join("sshub.json")
        }
        fn config_path(&self) -> PathBuf {
            self.dir.path().join(".ssh").join("config")
        }
        fn keys_dir(&self) -> PathBuf {
            self.dir.path().join("ssh_keys")
        }
        fn open(&self) -> Store {
            let mut s = Store::new(self.store_path(), self.config_path(), self.keys_dir(), self.keys_dir());
            s.load();
            s
        }
        fn write_config(&self, text: &str) {
            fs::create_dir_all(self.config_path().parent().unwrap()).unwrap();
            fs::write(self.config_path(), text).unwrap();
        }
        fn config(&self) -> String {
            fs::read_to_string(self.config_path()).unwrap()
        }
    }

    fn dto(name: &str) -> CreateServerDto {
        CreateServerDto {
            name: name.into(),
            host: "h".into(),
            username: "u".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    // ---- normalizeData (v1 읽기 경로) ----

    #[test]
    fn normalize_fills_defaults_for_an_empty_absent_file() {
        let d = normalize_data(None);
        assert_eq!(d.next_server_id, 1);
        assert_eq!(d.next_key_id, 1);
        assert!(d.servers.is_empty());
        assert!(d.keys.is_empty());
    }

    #[test]
    fn normalize_keeps_id_counters_ahead_of_existing_records() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "nextServerId": 2,
            "servers": [{"id": 3}, {"id": 7}],
            "keys": [{"id": 5}],
        }))
        .unwrap();
        let d = normalize_data(Some(raw));
        assert_eq!(d.next_server_id, 8); // max(2, 7+1)
        assert_eq!(d.next_key_id, 6); // max(0, 5+1)
    }

    #[test]
    fn normalize_does_not_lower_a_counter_that_is_already_ahead() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "nextServerId": 100,
            "servers": [{"id": 3}],
        }))
        .unwrap();
        assert_eq!(normalize_data(Some(raw)).next_server_id, 100);
    }

    #[test]
    fn normalize_scrubs_any_private_key_material() {
        let raw: StoreData = serde_json::from_value(serde_json::json!({
            "keys": [{"id": 1, "pemData": "PRIVATE"}],
        }))
        .unwrap();
        assert_eq!(normalize_data(Some(raw)).keys[0].pem_data, None);
    }

    // ---- Store ----

    #[test]
    fn persists_an_inserted_server_to_the_config_and_reloads_it() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let server = s.insert_server(&dto("web")).unwrap();
        assert_eq!(server.id, 1);
        assert!(ctx.config().contains("Host web"), "{}", ctx.config());

        let reloaded = ctx.open();
        let names: Vec<String> = reloaded.list_servers().into_iter().map(|x| x.name).collect();
        assert_eq!(names, vec!["web"]);
        assert_eq!(reloaded.find_server(1).unwrap().host, "h");
    }

    #[test]
    fn writes_the_sidecar_with_0600_permissions() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        s.insert_server(&dto("web")).unwrap();
        assert_eq!(fs::metadata(ctx.store_path()).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(ctx.config_path()).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn scrubs_secrets_present_in_an_existing_file_on_load() {
        let ctx = Ctx::new();
        fs::write(ctx.store_path(), r#"{"keys":[{"id":1,"pemData":"LEAK"}]}"#).unwrap();
        let s = ctx.open();
        assert_eq!(s.list_keys_raw()[0].pem_data, None);
    }

    #[test]
    fn recovers_from_a_corrupt_file_boots_empty_preserves_the_original() {
        let ctx = Ctx::new();
        fs::write(ctx.store_path(), "{ this is not valid json ").unwrap();
        let s = ctx.open(); // 패닉/에러 없이 부팅해야 한다
        assert!(s.list_servers().is_empty());
        let backups: Vec<_> = fs::read_dir(ctx.dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(ctx.open().list_servers().is_empty());
    }

    #[test]
    fn toggle_favorite_flips_and_persists() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let srv = s.insert_server(&dto("web")).unwrap();
        s.toggle_favorite(srv.id).unwrap();
        assert_eq!(ctx.open().find_server(srv.id).map(|x| x.is_favorite), Some(true));
    }

    #[test]
    fn get_key_error_includes_the_id() {
        let ctx = Ctx::new();
        let s = Store::new(ctx.store_path(), ctx.config_path(), ctx.keys_dir(), ctx.keys_dir());
        assert_eq!(s.get_key(42).unwrap_err().to_string(), "SSH key not found: 42");
    }

    #[test]
    fn load_does_not_create_any_file_when_none_exists() {
        let ctx = Ctx::new();
        let _ = ctx.open();
        assert!(!ctx.store_path().exists());
        assert!(!ctx.config_path().exists());
    }

    #[test]
    fn saved_sidecar_is_v2_and_carries_only_app_metadata() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let mut d = dto("web");
        d.group_name = Some("prod".into());
        d.notes = Some("메모".into());
        s.insert_server(&d).unwrap();
        let text = fs::read_to_string(ctx.store_path()).unwrap();
        assert!(text.starts_with("{\n  \"version\": 3,\n  \"nextHostId\": 2,"), "{text}");
        // 접속 정보는 사이드카에 없다 — config가 원본이다.
        assert!(!text.contains("hostName"), "{text}");
        assert!(!text.contains("\"host\""), "{text}");
        assert!(text.contains("\"prod\""));
        assert!(text.contains("\"group\""));
    }

    #[test]
    fn insert_refuses_an_alias_owned_by_a_read_only_block() {
        let ctx = Ctx::new();
        ctx.write_config("Host a b\n  User multi\n\nHost *\n  User any\n");
        let mut s = ctx.open();
        assert!(matches!(s.insert_server(&dto("a")), Err(CoreError::ServerNotFound)));
        assert!(matches!(s.insert_server(&dto("*")), Err(CoreError::ServerNotFound)));
        // 목록에는 구체 패턴 둘만 (읽기 전용), 와일드카드는 호스트가 아니다.
        let names: Vec<String> = s.list_servers().into_iter().map(|x| x.name).collect();
        assert_eq!(names, ["a", "b"]);
    }

    // ==================== Phase 3 ====================

    #[test]
    fn lists_every_concrete_pattern_of_a_multi_pattern_block_as_read_only() {
        let ctx = Ctx::new();
        ctx.write_config(concat!(
            "Host a b c\n",
            "  HostName shared.example.com\n",
            "  User deploy\n",
            "  Port 2222\n",
            "\n",
            "Host solo\n",
            "  HostName solo.example.com\n",
            "\n",
            "Host *.dev\n",
            "  User any\n",
            "\n",
            "Host *\n",
            "  ForwardAgent yes\n",
        ));
        let s = ctx.open();
        let listed: Vec<(String, bool)> =
            s.list_servers().into_iter().map(|x| (x.name, x.read_only)).collect();
        assert_eq!(
            listed,
            [
                ("a".to_string(), true),
                ("b".to_string(), true),
                ("c".to_string(), true),
                ("solo".to_string(), false),
            ]
        );
        // 각 패턴은 같은 블록의 접속 정보를 공유한다.
        let a = s.list_servers().into_iter().find(|x| x.name == "a").unwrap();
        assert_eq!(a.host, "shared.example.com");
        assert_eq!(a.port, 2222);
        assert_eq!(a.username, "deploy");
    }

    #[test]
    fn a_pattern_without_hostname_connects_to_its_own_name() {
        let ctx = Ctx::new();
        ctx.write_config("Host a b\n  User multi\n");
        let s = ctx.open();
        let hosts: Vec<String> = s.list_servers().into_iter().map(|x| x.host).collect();
        assert_eq!(hosts, ["a", "b"]);
    }

    #[test]
    fn read_only_pattern_ids_are_sidecar_assigned_and_stable_across_loads() {
        let ctx = Ctx::new();
        ctx.write_config("Host a b\n  User multi\n");
        let first: Vec<i64> = ctx.open().list_servers().into_iter().map(|x| x.id).collect();
        assert_eq!(first.len(), 2);
        assert_ne!(first[0], first[1]);
        let second: Vec<i64> = ctx.open().list_servers().into_iter().map(|x| x.id).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn update_and_delete_refuse_a_read_only_host_without_touching_the_config() {
        let ctx = Ctx::new();
        let original = "Host a b\n  User multi\n";
        ctx.write_config(original);
        let mut s = ctx.open();
        let a = s.list_servers().into_iter().find(|x| x.name == "a").unwrap();

        let err = s
            .update_server(&UpdateServerDto { id: a.id, host: Some("evil".into()), ..Default::default() })
            .unwrap_err();
        assert!(matches!(err, CoreError::ServerNotFound), "{err}");
        assert!(matches!(s.delete_server(a.id), Err(CoreError::ServerNotFound)));
        assert_eq!(ctx.config(), original);
    }

    #[test]
    fn favorite_works_for_a_read_only_host_because_it_is_sidecar_only() {
        // 즐겨찾기는 config에 표현할 수 없는 순수 메타데이터다 — 별칭(=패턴)을
        // 키로 사이드카에만 기록되므로 읽기 전용 블록에도 안전하게 붙는다.
        let ctx = Ctx::new();
        let original = "Host a b\n  User multi\n";
        ctx.write_config(original);
        let mut s = ctx.open();
        let b = s.list_servers().into_iter().find(|x| x.name == "b").unwrap();
        assert!(s.toggle_favorite(b.id).unwrap().is_favorite);
        assert_eq!(ctx.config(), original, "config는 한 바이트도 바뀌지 않는다");
        assert!(ctx.open().find_server(b.id).unwrap().is_favorite);
    }

    #[test]
    fn reload_if_changed_is_false_when_nothing_moved() {
        let ctx = Ctx::new();
        ctx.write_config("Host web\n  HostName 1.1.1.1\n");
        let mut s = ctx.open();
        assert!(!s.reload_if_changed());
        assert!(!s.reload_if_changed());
    }

    #[test]
    fn reload_if_changed_picks_up_a_hand_edited_config_without_rewriting_it() {
        let ctx = Ctx::new();
        ctx.write_config("Host web\n  HostName 1.1.1.1\n");
        let mut s = ctx.open();
        assert_eq!(s.list_servers().len(), 1);


        // 사용자가 편집기로 저장한 상황. mtime 해상도가 낮아도 감지되도록
        // 길이도 함께 달라지는 편집을 쓴다.
        let edited = "Host web\n  HostName 1.1.1.1\n\nHost added-by-hand\n  HostName 2.2.2.2\n";
        ctx.write_config(edited);

        assert!(s.reload_if_changed());
        let names: Vec<String> = s.list_servers().into_iter().map(|x| x.name).collect();
        assert_eq!(names, ["added-by-hand", "web"]);
        // 사용자의 config는 절대 되받아쓰지 않는다.
        assert_eq!(ctx.config(), edited);
        // 사이드카는 앱 소유 파일이라 새 별칭의 id를 저장한다 (아래 테스트가 이유).
        let sidecar = fs::read_to_string(ctx.store_path()).unwrap();
        assert!(sidecar.contains("added-by-hand"), "새 별칭의 id가 저장돼야 한다");
        // 같은 상태를 두 번 보고하지 않는다.
        assert!(!s.reload_if_changed());
    }

    #[test]
    fn ids_assigned_during_reload_survive_a_relaunch() {
        // id는 저장된 터미널 레이아웃의 `serverId`와 `pem_server_<id>` 파일명이
        // 참조한다. reload에서 배정한 id를 저장하지 않으면, 그 사이 config
        // **앞쪽에** 호스트가 늘었을 때 다음 실행에서 번호가 밀려 복원된 탭이
        // 엉뚱한 서버에 붙는다.
        let ctx = Ctx::new();
        ctx.write_config("Host zulu\n  HostName 1.1.1.1\n");
        let mut s = ctx.open();
        let zulu_id = s.list_servers()[0].id;

        // 사용자가 zulu 앞에 호스트를 하나 추가하고 앱이 그것을 감지한다.
        ctx.write_config("Host alpha\n  HostName 2.2.2.2\n\nHost zulu\n  HostName 1.1.1.1\n");
        assert!(s.reload_if_changed());
        let alpha_id = s.list_servers().iter().find(|x| x.name == "alpha").unwrap().id;
        assert_ne!(alpha_id, zulu_id);

        // 앱을 껐다 켠다 — 두 id 모두 그대로여야 한다.
        let fresh = ctx.open();
        let after = fresh.list_servers();
        let get = |n: &str| after.iter().find(|x| x.name == n).unwrap().id;
        assert_eq!(get("zulu"), zulu_id, "기존 호스트의 id가 밀렸다");
        assert_eq!(get("alpha"), alpha_id, "reload에서 배정한 id가 유실됐다");
    }

    #[test]
    fn reload_if_changed_reports_false_when_only_the_mtime_moved() {
        let ctx = Ctx::new();
        let text = "Host web\n  HostName 1.1.1.1\n";
        ctx.write_config(text);
        let mut s = ctx.open();
        // 같은 내용으로 다시 저장 = 조인 결과에 변화 없음.
        std::thread::sleep(std::time::Duration::from_millis(10));
        ctx.write_config(text);
        assert!(!s.reload_if_changed());
    }

    #[test]
    fn reload_if_changed_sees_a_deleted_block() {
        let ctx = Ctx::new();
        ctx.write_config("Host web\n  HostName 1.1.1.1\n\nHost db\n  HostName 2.2.2.2\n");
        let mut s = ctx.open();
        assert_eq!(s.list_servers().len(), 2);
        ctx.write_config("Host web\n  HostName 1.1.1.1\n");
        assert!(s.reload_if_changed());
        assert_eq!(s.list_servers().len(), 1);
    }

    #[test]
    fn the_apps_own_writes_do_not_look_like_an_external_edit() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        s.insert_server(&dto("web")).unwrap();
        assert!(!s.reload_if_changed(), "직접 쓴 파일을 외부 편집으로 오인하면 안 된다");
        s.toggle_favorite(1).unwrap();
        assert!(!s.reload_if_changed());
    }

    #[test]
    fn insert_refuses_a_duplicate_alias() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        s.insert_server(&dto("web")).unwrap();
        let err = s.insert_server(&dto("web")).unwrap_err();
        assert!(matches!(err, CoreError::ServerAliasTaken(_)), "{err}");
    }

    #[test]
    fn update_renames_the_host_block_and_keeps_the_id() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let srv = s.insert_server(&dto("web")).unwrap();
        let updated = s
            .update_server(&UpdateServerDto {
                id: srv.id,
                name: Some("web2".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(updated.id, srv.id);
        assert_eq!(updated.name, "web2");
        let cfg = ctx.config();
        assert!(cfg.contains("Host web2"), "{cfg}");
        assert!(!cfg.contains("Host web\n"), "{cfg}");
        assert_eq!(ctx.open().find_server(srv.id).unwrap().name, "web2");
    }

    #[test]
    fn update_clears_proxy_jump_from_the_config_because_the_dto_is_authoritative() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let mut d = dto("jump");
        d.proxy_jump = Some("bastion".into());
        let srv = s.insert_server(&d).unwrap();
        assert!(ctx.config().contains("ProxyJump bastion"));

        s.update_server(&UpdateServerDto { id: srv.id, ..Default::default() }).unwrap();
        assert!(!ctx.config().contains("ProxyJump"), "{}", ctx.config());
        assert_eq!(ctx.open().find_server(srv.id).unwrap().proxy_jump, None);
    }

    #[test]
    fn update_back_to_port_22_removes_the_port_line() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let mut d = dto("p");
        d.port = Some(2222);
        let srv = s.insert_server(&d).unwrap();
        assert!(ctx.config().contains("Port 2222"));

        s.update_server(&UpdateServerDto { id: srv.id, port: Some(22), ..Default::default() })
            .unwrap();
        assert!(!ctx.config().contains("Port"), "{}", ctx.config());
        assert_eq!(ctx.open().find_server(srv.id).unwrap().port, 22);
    }

    #[test]
    fn delete_removes_the_block_and_the_sidecar_entry() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        let srv = s.insert_server(&dto("gone")).unwrap();
        s.delete_server(srv.id).unwrap();
        assert!(!ctx.config().contains("Host gone"));
        assert!(!fs::read_to_string(ctx.store_path()).unwrap().contains("gone"));
        assert!(ctx.open().list_servers().is_empty());
    }

    #[test]
    fn key_mutations_never_touch_the_config() {
        let ctx = Ctx::new();
        let mut s = ctx.open();
        s.insert_server(&dto("web")).unwrap();
        let before = ctx.config();
        s.insert_key(&NewKey {
            name: "k".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            key_type: KeyType::Ed25519,
            key_size: 256,
            passphrase_protected: false,
        })
        .unwrap();
        assert_eq!(ctx.config(), before);
    }

    #[test]
    fn writes_are_refused_when_the_config_could_not_be_read() {
        let ctx = Ctx::new();
        ctx.write_config("Host keep\n  HostName 1.1.1.1\n");
        fs::set_permissions(ctx.config_path(), fs::Permissions::from_mode(0o000)).unwrap();
        let mut s = Store::new(ctx.store_path(), ctx.config_path(), ctx.keys_dir(), ctx.keys_dir());
        s.load();
        let err = s.insert_server(&dto("new")).unwrap_err();
        assert!(matches!(err, CoreError::ConfigUnreadable(_)), "{err}");
        // 원본은 그대로 남아 있어야 한다.
        fs::set_permissions(ctx.config_path(), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(ctx.config(), "Host keep\n  HostName 1.1.1.1\n");
    }
}
