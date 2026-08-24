//! 터미널 워크스페이스 인수 아티팩트 — 탭바 + 분할 + 세션 레지스트리.
//!
//! ```bash
//! cargo run -p sshub --example terminal_workspace_demo
//! ```
//!
//! 확인 항목:
//! - `+` 또는 Cmd+T 로 새 로컬 탭, 탭 더블클릭으로 이름 변경, `×`로 닫기
//!   (pane이 여럿이거나 서버 세션이면 확인 모달)
//! - Cmd+D / Cmd+Shift+D 분할, 디바이더 드래그로 크기 조절
//! - Opt+Cmd+방향키로 이웃 pane 포커스 이동, Cmd+Shift+I 동시 입력
//! - Cmd+Shift+= / Cmd+Shift+- 폰트 크기
//! - 탭을 끌어 순서 변경 / pane 우상단 그립을 끌어 이동·분리
//! - 종료 후 다시 실행하면 탭 구성과 스크롤백이 복원된다.

use gpui::{
    div, px, size, App, AppContext, Application, Bounds, Context, Entity, Focusable, IntoElement,
    ParentElement, Render, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use sshub::terminal_workspace::TerminalWorkspace;
use sshub::{keymap, state, theme, ui};

struct DemoRoot {
    workspace: Entity<TerminalWorkspace>,
}

impl Render for DemoRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(theme::theme(cx).bg)
            .child(self.workspace.clone())
    }
}

/// 데모는 사용자의 실제 데이터를 건드리지 않는다 — 특히 Phase 2 이후로는
/// 서버를 하나 만들기만 해도 진짜 `~/.ssh/config`가 수정되기 때문이다.
fn init_sandboxed_state(cx: &mut gpui::App) -> gpui::Entity<state::AppState> {
    let dir = std::env::temp_dir().join("sshub-demo");
    let _ = std::fs::create_dir_all(&dir);
    state::init_with_paths(sshub_core::AppPaths::in_dir(dir), cx)
}

fn main() {
    Application::new().run(|cx: &mut App| {
        theme::init(cx);
        let app_state = init_sandboxed_state(cx);
        // 위젯 바인딩 → 앱 단축키 순서를 지켜야 리바인드 후에도 둘 다 산다.
        ui::init(cx);
        let shortcuts = app_state.read(cx).settings.shortcuts.clone();
        keymap::register_all(cx, &shortcuts);

        let bounds = Bounds::centered(None, size(px(1000.0), px(680.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("sshub terminal workspace demo".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| DemoRoot {
                        workspace: cx.new(|cx| TerminalWorkspace::new(window, cx)),
                    })
                },
            )
            .expect("failed to open the demo window");

        window
            .update(cx, |root, window, cx| {
                let handle = root.workspace.read(cx).focus_handle(cx);
                window.focus(&handle);
                cx.activate(true);
            })
            .ok();
    });
}
