//! Response panel — replaces `components/response/response-panel.tsx`.

use crate::state::{AppState, Overlay, TabsState, ToastKind};
use crate::theme;
use std::hash::{DefaultHasher, Hash, Hasher};

use super::prelude::*;

pub struct ResponsePanel {
    tabs: Entity<TabsState>,
    app_state: Entity<AppState>,
    body_editor: Entity<EditorState>,
    search_input: Entity<InputState>,
    pretty: bool,
    last_body_key: Option<(String, u64, bool)>,
    _subscriptions: Vec<Subscription>,
}

impl ResponsePanel {
    pub fn new(
        tabs: Entity<TabsState>,
        app_state: Entity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let body_editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).language("json");
            state.set_highlighter_factory(crate::ui::json_view::JsonHighlighter::factory(), cx);
            state
        });
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search body"));
        let search_sub = cx.subscribe_in(
            &search_input,
            window,
            move |this, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    this.apply_search(&query, cx);
                }
            },
        );
        Self {
            tabs,
            app_state,
            body_editor,
            search_input,
            pretty: true,
            last_body_key: None,
            _subscriptions: vec![search_sub],
        }
    }

    /// Keep a selectable text editor in sync with the active response body.
    fn sync_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.read(cx).active_tab() else {
            return;
        };
        let Some(response) = tab.response.as_ref() else {
            return;
        };
        let mut hasher = DefaultHasher::new();
        response.body.hash(&mut hasher);
        let key = (tab.id.clone(), hasher.finish(), self.pretty);
        if self.last_body_key.as_ref() == Some(&key) {
            return;
        }
        let text = if self.pretty {
            crate::ui::json_view::try_format_json(&response.body)
                .unwrap_or_else(|| response.body.clone())
        } else {
            response.body.clone()
        };
        self.body_editor.update(cx, |state, cx| {
            state.set_value(text, window, cx);
        });
        self.last_body_key = Some(key);
    }

    fn apply_search(&mut self, query: &str, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            return;
        }
        let value = self.body_editor.read(cx).value().to_string();
        let needle = query.to_lowercase();
        if let Some(index) = value.to_lowercase().find(&needle) {
            self.body_editor.update(cx, |state, cx| {
                state.set_selected_range(index..index + needle.len(), cx);
            });
        }
    }

    fn toggle_pretty(&mut self, cx: &mut Context<Self>) {
        self.pretty = !self.pretty;
        self.last_body_key = None;
        cx.notify();
    }
}

