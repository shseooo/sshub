//! 탭·pane 닫기와 그 확인 대화상자.
//!
//! `terminal_workspace`의 **자식 모듈**이라 부모 타입의 비공개 필드에 그대로
//! 접근한다.
//!
//! 닫기는 되돌릴 수 없다 — PTY가 죽고 스크롤백까지 지워진다. 그래서 "무엇을
//! 닫을지"([`PendingClose`])와 "정말 닫을지"(확인 대화상자)를 갈라 두고,
//! 위험한 경우에만 묻는다([`super::is_risky_close`]).

use gpui::{AppContext as _, Context, Window};
use sshub_splits::{
    leaves, remove_leaf, tabs_except, tabs_from_inclusive, tabs_up_to_inclusive, PaneNode,
    SessionId, TabId, TerminalLeaf,
};

use super::{is_risky_close, TerminalWorkspace};
use crate::i18n::{tr, TrKey};
use crate::ui::ConfirmDialog;

/// 확인이 필요한 닫기 동작.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PendingClose {
    Pane(SessionId),
    Tab(TabId),
    Others(TabId),
    Right(TabId),
    Left(TabId),
}

impl TerminalWorkspace {
    /// 포커스된 pane 닫기 — 위험하면(여러 pane 또는 서버 세션) 확인 모달.
    pub fn close_focused_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.focused_pane.clone() else {
            return;
        };
        let risky = self
            .tab_of_pane(&session)
            .and_then(|tab| self.tab_index(&tab))
            .is_some_and(|i| is_risky_close(&self.tabs[i].root));
        if risky {
            self.ask(PendingClose::Pane(session), TrKey::TermConfirmCloseTab, window, cx);
        } else {
            self.apply_close(PendingClose::Pane(session), window, cx);
        }
    }

    pub fn close_tab(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let risky = self
            .tab_index(&tab_id)
            .is_some_and(|i| is_risky_close(&self.tabs[i].root));
        if risky {
            self.ask(PendingClose::Tab(tab_id), TrKey::TermConfirmCloseTab, window, cx);
        } else {
            self.apply_close(PendingClose::Tab(tab_id), window, cx);
        }
    }

    pub fn close_other_tabs(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.ask(PendingClose::Others(tab_id), TrKey::TermConfirmCloseOthers, window, cx);
        }
    }

    pub fn close_tabs_to_the_right(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let has_right = self
            .tab_index(&tab_id)
            .is_some_and(|i| i + 1 < self.tabs.len());
        if has_right {
            self.ask(PendingClose::Right(tab_id), TrKey::TermConfirmCloseRight, window, cx);
        }
    }

    /// 왼쪽 탭 전부 닫기 — 오른쪽 형제와 같은 확인 절차를 탄다.
    pub fn close_tabs_to_the_left(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let has_left = self.tab_index(&tab_id).is_some_and(|i| i > 0);
        if has_left {
            // i18n에 "왼쪽 탭을 모두 닫을까요?" 문구가 없어 항목 라벨을 그대로
            // 확인 문구로 쓴다 (새 사용자 문자열을 만들지 않기 위해).
            self.ask(PendingClose::Left(tab_id), TrKey::TermCloseLeft, window, cx);
        }
    }

    fn ask(
        &mut self,
        pending: PendingClose,
        message: TrKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                tr(self.lang, TrKey::TermConfirmCloseTitle),
                tr(self.lang, message),
                tr(self.lang, TrKey::TermConfirmCloseAction),
                tr(self.lang, TrKey::CommonCancel),
                cx,
            )
            .danger(true)
            .on_result(move |confirmed, window, cx| {
                this.update(cx, |this, cx| {
                    this.confirm = None;
                    if confirmed {
                        this.apply_close(pending, window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
        });
        dialog.read(cx).focus(window);
        self.confirm = Some(dialog);
        cx.notify();
    }

    fn apply_close(&mut self, pending: PendingClose, window: &mut Window, cx: &mut Context<Self>) {
        match pending {
            PendingClose::Pane(session) => {
                let Some(tab_id) = self.tab_of_pane(&session) else {
                    return;
                };
                let Some(index) = self.tab_index(&tab_id) else {
                    return;
                };
                let root = std::mem::replace(
                    &mut self.tabs[index].root,
                    PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
                );
                match remove_leaf(root, &session) {
                    Some(root) => {
                        self.tabs[index].root = root;
                        // 남은 pane 중 첫 번째로 포커스를 옮긴다.
                        self.focused_pane = leaves(&self.tabs[index].root)
                            .first()
                            .map(|l| l.session_id.clone());
                    }
                    None => self.remove_tab_at(index),
                }
            }
            PendingClose::Tab(tab_id) => {
                if let Some(index) = self.tab_index(&tab_id) {
                    self.remove_tab_at(index);
                }
            }
            PendingClose::Others(tab_id) => {
                self.tabs = tabs_except(std::mem::take(&mut self.tabs), &tab_id);
                self.active_tab = Some(tab_id);
            }
            PendingClose::Right(tab_id) => {
                self.tabs = tabs_up_to_inclusive(std::mem::take(&mut self.tabs), &tab_id);
                if self.active_index().is_none() {
                    self.active_tab = self.tabs.last().map(|t| t.id.clone());
                }
            }
            PendingClose::Left(tab_id) => {
                self.tabs = tabs_from_inclusive(std::mem::take(&mut self.tabs), &tab_id);
                if self.active_index().is_none() {
                    self.active_tab = self.tabs.first().map(|t| t.id.clone());
                }
            }
        }
        // 여러 탭을 한 번에 닫으면 포커스가 사라진 세션을 가리킬 수 있다.
        // 비워 두면 `focus_active_pane`이 활성 탭의 첫 pane으로 되돌린다.
        if self
            .focused_pane
            .as_ref()
            .is_some_and(|s| self.tab_of_pane(s).is_none())
        {
            self.focused_pane = None;
        }
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 탭 제거 + active 폴백(= 마지막 탭). TS `closeTab`과 동일.
    fn remove_tab_at(&mut self, index: usize) {
        let removed = self.tabs.remove(index);
        self.broadcast_tabs.remove(&removed.id);
        if self.active_tab.as_ref() == Some(&removed.id) {
            self.active_tab = self.tabs.last().map(|t| t.id.clone());
            self.focused_pane = self
                .active_index()
                .and_then(|i| leaves(&self.tabs[i].root).first().map(|l| l.session_id.clone()));
        }
    }
}
