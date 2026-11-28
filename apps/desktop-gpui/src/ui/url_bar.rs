//! URL bar — replaces `components/request/url-bar.tsx`.

use crate::models::{ConsoleLevel, HttpMethod};
use crate::services::{script, HttpService, WorkspaceService};
use crate::state::{AppState, Overlay, TabsState, ToastKind};
use crate::theme;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::prelude::*;

pub struct UrlBar {
    tabs: Entity<TabsState>,
    app_state: Entity<AppState>,
    url_input: Entity<InputState>,
    http: Entity<HttpService>,
    workspace: Entity<WorkspaceService>,
    cancel_flag: Arc<AtomicBool>,
    _subscriptions: Vec<Subscription>,
}

impl UrlBar {
    pub fn new(
        tabs: Entity<TabsState>,
        app_state: Entity<AppState>,
        http: Entity<HttpService>,
        workspace: Entity<WorkspaceService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/v1/resource"));

        let tabs_clone = tabs.clone();
        let app_state_clone = app_state.clone();
        let url_input_clone = url_input.clone();
        // Guard against the Change event re-entrantly fired by set_value() below.
        let importing = std::rc::Rc::new(std::cell::Cell::new(false));
        let importing_clone = importing.clone();
        let subscription = cx.subscribe_in(
            &url_input,
            window,
            move |_, input, event, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                if importing_clone.get() {
                    return;
                }
                let value = input.read(cx).value().to_string();

                // Detect pasted cURL commands (React `curl-detector.ts`).
                if crate::services::curl::looks_like_curl(&value) {
                    match crate::services::curl::parse_curl(&value) {
                        Ok(parsed) => {
                            tabs_clone.update(cx, |s, cx| s.apply_parsed_curl(&parsed, cx));
                            importing_clone.set(true);
                            // Show the imported URL directly instead of clearing — clearing
                            // re-fires Change and used to wipe the imported URL.
                            let imported_url = parsed.url.clone();
                            url_input_clone.update(cx, |state, cx| {
                                state.set_value(imported_url, window, cx);
                            });
                            importing_clone.set(false);
                            app_state_clone.update(cx, |s, cx| {
                                s.show_toast("cURL command imported", ToastKind::Success, cx);
                            });
                            return;
                        }
                        Err(e) => {
                            app_state_clone.update(cx, |s, cx| {
                                s.show_toast(format!("cURL parse error: {e}"), ToastKind::Error, cx);
                            });
                        }
                    }
                }

                tabs_clone.update(cx, |s, cx| s.set_url(value, cx));
            },
        );

