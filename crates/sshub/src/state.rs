//! 앱 전역 상태 (DESIGN-ui.md §7) — TanStack Query 대체.
//!
//! 무효화 전략은 "변경 후 재조회" 하나뿐이다: 뮤테이션이 끝나면 해당 컬렉션을
//! 코어에서 다시 읽는다. 데이터 규모(서버/키 수십 개)에서 캐시 키를 둘 이유가 없다.
//!
//! 코어는 동기 API이고 ssh-keygen·scrypt·lsof는 수 초가 걸릴 수 있어, 그런
//! 호출은 반드시 background executor로 보낸다. 스토어 접근은 `Arc<Mutex<..>>`
//! 하나로 직렬화해 동시 쓰기가 겹치지 않게 한다.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task};
use sshub_core::model::{CreateServerDto, Server, SshKeyView, UpdateServerDto};
use sshub_core::settings::Settings;
use sshub_core::store::Store;
use sshub_core::{keys_io, AppPaths, CoreError};

/// 코어 컨텍스트 — 워커 스레드로 넘길 수 있는 핸들.
pub struct CoreCtx {
    pub store: Store,
    pub keys_dir: PathBuf,
}

pub type SharedCore = Arc<Mutex<CoreCtx>>;

pub struct AppState {
    core: SharedCore,
    pub paths: AppPaths,
    pub settings: Settings,
    pub servers: Vec<Server>,
    pub keys: Vec<SshKeyView>,
    pub busy: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEvent {
    ServersChanged,
    KeysChanged,
    SettingsChanged,
}

impl EventEmitter<StateEvent> for AppState {}

struct AppStateHandle(Entity<AppState>);
impl Global for AppStateHandle {}

/// 전역 상태 초기화 — 앱 부트스트랩에서 1회.
pub fn init(cx: &mut App) -> Entity<AppState> {
    let paths = AppPaths::discover().expect("데이터 디렉터리를 찾을 수 없습니다");
    init_with_paths(paths, cx)
}

/// 경로를 지정해 초기화한다. 테스트는 반드시 이쪽을 써서 임시 디렉터리를
/// 넘긴다 — 기본 경로를 쓰면 사용자의 실제 서버·키·레이아웃 파일을 건드린다.
pub fn init_with_paths(paths: AppPaths, cx: &mut App) -> Entity<AppState> {
    let state = cx.new(|_| AppState::new(paths));
    cx.set_global(AppStateHandle(state.clone()));
    state
}

/// 초기화된 전역 상태 (init 이후에만 유효).
pub fn app_state(cx: &App) -> Entity<AppState> {
    cx.global::<AppStateHandle>().0.clone()
}

impl AppState {
    fn new(paths: AppPaths) -> Self {
        // 접속 정보의 원본은 ~/.ssh/config다 — 경로를 명시적으로 넘긴다
        // (Store에는 기본 경로가 없어서 테스트가 실수로 진짜 파일을 못 연다).
        let mut store = Store::new(
            paths.store_file.clone(),
            paths.ssh_config_file.clone(),
            paths.keys_dir.clone(),
            paths.app_keys_dir.clone(),
        );
        store.load();
        let settings = Settings::load(&paths.settings_file);

        let servers = store.list_servers();
        let keys = keys_io::get_ssh_keys(&store, &paths.keys_dir);
        let core = Arc::new(Mutex::new(CoreCtx { store, keys_dir: paths.keys_dir.clone() }));

        Self { core, paths, settings, servers, keys, busy: false, last_error: None }
    }

    pub fn core(&self) -> SharedCore {
        self.core.clone()
    }

    // ---- 재조회 (뮤테이션 후 항상 호출) ----

    pub fn reload_servers(&mut self, cx: &mut Context<Self>) {
        self.servers = self.core.lock().unwrap().store.list_servers();
        cx.emit(StateEvent::ServersChanged);
        cx.notify();
    }

    pub fn reload_keys(&mut self, cx: &mut Context<Self>) {
        let core = self.core.lock().unwrap();
        self.keys = keys_io::get_ssh_keys(&core.store, &core.keys_dir);
        drop(core);
        cx.emit(StateEvent::KeysChanged);
        cx.notify();
    }

    /// 앱 밖에서 `~/.ssh/config`(또는 사이드카)가 바뀌었으면 다시 읽는다.
    /// 실제로 달라졌을 때만 이벤트를 쏜다 — 창을 오갈 때마다 목록이
    /// 다시 그려지면 스크롤과 선택이 튄다. 두 파일 중 어느 것도 쓰지 않는다.
    /// 바뀐 게 있었으면 `true`.
    pub fn refresh_from_disk(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.core.lock().unwrap().store.reload_if_changed();
        if changed {
            self.reload_servers(cx);
        }
        changed
    }

    pub fn set_error(&mut self, err: Option<String>, cx: &mut Context<Self>) {
        self.last_error = err;
        cx.notify();
    }

    // ---- 서버 뮤테이션 (파일 I/O만 — 메인 스레드에서 충분히 빠르다) ----

    pub fn create_server(
        &mut self,
        dto: CreateServerDto,
        cx: &mut Context<Self>,
    ) -> Result<Server, CoreError> {
        let result = self.core.lock().unwrap().store.insert_server(&dto);
        if result.is_ok() {
            self.reload_servers(cx);
        }
        result
    }

    pub fn update_server(
        &mut self,
        dto: UpdateServerDto,
        cx: &mut Context<Self>,
    ) -> Result<Server, CoreError> {
        let result = self.core.lock().unwrap().store.update_server(&dto);
        if result.is_ok() {
            self.reload_servers(cx);
        }
        result
    }

