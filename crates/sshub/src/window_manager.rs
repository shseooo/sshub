//! 앱 스코프 창 레코드 소유자 (DESIGN-terminal.md §8).
//!
//! 다중 창에서 "설정에 레이아웃을 쓰는 주체"는 **여기 하나뿐**이다.
//! 각 `TerminalWorkspace`가 직접 `Settings.terminal_layout`에 쓰면 마지막에
//! 저장한 창이 나머지 창의 탭을 덮어써 버린다(§12에서 지적된 충돌). 그래서
//! 워크스페이스는 `WorkspaceEvent::LayoutChanged`만 올리고, 창 셸이 그것을
//! 받아 `update_layout`으로 자기 레코드만 갱신한 뒤 `persist`를 요청한다.
//!
//! 저장은 400ms 디바운스 — 탭 드래그·분할 리사이즈처럼 연속으로 들어오는
//! 변경마다 JSON을 쓰지 않기 위해서다. 종료 경로는 디바운스를 기다릴 수
//! 없으므로 `persist_now`(즉시)를 쓴다.

use std::time::Duration;

use gpui::{App, AppContext as _, Bounds, Context, Entity, Global, Pixels, Point, Task, WindowHandle};
use sshub_core::window_state::WindowBounds;
use sshub_splits::TerminalTab;

use crate::displays;
use crate::state::app_state;
use crate::window_session::{self, WindowRecord};
use crate::workspace::Workspace;

/// 연속 변경을 한 번의 파일 쓰기로 접는 창(窓).
pub const PERSIST_DEBOUNCE: Duration = Duration::from_millis(400);

/// 창 식별자 — 셸이 자기 값을 들고 있다가 갱신·해제에 쓴다.
/// gpui `WindowHandle`은 뷰 타입에 묶여 있어 키로 쓰기 불편해 별도 발급한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// 매니저가 창 하나에 대해 아는 전부.
struct WindowEntry {
    id: WindowId,
    record: WindowRecord,
    /// 창이 실제로 열린 뒤 셸이 채워 준다. 다른 창으로 탭을 밀어 넣을 때
    /// 필요하다 — 이게 없으면 목적 창의 뷰에 손댈 방법이 없다.
    handle: Option<WindowHandle<Workspace>>,
    /// 마지막으로 활성화된 순번. 화면에서 겹친 창 중 **위에 있는 창**을
    /// 고르는 유일한 근거다 (플랫폼 z-order는 gpui가 노출하지 않는다).
    activated: u64,
    /// 이 창의 **전역** 사각형 (모든 모니터를 관통하는 좌표계, `displays`).
    ///
    /// `record.bounds`와 따로 두는 이유: 저장·복원에 쓰는 그 값은 gpui가 창을
    /// 열 때 기대하는 **디스플레이 상대** 좌표라, 모니터가 둘이면 서로 다른
    /// 창의 값을 그대로 비교할 수 없다.
    global: Option<Bounds<Pixels>>,
}

pub struct WindowManager {
    /// 등록 순서 = 저장 순서 = 다음 실행의 복원 순서.
    records: Vec<WindowEntry>,
    next_id: u64,
    /// `activated` 발급기. 0은 "한 번도 활성화되지 않음"으로 남긴다.
    activation_clock: u64,
    /// 종료 중에는 창이 닫혀도 레코드를 지우지 않는다 — 종료 시 gpui가 창을
    /// 전부 드랍하므로, 그때 unregister를 허용하면 방금 저장한 창 목록이
    /// 빈 배열로 덮여 다음 실행에서 탭이 전부 사라진다.
    quitting: bool,
    _persist: Option<Task<()>>,
}

struct WindowManagerHandle(Entity<WindowManager>);
impl Global for WindowManagerHandle {}

/// 전역 등록 (앱 부트스트랩에서 1회).
pub fn init(cx: &mut App) -> Entity<WindowManager> {
    let manager = cx.new(|_| WindowManager::new());
    cx.set_global(WindowManagerHandle(manager.clone()));
    manager
}

