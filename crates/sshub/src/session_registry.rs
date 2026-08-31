//! 세션 레지스트리 (DESIGN-terminal.md §6·§7) — 살아 있는 `Entity<Terminal>`의
//! 유일한 소유자.
//!
//! **앱 스코프**(창 스코프가 아님)인 것이 핵심이다. 터미널 엔티티가 창 밖에
//! 살아 있어야 탭을 다른 창으로 옮길 때 PTY·그리드가 그대로 따라간다
//! (Electron 판 `terminalPool`의 DOM 재부모화에 대응 — §8).
//!
//! 책임:
//! - spawn 계획 수립(`session.rs`의 순수 함수) + PTY 기동, kill-before-respawn 가드
//! - 스크롤백 복원(spawn 전 주입) / 디바운스 저장(Wakeup 후 1500ms)
//! - 로컬 세션 cwd 스냅샷(종료 시 저장) 및 죽은 세션 파일 prune

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use sshub_core::model::{Server, SshKeyView};
use sshub_core::{AppPaths, ScrollbackStore, TerminalCwdStore};
use sshub_splits::SessionId;
use sshub_terminal::{
    Event as TerminalEvent, SpawnSpec, Terminal, TerminalBounds, TerminalBuilder,
};

use crate::session::{plan_local, plan_ssh, SessionEnv, SpawnPlan};

/// Wakeup 이후 이만큼 조용하면 스크롤백을 저장한다 (§7).
pub const SCROLLBACK_DEBOUNCE: Duration = Duration::from_millis(1500);

struct Session {
    terminal: Entity<Terminal>,
    /// 로컬 셸인가 — cwd 스냅샷/상속은 로컬에서만 의미가 있다.
    local: bool,
    /// 디바운스 저장 타이머. 새 Wakeup마다 교체되며, 교체 시 이전 Task는
    /// drop되어 취소된다 (그게 디바운스다).
    save: Option<Task<()>>,
    _events: Subscription,
}

pub struct SessionRegistry {
    sessions: HashMap<SessionId, Session>,
    /// 스크롤백 디렉터리를 못 만들면 영속화만 조용히 꺼진다(터미널은 정상 동작).
    scrollback: Option<Arc<ScrollbackStore>>,
    cwds: TerminalCwdStore,
    servers: Vec<Server>,
    keys: Vec<SshKeyView>,
    home: PathBuf,
    shell: String,
}

struct RegistryHandle(Entity<SessionRegistry>);
impl Global for RegistryHandle {}

/// 전역 레지스트리 초기화 — 앱/예제 부트스트랩에서 1회.
pub fn init(paths: &AppPaths, cx: &mut App) -> Entity<SessionRegistry> {
    let registry = cx.new(|_| SessionRegistry::new(paths));
    cx.set_global(RegistryHandle(registry.clone()));
    registry
}

/// 초기화된 전역 레지스트리 (init 이후에만 유효).
pub fn registry(cx: &App) -> Entity<SessionRegistry> {
    cx.global::<RegistryHandle>().0.clone()
}

/// 아직 초기화되지 않았으면 `None` — 부트스트랩 순서를 강제하지 않기 위해.
pub fn try_registry(cx: &App) -> Option<Entity<SessionRegistry>> {
    cx.try_global::<RegistryHandle>().map(|h| h.0.clone())
}

impl SessionRegistry {
    pub fn new(paths: &AppPaths) -> SessionRegistry {
        let scrollback = ScrollbackStore::new(paths.scrollback_dir.clone())
            .ok()
            .map(Arc::new);
        let mut cwds = TerminalCwdStore::new(paths.terminal_cwd_file.clone());
        cwds.load();
        SessionRegistry {
            sessions: HashMap::new(),
            scrollback,
            cwds,
            servers: Vec::new(),
            keys: Vec::new(),
            home: home_dir(),
            shell: login_shell(),
        }
    }

    /// 서버/키 목록 스냅샷 — SSH 계획을 세울 때 쓴다. `AppState`가 바뀔 때마다
    /// 워크스페이스가 다시 넣어 준다(레지스트리가 전역 상태를 직접 읽지 않게 해
    /// 테스트에서 코어 없이 쓸 수 있다).
    pub fn set_catalog(&mut self, servers: Vec<Server>, keys: Vec<SshKeyView>) {
        self.servers = servers;
        self.keys = keys;
    }

