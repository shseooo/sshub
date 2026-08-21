# sshub-core 설계 (Rust 포팅 — 비 UI 전 로직)

원본: electron/store.ts, electron/lib/*.ts, electron/{keys,backup,sshConfigFile,scrollbackStore,terminalCwd}.ts.
목표: 기존 on-disk 데이터와 바이트/동작 호환.

## 0. 소스 검증 사실 (인벤토리 정정 포함)

- `appDataDir = ~/Library/Application Support` **자체** (sshub/ 하위 디렉터리 아님).
  파일: `sshub.json`, `ssh_keys/`, `sshub_scrollback/`, `sshub_terminal_cwd.json`, `sshub_window.json`.
- `sshub_window.json`·`sshub_terminal_cwd.json`은 **compact** JSON, `sshub.json`은 pretty indent-2 0600 원자적.
  암호화 envelope compact, 평문 export pretty-2.
- updateServer 병합 3규칙: `name/host/port/username/authType`는 `??`(None=유지),
  `keyId/groupName/tags/notes`는 `!== undefined`(Some(None)=클리어), `proxyJump`는
  authoritative(`dto.proxyJump ?? null` — 부재 시 클리어).
- `hasPrivateFile`은 런타임 뷰 필드 (영속화 금지).
- 타임스탬프: `Date.toISOString()` = 밀리초 정밀 `YYYY-MM-DDTHH:MM:SS.mmmZ`;
  파일명 스탬프는 `[:.]` → `-`.

## 1. 모듈 레이아웃

```
crates/sshub-core/src/
  lib.rs        error.rs      paths.rs      time.rs       fsutil.rs
  model.rs      store.rs      crypto.rs     backup.rs
  key_files.rs  key_type.rs   keys_io.rs    ssh_args.rs
  ops/{mod,server_ops,key_ops,bundle_ops}.rs
  ssh_config/{mod,parse,render,backups,file}.rs
  scrollback.rs terminal_cwd.rs window_state.rs settings.rs
tests/
  fixtures/{node_sshub.json,node_envelope.enc,node_plain_export.json,gen_fixtures.mjs}
  compat_store.rs compat_crypto.rs keygen_integration.rs
```

## 2. 의존성

serde 1(derive), serde_json 1 (`to_string_pretty`=indent2, 구조체 선언 순서=JS 삽입 순서 → 바이트 일치),
scrypt 0.11 (`Params::new(14, 8, 1, 32)` = Node 기본값), aes-gcm 0.10 (Aes256Gcm, nonce 12B, tag 16B —
암호문 뒤에 붙는 tag 16B를 잘라 envelope의 ct/tag 분리 필드에), base64 0.22 STANDARD(패딩),
thiserror 2, anyhow 1(앱 레이어만), dirs 6, chrono 0.4 (`to_rfc3339_opts(SecondsFormat::Millis, true)`),
tempfile 3 (dev). 비동기 런타임 없음 — 코어는 100% 동기.

`dialogPaths` allowlist는 폐기 (네이티브 앱은 renderer 신뢰 경계 없음).

## 3. 데이터 모델 (serde — 필드 순서 절대 유지)

```rust
#[serde(rename_all = "lowercase")] pub enum AuthType { Key, Password, Pem, Agent }
#[serde(rename_all = "lowercase")] pub enum KeyType { Ed25519, Rsa, Ecdsa, Dsa }

#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: i64, pub name: String, pub host: String,
    pub port: i64,                    // u16 아님: normalizeData가 검증 안 함
    pub username: String, pub auth_type: AuthType,
    pub key_id: Option<i64>,          // None ⇒ JSON null (skip 금지)
    pub pem_data: Option<String>,     // normalize 후 항상 None
    pub proxy_jump: Option<String>, pub group_name: Option<String>,
    pub tags: Option<String>,         // JSON 인코딩된 문자열 배열 — String 그대로 유지
    pub is_favorite: bool, pub notes: Option<String>,
    pub last_connected_at: Option<String>, pub created_at: Option<String>, pub updated_at: Option<String>,
}

#[serde(rename_all = "camelCase")]
pub struct SshKey { pub id: i64, pub name: String, pub public_key: String,
    pub pem_data: Option<String>, pub key_type: KeyType, pub key_size: i64,
    pub passphrase_protected: bool, pub created_at: Option<String> }
pub struct SshKeyView { #[serde(flatten)] pub key: SshKey, pub has_private_file: bool }

#[serde(rename_all = "camelCase", default)]
pub struct StoreData { pub next_server_id: i64, pub next_key_id: i64,
    pub servers: Vec<Server>, pub keys: Vec<SshKey> }

pub struct UpdateServerDto {
    pub id: i64,
    pub name: Option<String>, pub host: Option<String>, pub port: Option<i64>,
    pub username: Option<String>, pub auth_type: Option<AuthType>,   // ?? 유지
    pub key_id: Option<Option<i64>>, pub group_name: Option<Option<String>>,
    pub tags: Option<Option<String>>, pub notes: Option<Option<String>>, // !==undefined
    pub proxy_jump: Option<String>,   // authoritative: None=클리어
}
```

Bundle/envelope (필드 순서 = 바이트 호환):
```rust
#[serde(rename_all = "camelCase")]
pub struct ExportBundle { pub version: i64 /*=1*/, pub servers: Vec<Server>,
    pub keys: Vec<SshKey>, pub shortcuts: Option<BTreeMap<String,String>> } // None⇒null