        Self {
            tabs,
            app_state,
            url_input,
            http,
            workspace,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            _subscriptions: vec![subscription],
        }
    }

    fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self
            .tabs
            .read(cx)
            .active_tab()
            .map(|t| t.url.clone())
            .unwrap_or_default();
        let current = self.url_input.read(cx).value().to_string();
        if current != url {
            self.url_input.update(cx, |input, cx| {
                input.set_value(url, window, cx);
            });
        }
    }

    /// Connect/disconnect a WebSocket or SSE session for the active tab.
    fn toggle_realtime(&mut self, cx: &mut Context<Self>) {
        let (id, protocol) = {
            let tabs = self.tabs.read(cx);
            match tabs.active_tab() {
                Some(tab) => (tab.id.clone(), tab.protocol),
                None => return,
            }
        };
        if !matches!(
            protocol,
            crate::models::TabProtocol::WebSocket | crate::models::TabProtocol::Sse
        ) {
            return;
        }
        let connected = self
            .app_state
            .read(cx)
            .realtime
            .lock()
            .map(|m| m.is_connected(&id))
            .unwrap_or(false);
        if connected {
            self.app_state.update(cx, |s, cx| s.realtime_disconnect(&id, cx));
            return;
        }
        let url = self.url_input.read(cx).value().to_string();
        let headers = self
            .tabs
            .read(cx)
            .active_tab()
            .map(|t| t.headers.clone())
            .unwrap_or_default();
        let is_ws = protocol == crate::models::TabProtocol::WebSocket;
        self.app_state.update(cx, |s, cx| {
            s.realtime_connect(id, url, headers, is_ws, cx);
        });
    }

    pub fn copy_as_curl(&mut self, cx: &mut App) {
        let Some(tab) = self.tabs.read(cx).active_tab().cloned() else {
            return;
        };
        let headers: std::collections::HashMap<String, String> = tab
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect();
        let basic = match &tab.auth {
            crate::models::AuthConfig::Basic { username, password } => {
                Some((username.as_str(), password.as_str()))
            }
            _ => None,
        };
        let body = if tab.body.content.trim().is_empty() {
            None
        } else {
            Some(tab.body.content.as_str())
        };
        let command = crate::services::curl::export_curl(
            tab.method.as_str(),
            &crate::models::replace_url_path_variables(&tab.url, &tab.path_params),
            &headers,
            body,
            basic,
        );
        cx.write_to_clipboard(ClipboardItem::new_string(command));
        self.app_state.update(cx, |state, cx| {
            state.show_toast("Copied as cURL", ToastKind::Success, cx);
        });
    }

    fn send_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.read(cx).active_tab().cloned() else {
            return;
        };
        if tab.url.trim().is_empty() {
            self.app_state.update(cx, |s, cx| {
                s.show_toast("Please enter a URL", ToastKind::Error, cx);
            });
            return;
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.cancel_flag = cancel_flag.clone();
        let cancel_error_flag = cancel_flag.clone();

        // Resolve {{variables}} from the active environment (root .env < env YAML < secrets).
        let mut variables = self.workspace.read(cx).resolved_variables(cx, &self.app_state);
        let mut script_headers = std::collections::HashMap::new();
        let mut script_body: Option<String> = None;
        if let Some(script_source) = tab
            .pre_request_script
            .as_deref()
            .filter(|script| !script.trim().is_empty())
        {
            let current_headers: std::collections::HashMap<String, String> = tab
                .headers
                .iter()
                .filter(|h| h.enabled && !h.key.is_empty())
                .map(|h| (h.key.clone(), h.value.clone()))
                .collect();
            match script::run_pre_request(
                script_source,
                &variables,
                &current_headers,
                Some(&tab.body.content),
            ) {
                Ok(changes) => {
                    variables.extend(changes.variables);
                    script_headers = changes.headers;
                    script_body = changes.body;
                    if !changes.console.is_empty() {
                        self.app_state.update(cx, |s, cx| {
                            for (level, message) in changes.console {
                                s.log_console(level, message, cx);
                            }
                        });
                    }
                }
                Err(e) => {
                    self.app_state.update(cx, |s, cx| {
                        s.show_toast(
                            format!("Pre-request script error: {e}"),
                            ToastKind::Error,
                            cx,
                        );
                    });
                    return;
                }
            }
        }
        let interpolate = |s: &str| -> String {
            let mut out = s.to_string();
            for (key, value) in &variables {
                out = out.replace(&format!("{{{{{}}}}}", key), value);
            }
            out
        };

        let (timeout_ms, verify_ssl, follow_redirects, proxy_url) = {
            let state = self.app_state.read(cx);
            (
                state.timeout_ms,
                state.verify_ssl,
                state.follow_redirects,
                state.proxy_url.clone(),
            )
        };
        let mut headers: Vec<crate::models::KeyValuePair> = tab
            .headers
            .iter()
            .map(|p| crate::models::KeyValuePair {
                key: p.key.clone(),
                value: interpolate(&p.value),
                ..p.clone()
            })
            .collect();
        let cookie_jar = self.app_state.read(cx).cookie_jar.clone();
        if !cookie_jar.is_empty()
            && !headers
                .iter()
                .any(|h| h.enabled && h.key.eq_ignore_ascii_case("cookie"))
        {
            let value = cookie_jar
                .iter()
                .map(|cookie| format!("{}={}", cookie.key, cookie.value))
                .collect::<Vec<_>>()
                .join("; ");
            headers.push(crate::models::KeyValuePair::new("Cookie", value));
        }

        for (key, value) in script_headers {
            if let Some(header) = headers
                .iter_mut()
                .find(|h| h.key.eq_ignore_ascii_case(&key))
            {
                header.key = key;
                header.value = value;
                header.enabled = true;
            } else {
                headers.push(crate::models::KeyValuePair::new(&key, &value));
            }
        }

        use crate::services::http::SendParams;
        let params = SendParams {
            method: tab.method,
            url: crate::models::replace_url_path_variables(&interpolate(&tab.url), &tab.path_params),
            headers,
            params: tab
                .params
                .iter()
                .map(|p| crate::models::KeyValuePair {
                    key: p.key.clone(),
                    value: interpolate(&p.value),
                    ..p.clone()
                })
                .collect(),
            body: crate::models::RequestBody {
                content: interpolate(script_body.as_deref().unwrap_or(&tab.body.content)),
                ..tab.body.clone()
            },
            auth: match &tab.auth {
                crate::models::AuthConfig::Oauth2 { .. } => {
                    match self.workspace.read(cx).oauth_cached_token(&tab.auth) {
                        Some(token) => crate::models::AuthConfig::Bearer { token },
                        None => {
                            self.app_state.update(cx, |s, cx| {
                                s.show_toast(
                                    "OAuth token not available — click Get Token in the Auth tab",
                                    ToastKind::Error,
                                    cx,
                                );
                            });
                            return;
                        }
                    }
                }
                other => other.clone(),
            },
            timeout_ms,
            follow_redirects,
            verify_ssl,
            proxy_url,
        };

        let tabs = self.tabs.clone();
        let app_state = self.app_state.clone();
        let workspace = self.workspace.clone();
        let tab_id = tab.id.clone();
        let req_method = tab.method.as_str().to_string();
        let req_url = tab.url.clone();
        let collection_path = tab.collection_path.clone();
        let request_name = tab.name.clone();
        let request_json = serde_json::to_string(&tab.to_request_file()).unwrap_or_else(|_| "{}".into());

        self.tabs.update(cx, |s, cx| s.set_loading(true, cx));
        self.app_state.update(cx, |state, cx| {
            state.log_console(
                ConsoleLevel::Info,
                format!("→ {} {}", req_method, req_url),
                cx,
            );
        });

        cx.spawn_in(window, async move |_, cx| {
            // reqwest needs a Tokio reactor; run it on the dedicated runtime.
            let flag = cancel_flag.clone();
            let result = crate::runtime::run_on_tokio(async move {
                let send = HttpService::send_owned(params);
                let cancel = async move {
                    loop {
                        if flag.load(Ordering::Relaxed) {
                            return Err("Request cancelled".to_string());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                };
                tokio::select! {
                    result = send => result,
                    result = cancel => result,
                }
            })
            .await
            .unwrap_or_else(|e| Err(e));
            cx.update(|_window, cx| {
                tabs.update(cx, |s, cx| {
                    if let Some(t) = s.tabs_mut().iter_mut().find(|t| t.id == tab_id) {
                        t.loading = false;
                        match result {
                            Ok(response) => {
                                // Persist to history (same DB as the Tauri version).
                                let entry = crate::models::HistoryEntry {
                                    id: 0,
                                    method: req_method.clone(),
                                    url: req_url.clone(),
                                    status: Some(response.status as i32),
                                    status_text: Some(response.status_text.clone()),
                                    time_ms: Some(response.time_ms as i64),
                                    size_bytes: Some(response.size_bytes as i64),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    collection_path: collection_path.clone(),
                                    request_name: Some(request_name.clone()),
                                    request_json: request_json.clone(),
                                    response_json: serde_json::to_string(&response).ok(),
                                };
                                if let Err(e) = workspace.update(cx, |ws, cx| {
                                    let result = ws.insert_history(&entry);
                                    ws.refresh_history(&app_state, cx);
                                    result
                                }) {
                                    tracing::warn!("Failed to record history: {e}");
                                }

                                let cookies = response.cookies.clone();
                                app_state.update(cx, |s, cx| s.merge_cookies(cookies, cx));

                                let post_script = t.post_response_script.clone().unwrap_or_default();
                                let test_script = t.test_script.clone().unwrap_or_default();
                                t.response = Some(response.clone());

                                if let Err(e) = script::run_post_response(&post_script, &response) {
                                    tracing::warn!("Post-response script error: {e}");
                                }
                                match script::run_tests(&test_script, &response) {
                                    Ok(results) => {
                                        let failed = results.iter().filter(|r| !r.passed).count();
                                        t.test_results = results;
                                        if failed > 0 {
                                            app_state.update(cx, |s, cx| {
                                                s.show_toast(
                                                    format!("{failed} test(s) failed"),
                                                    ToastKind::Error,
                                                    cx,
                                                );
                                            });
                                        } else if !test_script.trim().is_empty() {
                                            app_state.update(cx, |s, cx| {
                                                s.show_toast("Tests passed", ToastKind::Success, cx);
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Test script error: {e}");
                                        app_state.update(cx, |s, cx| {
                                            s.show_toast(
                                                format!("Test script error: {e}"),
                                                ToastKind::Error,
                                                cx,
                                            );
                                        });
                                    }
                                }

                                app_state.update(cx, |s, cx| {
                                    s.log_console(
                                        ConsoleLevel::Success,
                                        format!(
                                            "← {} {} ({} ms)",
                                            response.status, response.status_text, response.time_ms
                                        ),
                                        cx,
                                    );
                                    s.show_toast("Request completed", ToastKind::Success, cx);
                                });
                            }
                            Err(e) => {
                                let message = if cancel_error_flag.load(Ordering::Relaxed) {
                                    "Request cancelled".to_string()
                                } else {
                                    e
                                };
                                let cancelled = cancel_error_flag.load(Ordering::Relaxed);
                                app_state.update(cx, |s, cx| {
                                    s.log_console(
                                        if cancelled {
                                            ConsoleLevel::Warning
                                        } else {
                                            ConsoleLevel::Error
                                        },
                                        message.clone(),
                                        cx,
                                    );
                                    s.show_toast(message, ToastKind::Info, cx);
                                });
                            }
                        }
                        cx.notify();
                    }
                });
            })
            .ok();
        })
        .detach();
    }
}

impl Render for UrlBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_input(window, cx);

        let (method, loading, protocol) = self
            .tabs
            .read(cx)
            .active_tab()
            .map(|t| (t.method, t.loading, t.protocol))
            .unwrap_or_default();
        let is_realtime = matches!(
            protocol,
            crate::models::TabProtocol::WebSocket | crate::models::TabProtocol::Sse
        );
        let realtime_connected = self
            .tabs
            .read(cx)
            .active_tab()
            .and_then(|tab| {
                self.app_state
                    .read(cx)
                    .realtime
                    .lock()
                    .ok()
                    .map(|m| m.is_connected(&tab.id))
            })
            .unwrap_or(false);
        let method_color = match method {
            HttpMethod::Get => theme::method::get(),
            HttpMethod::Post => theme::method::post(),
            HttpMethod::Put => theme::method::put(),
            HttpMethod::Patch => theme::method::patch(),
            HttpMethod::Delete => theme::method::delete(),
            _ => theme::text_primary(),
        };

        // The menu itself is rendered by AppView as a top-level overlay. GPUI
        // has no z-index, so a local absolutely-positioned menu would be
        // painted over by later siblings and could never be clicked.
        let method_select = div()
            .id("method-select")
            .flex()
            .items_center()
            .gap_2()
            .h(px(30.0))
            .px_3()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border())
            .cursor_pointer()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(method_color)
                    .child(SharedString::from(method.as_str())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child(SharedString::from("▾")),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let x = event.position.x.as_f32();
                let y = event.position.y.as_f32() + 28.0;
                this.app_state.update(cx, |state, cx| {
                    state.set_overlay(Overlay::Method { x, y }, cx);
                });
            }));

        div()
            .id("url-bar")
            .flex()
            .items_center()
            .gap_2()
            .h(px(44.0))
            .w_full()
            .px_3()
            .bg(theme::bg_panel())
            .rounded_lg()
            .border_b_1()
            .border_color(theme::border())
            .child(method_select)
            .when(is_realtime, |el| {
                let label = match protocol {
                    crate::models::TabProtocol::WebSocket => "WS",
                    crate::models::TabProtocol::Sse => "SSE",
                    _ => "",
                };
                el.child(
                    div()
                        .id("protocol-badge")
                        .px_2()
                        .py_1()
                        .rounded_lg()
                        .bg(theme::accent())
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(white())
                        .child(SharedString::from(label)),
                )
            })
            .child(div().flex_1().child(Input::new(&self.url_input)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(if is_realtime {
                        Button::new("realtime-connect-button")
                            .primary()
                            .text_sm()
                            .label(if realtime_connected { "Disconnect" } else { "Connect" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_realtime(cx);
                            }))
                            .into_any_element()
                    } else if loading {
                        div()
                            .id("cancel-request-button")
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::error())
                            .text_sm()
                            .text_color(white())
                            .cursor_pointer()
                            .child(SharedString::from("Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.cancel_flag.store(true, Ordering::Relaxed);
                            }))
                            .into_any_element()
                    } else {
                        Button::new("send-button")
                            .primary()
                            .text_sm()
                            .label("Send")
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.send_request(window, cx);
                            }))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .id("save-menu-button")
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(30.0))
                            .px_3()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .text_sm()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("Save"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from("▾")),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    let x = event.position.x.as_f32() - 90.0;
                                    let y = event.position.y.as_f32() + 28.0;
                                    this.app_state.update(cx, |state, cx| {
                                        state.set_overlay(Overlay::SaveMenu { x, y }, cx);
                                    });
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("url-more-button")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(34.0))
                            .h(px(30.0))
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .text_sm()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("⋯"))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    let x = event.position.x.as_f32() - 140.0;
                                    let y = event.position.y.as_f32() + 28.0;
                                    this.app_state.update(cx, |state, cx| {
                                        state.set_overlay(Overlay::UrlMoreMenu { x, y }, cx);
                                    });
                                }),
                            ),
                    ),
            )
    }
}
