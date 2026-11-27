//! Tab bar — replaces `components/tabs/tab-bar.tsx`.

use crate::state::{AppState, Overlay, TabsState};
use crate::theme;

use super::prelude::*;

pub struct TabBar {
    tabs: Entity<TabsState>,
    #[allow(dead_code)]
    app_state: Entity<AppState>,
    scroll_offset: f32,
    last_revealed: Option<String>,
}

impl TabBar {
    pub fn new(tabs: Entity<TabsState>, app_state: Entity<AppState>) -> Self {
        Self {
            tabs,
            app_state,
            scroll_offset: 0.0,
            last_revealed: None,
        }
    }

    pub fn save_active(&mut self, cx: &mut Context<Self>) {
        super::save_active_tab(&self.tabs, &self.app_state, cx);
    }
}

impl Render for TabBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs_state = self.tabs.read(cx);
        let active_id = tabs_state.active_tab_id().map(|s| s.to_string());
        let tab_width = 180.0f32;
        let content_width = tabs_state.tabs().len() as f32 * tab_width;
        let viewport_width = (window.viewport_size().width.as_f32() - 440.0).max(240.0);
        let max_offset = (content_width - viewport_width).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_offset);

        if self.last_revealed.as_deref() != active_id.as_deref() {
            self.last_revealed = active_id.clone();
            if let Some(id) = active_id.as_ref() {
                if let Some(index) = tabs_state.tabs().iter().position(|tab| &tab.id == id) {
                    let left = index as f32 * tab_width;
                    let right = left + tab_width;
                    if left < self.scroll_offset {
                        self.scroll_offset = left;
                    } else if right > self.scroll_offset + viewport_width {
                        self.scroll_offset = right - viewport_width;
                    }
                }
            }
        }
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_offset);

        let mut strip = div()
            .flex()
            .flex_none()
            .relative()
            .w(px(viewport_width))
            .h(px(38.0))
            .overflow_hidden()
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                let delta = match event.delta {
                    ScrollDelta::Pixels(point) => {
                        if event.modifiers.shift {
                            point.y.as_f32()
                        } else {
                            point.x.as_f32()
                        }
                    }
                    ScrollDelta::Lines(point) => {
                        let value = if event.modifiers.shift { point.y } else { point.x };
                        value * 40.0
                    }
                };
                let viewport = (window.viewport_size().width.as_f32() - 440.0).max(240.0);
                let max = (this.tabs.read(cx).tabs().len() as f32 * 180.0 - viewport).max(0.0);
                this.scroll_offset = (this.scroll_offset + delta).clamp(0.0, max);
                cx.notify();
            }));

        let mut row = div()
            .id("tab-bar")
            .flex()
            .h(px(38.0))
            .w_full()
            .bg(theme::bg_base())
            .rounded_lg()
            .border_b_1()
            .border_color(theme::border())
            .items_center();

        let scroll_offset = self.scroll_offset;
        let mut strip_inner = div()
            .flex()
            .flex_none()
            .relative()
            .left(px(-scroll_offset))
            .h(px(38.0));

        for tab in tabs_state.tabs() {
            let is_active = active_id.as_deref() == Some(tab.id.as_str());
            let pinned = tabs_state.is_pinned(&tab.id);
            let tab_id = tab.id.clone();
            let tabs_entity = self.tabs.clone();

            let method_color = match tab.method {
                crate::models::HttpMethod::Get => theme::method::get(),
                crate::models::HttpMethod::Post => theme::method::post(),
                crate::models::HttpMethod::Put => theme::method::put(),
                crate::models::HttpMethod::Patch => theme::method::patch(),
                crate::models::HttpMethod::Delete => theme::method::delete(),
                _ => theme::text_muted(),
            };

            let close_tabs = self.tabs.clone();
            let close_id = tab_id.clone();
            let context_state = self.app_state.clone();
            let context_id = tab_id.clone();
            strip_inner = strip_inner.child(
                div()
                    .id(SharedString::from(format!("tab-{}", tab.id)))
                    .flex()
                    .items_center()
                    .h(px(38.0))
                    .w(px(tab_width))
                    .rounded_lg()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(theme::border())
                    .border_b_2()
                    .border_color(if is_active {
                        theme::accent()
                    } else {
                        transparent_black()
                    })
                    .bg(if is_active {
                        theme::bg_panel()
                    } else {
                        theme::bg_base()
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("tab-body-{}", tab.id)))
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .pl_3()
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                tabs_entity.update(cx, |s, cx| s.set_active_tab(&tab_id, cx));
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(method_color)
                                    .child(SharedString::from(tab.method.as_str())),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(if is_active {
                                        theme::text_primary()
                                    } else {
                                        theme::text_muted()
                                    })
                                    .child(SharedString::from(format!(
                                        "{}{}",
                                        if pinned { "📌 " } else { "" },
                                        tab.label()
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("close-tab-{}", tab.id)))
                            .w(px(24.0))
                            .h(px(24.0))
                            .mr_2()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .text_sm()
                            .text_color(theme::text_muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()).text_color(theme::error()))
                            .child(SharedString::from("×"))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |_, _, _, cx| {
                                close_tabs.update(cx, |s, cx| s.close_tab(&close_id, cx));
                            })),
                    ),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    context_state.update(cx, |state, cx| {
                        state.set_overlay(
                            Overlay::TabContext {
                                x: event.position.x.as_f32(),
                                y: event.position.y.as_f32(),
                                tab_id: context_id.clone(),
                            },
                            cx,
                        );
                    });
                }),
            );
        }

        strip = strip.child(strip_inner);
        row = row.child(strip);

        let new_tabs = self.tabs.clone();
        row = row.child(
            div()
                .id("new-tab-button")
                .flex()
                .items_center()
                .justify_center()
                .w(px(30.0))
                .h(px(30.0))
                .ml_1()
                .rounded_lg()
                .bg(theme::bg_surface())
                .border_1()
                .border_color(theme::border())
                .text_sm()
                .text_color(theme::text_muted())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()).text_color(theme::text_primary()))
                .child(SharedString::from("+"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    new_tabs.update(cx, |s, cx| {
                        s.new_tab(cx);
                    });
                })),
        );

        let prev_tabs = self.tabs.clone();
        let next_tabs = self.tabs.clone();
        row = row
            .child(div().flex_1().h(px(38.0)))
            .child(
                div()
                    .id("prev-tab-button")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(34.0))
                    .h(px(30.0))
                    .mx_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .text_sm()
                    .text_color(theme::text_primary())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::bg_hover()))
                    .child(SharedString::from("‹"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        prev_tabs.update(cx, |s, cx| s.activate_prev_tab(cx));
                    })),
            )
            .child(
                div()
                    .id("next-tab-button")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(34.0))
                    .h(px(30.0))
                    .mr_2()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .text_sm()
                    .text_color(theme::text_primary())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::bg_hover()))
                    .child(SharedString::from("›"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        next_tabs.update(cx, |s, cx| s.activate_next_tab(cx));
                    })),
            );

        row
    }
}
