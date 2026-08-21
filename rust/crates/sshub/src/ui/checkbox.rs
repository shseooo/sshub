//! Checkbox — 상태 없는 리프 컨트롤. 체크 상태는 부모가 소유한다.
use crate::theme::theme;
use crate::ui::icon::Icon;
use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window,
};

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_toggle: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            label: None,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 콜백은 *토글 후* 값을 받는다.
    pub fn on_toggle(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx).clone();
        let checked = self.checked;
        let disabled = self.disabled;

        let box_bg = if checked { t.accent } else { t.elevated };
        let box_border = if checked { t.accent } else { t.border };
        let check_color = if disabled { t.text_disabled } else { t.bg };

        let boxel = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded(px(4.))
            .border_1()
            .border_color(box_border)
            .bg(box_bg)
            .text_size(px(11.))
            .text_color(check_color)
            .when(checked, |el| el.child(Icon::Check.glyph()));

        let mut row = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(boxel);

        if let Some(label) = self.label {
            row = row.child(
                div()
                    .text_size(px(13.))
                    .text_color(if disabled { t.text_disabled } else { t.text })
                    .child(label),
            );
        }

        if disabled {
            row.opacity(0.6)
        } else {
            let row = row.cursor_pointer();
            match self.on_toggle {
                Some(handler) => row.on_click(move |_: &ClickEvent, window, cx| {
                    handler(&!checked, window, cx);
                }),
                None => row,
            }
        }
    }
}
