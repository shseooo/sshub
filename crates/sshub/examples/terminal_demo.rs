//! 터미널 엘리먼트 인수 아티팩트 — 창 하나에 로컬 셸 터미널 하나.
//!
//! ```bash
//! cargo run -p sshub --example terminal_demo
//! ```
//!
//! 확인 항목: 에코/색/커서/리사이즈/휠/선택/복사(cmd-c)/붙여넣기(cmd-v)/
//! cmd+클릭 링크, 그리고 **한글 IME 조합**(조합 중에는 밑줄 오버레이, 확정 시에만
//! PTY로 전송).

use gpui::{
    div, px, size, App, AppContext, Application, Bounds, Context, Focusable, IntoElement,
    ParentElement, Render, Styled,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use sshub::terminal_view::TerminalView;
use sshub::theme::Theme;
use sshub_terminal::{SpawnSpec, TerminalBounds};

struct DemoRoot {
    terminal: gpui::Entity<TerminalView>,
}

impl Render for DemoRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = sshub::theme::theme(cx);
        div()
            .size_full()
            .bg(theme.bg)
            .child(div().size_full().child(self.terminal.clone()))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.set_global(Theme::default_dark());

        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("sshub terminal demo".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_window, cx| {
                    cx.new(|cx| {
                        let mut spec = SpawnSpec::local_shell(std::env::current_dir().ok());
                        spec.initial_bounds = TerminalBounds::default();
                        let terminal = cx
                            .new(|cx| {
                                TerminalView::new(spec, cx).expect("failed to spawn the shell")
                            });
                        DemoRoot { terminal }
                    })
                },
            )
            .expect("failed to open the demo window");

        // 창이 뜨자마자 키 입력을 받도록 터미널에 포커스를 준다.
        window
            .update(cx, |root, window, cx| {
                let handle = root.terminal.read(cx).focus_handle(cx);
                window.focus(&handle);
                cx.activate(true);
            })
            .ok();
    });
}