    pub fn delete_server(&mut self, id: i64, cx: &mut Context<Self>) -> Result<(), CoreError> {
        let result = {
            let mut core = self.core.lock().unwrap();
            let keys_dir = core.keys_dir.clone();
            // pem 인증 서버는 개인 키 파일도 함께 지운다 — 남으면 유령 비밀이 된다.
            let _ = keys_io::delete_server_pem(&keys_dir, id);
            core.store.delete_server(id)
        };
        if result.is_ok() {
            self.reload_servers(cx);
        }
        result
    }

    pub fn toggle_favorite(&mut self, id: i64, cx: &mut Context<Self>) -> Result<(), CoreError> {
        let result = self.core.lock().unwrap().store.toggle_favorite(id);
        if result.is_ok() {
            self.reload_servers(cx);
        }
        result.map(|_| ())
    }

    // ---- 오래 걸리는 작업: 반드시 background executor ----

    /// ssh-keygen 등 초 단위 작업을 워커로 보내고, 완료 후 UI를 갱신한다.
    /// `op`는 워커 스레드에서 코어를 잠근 채 실행된다.
    pub fn spawn_core<T, F>(&mut self, cx: &mut Context<Self>, op: F) -> Task<Result<T, CoreError>>
    where
        T: Send + 'static,
        F: FnOnce(&mut CoreCtx) -> Result<T, CoreError> + Send + 'static,
    {
        self.busy = true;
        cx.notify();
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut guard = core.lock().unwrap();
                    op(&mut guard)
                })
                .await;
            this.update(cx, |state, cx| {
                state.busy = false;
                match &result {
                    Ok(_) => state.last_error = None,
                    Err(e) => state.last_error = Some(e.to_string()),
                }
                state.reload_servers(cx);
                state.reload_keys(cx);
            })
            .ok();
            result
        })
    }

    // ---- 설정 ----

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        f(&mut self.settings);
        self.settings.normalize();
        self.settings.save(&self.paths.settings_file);
        cx.emit(StateEvent::SettingsChanged);
        cx.notify();
    }

    /// 서버 목록 필터링 (검색어 + 그룹) — ServerList 화면 로직, 순수하게 유지해
    /// 테스트할 수 있게 분리했다.
    pub fn filter_servers<'a>(
        servers: &'a [Server],
        query: &str,
        group: Option<&str>,
    ) -> Vec<&'a Server> {
        let q = query.trim().to_lowercase();
        servers
            .iter()
            .filter(|s| group.is_none_or(|g| s.group_name.as_deref() == Some(g)))
            .filter(|s| {
                if q.is_empty() {
                    return true;
                }
                let haystack = [
                    Some(s.name.as_str()),
                    Some(s.host.as_str()),
                    Some(s.username.as_str()),
                    s.group_name.as_deref(),
                    s.tags.as_deref(),
                ];
                haystack.iter().flatten().any(|f| f.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// 그룹 드롭다운 목록 — 중복 제거 후 정렬.
    pub fn server_groups(servers: &[Server]) -> Vec<String> {
        let mut groups: Vec<String> = servers
            .iter()
            .filter_map(|s| s.group_name.as_ref())
            .filter(|g| !g.trim().is_empty())
            .cloned()
            .collect();
        groups.sort_by_key(|g| g.to_lowercase());
        groups.dedup();
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshub_core::model::AuthType;

    fn server(name: &str, host: &str, user: &str, group: Option<&str>, tags: Option<&str>) -> Server {
        Server {
            name: name.into(),
            host: host.into(),
            username: user.into(),
            group_name: group.map(Into::into),
            tags: tags.map(Into::into),
            auth_type: AuthType::Key,
            port: 22,
            ..Server::default()
        }
    }

    #[test]
    fn filters_by_any_visible_field_case_insensitively() {
        let servers = vec![
            server("prod-web", "10.0.0.1", "deploy", Some("production"), Some(r#"["nginx"]"#)),
            server("db", "db.internal", "postgres", None, None),
        ];
        let hit = |q: &str| AppState::filter_servers(&servers, q, None).len();
        assert_eq!(hit("PROD"), 1, "이름");
        assert_eq!(hit("db.INTERNAL"), 1, "호스트");
        assert_eq!(hit("postgres"), 1, "사용자");
        assert_eq!(hit("production"), 1, "그룹");
        assert_eq!(hit("nginx"), 1, "태그");
        assert_eq!(hit(""), 2, "빈 검색어는 전부");
        assert_eq!(hit("없는값"), 0);
    }

    #[test]
    fn group_filter_combines_with_query() {
        let servers = vec![
            server("alpha-web", "1.1.1.1", "u", Some("prod"), None),
            server("bravo-web", "2.2.2.2", "u", Some("dev"), None),
        ];
        assert_eq!(AppState::filter_servers(&servers, "web", Some("prod")).len(), 1);
        // 그룹이 맞아도 검색어가 어긋나면 제외된다.
        assert_eq!(AppState::filter_servers(&servers, "bravo", Some("prod")).len(), 0);
        assert_eq!(AppState::filter_servers(&servers, "bravo", None).len(), 1);
    }

    #[test]
    fn groups_are_deduped_sorted_and_blank_free() {
        let servers = vec![
            server("a", "h", "u", Some("zeta"), None),
            server("b", "h", "u", Some("Alpha"), None),
            server("c", "h", "u", Some("zeta"), None),
            server("d", "h", "u", Some("  "), None),
            server("e", "h", "u", None, None),
        ];
        assert_eq!(AppState::server_groups(&servers), vec!["Alpha", "zeta"]);
    }
}
