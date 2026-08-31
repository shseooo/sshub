//! 터미널 컨텍스트 메뉴 (pane·탭 우클릭).
//!
//! `terminal_workspace`의 **자식 모듈**이라 부모 타입의 비공개 필드에 그대로
//! 접근한다 — 메뉴를 떼어 내려고 워크스페이스 내부를 공개할 이유가 없다.
//!
//! 여기서는 새 상태를 만들지 않는다. 항목은 전부 워크스페이스가 이미 가진
//! 동작을 부르기만 한다.

use gpui::{App, AppContext as _, Context, DismissEvent, Pixels, Point, SharedString, Window};
use sshub_splits::{SessionId, SplitDirection, TabId};

use super::TerminalWorkspace;
use crate::i18n::{tr, TrKey};
use crate::keymap::display_combo;
use crate::ui::{ContextMenu, ContextMenuItem};
use crate::workspace::MoveTabToNewWindow;

impl TerminalWorkspace {
    /// 사용자가 지정한 단축키의 표시 라벨. 미지정이면 기본값으로 떨어진다
    /// (`keymap::register_all`이 실제로 등록하는 값과 같은 규칙).
    fn shortcut_hint(&self, action: &str, cx: &App) -> Option<SharedString> {
        let settings = &self.state.read(cx).settings;
        let defaults = sshub_core::settings::default_shortcuts();
        let combo = settings
            .shortcuts
            .get(action)
            .filter(|c| crate::keymap::is_valid_combo(c))
            .or_else(|| defaults.get(action))?;
        Some(SharedString::from(display_combo(combo)))
    }

    fn open_menu(
        &mut self,
        at: Point<Pixels>,
        items: Vec<ContextMenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| ContextMenu::new(at, items, cx));
        let dismiss = cx.subscribe_in(
            &menu,
            window,
            |this: &mut Self, _menu, _: &DismissEvent, window, cx| {
                this.menu = None;
                // 항목 동작이 확인 모달을 띄웠다면 포커스를 되찾아 오면 안 된다
                // — 방금 연 모달에서 포커스를 빼앗는 꼴이 된다.
                if this.confirm.is_none() {
                    this.focus_active_pane(window, cx);
                }
                cx.notify();
            },
        );
        menu.read(cx).focus(window);
        self.menu_sub = Some(dismiss);
        self.menu = Some(menu);
        cx.notify();
    }

    /// pane 우클릭 메뉴. 복사/붙여넣기는 **우클릭한 pane**을 대상으로 해야 하므로
    /// 먼저 그 pane에 포커스를 준다.
    pub fn open_pane_menu(
        &mut self,
        session: SessionId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane(session.clone(), window, cx);
        let lang = self.lang;
        let this = cx.entity().downgrade();
        // ⌘C/⌘V는 터미널 뷰가 직접 처리하는 고정 키다(리바인딩 대상 아님).
        let copy_hint = Some(SharedString::from(display_combo("cmd-c")));
        let paste_hint = Some(SharedString::from(display_combo("cmd-v")));
        let split_right_hint = self.shortcut_hint("splitRight", cx);
        let split_down_hint = self.shortcut_hint("splitDown", cx);
        let close_hint = self.shortcut_hint("closePane", cx);

        let items = vec![
            {
                let this = this.clone();
                ContextMenuItem::entry(tr(lang, TrKey::ShortcutSplitRight), move |window, cx| {
                    this.update(cx, |this, cx| this.split(SplitDirection::Row, window, cx)).ok();
                })
                .hint(split_right_hint)
            },
            {
                let this = this.clone();
                ContextMenuItem::entry(tr(lang, TrKey::ShortcutSplitDown), move |window, cx| {
                    this.update(cx, |this, cx| this.split(SplitDirection::Column, window, cx)).ok();
                })
                .hint(split_down_hint)
            },
            ContextMenuItem::separator(),
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermCopy), move |_window, cx| {
                    // 뷰 핸들만 먼저 꺼내고 워크스페이스 대여를 **끝낸 뒤** 호출한다
                    // — 뷰 쪽에서 워크스페이스를 다시 건드릴 수 있기 때문(브로드캐스트).
                    let view = this
                        .update(cx, |this, _| this.views.get(&session).cloned())
                        .ok()
                        .flatten();
                    if let Some(view) = view {
                        view.update(cx, |view, cx| view.copy(cx));
                    }
                })
                .hint(copy_hint)
            },
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermPaste), move |_window, cx| {
                    // 뷰 핸들만 먼저 꺼내고 워크스페이스 대여를 **끝낸 뒤** 호출한다
                    // — 뷰 쪽에서 워크스페이스를 다시 건드릴 수 있기 때문(브로드캐스트).
                    let view = this
                        .update(cx, |this, _| this.views.get(&session).cloned())
                        .ok()
                        .flatten();
                    if let Some(view) = view {
                        view.update(cx, |view, cx| view.paste(cx));
                    }
                })
                .hint(paste_hint)
            },
            ContextMenuItem::separator(),
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermReconnect), move |window, cx| {
                    this.update(cx, |this, cx| this.reconnect_pane(session.clone(), window, cx)).ok();
                })
            },
            ContextMenuItem::entry(tr(lang, TrKey::ShortcutClosePane), move |window, cx| {
                this.update(cx, |this, cx| this.close_focused_pane(window, cx)).ok();
            })
            .hint(close_hint),
        ];
        self.open_menu(at, items, window, cx);
    }

    /// 탭 우클릭 메뉴. **활성 탭을 바꾸지 않는다** — 우클릭한 탭이 대상이다.
    /// 아무 일도 하지 않을 항목(끝 탭의 "오른쪽 닫기" 등)은 지우지 않고 비활성으로
    /// 남긴다 — 항목 위치가 흔들리면 근육 기억이 깨진다.
    pub fn open_tab_menu(
        &mut self,
        tab_id: TabId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let lang = self.lang;
        let last = self.tabs.len() - 1;
        let only_tab = self.tabs.len() <= 1;
        let this = cx.entity().downgrade();

        let items = vec![
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseTab), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tab(id.clone(), window, cx)).ok();
                })
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseOthers), move |window, cx| {
                    this.update(cx, |this, cx| this.close_other_tabs(id.clone(), window, cx)).ok();
                })
                .disabled(only_tab)
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseRight), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tabs_to_the_right(id.clone(), window, cx))
                        .ok();
                })
                .disabled(index == last)
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseLeft), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tabs_to_the_left(id.clone(), window, cx))
                        .ok();
                })
                .disabled(index == 0)
            },
            ContextMenuItem::separator(),
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermMoveToNewWindow), move |window, cx| {
                    // 이동 대상은 **활성 탭**이므로(창 셸의 액션) 먼저 활성화한다.
                    this.update(cx, |this, cx| {
                        this.active_tab = Some(id.clone());
                        this.focus_active_pane(window, cx);
                        cx.notify();
                    })
                    .ok();
                    // 액션은 포커스 경로로 올라가 창 셸이 받는다. 메뉴 dismiss가
                    // 같은 프레임에 포커스를 되돌리므로 다음 프레임까지 미룬다.
                    window.dispatch_action(Box::new(MoveTabToNewWindow), cx);
                })
                .disabled(only_tab)
            },
            ContextMenuItem::entry(tr(lang, TrKey::TermReconnect), move |window, cx| {
                this.update(cx, |this, cx| this.reconnect_tab(tab_id.clone(), window, cx)).ok();
            }),
        ];
        self.open_menu(at, items, window, cx);
    }
}
