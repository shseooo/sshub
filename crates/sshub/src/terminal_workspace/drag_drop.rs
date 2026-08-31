//! 드래그로 탭·pane을 재배치한다 (DESIGN-terminal.md §5·§8.1).
//!
//! `terminal_workspace`의 **자식 모듈**이라 부모 타입의 비공개 필드에 그대로
//! 접근한다 — 떼어 내려고 워크스페이스 내부를 공개할 이유가 없다.
//!
//! 트리 연산 자체는 전부 `sshub-splits`(순수)가 한다. 여기 있는 것은 "어떤
//! 연산을 언제 부르는지"와 드롭 미리보기 상태뿐이다.

use gpui::{Context, Pixels, Window};
use sshub_splits::{
    detach_pane, merge_tab, move_pane, reorder_tabs, DropSide, PaneNode, SessionId, SplitId,
    TabId, TerminalLeaf,
};

use super::{new_id, TerminalWorkspace};
use crate::split_view::{drop_side, PaneDrag, TabDrag};

impl TerminalWorkspace {
    /// 탭바 캐럿을 어디에 그릴지 (`None`이면 감춘다).
    ///
    /// 창 밖에서 끌고 오는 경우 목적 창은 드래그 중 마우스 이벤트를 받지 못하므로
    /// (macOS implicit capture), 소스 창이 이 메서드로 밀어 넣는다.
    pub fn set_tab_insert(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let at = at.filter(|index| *index <= self.tabs.len());
        if self.tab_insert != at {
            self.tab_insert = at;
            cx.notify();
        }
    }

    /// 창 좌표 x → 캐럿 위치. 탭바 밖(`None`)이면 감춘다.
    pub fn set_tab_insert_at_x(&mut self, x: Option<Pixels>, cx: &mut Context<Self>) {
        let at = x.map(|x| self.tab_boundary_for_x(x));
        self.set_tab_insert(at, cx);
    }

    /// 드래그 호버 갱신. pane은 **자기 자신에 대해서만** 보고하므로, 이탈
    /// 보고는 현재 하이라이트가 그 pane일 때만 지운다 — 그래야 리스너 호출
    /// 순서(들어온 pane이 먼저인지 나간 pane이 먼저인지)와 무관하게 안정적이다.
    pub(super) fn set_drag_over(&mut self, session: SessionId, side: Option<DropSide>, cx: &mut Context<Self>) {
        let next = match side {
            Some(side) => Some((session, side)),
            None if self.drag_over.as_ref().is_some_and(|(id, _)| *id == session) => None,
            None => return,
        };
        if self.drag_over != next {
            self.drag_over = next;
            cx.notify();
        }
    }

    pub(super) fn reorder_or_detach_on_tab_bar(&mut self, boundary: usize, drag: Option<TabDrag>, pane: Option<PaneDrag>, window: &mut Window, cx: &mut Context<Self>) {
        self.drag_over = None;
        self.tab_insert = None;
        if let Some(TabDrag { tab_id }) = drag {
            self.tabs = reorder_tabs(std::mem::take(&mut self.tabs), &tab_id, boundary);
        } else if let Some(PaneDrag { tab_id, session_id }) = pane {
            let new_tab = TabId::new(new_id());
            self.tabs = detach_pane(
                std::mem::take(&mut self.tabs),
                &tab_id,
                &session_id,
                Some(boundary),
                new_tab.clone(),
            );
            if self.tab_index(&new_tab).is_some() {
                self.active_tab = Some(new_tab);
                self.focused_pane = Some(session_id);
            }
        }
        self.sync_sessions(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 드래그 드롭의 모델 연산 — 탭 전체(`drag`) 또는 pane 하나(`pane`)를
    /// `target` pane 옆에 붙인다. 드롭 방향은 현재 포인터 위치에서 정한다.
    /// (테스트에서 직접 호출하려고 공개해 둔다 — 드래그 제스처는 자동화가 어렵다.)
    pub fn drop_on_pane(&mut self, target: SessionId, drag: Option<TabDrag>, pane: Option<PaneDrag>, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.side_for(&target, window);
        // 미리보기는 여기서 소임을 다한다 — 다음 드래그가 첫 이동 이벤트를
        // 받기 전에 옛 하이라이트가 깜빡이지 않도록 즉시 지운다.
        self.drag_over = None;
        self.tab_insert = None;
        let Some(dst_tab) = self.tab_of_pane(&target) else {
            return;
        };
        if let Some(TabDrag { tab_id }) = drag {
            if tab_id == dst_tab {
                return;
            }
            self.tabs = merge_tab(
                std::mem::take(&mut self.tabs),
                &tab_id,
                &dst_tab,
                &target,
                side,
                &SplitId::new(new_id()),
            );
            self.active_tab = Some(dst_tab);
        } else if let Some(PaneDrag { tab_id, session_id }) = pane {
            if session_id == target {
                return;
            }
            if tab_id == dst_tab {
                let Some(index) = self.tab_index(&dst_tab) else {
                    return;
                };
                let root = std::mem::replace(
                    &mut self.tabs[index].root,
                    PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
                );
                self.tabs[index].root =
                    move_pane(root, &session_id, &target, side, &SplitId::new(new_id()));
            } else {
                // 다른 탭의 pane: 원본에서 떼어내 임시 탭으로 만든 뒤 병합한다
                // (detach + merge 조합이 TS 판의 크로스 탭 이동과 동일한 결과).
                let staging = TabId::new(new_id());
                self.tabs = detach_pane(
                    std::mem::take(&mut self.tabs),
                    &tab_id,
                    &session_id,
                    None,
                    staging.clone(),
                );
                if self.tab_index(&staging).is_some() {
                    self.tabs = merge_tab(
                        std::mem::take(&mut self.tabs),
                        &staging,
                        &dst_tab,
                        &target,
                        side,
                        &SplitId::new(new_id()),
                    );
                }
            }
            self.focused_pane = Some(session_id);
            self.active_tab = Some(dst_tab);
        }
        self.sync_sessions(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn side_for(&self, target: &SessionId, window: &mut Window) -> DropSide {
        self.geometry
            .borrow()
            .panes
            .get(target)
            .map(|bounds| drop_side(*bounds, window.mouse_position()))
            .unwrap_or(DropSide::Right)
    }
}
