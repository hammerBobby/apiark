//! Status bar — replaces `components/layout/status-bar.tsx`.

use gpui_kit::component::{Theme, ThemeMode};

use crate::state::{AppState, TabsState};
use crate::theme;

use super::prelude::*;

pub struct StatusBar {
    tabs: Entity<TabsState>,
    app_state: Entity<AppState>,
}

impl StatusBar {
    pub fn new(tabs: Entity<TabsState>, app_state: Entity<AppState>) -> Self {
        Self { tabs, app_state }
    }

    /// Cycle to the next environment on click.
    fn cycle_environment(&mut self, cx: &mut Context<Self>) {
        let envs = self.app_state.read(cx).environments.clone();
        if envs.is_empty() {
            return;
        }
        let current = self.app_state.read(cx).environment.clone();
        let next = current
            .and_then(|c| envs.iter().position(|e| e.name == c))
            .map(|i| (i + 1) % envs.len())
            .unwrap_or(0);
        let name = envs[next].name.clone();
        self.app_state.update(cx, |s, cx| s.set_environment(Some(name), cx));
    }

    fn toggle_zen(&mut self, cx: &mut Context<Self>) {
        self.app_state.update(cx, |s, cx| s.toggle_zen_mode(cx));
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let dark = !self.app_state.read(cx).dark_mode;
        self.app_state.update(cx, |state, cx| state.set_dark_mode(dark, cx));
        let mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        Theme::change(mode, None, cx);
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app_state.read(cx);
        let tabs = self.tabs.read(cx);
        let tab_count = tabs.tabs().len();
        let env_count = app.environments.len();
        let env = app.environment.as_deref().unwrap_or("No Environment");
        let zen_mode = app.zen_mode;
        let dark_mode = app.dark_mode;
        let bottom_panel_visible = app.bottom_panel_visible;

        div()
            .id("status-bar")
            .flex()
            .items_center()
            .justify_between()
            .h(px(24.0))
            .w_full()
            .px_3()
            .bg(theme::bg_panel())
            .rounded_lg()
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(format!("{} tabs", tab_count)))
                    .child(SharedString::from("● Ready")),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(
                        div()
                            .id("environment-selector")
                            .flex()
                            .gap_1()
                            .px_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(format!("🌐 {}", env)))
                            .child(SharedString::from(format!("({})", env_count)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cycle_environment(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("theme-toggle")
                            .flex()
                            .gap_1()
                            .px_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(if dark_mode { "🌙" } else { "☀" }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_theme(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("console-toggle")
                            .flex()
                            .gap_1()
                            .px_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(if bottom_panel_visible {
                                "Console ✓"
                            } else {
                                "Console"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app_state.update(cx, |state, cx| {
                                    state.toggle_bottom_panel(cx)
                                });
                            })),
                    )
                    .child(
                        div()
                            .id("zen-toggle")
                            .flex()
                            .gap_1()
                            .px_2()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(if zen_mode { "Zen ✓" } else { "Zen" }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_zen(cx);
                            })),
                    )
                    .child(SharedString::from("UTF-8"))
                    .child(SharedString::from("ApiArk GPUI")),
            )
    }
}
