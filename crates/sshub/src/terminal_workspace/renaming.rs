//! 탭 이름·pane 라벨 인라인 편집.
//!
//! `terminal_workspace`의 **자식 모듈**이라 부모 타입의 비공개 필드에 그대로
//! 접근한다.
//!
//! 탭 이름과 pane 라벨은 편집 상태가 서로 독립이다(둘 다 더블클릭으로 시작하고
//! 동시에 열릴 수 있다). 그래서 `rename`/`pane_rename` 두 슬롯을 따로 둔다.

use gpui::{AppContext as _, Context, Focusable as _, Window};
use sshub_splits::{
    leaves, rename_leaf, tab_title, PaneNode, SessionId, TabId, TerminalLeaf,
};

use super::TerminalWorkspace;
use crate::ui::TextInput;

impl TerminalWorkspace {
    pub fn start_rename(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let title = tab_title(&self.tabs[index]).to_string();
        let input = cx.new(|cx| TextInput::new(window, cx).with_text(title));
        let editing = tab_id.clone();
        let sub = cx.subscribe(&input, move |this: &mut Self, input, event, cx| {
            use crate::ui::InputEvent;
            match event {
                InputEvent::Submitted => {
                    let text = input.read(cx).text().to_string();
                    this.commit_rename(&editing, &text, cx);
                }
                InputEvent::Blurred => {
                    this.rename = None;
                    cx.notify();
                }
                InputEvent::Changed => {}
            }
        });
        self._subscriptions.push(sub);
        window.focus(&input.read(cx).focus_handle(cx));
        self.rename = Some((tab_id, input));
        cx.notify();
    }

    fn commit_rename(&mut self, tab_id: &TabId, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim();
        if let Some(index) = self.tab_index(tab_id) {
            // 빈 이름은 "이름 없음"으로 되돌린다 — 그러면 첫 pane 라벨을 따른다.
            self.tabs[index].name = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        self.rename = None;
        self.persist_layout(cx);
        cx.notify();
    }

    /// pane 라벨 변경 (트리 내 leaf).
    pub fn rename_pane(&mut self, session: &SessionId, label: &str, cx: &mut Context<Self>) {
        let Some(tab_id) = self.tab_of_pane(session) else {
            return;
        };
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let root = std::mem::replace(
            &mut self.tabs[index].root,
            PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
        );
        self.tabs[index].root = rename_leaf(root, session, label);
        self.persist_layout(cx);
        cx.notify();
    }

    /// pane 헤더 더블클릭 — 인라인 라벨 편집. 탭 이름 편집과 같은 수명 규칙을
    /// 쓴다(Submit이면 반영, Blur면 취소).
    pub fn start_pane_rename(
        &mut self,
        session: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(label) = self
            .tabs
            .iter()
            .flat_map(|tab| leaves(&tab.root))
            .find(|leaf| leaf.session_id == session)
            .map(|leaf| leaf.label.clone())
        else {
            return;
        };
        let input = cx.new(|cx| TextInput::new(window, cx).with_text(label));
        let editing = session.clone();
        let sub = cx.subscribe(&input, move |this: &mut Self, input, event, cx| {
            use crate::ui::InputEvent;
            match event {
                InputEvent::Submitted => {
                    let text = input.read(cx).text().to_string();
                    this.commit_pane_rename(&editing, &text, cx);
                }
                InputEvent::Blurred => {
                    this.pane_rename = None;
                    cx.notify();
                }
                InputEvent::Changed => {}
            }
        });
        self._subscriptions.push(sub);
        window.focus(&input.read(cx).focus_handle(cx));
        self.pane_rename = Some((session, input));
        cx.notify();
    }

    fn commit_pane_rename(&mut self, session: &SessionId, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim();
        // 빈 라벨은 탭 제목(`tab.name ?? 첫 leaf.label`)까지 비워버리므로 무시한다.
        if !trimmed.is_empty() {
            self.rename_pane(session, trimmed, cx);
        }
        self.pane_rename = None;
        cx.notify();
    }
}
