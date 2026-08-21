//! 유일한 alacritty_terminal import 지점 (seam — DESIGN-terminal.md §1)
pub use alacritty_terminal::event::{Event as AlacEvent, EventListener, WindowSize};
pub use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
pub use alacritty_terminal::grid::Dimensions;
pub use alacritty_terminal::index::{Column, Line, Point as AlacPoint};
pub use alacritty_terminal::sync::FairMutex;
pub use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
pub use alacritty_terminal::tty;
