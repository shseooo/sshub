//! Phase 0 스파이크: upstream alacritty_terminal 0.26이 우리 아키텍처(DESIGN-terminal.md §1,3)
//! 대로 동작하는지 헤드리스로 검증 — PTY spawn → EventLoop 펌프 → grid 스냅샷.
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event as AlacEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;
use alacritty_terminal::event::Notify;

#[derive(Clone)]
struct ChannelListener(mpsc::Sender<AlacEvent>);

impl EventListener for ChannelListener {
    fn send_event(&self, event: AlacEvent) {
        let _ = self.0.send(event);
    }
}

struct Size {
    lines: usize,
    cols: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

fn grid_text<T: EventListener>(term: &Term<T>) -> String {
    let mut out = String::new();
    let content = term.renderable_content();
    let mut last_line = None;
    for cell in content.display_iter {
        if last_line != Some(cell.point.line) {
            if last_line.is_some() {
                out.push('\n');
            }
            last_line = Some(cell.point.line);
        }
        out.push(cell.c);
    }
    out
}

#[test]
fn spawns_shell_and_reads_output() {
    let (tx, _rx) = mpsc::channel();
    let listener = ChannelListener(tx);

    let window_size = WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 };
    let mut env = HashMap::new();
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    let options = tty::Options {
        shell: Some(tty::Shell::new("/bin/zsh".to_string(), vec!["-f".to_string()])),
        working_directory: None,
        drain_on_exit: false,
        env,
    };
    let pty = tty::new(&options, window_size, 0).expect("pty spawn");

    let term_config = TermConfig { scrolling_history: 1000, ..Default::default() };
    let size = Size { lines: 24, cols: 80 };
    let term = Arc::new(FairMutex::new(Term::new(term_config, &size, listener.clone())));

    let event_loop =
        EventLoop::new(Arc::clone(&term), listener, pty, false, false).expect("event loop");
    let sender = event_loop.channel();
    let _join = event_loop.spawn();

    let notifier = Notifier(sender.clone());
    notifier.notify(b"echo sshub_spike_$((40+2))\r".to_vec());

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found = false;
    while Instant::now() < deadline {
        {
            let term = term.lock();
            if grid_text(&term).contains("sshub_spike_42") {
                found = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = sender.send(Msg::Shutdown);
    assert!(found, "shell output did not appear in the grid within 10s");
}
