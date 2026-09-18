//! 터미널 컨텍스트 메뉴 (pane·탭 우클릭).
//!
//! `terminal_workspace`의 **자식 모듈**이라 부모 타입의 비공개 필드에 그대로
//! 접근한다 — 메뉴를 떼어 내려고 워크스페이스 내부를 공개할 이유가 없다.
//!
//! 여기서는 새 상태를 만들지 않는다. 항목은 전부 워크스페이스가 이미 가진
//! 동작을 부르기만 한다.

use gpui::{App, AppContext as _, Context, DismissEvent, Pixels, Point, SharedString, Window};
use sshub_core::agent_sessions::{self, AgentGroup};
use sshub_core::favorite_paths;
use sshub_splits::{leaves, SessionId, SplitDirection, TabId};

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
            |this: &mut Self, menu, _: &DismissEvent, window, cx| {
                // 내가 연 메뉴만 지운다 — 항목 동작이 이미 다음 메뉴를 열어
                // 두었을 수 있다(에이전트 세션 목록처럼 비동기로 뜨는 2단 메뉴).
                if this.menu.as_ref() != Some(menu) {
                    return;
                }
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
                // 서버 세션(원격 셸)에는 로컬 세션 파일이 없다 — 항목은 남기고
                // 비활성으로 둔다(위치 고정).
                let local = self
                    .tabs
                    .iter()
                    .flat_map(|t| leaves(&t.root))
                    .find(|l| l.session_id == session)
                    .is_some_and(|l| l.server_id.is_none());
                ContextMenuItem::entry(tr(lang, TrKey::TermAgentSessions), move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.open_agent_sessions_menu(session.clone(), at, window, cx)
                    })
                    .ok();
                })
                .disabled(!local)
            },
            {
                let this = this.clone();
                let session = session.clone();
                // 원격 pane에서도 켜 둔다 — `~/…`처럼 양쪽에 있는 폴더를 즐겨찾기로
                // 두는 쓰임이 있고, 없는 폴더면 셸이 알려 준다.
                ContextMenuItem::entry(tr(lang, TrKey::TermFavoritePaths), move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.open_favorite_paths_menu(session.clone(), at, window, cx)
                    })
                    .ok();
                })
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

/// 에이전트별로 메뉴에 보여 줄 최대 세션 수.
const AGENT_SESSIONS_PER_AGENT: usize = 8;

impl TerminalWorkspace {
    /// pane 메뉴의 "코딩 에이전트 세션…" — 그 pane의 **현재 디렉터리**에서 열었던
    /// 세션을 백그라운드에서 읽어 2단 메뉴로 띄운다. 세션 파일은 수 MB라 메인
    /// 스레드에서 읽지 않는다.
    pub fn open_agent_sessions_menu(
        &mut self,
        session: SessionId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cwd) = self
            .registry
            .update(cx, |registry, cx| registry.live_local_cwd(&session, cx))
        else {
            return;
        };
        let Some(env) = agent_sessions::Env::from_process() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let groups = cx
                .background_spawn(async move {
                    agent_sessions::list_all(&env, &cwd, AGENT_SESSIONS_PER_AGENT)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.show_agent_sessions_menu(session, at, groups, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn show_agent_sessions_menu(
        &mut self,
        session: SessionId,
        at: Point<Pixels>,
        groups: Vec<AgentGroup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lang = self.lang;
        let this = cx.entity().downgrade();
        let mut items: Vec<ContextMenuItem> = Vec::new();

        if groups.is_empty() {
            items.push(
                ContextMenuItem::entry(tr(lang, TrKey::TermAgentNoneInstalled), |_, _| {})
                    .disabled(true),
            );
        }
        for (i, group) in groups.into_iter().enumerate() {
            if i > 0 {
                items.push(ContextMenuItem::separator());
            }
            // 에이전트 이름은 헤더 — 고를 수 없는 항목으로 둔다.
            items.push(ContextMenuItem::entry(group.display_name, |_, _| {}).disabled(true));
            if group.sessions.is_empty() {
                items.push(
                    ContextMenuItem::entry(tr(lang, TrKey::TermAgentNoSessions), |_, _| {})
                        .disabled(true),
                );
            }
            for s in group.sessions {
                // 명령줄은 코어가 만든다 — id 새니타이즈도 그쪽에서 한다.
                let Some(command) = agent_sessions::resume_command(s.agent, &s.id) else {
                    continue;
                };
                let (this, session) = (this.clone(), session.clone());
                items.push(
                    ContextMenuItem::entry(s.title, move |_window, cx| {
                        Self::run_in_pane(&this, &session, &command, cx);
                    })
                    .hint(Some(SharedString::from(agent_sessions::format_updated(s.updated_at)))),
                );
            }
            let (this, session) = (this.clone(), session.clone());
            let command = group.new_session_command;
            items.push(ContextMenuItem::entry(
                tr(lang, TrKey::TermAgentNewSession),
                move |_window, cx| {
                    Self::run_in_pane(&this, &session, &command, cx);
                },
            ));
        }
        self.open_menu(at, items, window, cx);
    }

    /// pane 메뉴의 "즐겨찾기 경로…" — 설정에 저장된 폴더 목록을 2단 메뉴로 띄우고,
    /// 고르면 `cd <경로>` + Enter를 그 pane으로 보낸다. 목록이 비면 설정으로
    /// 안내하는 비활성 항목 하나만 보인다(메뉴가 통째로 안 뜨면 고장으로 보인다).
    pub fn open_favorite_paths_menu(
        &mut self,
        session: SessionId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lang = self.lang;
        let this = cx.entity().downgrade();
        let paths = self.state.read(cx).settings.favorite_paths.clone();
        let mut items: Vec<ContextMenuItem> = Vec::new();
        if paths.is_empty() {
            items.push(
                ContextMenuItem::entry(tr(lang, TrKey::TermFavoritePathsEmpty), |_, _| {})
                    .disabled(true),
            );
        }
        for path in paths {
            // 인용은 코어가 한다 — 설정 파일 값도 셸에 그대로 넣지 않는다.
            let Some(command) = favorite_paths::cd_command(&path) else {
                continue;
            };
            let (this, session) = (this.clone(), session.clone());
            items.push(ContextMenuItem::entry(path, move |_window, cx| {
                Self::run_in_pane(&this, &session, &command, cx);
            }));
        }
        self.open_menu(at, items, window, cx);
    }

    /// 명령줄 + Enter를 pane의 PTY로 보낸다. 터미널 핸들만 먼저 꺼내고
    /// 워크스페이스 대여를 끝낸 뒤 쓴다(메뉴 항목의 다른 동작과 같은 규칙).
    fn run_in_pane(
        this: &gpui::WeakEntity<Self>,
        session: &SessionId,
        command: &str,
        cx: &mut App,
    ) {
        let terminal = this
            .update(cx, |this, cx| this.registry.read(cx).get(session))
            .ok()
            .flatten();
        if let Some(terminal) = terminal {
            let mut bytes = command.as_bytes().to_vec();
            bytes.push(b'\r');
            terminal.update(cx, |terminal, _| terminal.input(bytes));
        }
    }
}