    pub fn get(&self, session_id: &SessionId) -> Option<Entity<Terminal>> {
        self.sessions.get(session_id).map(|s| s.terminal.clone())
    }

    pub fn is_live(&self, session_id: &SessionId) -> bool {
        self.sessions.contains_key(session_id)
    }

    pub fn live_ids(&self) -> Vec<String> {
        self.sessions.keys().map(|id| id.0.clone()).collect()
    }

    /// 세션 시작. 같은 id가 이미 살아 있으면 **먼저 죽인다** — 재연결·재시작에서
    /// 유령 PTY가 남으면 fd와 자식 프로세스가 새어나간다.
    pub fn start(
        &mut self,
        session_id: &SessionId,
        server_id: Option<i64>,
        cwd_from: Option<&SessionId>,
        cx: &mut Context<Self>,
    ) -> Result<Entity<Terminal>> {
        if let Some(old) = self.sessions.remove(session_id) {
            old.terminal.update(cx, |terminal, _| terminal.kill());
        }

        let (plan, local) = self.plan_for(session_id, server_id, cwd_from, cx)?;
        // 복원은 spawn 전에 결정한다 — 빌더가 PTY를 열기 전에 주입해야
        // 셸 프롬프트보다 위에 옛 화면이 놓인다.
        let restored = self
            .scrollback
            .as_ref()
            .and_then(|store| store.load(session_id.as_str()))
            .filter(|text| !text.is_empty());

        let spec = SpawnSpec {
            program: plan.program,
            args: plan.args,
            cwd: Some(plan.cwd),
            env: plan.env.into_iter().collect(),
            banner: plan.banner,
            restored_scrollback: restored,
            initial_bounds: TerminalBounds::default(),
        };

        let builder = TerminalBuilder::new(spec)?;
        let terminal = cx.new(|cx| builder.subscribe(cx));

        let id = session_id.clone();
        let events = cx.subscribe(&terminal, move |this, _terminal, event, cx| {
            if matches!(event, TerminalEvent::Wakeup) {
                this.arm_save(&id, cx);
            }
        });

        self.sessions.insert(
            session_id.clone(),
            Session {
                terminal: terminal.clone(),
                local,
                save: None,
                _events: events,
            },
        );
        Ok(terminal)
    }

    /// 세션 종료: PTY kill + 스크롤백/저장된 cwd 삭제. 사용자가 pane을 닫은
    /// 것이므로 그 히스토리를 되살릴 이유가 없다(TS `closeSession`과 동일).
    pub fn close(&mut self, session_id: &SessionId, cx: &mut App) {
        if let Some(session) = self.sessions.remove(session_id) {
            session.terminal.update(cx, |terminal, _| terminal.kill());
        }
        if let Some(store) = &self.scrollback {
            store.delete(session_id.as_str());
        }
        self.cwds.delete(session_id.as_str());
    }

    /// 앱 종료 경로: cwd 스냅샷 → 스크롤백 flush → kill (§6의 순서).
    pub fn shutdown_all(&mut self, cx: &mut App) {
        self.snapshot_cwds(cx);
        self.flush_scrollback(cx);
        let ids: Vec<SessionId> = self.sessions.keys().cloned().collect();
        for id in ids {
            if let Some(session) = self.sessions.remove(&id) {
                session.terminal.update(cx, |terminal, _| terminal.kill());
            }
        }
    }

    /// 살아 있는 **로컬** 세션의 현재 디렉터리 (분할 시 상속원).
    /// 블로킹 가능(libproc/lsof)이지만 분할은 사용자 동작 1회라 허용한다.
    pub fn live_local_cwd(&self, session_id: &SessionId, cx: &mut App) -> Option<PathBuf> {
        let terminal = {
            let session = self.sessions.get(session_id)?;
            if !session.local {
                return None;
            }
            session.terminal.clone()
        };
        terminal
            .update(cx, |terminal, _| terminal.refresh_cwd())
            .map(PathBuf::from)
    }