impl Render for ResponsePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tabs.read(cx).active_tab().cloned();
        let response = tab.as_ref().and_then(|t| t.response.clone());

        let view = self.app_state.read(cx).response_view;
        if view == crate::models::ResponseView::Body {
            self.sync_body(window, cx);
        }
        let app_state = self.app_state.clone();

        let views = [
            crate::models::ResponseView::Body,
            crate::models::ResponseView::Headers,
            crate::models::ResponseView::Cookies,
            crate::models::ResponseView::Tests,
            crate::models::ResponseView::Timing,
            crate::models::ResponseView::Console,
            crate::models::ResponseView::Code,
        ];

        let mut top_row = div()
            .flex()
            .items_center()
            .h(px(36.0))
            .w_full()
            .min_w(px(0.0))
            .px_3()
            .border_b_1()
            .border_color(theme::border());

        let mut view_tabs = div().flex().items_center().gap_1().flex_1();
        for v in views {
            let label = match v {
                crate::models::ResponseView::Body => "Body",
                crate::models::ResponseView::Headers => "Headers",
                crate::models::ResponseView::Cookies => "Cookies",
                crate::models::ResponseView::Tests => "Tests",
                crate::models::ResponseView::Timing => "Timing",
                crate::models::ResponseView::Console => "Console",
                crate::models::ResponseView::Code => "Code",
            };
            let is_active = view == v;
            let state_clone = app_state.clone();
            view_tabs = view_tabs.child(
                div()
                    .id(SharedString::from(format!("resp-view-{:?}", v)))
                    .px_2()
                    .h_full()
                    .flex()
                    .items_center()
                    .text_xs()
                    .cursor_pointer()
                    .text_color(if is_active { theme::accent() } else { theme::text_muted() })
                    .border_b_2()
                    .border_color(if is_active { theme::accent() } else { transparent_black() })
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        state_clone.update(cx, |s, cx| s.set_response_view(v, cx));
                    })),
            );
        }
        top_row = top_row.child(view_tabs);

        let mut status_row = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(24.0))
            .w_full()
            .px_3()
            .bg(theme::bg_base())
            .border_b_1()
            .border_color(theme::border());

        let mut sub_row = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(34.0))
            .w_full()
            .px_3()
            .bg(theme::bg_panel())
            .border_b_1()
            .border_color(theme::border());

        if let Some(ref resp) = response {
            let status_color = match resp.status {
                200..=299 => theme::success(),
                300..=399 => theme::warning(),
                _ => theme::error(),
            };
            status_row = status_row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(status_color)
                                .child(SharedString::from(format!(
                                    "{} {}",
                                    resp.status, resp.status_text
                                ))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(SharedString::from(format!("{} ms", resp.time_ms))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(SharedString::from(format_size(resp.size_bytes))),
                        ),
                )
                .child(div().w(px(12.0)));

            let copy_text = crate::ui::json_view::try_format_json(&resp.body)
                .unwrap_or_else(|| resp.body.clone());
            let copy_state = app_state.clone();
            let save_text = copy_text.clone();
            let save_state = app_state.clone();
            let diff_state = app_state.clone();
            let save_diff_state = app_state.clone();
            let save_diff_label = tab
                .as_ref()
                .map(|tab| {
                    format!(
                        "{} {} · {}ms",
                        tab.method.as_str(),
                        resp.status,
                        resp.time_ms
                    )
                })
                .unwrap_or_else(|| format!("{} · {}ms", resp.status, resp.time_ms));
            let save_diff_response = resp.clone();
            let mut actions = div().flex().items_center().gap_1();
            actions = actions
                .child(
                    div()
                        .id("copy-response")
                        .px_2()
                        .py_1()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .text_xs()
                        .text_color(theme::text_primary())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from("Copy"))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                            copy_state.update(cx, |state, cx| {
                                state.show_toast("Response copied", ToastKind::Success, cx);
                            });
                        }),
                )
                .child(
                    div()
                        .id("save-response")
                        .px_2()
                        .py_1()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .text_xs()
                        .text_color(theme::text_primary())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from("Save"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, window, cx| {
                                let text = save_text.clone();
                                let state = save_state.clone();
                                cx.spawn_in(window, async move |_, cx| {
                                    let Some(file) = rfd::AsyncFileDialog::new()
                                        .set_file_name("response.json")
                                        .add_filter("JSON", &["json"])
                                        .add_filter("Text", &["txt", "log"])
                                        .save_file()
                                        .await
                                    else {
                                        return;
                                    };
                                    let result = std::fs::write(file.path(), text);
                                    let _ = cx.update(|_, cx| match result {
                                        Ok(()) => state.update(cx, |s, cx| {
                                            s.show_toast("Response saved", ToastKind::Success, cx);
                                        }),
                                        Err(e) => state.update(cx, |s, cx| {
                                            s.show_toast(
                                                format!("Save failed: {e}"),
                                                ToastKind::Error,
                                                cx,
                                            );
                                        }),
                                    });
                                })
                                .detach();
                            }),
                        ),
                )
                .child(
                    div()
                        .id("save-diff-response")
                        .px_2()
                        .py_1()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .text_xs()
                        .text_color(theme::text_primary())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from("Save for Diff"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                let result = save_diff_state.update(cx, |state, cx| {
                                    state.save_diff_snapshot(
                                        save_diff_label.clone(),
                                        &save_diff_response,
                                        cx,
                                    )
                                });
                                save_diff_state.update(cx, |state, cx| match result {
                                    Ok(()) => state.show_toast(
                                        "Response saved for diff",
                                        ToastKind::Success,
                                        cx,
                                    ),
                                    Err(e) => state.show_toast(
                                        format!("Save for diff failed: {e}"),
                                        ToastKind::Error,
                                        cx,
                                    ),
                                });
                            }),
                        ),
                )
                .child(
                    div()
                        .id("diff-response")
                        .px_2()
                        .py_1()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .text_xs()
                        .text_color(theme::text_primary())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from("Compare"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                diff_state
                                    .update(cx, |s, cx| s.set_overlay(Overlay::ResponseDiff, cx));
                            }),
                        ),
                );
            status_row = status_row.child(div().flex_1()).child(actions);

            if view == crate::models::ResponseView::Body {
                sub_row = sub_row
                    .child(
                        div()
                            .id("response-format-toggle")
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .text_xs()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(if self.pretty { "Pretty" } else { "Raw" }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_pretty(cx);
                            })),
                    )
                    .child(
                        div()
                            .w(px(180.0))
                            .child(Input::new(&self.search_input)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(SharedString::from("JSON")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(SharedString::from("utf-8")),
                    );
            }
        }

        let header = div()
            .flex()
            .flex_col()
            .w_full()
            .child(status_row)
            .child(top_row)
            .child(sub_row);

        let body_content: AnyElement = if let Some(ref resp) = response {
            match view {
                crate::models::ResponseView::Body => {
                    let mut body = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .p_3()
                        .overflow_hidden()
                        .font_family("monospace");
                    if resp.truncated == Some(true) {
                        let load_tabs = self.tabs.clone();
                        body = body.child(
                            div()
                                .mb_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(theme::warning())
                                .child(SharedString::from(format!(
                                    "Response truncated at 1 MB of {}",
                                    resp.full_size.unwrap_or(resp.size_bytes)
                                )))
                                .child(
                                    div()
                                        .id("load-full-response")
                                        .px_2()
                                        .py_1()
                                        .rounded_lg()
                                        .bg(theme::bg_surface())
                                        .text_color(theme::accent())
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme::bg_hover()))
                                        .child(SharedString::from("Load full"))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |_, _, _, cx| {
                                                cx.stop_propagation();
                                                load_tabs.update(cx, |state, cx| {
                                                    state.load_full_response(cx);
                                                });
                                            }),
                                        ),
                                ),
                        );
                    }
                    body.child(
                        div()
                            .flex_1()
                            .child(
                                Editor::new(&self.body_editor)
                                    .h_full()
                                    .bordered(false)
                                    .readonly(true),
                            ),
                    )
                    .into_any_element()
                }
                crate::models::ResponseView::Headers => {
                    let mut rows = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .min_w(px(0.0))
                        .overflow_scrollbar()
                        .p_3()
                        .gap_1();
                    for h in &resp.headers {
                        rows = rows.child(
                            div()
                                .flex()
                                .gap_3()
                                .text_sm()
                                .child(
                                    div()
                                        .w(px(200.0))
                                        .text_color(theme::accent())
                                        .child(SharedString::from(h.key.clone())),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(theme::text_primary())
                                        .child(SharedString::from(h.value.clone())),
                                ),
                        );
                    }
                    rows.into_any_element()
                }
                crate::models::ResponseView::Cookies => {
                    let cookies = self.app_state.read(cx).cookie_jar.clone();
                    let mut rows = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .min_w(px(0.0))
                        .overflow_scrollbar()
                        .p_3()
                        .gap_1();
                    if cookies.is_empty() {
                        rows = rows.child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child(SharedString::from("No cookies")),
                        );
                    }
                    for cookie in &cookies {
                        let key = cookie.key.clone();
                        let delete_state = app_state.clone();
                        rows = rows.child(
                            div()
                                .flex()
                                .gap_3()
                                .text_sm()
                                .child(
                                    div()
                                        .w(px(200.0))
                                        .text_color(theme::accent())
                                        .child(SharedString::from(cookie.key.clone())),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(theme::text_primary())
                                        .child(SharedString::from(cookie.value.clone())),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("cookie-{}", cookie.key)))
                                        .px_1()
                                        .text_color(theme::text_muted())
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(theme::error()))
                                        .child(SharedString::from("×"))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_, _, cx| {
                                                delete_state.update(cx, |s, cx| {
                                                    s.remove_cookie(&key, cx)
                                                });
                                            },
                                        ),
                                ),
                        );
                    }
                    if !cookies.is_empty() {
                        let clear_state = app_state.clone();
                        rows = rows.child(
                            div()
                                .mt_2()
                                .px_2()
                                .py_1()
                                .rounded_lg()
                                .text_sm()
                                .text_color(theme::error())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::bg_hover()))
                                .child(SharedString::from("Clear Cookies"))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    clear_state.update(cx, |s, cx| s.clear_cookies(cx));
                                }),
                        );
                    }
                    rows.into_any_element()
                }
                crate::models::ResponseView::Tests => {
                    let script = tab
                        .as_ref()
                        .and_then(|tab| tab.test_script.clone())
                        .unwrap_or_default();
                    let results = tab
                        .as_ref()
                        .map(|tab| tab.test_results.clone())
                        .unwrap_or_default();
                    let passed = results.iter().filter(|result| result.passed).count();
                    let mut body = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .p_3()
                        .gap_2()
                        .overflow_scrollbar();
                    if !results.is_empty() {
                        body = body.child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if passed == results.len() {
                                    theme::success()
                                } else {
                                    theme::error()
                                })
                                .child(SharedString::from(format!(
                                    "{passed}/{} tests passed",
                                    results.len()
                                ))),
                        );
                        for result in results {
                            body = body.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .px_2()
                                    .py_1()
                                    .rounded_lg()
                                    .bg(theme::bg_surface())
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .text_sm()
                                            .text_color(if result.passed {
                                                theme::success()
                                            } else {
                                                theme::error()
                                            })
                                            .child(SharedString::from(if result.passed {
                                                "✓"
                                            } else {
                                                "×"
                                            }))
                                            .child(SharedString::from(result.name)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from(
                                                result.error.unwrap_or_default(),
                                            )),
                                    ),
                            );
                        }
                    } else {
                        body = body.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(SharedString::from(
                                    "No test results yet. Send the request to run the test script.",
                                )),
                        );
                    }
                    body.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(SharedString::from("Test Script")),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(theme::bg_base())
                            .font_family("monospace")
                            .text_sm()
                            .child(SharedString::from(if script.trim().is_empty() {
                                "No test script configured. Add one in the request Scripts tab."
                                    .to_string()
                            } else {
                                script
                            })),
                    )
                    .into_any_element()
                }
                crate::models::ResponseView::Timing => div()
                    .flex_1()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .child(SharedString::from(format!("Total: {} ms", resp.time_ms))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .child(SharedString::from(format!(
                                "Size: {} bytes",
                                resp.size_bytes
                            ))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .child(SharedString::from(format!(
                                "Status: {} {}",
                                resp.status, resp.status_text
                            ))),
                    )
                    .into_any_element(),
                crate::models::ResponseView::Code => {
                    let code = tab
                        .as_ref()
                        .map(|tab| crate::app::generate_request_code(tab, "curl"))
                        .unwrap_or_default();
                    div()
                        .flex_1()
                        .p_3()
                        .font_family("monospace")
                        .text_sm()
                        .child(SharedString::from(code))
                        .into_any_element()
                }
                crate::models::ResponseView::Console => {
                    let entries = self.app_state.read(cx).console_entries.clone();
                    let mut list = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .p_3()
                        .gap_1()
                        .overflow_scrollbar();
                    if entries.is_empty() {
                        list = list.child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child(SharedString::from("No console output yet.")),
                        );
                    }
                    for entry in entries.iter().rev().take(80) {
                        let color = match entry.level {
                            crate::models::ConsoleLevel::Info => theme::text_primary(),
                            crate::models::ConsoleLevel::Success => theme::success(),
                            crate::models::ConsoleLevel::Warning => theme::warning(),
                            crate::models::ConsoleLevel::Error => theme::error(),
                        };
                        list = list.child(
                            div()
                                .id(SharedString::from(format!("resp-console-{}", entry.id)))
                                .flex()
                                .gap_2()
                                .text_xs()
                                .child(
                                    div()
                                        .w(px(60.0))
                                        .text_color(theme::text_muted())
                                        .child(SharedString::from(entry.timestamp.clone())),
                                )
                                .child(
                                    div()
                                        .w(px(54.0))
                                        .text_color(color)
                                        .child(SharedString::from(format!("{:?}", entry.level))),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(theme::text_primary())
                                        .child(SharedString::from(entry.message.clone())),
                                ),
                        );
                    }
                    list.into_any_element()
                }
            }
        } else if tab.as_ref().map(|t| t.loading).unwrap_or(false) {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::accent())
                        .child(SharedString::from("Sending request…")),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_muted())
                        .child(SharedString::from("Send a request to see the response")),
                )
                .into_any_element()
        };

        div()
            .id("response-panel")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(150.0))
            .bg(theme::bg_panel())
            .rounded_lg()
            .border_t_1()
            .border_color(theme::border())
            .child(header)
            .child(body_content)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
