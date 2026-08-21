//! Button — 상태 없는 리프 컨트롤 (DESIGN-ui.md §2).
use crate::theme::{theme, with_alpha};
use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    disabled: bool,
    loading: bool,
    full_width: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::Secondary,
            disabled: false,
            loading: false,
            full_width: false,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// loading 중에는 클릭이 차단되고 라벨 앞에 스피너 글리프가 붙는다.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx).clone();
        let inert = self.disabled || self.loading;

        // (bg, hover_bg, fg, border)
        let (bg, hover_bg, fg, border) = match self.variant {
            ButtonVariant::Primary => (t.accent, with_alpha(t.accent, 0.85), t.bg, t.accent),
            ButtonVariant::Secondary => (t.elevated, t.hover, t.text, t.border),
            ButtonVariant::Ghost => (with_alpha(t.hover, 0.0), t.hover, t.text_muted, {
                let mut c = t.border;
                c.a = 0.0;
                c
            }),
            ButtonVariant::Danger => (t.danger, with_alpha(t.danger, 0.85), t.bg, t.danger),
        };
        let fg = if inert { t.text_disabled } else { fg };

        let label = if self.loading {
            SharedString::from(format!("{} {}", crate::ui::icon::Icon::Spinner.glyph(), self.label))
        } else {
            self.label.clone()
        };

        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(px(30.))
            .px(px(12.))
            .rounded(px(6.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_size(px(13.))
            .text_color(fg)
            .child(label);

        if self.full_width {
            el = el.w_full().flex_1();
        }

        if inert {
            el.opacity(0.6)
        } else {
            let el = el.cursor_pointer().hover(move |s| s.bg(hover_bg));
            match self.on_click {
                Some(handler) => el.on_click(move |ev, window, cx| handler(ev, window, cx)),
                None => el,
            }
        }
    }
}
