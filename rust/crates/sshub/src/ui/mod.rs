//! 손으로 만든 GPUI 위젯 킷 (DESIGN-ui.md §2).
//!
//! 상태 없는 리프 컨트롤은 `RenderOnce`, 포커스/편집 상태를 가지는 것은 Entity 뷰.
pub mod button;
pub mod checkbox;
pub mod context_menu;
pub mod form;
pub mod icon;
pub mod list;
pub mod modal;
pub mod select;
pub mod text_area;
pub mod text_input;
pub mod toast;

pub use button::{Button, ButtonVariant};
pub use checkbox::Checkbox;
pub use context_menu::{ContextMenu, ContextMenuItem};
pub use form::FormField;
pub use icon::{icon, Icon};
pub use list::ListItem;
pub use modal::{ConfirmDialog, ModalOverlay};
pub use select::{Select, SelectEvent, SelectOption};
pub use text_area::TextArea;
pub use text_input::{InputEvent, TextInput};
pub use toast::{Toast, ToastKind};

use gpui::{App, KeyBinding};

/// 위젯 킷의 **모든** 키 바인딩을 등록한다.
///
/// gpui의 `App::clear_key_bindings()`는 전역 키맵을 통째로 비우므로,
/// 사용자 단축키 리바인딩은 `clear_key_bindings()` → `ui::init(cx)` →
/// 사용자 바인딩 재등록 순서로 진행해야 한다. 즉 **미래의 `keymap.rs`가
/// 이 함수를 호출**하며, 위젯 바인딩이 여기 한 곳에만 존재해야 리바인드 후에도
/// 살아남는다. 위젯 모듈에서 개별적으로 `bind_keys`를 부르지 말 것.
pub fn init(cx: &mut App) {
    use context_menu::{
        ContextMenuCancel, ContextMenuConfirm, ContextMenuDown, ContextMenuUp,
    };
    use modal::{ConfirmDialogCancel, ConfirmDialogConfirm};
    use select::{SelectCancel, SelectConfirm, SelectDown, SelectFirst, SelectLast, SelectUp};
    use text_area::{
        AreaBackspace, AreaCopy, AreaCut, AreaDelete, AreaDown, AreaEnd, AreaEscape, AreaHome,
        AreaLeft, AreaNewline, AreaPaste, AreaRight, AreaSelectAll, AreaSelectDown, AreaSelectLeft,
        AreaSelectRight, AreaSelectUp, AreaUp,
    };
    use text_input::{
        Backspace, Copy, Cut, Delete, End, Enter, Escape, Home, Left, Paste, Right, SelectAll,
        SelectLeft, SelectRight, SelectToEnd, SelectToHome, ShowCharacterPalette,
    };

    const TEXT_INPUT: Option<&str> = Some("TextInput");
    const TEXT_AREA: Option<&str> = Some("TextArea");
    const SELECT: Option<&str> = Some("Select");
    const CONFIRM: Option<&str> = Some("ConfirmDialog");
    const CONTEXT_MENU: Option<&str> = Some("ContextMenu");

    cx.bind_keys([
        // --- TextInput ---
        KeyBinding::new("backspace", Backspace, TEXT_INPUT),
        KeyBinding::new("delete", Delete, TEXT_INPUT),
        KeyBinding::new("left", Left, TEXT_INPUT),
        KeyBinding::new("right", Right, TEXT_INPUT),
        KeyBinding::new("shift-left", SelectLeft, TEXT_INPUT),
        KeyBinding::new("shift-right", SelectRight, TEXT_INPUT),
        KeyBinding::new("home", Home, TEXT_INPUT),
        KeyBinding::new("end", End, TEXT_INPUT),
        KeyBinding::new("cmd-left", Home, TEXT_INPUT),
        KeyBinding::new("cmd-right", End, TEXT_INPUT),
        KeyBinding::new("shift-home", SelectToHome, TEXT_INPUT),
        KeyBinding::new("shift-end", SelectToEnd, TEXT_INPUT),
        KeyBinding::new("cmd-shift-left", SelectToHome, TEXT_INPUT),
        KeyBinding::new("cmd-shift-right", SelectToEnd, TEXT_INPUT),
        KeyBinding::new("cmd-a", SelectAll, TEXT_INPUT),
        KeyBinding::new("cmd-c", Copy, TEXT_INPUT),
        KeyBinding::new("cmd-x", Cut, TEXT_INPUT),
        KeyBinding::new("cmd-v", Paste, TEXT_INPUT),
        KeyBinding::new("enter", Enter, TEXT_INPUT),
        KeyBinding::new("escape", Escape, TEXT_INPUT),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, TEXT_INPUT),
        // --- TextArea ---
        KeyBinding::new("backspace", AreaBackspace, TEXT_AREA),
        KeyBinding::new("delete", AreaDelete, TEXT_AREA),
        KeyBinding::new("left", AreaLeft, TEXT_AREA),
        KeyBinding::new("right", AreaRight, TEXT_AREA),
        KeyBinding::new("up", AreaUp, TEXT_AREA),
        KeyBinding::new("down", AreaDown, TEXT_AREA),
        KeyBinding::new("shift-left", AreaSelectLeft, TEXT_AREA),
        KeyBinding::new("shift-right", AreaSelectRight, TEXT_AREA),
        KeyBinding::new("shift-up", AreaSelectUp, TEXT_AREA),
        KeyBinding::new("shift-down", AreaSelectDown, TEXT_AREA),
        KeyBinding::new("home", AreaHome, TEXT_AREA),
        KeyBinding::new("end", AreaEnd, TEXT_AREA),
        KeyBinding::new("cmd-a", AreaSelectAll, TEXT_AREA),
        KeyBinding::new("cmd-c", AreaCopy, TEXT_AREA),
        KeyBinding::new("cmd-x", AreaCut, TEXT_AREA),
        KeyBinding::new("cmd-v", AreaPaste, TEXT_AREA),
        KeyBinding::new("enter", AreaNewline, TEXT_AREA),
        KeyBinding::new("escape", AreaEscape, TEXT_AREA),
        // --- Select ---
        KeyBinding::new("up", SelectUp, SELECT),
        KeyBinding::new("down", SelectDown, SELECT),
        KeyBinding::new("home", SelectFirst, SELECT),
        KeyBinding::new("end", SelectLast, SELECT),
        KeyBinding::new("enter", SelectConfirm, SELECT),
        KeyBinding::new("space", SelectConfirm, SELECT),
        KeyBinding::new("escape", SelectCancel, SELECT),
        // --- ConfirmDialog ---
        KeyBinding::new("enter", ConfirmDialogConfirm, CONFIRM),
        KeyBinding::new("escape", ConfirmDialogCancel, CONFIRM),
        // --- ContextMenu ---
        KeyBinding::new("up", ContextMenuUp, CONTEXT_MENU),
        KeyBinding::new("down", ContextMenuDown, CONTEXT_MENU),
        KeyBinding::new("enter", ContextMenuConfirm, CONTEXT_MENU),
        KeyBinding::new("escape", ContextMenuCancel, CONTEXT_MENU),
    ]);
}
