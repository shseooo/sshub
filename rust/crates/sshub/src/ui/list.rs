//! ListItem — 서버/키 목록 행.
//!
//! trailing 액션은 gpui 0.2.2의 `InteractiveElement::group` + `group_hover`로
//! 호버 시 강조된다(항상 렌더되고 평소엔 dim — `visibility` 스타일 헬퍼는
//! 0.2.2 `Styled`에 없어 색 전환으로 구현).
use crate::theme::theme;
use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, ElementId, IntoElement, RenderOnce,
    SharedString, Window,
};

#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    leading: Option<AnyElement>,
    title: SharedString,
    subtitle: Option<SharedString>,
    trailing: Vec<AnyElement>,
    selected: bool,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            leading: None,
            title: title.into(),
            subtitle: None,
            trailing: Vec::new(),
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing.push(el.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx).clone();
        // group 이름은 행마다 유일해야 다른 행 호버에 반응하지 않는다.
        let group_name: SharedString = format!("list-item-{}", self.id).into();
        let title_color = if self.disabled {
            t.text_disabled
        } else {
            t.text
        };

        let body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(2.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(title_color)
                    .child(self.title),
            )
            .when_some(self.subtitle, |el, subtitle| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text_muted)
                        .child(subtitle),
                )
            });

        let trailing = if self.trailing.is_empty() {
            None
        } else {
            Some(
                div()
                    .flex()
                    .flex_none()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .text_color(t.text_disabled)
                    .group_hover(group_name.clone(), move |s| s.text_color(t.text))
                    .children(self.trailing),
            )
        };

        let row = div()
            .id(self.id)
            .group(group_name)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(if self.selected { t.border } else { t.bg })
            .when(self.selected, |el| el.bg(t.selected))
            .when(!self.disabled, |el| {
                el.cursor_pointer().hover(move |s| s.bg(t.hover))
            })
            .when_some(self.leading, |el, leading| el.child(leading))
            .child(body)
            .children(trailing);

        match self.on_click {
            Some(handler) if !self.disabled => {
                row.on_click(move |ev, window, cx| handler(ev, window, cx))
            }
            _ => row,
        }
    }
}