    /// 살아 있는 로컬 세션들의 cwd를 저장한다. **PTY를 죽이기 전에** 부를 것 —
    /// 죽은 프로세스의 cwd는 읽을 수 없다.
    pub fn snapshot_cwds(&mut self, cx: &mut App) {
        let locals: Vec<(SessionId, Entity<Terminal>)> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.local)
            .map(|(id, s)| (id.clone(), s.terminal.clone()))
            .collect();
        for (id, terminal) in locals {
            if let Some(cwd) = terminal.update(cx, |terminal, _| terminal.refresh_cwd()) {
                self.cwds.set(id.as_str(), &cwd);
            }
        }
    }

    /// 죽은 세션의 스크롤백/cwd 파일 정리 (시작 시 1회).
    pub fn prune_scrollback(&mut self, live_ids: &[String]) {
        if let Some(store) = &self.scrollback {
            store.prune(live_ids);
        }
        self.cwds.prune(live_ids);
    }

    /// 모든 세션의 스크롤백을 즉시 저장 (종료 경로). 디바운스를 기다리지 않는다.
    pub fn flush_scrollback(&mut self, cx: &mut App) {
        let Some(store) = self.scrollback.clone() else {
            return;
        };
        let entries: Vec<(SessionId, Entity<Terminal>)> = self
            .sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.terminal.clone()))
            .collect();
        for (id, terminal) in entries {
            let terminal = terminal.read(cx);
            // 한 번도 화면에 뜨지 않은(=hydrate 안 된) 터미널의 빈 버퍼로
            // 진짜 히스토리를 덮어쓰면 안 된다 (§7 no-clobber).
            if !terminal.hydrated {
                continue;
            }
            let _ = store.save(id.as_str(), &terminal.serialize_scrollback_for_disk());
        }
    }

    /// Wakeup마다 저장 타이머를 재장전한다 (디바운스 1500ms).
    fn arm_save(&mut self, session_id: &SessionId, cx: &mut Context<Self>) {
        let Some(store) = self.scrollback.clone() else {
            return;
        };
        if !self.sessions.contains_key(session_id) {
            return;
        }
        let id = session_id.clone();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCROLLBACK_DEBOUNCE).await;
            // 직렬화는 엔티티를 읽어야 하므로 메인에서, 파일 쓰기만 워커로 보낸다.
            let text = this
                .update(cx, |this, cx| {
                    let session = this.sessions.get(&id)?;
                    let terminal = session.terminal.read(cx);
                    if !terminal.hydrated {
                        return None;
                    }
                    Some(terminal.serialize_scrollback_for_disk())
                })
                .ok()
                .flatten();
            if let Some(text) = text {
                cx.background_spawn(async move {
                    let _ = store.save(id.as_str(), &text);
                })
                .await;
            }
        });
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.save = Some(task);
        }
    }

    /// 실행 계획 + 로컬 여부.
    fn plan_for(
        &self,
        session_id: &SessionId,
        server_id: Option<i64>,
        cwd_from: Option<&SessionId>,
        cx: &mut App,
    ) -> Result<(SpawnPlan, bool)> {
        let Some(server_id) = server_id else {
            // ① 분할 원본의 라이브 cwd → ② 저장된 cwd → ③ 홈 (session.rs가 판정).
            let live = cwd_from.and_then(|src| self.live_local_cwd(src, cx));
            let saved = self.cwds.get(session_id.as_str()).map(PathBuf::from);
            let live_fn = move |_: &str| live.clone();
            let saved_fn = move |_: &str| saved.clone();
            let env = SessionEnv {
                live_local_cwd: &live_fn,
                saved_cwd: &saved_fn,
                home: self.home.clone(),
                shell: self.shell.clone(),
            };
            let plan = plan_local(
                session_id.as_str(),
                cwd_from.map(|id| id.as_str()),
                &env,
            );
            return Ok((plan, true));
        };

        let server = self
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| anyhow!("서버 {server_id}를 찾을 수 없습니다"))?;
        // 키 경로를 여기서 풀지 않는다 — `ssh <alias>`가 config 블록의
        // `IdentityFile`을 그대로 쓴다 (Phase 3).
        let plan = plan_ssh(server, self.home.clone());
        Ok((plan, false))
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn paths(dir: &std::path::Path) -> AppPaths {
        AppPaths::in_dir(dir.to_path_buf())
    }

    #[gpui::test]
    fn start_reuses_the_id_and_kills_the_previous_pty(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let id = SessionId::new("s1");

        let first = registry
            .update(cx, |reg, cx| reg.start(&id, None, None, cx))
            .expect("첫 세션");
        let second = registry
            .update(cx, |reg, cx| reg.start(&id, None, None, cx))
            .expect("재시작");

        assert_ne!(first.entity_id(), second.entity_id(), "새 엔티티로 교체");
        registry.update(cx, |reg, _| {
            assert_eq!(reg.sessions.len(), 1, "id당 세션은 하나만 남는다");
        });
    }

    #[gpui::test]
    fn ssh_start_fails_loudly_when_the_server_is_gone(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let result = registry.update(cx, |reg, cx| {
            reg.start(&SessionId::new("s1"), Some(42), None, cx)
        });
        assert!(result.is_err(), "없는 서버로는 PTY를 띄우지 않는다");
    }

    #[gpui::test]
    fn close_kills_the_session_and_removes_its_scrollback(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let id = SessionId::new("s1");

        registry
            .update(cx, |reg, cx| reg.start(&id, None, None, cx))
            .expect("세션");
        // 저장된 히스토리가 있는 상태를 만든다.
        registry.update(cx, |reg, _| {
            reg.scrollback
                .as_ref()
                .unwrap()
                .save(id.as_str(), "old output")
                .unwrap();
            reg.cwds.set(id.as_str(), "/tmp");
        });

        registry.update(cx, |reg, cx| reg.close(&id, cx));
        registry.update(cx, |reg, _| {
            assert!(!reg.is_live(&id));
            assert_eq!(reg.scrollback.as_ref().unwrap().load(id.as_str()), None);
            assert_eq!(reg.cwds.get(id.as_str()), None);
        });
    }

    /// 회귀 방지: 스크롤백이 **종료 훅에만** 의존하면 강제 종료·크래시에서
    /// 히스토리가 통째로 사라진다. 출력이 멎으면 디바운스 뒤 저장돼야 한다
    /// (DESIGN-terminal.md §7).
    #[gpui::test]
    fn output_arms_a_debounced_save_so_history_survives_a_crash(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let id = SessionId::new("s1");
        let terminal = registry
            .update(cx, |reg, cx| reg.start(&id, None, None, cx))
            .expect("세션");

        // 한 번도 그려지지 않은 터미널은 저장하지 않는 게 사양이다(§7 no-clobber).
        // 여기서는 화면에 떴다고 치고 저장 경로만 본다.
        terminal.update(cx, |t, _| {
            t.hydrated = true;
            t.inject_local(b"hello-from-the-pty\r\n");
        });

        // 셸이 뜨고 출력이 오갈 **실시간**을 흘려보낸다. 테스트 executor의
        // 타이머는 가상 시계라 디바운스는 따로 감아 준다.
        for _ in 0..40 {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(25));
        }
        cx.run_until_parked();
        cx.executor().advance_clock(SCROLLBACK_DEBOUNCE * 2);
        cx.run_until_parked();

        let saved = registry.update(cx, |reg, _| {
            reg.scrollback.as_ref().unwrap().load(id.as_str())
        });
        assert!(
            saved.is_some_and(|text| !text.is_empty()),
            "출력이 멎으면 디바운스 뒤에 스크롤백이 저장돼야 한다",
        );
    }

    #[gpui::test]
    fn a_never_shown_terminal_does_not_clobber_saved_history(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let id = SessionId::new("s1");

        registry.update(cx, |reg, _| {
            reg.scrollback.as_ref().unwrap().save(id.as_str(), "real history").unwrap();
        });
        registry
            .update(cx, |reg, cx| reg.start(&id, None, None, cx))
            .expect("세션");

        // hydrated=false (한 번도 레이아웃되지 않음) → flush가 건너뛴다.
        registry.update(cx, |reg, cx| reg.flush_scrollback(cx));
        registry.update(cx, |reg, _| {
            assert_eq!(
                reg.scrollback.as_ref().unwrap().load(id.as_str()).as_deref(),
                Some("real history")
            );
        });
    }

    #[gpui::test]
    fn saved_cwd_is_used_when_no_split_source(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let paths = paths(dir.path());
        let registry = cx.update(|cx| cx.new(|_| SessionRegistry::new(&paths)));
        let id = SessionId::new("s1");

        registry.update(cx, |reg, _| {
            reg.cwds.set(id.as_str(), work.to_str().unwrap());
        });
        let plan = registry.update(cx, |reg, cx| {
            reg.plan_for(&id, None, None, cx).map(|(p, local)| (p.cwd, local))
        });
        let (cwd, local) = plan.unwrap();
        assert_eq!(cwd, work);
        assert!(local);
    }
}
