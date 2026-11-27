//! Left activity bar — icon-only vertical strip (VS Code style).
//! Replaces `components/layout/activity-bar.tsx`.

use crate::models::ActivityView;
use crate::state::AppState;
use crate::theme;

use super::prelude::*;

pub struct ActivityBar {
    app_state: Entity<AppState>,
}

impl ActivityBar {
    pub fn new(app_state: Entity<AppState>) -> Self {
        Self { app_state }
    }

    fn item(
        &self,
        view: ActivityView,
        glyph: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.app_state.read(cx).active_view == view;
        let state = self.app_state.clone();

        div()
            .id(SharedString::from(format!("activity-{:?}", view)))
            .w_full()
            .h(px(44.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if active { theme::bg_hover() } else { theme::bg_panel() })
            .border_l_2()
            .border_color(if active { theme::accent() } else { transparent_black() })
            .child(
                div()
                    .text_xl()
                    .text_color(if active {
                        theme::accent()
                    } else {
                        theme::text_muted()
                    })
                    .child(SharedString::from(glyph)),
            )
            .on_click(cx.listener(move |_, _, _, cx| {
                state.update(cx, |s, cx| s.set_view(view, cx));
            }))
            .into_any_element()
    }
}

impl Render for ActivityBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(48.0))
            .min_w(px(48.0))
            .h_full()
            .bg(theme::bg_panel())
            .rounded_lg()
            .border_r_1()
            .border_color(theme::border())
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .child(self.item(ActivityView::Collections, "▤", cx))
                    .child(self.item(ActivityView::History, "◷", cx))
                    .child(self.item(ActivityView::Environments, "◈", cx))
                    .child(self.item(ActivityView::Trash, "🗑", cx))
                    .child(self.item(ActivityView::Search, "⌕", cx)),
            )
            .child(div().w_full().child(self.item(ActivityView::Extensions, "⊕", cx)))
    }
}