#[serde(rename_all = "camelCase")]
pub struct ImportSummary { pub servers_added: u32, pub servers_skipped: u32,
    pub keys_added: u32, pub keys_skipped: u32, pub shortcuts: Option<BTreeMap<String,String>> }
pub struct SecureBundle { pub bundle: ExportBundle,
    #[serde(rename="privateKeys")] pub private_keys: Vec<PrivateKeyEntry> }
pub struct PrivateKeyEntry { pub name: String, pub pem: String }
struct Envelope { magic: String, salt: String, iv: String, ct: String, tag: String } // 이 순서
```
주의: shortcuts를 BTreeMap으로 두면 키 순서가 정렬됨 — 시맨틱 호환(merge는 키 기준)이라 수용.

## 4. API 표면·스레딩

코어는 동기. UI가 `Arc<Mutex<CoreCtx>>`로 감싸 GPUI BackgroundExecutor에서 호출.
백그라운드 필수: keys_io(ssh-keygen — RSA 수 초), read_pid_cwd(lsof), backup(scrypt ~100ms), ssh_config file IO.

주요 시그니처 (전체는 원 설계 참조 — 그대로 구현):

```rust
// paths.rs
pub struct AppPaths { app_data, store_file, keys_dir, scrollback_dir,
    terminal_cwd_file, window_file, settings_file: PathBuf }
impl AppPaths { pub fn discover() -> Result<AppPaths, CoreError>; } // dirs::data_dir()

// fsutil.rs — JS 시퀀스 그대로
pub fn atomic_write_0600(path: &Path, bytes: &[u8]) -> io::Result<()>;
// open <path>.tmp O_WRONLY|CREATE|TRUNC mode 0600 → write → sync_all → rename → set_permissions 0600

// store.rs
impl Store {
    pub fn new(path: PathBuf) -> Store;
    pub fn load(&mut self);   // 실패 없음: 손상 → .corrupt.<ts> 복사 + 빈 상태
    pub fn list_servers(&self) -> Vec<Server>;   // 즐겨찾기 우선, 이름 lowercase asc (stable)
    pub fn insert_server(&mut self, dto: CreateServerDto) -> Result<Server, CoreError>;
    pub fn update_server(&mut self, dto: UpdateServerDto) -> Result<Server, CoreError>;
    pub fn delete_server(&mut self, id: i64) -> Result<(), CoreError>;
    pub fn toggle_favorite(&mut self, id: i64) -> Result<Server, CoreError>;
    pub fn touch_last_connected(&mut self, id: i64) -> Result<(), CoreError>;
    pub fn list_keys / find_key / get_key / insert_key / update_key_meta
        / set_key_passphrase_protected / delete_key;
    pub fn export_bundle(&self, filter: &ExportFilter) -> ExportBundle;
    pub fn import_bundle(&mut self, bundle: ExportBundle) -> Result<ImportSummary, CoreError>;
}
pub fn normalize_data(raw: Option<StoreData>) -> StoreData;
// nextServerId = max(raw ?? 0, max(id)+1); 모든 key pemData=None; 스크럽/복구 시에만 재저장

// crypto.rs
pub fn encrypt_bundle(plaintext: &str, passphrase: &str) -> Result<String, CoreError>;
pub fn decrypt_bundle(envelope: &str, passphrase: &str) -> Result<String, CoreError>;
pub fn is_encrypted_envelope(text: &str) -> bool;

// backup.rs — 암호화 + passphrase 없음 → CoreError::NeedsPassphrase (Display == "ENCRYPTED")
pub fn export_data(store: &Store, keys_dir: &Path, path: &Path, opts: &ExportOptions) -> Result<(), CoreError>;
pub fn import_data(store: &mut Store, keys_dir: &Path, path: &Path, passphrase: Option<&str>) -> Result<ImportSummary, CoreError>;

// key_files.rs
pub fn key_file_name(name: &str) -> String;      // chars() 루프, [A-Za-z0-9_-] 외 '_', "id_" 접두
pub fn server_pem_file_name(id: i64) -> String;  // pem_server_{id}

// keys_io.rs — ssh-keygen 서브프로세스
pub fn get_ssh_keys(store, keys_dir) -> Vec<SshKeyView>;
pub fn create_ssh_key(ctx, dto) / import_ssh_key / update_ssh_key
    / change_key_passphrase / delete_ssh_key / load_key_file
    / derive_public_key_from_pem / write_server_pem / delete_server_pem;

// ssh_args.rs
pub fn build_ssh_args(server: &Server, paths: &SshPaths) -> Vec<String>;
pub fn build_connect_banner(server: &Server) -> String;

// ssh_config/
pub fn parse_ssh_config(content: &str) -> Vec<CreateServerDto>;
pub fn render_ssh_config(servers: &[Server]) -> String;
pub fn backups_to_prune(filenames: &[String], max: usize) -> Vec<String>;
pub fn sync_servers_to_config(store: &Store) -> Result<(), CoreError>;
pub fn sync_config_to_servers(store: &mut Store) -> Result<Vec<Server>, CoreError>;

// scrollback.rs
impl ScrollbackStore { new(dir) /*mkdir 0700+chmod*/, save(id,data) /*0600*/,
    load(id) -> Option<String>, delete(id), prune(live_ids) }
pub fn scrollback_file_name(session_id: &str) -> String;
pub const SCROLLBACK_LINES: usize = 1000;   // 영속 한도 (라이브 20000과 별개)

// terminal_cwd.rs
pub fn read_pid_cwd(pid: u32) -> Option<String>; // darwin: /usr/sbin/lsof -a -d cwd -Fn -p, 첫 'n' 라인
impl TerminalCwdStore { new/load/get(존재 확인)/set/delete/prune }  // compact 0600 best-effort

// window_state.rs
pub struct WindowBounds { x: Option<i32>, y: Option<i32>, width: u32, height: u32 }
pub fn sanitize_bounds(...) -> WindowBounds; // MIN 600x400, x/y 둘 다 숫자일 때만, round
pub fn load_window_bounds / save_window_bounds;  // compact, best-effort
```

## 5. settings.rs (localStorage 대체 — 클린 스타트, LevelDB 파싱 안 함)

`~/Library/Application Support/sshub_settings.json`, atomic 0600:
```jsonc
{ "version": 1,
  "language": "ko|en|ja",            // 부재 → 시스템 로케일 감지
  "startPage": "dashboard|servers|terminal|keys|settings",
  "sidebarCollapsed": false,
  "appearance": { "accent": "#74ade8", "translucency": 0,
                  "terminal": { "fontSize": 14, "foreground": null, "background": null } },
  "shortcuts": { "newTab": "cmd-t", ... },   // gpui Keystroke 직렬화 형식
  "terminalLayout": { "tabs": [{ "root": SavedNode, "name": "…" }], "activeIndex": 0 },
  "windows": [ { "bounds": {...}, "tabs": [...], "activeTab": 0 } ]   // 다중 창 (신규)
}
```
- 필드별 `#[serde(default)]` — 부분 파일은 기본값과 병합. 손상 → Default.
- `SavedNode` 형태는 TS와 동일 (`{type:"leaf",sessionId,serverId,label}` /
  `{type:"split",direction,sizes,children}`) — sessionId 보존으로 기존 스크롤백/cwd 연결 유지.
- 구 shortcuts 포맷(`meta+KeyT`) → gpui 형식(`cmd-t`) 변환 테이블은 앱 keymap.rs 소관
  (백업 import 시 필요).

## 6. 정밀 호환 체크리스트 (바이트 그대로)

- Magic `"sshub-enc-v1"`; scrypt log_n=14/r=8/p=1/dk=32; salt 16B; IV 12B; tag 16B;
  base64 STANDARD 패딩; envelope 키 순서 magic,salt,iv,ct,tag; compact.
- 센티널 `ENCRYPTED` (정확히 이 문자열).
- 한국어 에러 (바이트 그대로):
  - `암호화된 sshub 백업 파일이 아닙니다.`
  - `복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.`
  - `공개 키 또는 개인 키(PEM) 중 하나는 필요합니다.`
  - `같은 이름의 키 파일이 이미 있습니다.`
  - `이 기기에 개인 키 파일이 없습니다.`
  - `개인 키(PEM)가 비어 있습니다.`
  - `개인 키에서 공개 키를 추출하지 못했습니다. 암호로 보호된 키라면 passphrase를 입력하세요. ({msg})`
  - `패스프레이즈 변경 실패 — 현재 패스프레이즈가 맞는지 확인하세요. ({msg})`
  - `등록된 서버가 없어 ~/.ssh/config를 덮어쓰지 않았습니다.`
  - 배너: `\x1b[90m── sshub ──▶ ssh{jump} {user}@{host}{port} \x1b[0m(연결 중, 15초 내 응답 없으면 시간 초과)\r\n` (port 접미사는 ≠22일 때만, jump는 ` -J <pj>`)
- 영어: `Server not found`, `SSH key not found`, `SSH key not found: {id}`,
  `Unsupported key type: {t}`, `Key file already exists: {path}`.
- 파일 모드: sshub.json 0600(+rename 후 chmod), 키/PEM 0600(+chmod), scrollback dir 0700/파일 0600, cwd 파일 0600.
- 파일명: `sshub.json{,.tmp,.corrupt.<stamp>}`, `ssh_keys/id_<s>{,.pub}`, `ssh_keys/pem_server_<id>`,
  `ssh_keys/.derive.tmp`, `sshub_scrollback/<s>.txt`, `~/.ssh/config{,.tmp,.bak.<stamp>}`
  (문자열 정렬로 최신 10개 유지).
- ssh-keygen: 생성 `-t <type> -f <path> -C connectunnel-generated -N <pass>` (+rsa만 `-b`);
  유도 `-y -f <path> -P <pass|"">`; 변경 `-p -f <path> -P <cur|""> -N <new|"">`; 실패 시 stderr trim 노출.
- detect_key_type: `ssh-ed25519`/`sk-ssh-ed25519@openssh.com`→ed25519, `ssh-rsa`→rsa,
  `ssh-dss`→dsa, `ecdsa-*`/`sk-ecdsa-*`→ecdsa. default_key_size: rsa 3072 else 256;
  import는 keySize 256 고정; 생성 가능 {ed25519,rsa,ecdsa}.
- ssh args 순서: `-o StrictHostKeyChecking=accept-new`, `-o ConnectTimeout=15`,
  `-o ServerAliveInterval=15`, `-o ServerAliveCountMax=3` → `-p`(≠22만) → 인증
  (password: `PreferredAuthentications=keyboard-interactive,password`+`PubkeyAuthentication=no`;
  pem/key: 경로 있을 때만 `-i <p>`+`IdentitiesOnly=yes`; agent: `PreferredAuthentications=publickey`)
  → `-J <trimmed pj>`(비어있지 않을 때) → `user@host`.
- parse: `hostname|user|port|proxyjump`(case-insensitive)만, 첫 `=`/공백 분리, `*`/`?` 스킵,
  user 기본 `"user"`, 포트 NaN→22, import authType `key`.
- render: `Host {group-name|name}` + 4칸 들여쓰기 HostName/Port/User + 빈 줄; C0(<0x20)+DEL 제거.
- Bundle: version 1, export는 pemData 스크럽, merge는 이름 정확 일치 스킵·새 id·서버 keyId=null;
  0 서버 시 config 덮어쓰기 거부. 복원 개인 키 파일은 없을 때만 (0600).
- Window: min 600×400, x/y 쌍일 때만, round.
- 정렬: 서버 favorites-first + lowercase 이름, 키 lowercase 이름 (stable sort).

## 7. 테스트 계획

모듈 옆 `#[cfg(test)]`; I/O는 tempfile::TempDir. 기존 vitest 매핑:
serverOps/keyOps/bundleOps/crypto/keyFiles/keyType/scrollback/sshConfig/configBackups/ssh/
windowState/store/keys(ssh-keygen 통합, ssh-keygen 존재 시만)/backup/scrollbackStore/terminalCwd.

compat 픽스처 (tests/fixtures — gen_fixtures.mjs를 Node로 1회 실행해 생성·커밋):
1. node_sshub.json 역직렬화→pretty 재직렬화 **바이트 동일** 검증 (최우선 게이트).
2. node_envelope.enc를 "test-pass"로 복호화; 오답 passphrase → 정확한 한국어 에러.
3. 역방향(Rust 암호화→Node 복호화)은 릴리스 전 수동.
4. 평문 export 바이트 비교. 5. normalizeData 4케이스 재현.

구현 순서: model+fsutil+time → ops(순수) → store+픽스처 게이트 → crypto/backup →
key_files/key_type/keys_io → ssh_args/ssh_config → scrollback/cwd/window → settings.
최대 위험: 병합 3규칙 오구현(데이터 손상), serde 필드 순서/null 방출(바이트 호환 파괴).