/// 초기화된 전역 매니저 (init 이후에만 유효).
pub fn manager(cx: &App) -> Entity<WindowManager> {
    cx.global::<WindowManagerHandle>().0.clone()
}

pub fn try_manager(cx: &App) -> Option<Entity<WindowManager>> {
    cx.try_global::<WindowManagerHandle>().map(|h| h.0.clone())
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowManager {
    pub fn new() -> Self {
        WindowManager {
            records: Vec::new(),
            next_id: 0,
            activation_clock: 0,
            quitting: false,
            _persist: None,
        }
    }

    /// 종료 시퀀스 시작을 알린다 (이후 `unregister`는 무시된다).
    pub fn begin_quit(&mut self) {
        self.quitting = true;
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// 새 창을 등록하고 id를 발급한다. 탭은 셸이 뜬 뒤 `update_layout`으로 채운다.
    pub fn register(&mut self, bounds: WindowBounds) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.records.push(WindowEntry {
            id,
            record: WindowRecord {
                bounds,
                ..WindowRecord::empty()
            },
            handle: None,
            activated: 0,
            global: None,
        });
        id
    }

    /// 창이 열린 직후 셸 핸들을 붙인다 (`workspace::open`).
    pub fn set_handle(&mut self, id: WindowId, handle: WindowHandle<Workspace>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.handle = Some(handle);
        }
    }

    pub fn handle(&self, id: WindowId) -> Option<WindowHandle<Workspace>> {
        self.entry(id).and_then(|entry| entry.handle)
    }

    /// 이 창이 방금 활성화됐다 — 겹친 창 중 무엇이 위인지의 근거를 갱신한다.
    pub fn touch(&mut self, id: WindowId) {
        self.activation_clock += 1;
        let clock = self.activation_clock;
        if let Some(entry) = self.entry_mut(id) {
            entry.activated = clock;
        }
    }

    /// 화면 좌표 위에 있는 창 — 겹쳤으면 **가장 최근에 활성화된** 창.
    ///
    /// 창을 넘나드는 탭 드래그에서 목적지를 정하는 유일한 판정이다. macOS는
    /// 마우스를 누른 창이 버튼을 뗄 때까지 이벤트를 독점하므로(implicit
    /// capture), 목적 창은 자기가 호버됐다는 사실조차 모른다. 그래서 드래그를
    /// 시작한 창이 저장된 창 사각형으로 직접 맞혀야 한다.
    ///
    /// 아직 화면에 자리 잡지 않은 창(`global`이 없음)은 후보에서 뺀다.
    ///
    /// `at`은 **전역 좌표**여야 한다([`crate::displays`]) — 모니터가 둘 이상이면
    /// gpui의 창 좌표가 모니터마다 다른 공간에 있어서, 그대로 비교하면 다른
    /// 모니터의 창이 잡히지 않거나 엉뚱하게 잡힌다.
    pub fn window_at(&self, at: Point<Pixels>, except: Option<WindowId>) -> Option<WindowId> {
        self.records
            .iter()
            .filter(|entry| Some(entry.id) != except)
            .filter(|entry| {
                entry
                    .global
                    .as_ref()
                    .is_some_and(|rect| displays::contains(rect, at))
            })
            .max_by_key(|entry| entry.activated)
            .map(|entry| entry.id)
    }

    /// 그 창의 탭 구성만 교체한다 (다른 창 레코드는 건드리지 않는다).
    pub fn update_layout(&mut self, id: WindowId, tabs: Vec<TerminalTab>, active: usize) {
        if let Some(record) = self.record_mut(id) {
            record.tabs = tabs;
            record.active_index = active;
        }
    }

    /// 저장용 지오메트리(`bounds`)와 화면 판정용 전역 사각형(`global`)을 함께
    /// 갱신한다 — 둘은 같은 창에서 나오지만 좌표계가 다르다.
    pub fn update_bounds(&mut self, id: WindowId, bounds: WindowBounds, global: Bounds<Pixels>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.record.bounds = bounds;
            entry.global = Some(global);
        }
    }

    /// 창이 닫혔다. 남은 창들의 순서는 그대로 유지된다.
    /// 종료 중이라면 무시한다(위 `quitting` 주석 참조).
    pub fn unregister(&mut self, id: WindowId) {
        if self.quitting {
            return;
        }
        self.records.retain(|entry| entry.id != id);
    }

    fn entry(&self, id: WindowId) -> Option<&WindowEntry> {
        self.records.iter().find(|entry| entry.id == id)
    }

    fn entry_mut(&mut self, id: WindowId) -> Option<&mut WindowEntry> {
        self.records.iter_mut().find(|entry| entry.id == id)
    }

    fn record_mut(&mut self, id: WindowId) -> Option<&mut WindowRecord> {
        self.entry_mut(id).map(|entry| &mut entry.record)
    }

    pub fn record(&self, id: WindowId) -> Option<&WindowRecord> {
        self.entry(id).map(|entry| &entry.record)
    }

    pub fn ids(&self) -> Vec<WindowId> {
        self.records.iter().map(|entry| entry.id).collect()
    }

    pub fn records(&self) -> Vec<WindowRecord> {
        self.records
            .iter()
            .map(|entry| entry.record.clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 살아 있어야 할 세션 id 전체 — 스크롤백/cwd 정리 기준.
    pub fn live_session_ids(&self) -> Vec<String> {
        window_session::live_session_ids(&self.records())
    }

    /// 디바운스 저장. 연속 호출은 마지막 것만 살아남는다(이전 Task 드랍 = 취소).
    pub fn persist(&mut self, cx: &mut Context<Self>) {
        self._persist = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PERSIST_DEBOUNCE).await;
            this.update(cx, |this, cx| this.persist_now(cx)).ok();
        }));
    }

    /// 즉시 저장 (종료 경로 — 디바운스를 기다릴 수 없다).
    pub fn persist_now(&mut self, cx: &mut App) {
        // 대기 중이던 디바운스는 의미가 없어졌다.
        self._persist = None;
        let records = self.records();
        let state = app_state(cx);
        state.update(cx, |state, cx| {
            state.update_settings(|settings| window_session::persist_windows(settings, &records), cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshub_core::settings::Settings;
    use sshub_splits::{PaneNode, SessionId, TabId, TerminalLeaf, TerminalTab};

    const B: WindowBounds = WindowBounds {
        width: 1000,
        height: 700,
        x: None,
        y: None,
    };

    fn tab(id: &str, session: &str) -> TerminalTab {
        TerminalTab {
            id: TabId::new(id),
            root: PaneNode::Leaf(TerminalLeaf::new(SessionId::new(session), None, "sh")),
            name: None,
        }
    }

    #[test]
    fn ids_are_unique_and_registration_order_is_the_save_order() {
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);
        let third = manager.register(B);
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_eq!(manager.ids(), vec![first, second, third]);
    }

    #[test]
    fn each_window_keeps_its_own_tabs() {
        // 회귀 방지: 한 창의 저장이 다른 창의 탭을 덮어쓰면 안 된다.
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);

        manager.update_layout(first, vec![tab("t1", "s1")], 0);
        manager.update_layout(second, vec![tab("t2", "s2"), tab("t3", "s3")], 1);

        let records = manager.records();
        assert_eq!(records[0].session_ids(), vec!["s1"]);
        assert_eq!(records[1].session_ids(), vec!["s2", "s3"]);
        assert_eq!(records[1].active_index, 1);
    }

    #[test]
    fn updating_an_unknown_window_is_a_no_op() {
        // 닫히는 중인 창에서 늦게 도착한 이벤트가 남의 레코드를 건드리면 안 된다.
        let mut manager = WindowManager::new();
        let id = manager.register(B);
        manager.unregister(id);
        manager.update_layout(id, vec![tab("t1", "s1")], 0);
        manager.update_bounds(
            id,
            WindowBounds { width: 1, height: 1, x: None, y: None },
            Bounds::default(),
        );
        assert!(manager.is_empty());
        assert!(manager.records().is_empty());
    }

    #[test]
    fn unregister_removes_only_that_window_and_keeps_order() {
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);
        let third = manager.register(B);
        manager.update_layout(first, vec![tab("t1", "s1")], 0);
        manager.update_layout(second, vec![tab("t2", "s2")], 0);
        manager.update_layout(third, vec![tab("t3", "s3")], 0);

        manager.unregister(second);

        assert_eq!(manager.ids(), vec![first, third]);
        let ids: Vec<Vec<String>> = manager.records().iter().map(|r| r.session_ids()).collect();
        assert_eq!(ids, vec![vec!["s1"], vec!["s3"]]);
    }

    #[test]
    fn closing_windows_during_quit_does_not_erase_the_saved_session() {
        // 회귀 방지: 종료 시 gpui가 창을 드랍하며 unregister를 부르면,
        // 방금 저장한 창 목록이 빈 배열이 되어 다음 실행에서 탭이 사라진다.
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);
        manager.update_layout(first, vec![tab("t1", "s1")], 0);
        manager.update_layout(second, vec![tab("t2", "s2")], 0);

        manager.begin_quit();
        manager.unregister(first);
        manager.unregister(second);

        assert!(manager.is_quitting());
        assert_eq!(manager.records().len(), 2, "종료 중 레코드는 보존된다");
        assert_eq!(manager.live_session_ids(), vec!["s1", "s2"]);
    }

    #[test]
    fn reusing_a_freed_id_is_impossible() {
        // id를 재사용하면 늦게 온 이벤트가 엉뚱한 창에 붙는다.
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        manager.unregister(first);
        let second = manager.register(B);
        assert_ne!(first, second);
    }

    #[test]
    fn bounds_updates_land_on_the_right_record() {
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);
        manager.update_bounds(
            second,
            WindowBounds { width: 1280, height: 800, x: Some(40), y: Some(50) },
            Bounds::default(),
        );
        assert_eq!(manager.record(first).unwrap().bounds, B);
        let moved = manager.record(second).unwrap().bounds.clone();
        assert_eq!((moved.width, moved.height), (1280, 800));
        assert_eq!((moved.x, moved.y), (Some(40), Some(50)));
    }

    #[test]
    fn persist_output_round_trips_into_restorable_windows() {
        let mut manager = WindowManager::new();
        let first = manager.register(WindowBounds {
            width: 1200,
            height: 900,
            x: Some(10),
            y: Some(20),
        });
        let second = manager.register(B);
        manager.update_layout(first, vec![tab("t1", "s1")], 0);
        manager.update_layout(second, vec![tab("t2", "s2"), tab("t3", "s3")], 1);

        let mut settings = Settings::default();
        window_session::persist_windows(&mut settings, &manager.records());

        // 파일을 거쳐도 창 수·탭·활성 인덱스가 그대로 돌아온다.
        let json = serde_json::to_string(&settings).unwrap();
        let reloaded: Settings = serde_json::from_str(&json).unwrap();
        let restored = window_session::restore_windows(&reloaded, None);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].bounds.width, 1200);
        assert_eq!(restored[0].session_ids(), vec!["s1"]);
        assert_eq!(restored[1].session_ids(), vec!["s2", "s3"]);
        assert_eq!(restored[1].active_index(), 1);
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: gpui::point(gpui::px(x), gpui::px(y)),
            size: gpui::size(gpui::px(w), gpui::px(h)),
        }
    }

    fn at(x: f32, y: f32) -> Point<Pixels> {
        gpui::point(gpui::px(x), gpui::px(y))
    }

    /// 창을 등록하고 전역 사각형까지 채운다 (실제 경로는 `sync_bounds`).
    fn placed(manager: &mut WindowManager, x: f32, y: f32, w: f32, h: f32) -> WindowId {
        let id = manager.register(B);
        manager.update_bounds(id, B, rect(x, y, w, h));
        id
    }

    #[test]
    fn window_at_finds_the_window_under_a_screen_point() {
        let mut manager = WindowManager::new();
        let left = placed(&mut manager, 0.0, 0.0, 800.0, 600.0);
        let right = placed(&mut manager, 900.0, 100.0, 800.0, 600.0);

        assert_eq!(manager.window_at(at(10.0, 10.0), None), Some(left));
        assert_eq!(manager.window_at(at(1000.0, 200.0), None), Some(right));
        assert_eq!(manager.window_at(at(850.0, 200.0), None), None, "창 사이 빈 화면");
        // 경계: 좌상단은 포함, 우하단은 배타 (인접한 두 창이 겹쳐 잡히지 않게).
        assert_eq!(manager.window_at(at(0.0, 0.0), None), Some(left));
        assert_eq!(manager.window_at(at(800.0, 600.0), None), None);
    }

    #[test]
    fn a_window_on_another_monitor_is_a_valid_drop_target() {
        // 회귀 방지(실제 신고): 좌표를 디스플레이 상대로 비교하면 다른 모니터의
        // 창이 후보에서 통째로 빠진다. 전역 좌표로 올리면 두 번째 모니터
        // (전역 x = 2560~)에 있는 창도 그냥 잡힌다.
        let mut manager = WindowManager::new();
        let on_primary = placed(&mut manager, 100.0, 100.0, 1000.0, 700.0);
        let on_second = placed(&mut manager, 2700.0, 200.0, 1000.0, 700.0);

        assert_eq!(manager.window_at(at(200.0, 200.0), None), Some(on_primary));
        assert_eq!(manager.window_at(at(3000.0, 400.0), None), Some(on_second));
        // 주 모니터 왼쪽에 붙은 모니터는 전역 x가 음수다.
        let on_left = placed(&mut manager, -1800.0, 50.0, 900.0, 600.0);
        assert_eq!(manager.window_at(at(-1500.0, 100.0), None), Some(on_left));
    }

    #[test]
    fn overlapping_windows_resolve_to_the_most_recently_activated() {
        // 창이 겹치면 "위에 있는 창"에 떨어져야 한다. 플랫폼 z-order를 gpui가
        // 주지 않으므로 활성 순번으로 근사한다.
        let mut manager = WindowManager::new();
        let below = placed(&mut manager, 0.0, 0.0, 800.0, 600.0);
        let above = placed(&mut manager, 100.0, 100.0, 800.0, 600.0);

        manager.touch(below);
        assert_eq!(manager.window_at(at(200.0, 200.0), None), Some(below));
        manager.touch(above);
        assert_eq!(manager.window_at(at(200.0, 200.0), None), Some(above));
        // 겹치지 않는 자리는 활성 순번과 무관하다.
        assert_eq!(manager.window_at(at(50.0, 50.0), None), Some(below));
    }

    #[test]
    fn window_at_can_exclude_a_window_and_ignores_unplaced_ones() {
        let mut manager = WindowManager::new();
        let target = placed(&mut manager, 0.0, 0.0, 800.0, 600.0);
        // 아직 화면에 자리 잡지 않은 창을 원점에 있다고 보면 좌상단 드롭이
        // 빨려 들어간다.
        let unplaced = manager.register(B);

        assert_eq!(manager.window_at(at(10.0, 10.0), Some(target)), None);
        assert_ne!(manager.window_at(at(10.0, 10.0), None), Some(unplaced));
    }

    #[test]
    fn a_closed_window_is_no_longer_a_drop_target() {
        let mut manager = WindowManager::new();
        let id = placed(&mut manager, 0.0, 0.0, 800.0, 600.0);
        manager.touch(id);
        manager.unregister(id);
        assert_eq!(manager.window_at(at(10.0, 10.0), None), None);
    }

    #[test]
    fn live_session_ids_span_every_window_deduped() {
        let mut manager = WindowManager::new();
        let first = manager.register(B);
        let second = manager.register(B);
        manager.update_layout(first, vec![tab("t1", "s1")], 0);
        manager.update_layout(second, vec![tab("t2", "s2")], 0);
        assert_eq!(manager.live_session_ids(), vec!["s1", "s2"]);
    }
}
