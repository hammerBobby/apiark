//! Root application view — composes the full ApiArk desktop layout.

use gpui_kit::component::input::{Editor, EditorState, Input, InputState};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;
use std::time::Instant;

use crate::models::{ActivityView, CollectionNode, CollectionNodeKind, HttpMethod};
use crate::services::{collection as collection_service, HttpService, WorkspaceService};
use crate::state::{AppState, Overlay, ToastKind, TabsState};
use crate::theme;
use crate::ui::{
    ActivityBar, RequestPanel, ResponsePanel, SidePanel, StatusBar, TabBar, UrlBar,
};

gpui_kit::actions!(apiark, [SaveRequest, OpenCommandPalette, FocusEnvironment, NewTab]);

const SETTINGS_SHORTCUTS: &[(&str, &str)] = &[
    ("Save Request", "Ctrl/Cmd+S"),
    ("Command Palette", "Ctrl/Cmd+P"),
    ("Focus Environment", "Ctrl/Cmd+E"),
    ("New Tab", "Ctrl/Cmd+N"),
    ("Close Tab", "Ctrl/Cmd+W"),
    ("Send Request", "Ctrl/Cmd+Enter"),
    ("Format JSON", "Ctrl/Cmd+Shift+F"),
    ("Toggle Zen Mode", "Ctrl/Cmd+Shift+Z"),
    ("Toggle Sidebar", "Ctrl/Cmd+B"),
    ("Toggle Bottom Panel", "Ctrl/Cmd+J"),
    ("Copy as cURL", "Ctrl/Cmd+Shift+C"),
    ("Clear History", "Ctrl/Cmd+Shift+H"),
    ("Run Collection", "Ctrl/Cmd+R"),
    ("Open Collection Folder", "Ctrl/Cmd+O"),
    ("New Collection", "Ctrl/Cmd+Shift+N"),
    ("Prev Tab", "Ctrl/Cmd+PageUp"),
    ("Next Tab", "Ctrl/Cmd+PageDown"),
];

fn shortcut_conflicts() -> Vec<&'static str> {
    let mut seen = std::collections::HashMap::new();
    let mut conflicts = Vec::new();
    for (label, key) in SETTINGS_SHORTCUTS {
        if seen.insert(*key, *label).is_some() {
            conflicts.push(*label);
        }
    }
    conflicts
}

#[derive(Clone, PartialEq, Eq)]
enum OverlaySyncKey {
    None,
    NewCollection,
    NewNode {
        parent_path: String,
        kind: CollectionNodeKind,
        name: String,
    },
    Rename {
        path: String,
        kind: CollectionNodeKind,
        name: String,
    },
    EnvironmentEditor {
        original_name: Option<String>,
        name: String,
        scope: crate::models::EnvironmentScope,
    },
    SaveCase {
        name: String,
    },
    SaveAs {
        name: String,
        location: String,
    },
    Runner {
        collection_path: String,
        data_path: String,
    },
    Settings,
}

pub struct AppView {
    focus_handle: FocusHandle,
    app_state: Entity<AppState>,
    tabs: Entity<TabsState>,
    #[allow(dead_code)]
    http: Entity<HttpService>,
    workspace: Entity<WorkspaceService>,

    activity_bar: Entity<ActivityBar>,
    side_panel: Entity<SidePanel>,
    tab_bar: Entity<TabBar>,
    url_bar: Entity<UrlBar>,
    request_panel: Entity<RequestPanel>,
    response_panel: Entity<ResponsePanel>,
    status_bar: Entity<StatusBar>,

    new_collection_name: Entity<InputState>,
    new_collection_location: Entity<InputState>,
    new_node_name: Entity<InputState>,
    rename_input: Entity<InputState>,
    save_as_name: Entity<InputState>,
    command_palette_input: Entity<InputState>,
    settings_timeout: Entity<InputState>,
    settings_proxy: Entity<InputState>,
    env_name_input: Entity<InputState>,
    env_vars_editor: Entity<EditorState>,
    env_secrets_editor: Entity<EditorState>,
    curl_input: Entity<EditorState>,
    case_name_input: Entity<InputState>,
    runner_data_input: Entity<InputState>,
    next_watch_check: Instant,
    next_realtime_check: Instant,
    synced_overlay: OverlaySyncKey,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let focus_handle = cx.focus_handle();
        let app_state = AppState::create(cx);

        let tabs = TabsState::create(cx);
        let http = cx.new(|_| HttpService::new());

        let workspace = cx.new(|_| {
            WorkspaceService::new().expect("Failed to initialize workspace storage")
        });

        // Load history and restore the collection that was open last time.
        workspace.update(cx, |ws, cx| {
            ws.refresh_history(&app_state, cx);
            ws.refresh_trash(&app_state, cx);
            app_state.update(cx, |state, cx| state.refresh_diff_snapshots(cx));
            if let Err(e) = ws.refresh_collections(&app_state, cx) {
                tracing::debug!("Failed to list collections: {e}");
            }
            if let Err(e) = ws.restore_last_collection(&tabs, &app_state, cx) {
                tracing::debug!("No collection to restore: {e}");
            }
        });

        let has_collections = !app_state.read(cx).collections.is_empty();
        if has_collections {
            app_state.update(cx, |state, cx| state.dismiss_onboarding(cx));
        }

        let activity_bar = cx.new(|_| ActivityBar::new(app_state.clone()));
        let side_panel = cx.new(|cx| SidePanel::new(
            app_state.clone(),
            tabs.clone(),
            workspace.clone(),
            window,
            cx,
        ));
        let tab_bar = cx.new(|_| TabBar::new(tabs.clone(), app_state.clone()));
        let request_panel = cx.new(|cx| {
            RequestPanel::new(
                tabs.clone(),
                app_state.clone(),
                workspace.clone(),
                window,
                cx,
            )
        });
        let response_panel = cx.new(|cx| ResponsePanel::new(
            tabs.clone(),
            app_state.clone(),
            window,
            cx,
        ));
        let status_bar = cx.new(|_| StatusBar::new(tabs.clone(), app_state.clone()));
        let url_bar = cx.new(|cx| {
            UrlBar::new(
                tabs.clone(),
                app_state.clone(),
                http.clone(),
                workspace.clone(),
                window,
                cx,
            )
        });

        let new_collection_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Collection name")
                .default_value("New Collection")
        });
        let new_collection_location = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Parent directory")
                .default_value(WorkspaceService::collections_root())
        });
        let new_node_name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("New name"));
        let save_as_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Request name")
                .default_value("Untitled Request")
        });
        let command_palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a command..."));
        let settings_timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Timeout (ms)")
                .default_value("30000")
        });
        let settings_proxy =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://127.0.0.1:7890"));
        let env_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Environment name"));
        let env_vars_editor = cx.new(|cx| EditorState::new(window, cx));
        let env_secrets_editor = cx.new(|cx| EditorState::new(window, cx));
        let curl_input = cx.new(|cx| EditorState::new(window, cx));
        let case_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Case name"));
        let runner_data_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional CSV data file"));

        let view = cx.new(|_| Self {
            focus_handle,
            app_state,
            tabs,
            http,
            workspace,
            activity_bar,
            side_panel,
            tab_bar,
            url_bar,
            request_panel,
            response_panel,
            status_bar,
            new_collection_name,
            new_collection_location,
            new_node_name,
            rename_input,
            save_as_name,
            command_palette_input,
            settings_timeout,
            settings_proxy,
            env_name_input,
            env_vars_editor,
            env_secrets_editor,
            curl_input,
            case_name_input,
            runner_data_input,
            next_watch_check: Instant::now(),
            next_realtime_check: Instant::now(),
            synced_overlay: OverlaySyncKey::None,
        });
        view
    }

    fn start_runner(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        collection_path: String,
        data_path: String,
    ) {
        let data_rows = match crate::services::runner::load_data_rows(
            if data_path.trim().is_empty() {
                None
            } else {
                Some(std::path::Path::new(&data_path))
            },
        ) {
            Ok(rows) => rows,
            Err(e) => {
                self.app_state.update(cx, |state, cx| {
                    state.show_toast(
                        format!("Failed to read runner data: {e}"),
                        ToastKind::Error,
                        cx,
                    );
                });
                return;
            }
        };

        let variables = self
            .workspace
            .read(cx)
            .resolved_variables(cx, &self.app_state);
        let (timeout_ms, verify_ssl, follow_redirects, proxy_url) = {
            let state = self.app_state.read(cx);
            (
                state.timeout_ms,
                state.verify_ssl,
                state.follow_redirects,
                state.proxy_url.clone(),
            )
        };
        let state = self.app_state.clone();
        state.update(cx, |s, cx| s.set_runner_running(true, cx));

        cx.spawn_in(window, async move |_, cx| {
            let result = crate::services::runner::run_collection(
                &collection_path,
                &variables,
                data_rows,
                timeout_ms,
                verify_ssl,
                follow_redirects,
                proxy_url,
            )
            .await;
            let _ = cx.update(|_, cx| {
                state.update(cx, |s, cx| match result {
                    Ok(results) => {
                        let passed = results.iter().filter(|result| result.passed).count();
                        let total = results.len();
                        s.set_runner_results(results, cx);
                        s.show_toast(
                            format!("Runner finished: {passed}/{total} passed"),
                            ToastKind::Success,
                            cx,
                        );
                    }
                    Err(e) => {
                        s.set_runner_running(false, cx);
                        s.show_toast(format!("Runner failed: {e}"), ToastKind::Error, cx);
                    }
                });
            });
        })
        .detach();
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs = self.tabs.clone();
        div()
            .id("empty-request-state")
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .bg(theme::bg_base())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(SharedString::from("No request open")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_muted())
                            .child(SharedString::from(
                                "Create a new request or open one from the collection tree.",
                            )),
                    )
                    .child(
                        div()
                            .id("empty-new-request")
                            .px_4()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::accent())
                            .text_sm()
                            .text_color(white())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::accent_hover()))
                            .child(SharedString::from("New Request  (Ctrl+N)"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                tabs.update(cx, |state, cx| {
                                    state.new_tab(cx);
                                });
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_bottom_console(&self, cx: &mut Context<Self>) -> AnyElement {
        let entries = self.app_state.read(cx).console_entries.clone();
        let clear_state = self.app_state.clone();
        let close_state = self.app_state.clone();
        let mut list = div().flex().flex_col().gap_1().py_1();
        for entry in entries.iter().rev().take(80) {
            let color = match entry.level {
                crate::models::ConsoleLevel::Info => theme::text_primary(),
                crate::models::ConsoleLevel::Success => theme::success(),
                crate::models::ConsoleLevel::Warning => theme::warning(),
                crate::models::ConsoleLevel::Error => theme::error(),
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("console-{}", entry.id)))
                    .flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .text_xs()
                    .child(
                        div()
                            .w(px(60.0))
                            .text_color(theme::text_muted())
                            .child(SharedString::from(entry.timestamp.clone())),
                    )
                    .child(
                        div()
                            .w(px(50.0))
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
        if entries.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_2()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(SharedString::from("No console output yet.")),
            );
        }
        div()
            .id("bottom-console")
            .flex()
            .flex_col()
            .h(px(160.0))
            .px_3()
            .pb_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(28.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from("Console")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("console-clear")
                                    .px_2()
                                    .py_1()
                                    .rounded_lg()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::bg_hover()))
                                    .child(SharedString::from("Clear"))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        clear_state.update(cx, |state, cx| state.clear_console(cx));
                                    })),
                            )
                            .child(
                                div()
                                    .id("console-close")
                                    .px_2()
                                    .py_1()
                                    .rounded_lg()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::bg_hover()))
                                    .child(SharedString::from("×"))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        close_state.update(cx, |state, cx| {
                                            state.toggle_bottom_panel(cx)
                                        });
                                    })),
                            ),
                    ),
            )
            .child(div().flex_1().overflow_scrollbar().child(list))
            .into_any_element()
    }

    fn run_command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            "New Tab" => {
                self.tabs.update(cx, |s, cx| {
                    s.new_tab(cx);
                });
            }
            "New WebSocket" => {
                self.tabs.update(cx, |s, cx| {
                    s.new_tab_with_protocol(crate::models::TabProtocol::WebSocket, cx);
                });
                self.app_state.update(cx, |s, cx| {
                    s.set_response_view(crate::models::ResponseView::Live, cx);
                });
            }
            "New SSE" => {
                self.tabs.update(cx, |s, cx| {
                    s.new_tab_with_protocol(crate::models::TabProtocol::Sse, cx);
                });
                self.app_state.update(cx, |s, cx| {
                    s.set_response_view(crate::models::ResponseView::Live, cx);
                });
            }
            "Close Tab" => {
                let id = self.tabs.read(cx).active_tab_id().map(|s| s.to_string());
                if let Some(id) = id {
                    self.tabs.update(cx, |s, cx| s.close_tab(&id, cx));
                }
            }
            "Save Request" => {
                crate::ui::save_active_tab(&self.tabs, &self.app_state, cx);
            }
            "Toggle Zen Mode" => {
                self.app_state.update(cx, |s, cx| s.toggle_zen_mode(cx));
            }
            "Toggle Side Panel" => {
                self.app_state.update(cx, |s, cx| s.toggle_side_panel(cx));
            }
            "Focus Collections" => {
                self.app_state.update(cx, |s, cx| {
                    s.active_view = crate::models::ActivityView::Collections;
                    s.side_panel_visible = true;
                    cx.notify();
                });
            }
            "Focus History" => {
                self.app_state.update(cx, |s, cx| {
                    s.active_view = crate::models::ActivityView::History;
                    s.side_panel_visible = true;
                    cx.notify();
                });
            }
            "New Collection" => {
                self.app_state.update(cx, |s, cx| {
                    s.set_overlay(
                        Overlay::NewCollection {
                            name: "New Collection".into(),
                            location: WorkspaceService::collections_root(),
                        },
                        cx,
                    );
                });
            }
            "Open Collection Folder" => {
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                window.spawn(cx, async move |cx| {
                    let Some(dir) = rfd::AsyncFileDialog::new()
                        .set_directory(WorkspaceService::collections_root())
                        .pick_folder()
                        .await
                    else {
                        return;
                    };
                    let path = dir.path().to_string_lossy().to_string();
                    let _ = cx.update(|_, cx| {
                        let _ = ws.update(cx, |ws, cx| ws.open_collection(&path, &state, cx));
                    });
                })
                .detach();
            }
            "Clear History" => {
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let _ = ws.update(cx, |ws, cx| ws.clear_history(&state, cx));
            }
            _ => {}
        }
        self.app_state.update(cx, |s, cx| s.set_command_palette_open(false, cx));
    }

    #[allow(dead_code)]
    fn sync_overlay_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let overlay = self.app_state.read(cx).overlay.clone();
        let key = match &overlay {
            Overlay::None => OverlaySyncKey::None,
            Overlay::NewCollection { .. } => OverlaySyncKey::NewCollection,
            Overlay::NewNode { parent_path, kind, name } => OverlaySyncKey::NewNode {
                parent_path: parent_path.clone(),
                kind: *kind,
                name: name.clone(),
            },
            Overlay::Rename { path, kind, name } => OverlaySyncKey::Rename {
                path: path.clone(),
                kind: *kind,
                name: name.clone(),
            },
            Overlay::EnvironmentEditor {
                original_name,
                name,
                scope,
                ..
            } => OverlaySyncKey::EnvironmentEditor {
                original_name: original_name.clone(),
                name: name.clone(),
                scope: *scope,
            },
            Overlay::SaveCase { name } => OverlaySyncKey::SaveCase { name: name.clone() },
            Overlay::SaveAs { name, location } => OverlaySyncKey::SaveAs {
                name: name.clone(),
                location: location.clone(),
            },
            Overlay::Runner {
                collection_path,
                data_path,
            } => OverlaySyncKey::Runner {
                collection_path: collection_path.clone(),
                data_path: data_path.clone(),
            },
            Overlay::Settings { .. } => OverlaySyncKey::Settings,
            _ => return,
        };
        if key == self.synced_overlay {
            return;
        }
        self.synced_overlay = key;

        match overlay {
            Overlay::NewCollection { name, location } => {
                self.new_collection_name.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
                self.new_collection_location.update(cx, |input, cx| {
                    input.set_value(location, window, cx);
                });
            }
            Overlay::NewNode { name, .. } => {
                self.new_node_name.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
            }
            Overlay::Rename { name, .. } => {
                self.rename_input.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
            }
            Overlay::SaveCase { name } => {
                self.case_name_input.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
            }
            Overlay::SaveAs { name, .. } => {
                self.save_as_name.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
            }
            Overlay::Runner { data_path, .. } => {
                self.runner_data_input.update(cx, |input, cx| {
                    input.set_value(data_path, window, cx);
                });
            }
            Overlay::Settings { .. } => {
                let (timeout, proxy) = {
                    let state = self.app_state.read(cx);
                    (state.timeout_ms.to_string(), state.proxy_url.clone())
                };
                self.settings_timeout.update(cx, |input, cx| {
                    input.set_value(timeout, window, cx);
                });
                self.settings_proxy.update(cx, |input, cx| {
                    input.set_value(proxy, window, cx);
                });
            }
            Overlay::EnvironmentEditor {
                name,
                variables,
                secrets,
                ..
            } => {
                self.env_name_input.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                });
                self.env_vars_editor.update(cx, |state, cx| {
                    state.set_value(variables, window, cx);
                });
                self.env_secrets_editor.update(cx, |state, cx| {
                    state.set_value(secrets, window, cx);
                });
            }
            _ => {}
        }
    }

    fn render_save_as_node(
        &self,
        node: &CollectionNode,
        depth: usize,
        selected: &str,
        request_name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = node.path().to_string();
        let expanded_key = format!("save-as-expanded-{}", path);
        let is_expanded = self.app_state.read(cx).is_node_expanded(&expanded_key)
            || (selected.starts_with(&path) && path != selected);
        let is_container = matches!(
            node,
            CollectionNode::Collection { .. } | CollectionNode::Folder { .. }
        );
        let is_selected = selected == path;

        let mut row = div()
            .id(SharedString::from(format!("save-as-node-{}", path)))
            .flex()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .pl(px(10.0 + depth as f32 * 16.0))
            .pr_2()
            .rounded_lg()
            .bg(if is_selected {
                theme::bg_hover()
            } else {
                transparent_black()
            })
            .cursor_pointer()
            .hover(|s| s.bg(theme::bg_hover()));

        if is_container {
            let toggle_key = expanded_key.clone();
            let toggle_state = self.app_state.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("save-as-toggle-{}", path)))
                    .w(px(14.0))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(if is_expanded { "▾" } else { "▸" }))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        toggle_state.update(cx, |state, cx| {
                            state.toggle_node(toggle_key.clone(), cx);
                        });
                    })),
            );

            let select_state = self.app_state.clone();
            let select_path = path.clone();
            let select_name = request_name.to_string();
            row = row.child(
                div()
                    .id(SharedString::from(format!("save-as-select-{}", path)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(if is_selected {
                        theme::accent()
                    } else {
                        theme::text_primary()
                    })
                    .child(SharedString::from("📁"))
                    .child(SharedString::from(node.name().to_string()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        select_state.update(cx, |state, cx| {
                            state.set_overlay(
                                Overlay::SaveAs {
                                    name: select_name.clone(),
                                    location: select_path.clone(),
                                },
                                cx,
                            );
                        });
                    })),
            );
        }

        let mut result = div().flex().flex_col().child(row);
        if is_expanded {
            for child in node.children() {
                if matches!(
                    child,
                    CollectionNode::Collection { .. } | CollectionNode::Folder { .. }
                ) {
                    result = result.child(self.render_save_as_node(
                        child,
                        depth + 1,
                        selected,
                        request_name,
                        cx,
                    ));
                }
            }
        }
        result.into_any_element()
    }

    fn render_move_target_node(
        &self,
        node: &CollectionNode,
        depth: usize,
        source_path: &str,
        move_path: &str,
        move_name: &str,
        move_kind: CollectionNodeKind,
        selected: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = node.path().to_string();
        let expanded_key = format!("move-expanded-{}", path);
        let is_expanded = self.app_state.read(cx).is_node_expanded(&expanded_key)
            || (selected.starts_with(&path) && path != selected);
        let is_selected = selected == path;

        let mut row = div()
            .id(SharedString::from(format!("move-node-{}", path)))
            .flex()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .pl(px(10.0 + depth as f32 * 16.0))
            .pr_2()
            .rounded_lg()
            .bg(if is_selected {
                theme::bg_hover()
            } else {
                transparent_black()
            })
            .cursor_pointer()
            .hover(|s| s.bg(theme::bg_hover()));

        let toggle_key = expanded_key.clone();
        let toggle_state = self.app_state.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("move-toggle-{}", path)))
                .w(px(14.0))
                .text_xs()
                .text_color(theme::text_muted())
                .child(SharedString::from(if is_expanded { "▾" } else { "▸" }))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    toggle_state.update(cx, |state, cx| {
                        state.toggle_node(toggle_key.clone(), cx);
                    });
                })),
        );

        let select_state = self.app_state.clone();
        let target_path = path.clone();
        let target_name = move_name.to_string();
        let target_source = move_path.to_string();
        row = row.child(
            div()
                .id(SharedString::from(format!("move-select-{}", path)))
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(if is_selected {
                    theme::accent()
                } else {
                    theme::text_primary()
                })
                .child(SharedString::from("📁"))
                .child(SharedString::from(node.name().to_string()))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    select_state.update(cx, |state, cx| {
                        state.set_overlay(
                            Overlay::MoveNode {
                                path: target_source.clone(),
                                kind: move_kind,
                                name: target_name.clone(),
                                target: target_path.clone(),
                            },
                            cx,
                        );
                    });
                })),
        );

        let mut result = div().flex().flex_col().child(row);
        if is_expanded {
            for child in node.children() {
                if matches!(
                    child,
                    CollectionNode::Collection { .. } | CollectionNode::Folder { .. }
                ) && !child.path().starts_with(source_path)
                {
                    result = result.child(self.render_move_target_node(
                        child,
                        depth + 1,
                        source_path,
                        move_path,
                        move_name,
                        move_kind,
                        selected,
                        cx,
                    ));
                }
            }
        }
        result.into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_overlay_inputs(window, cx);

        if Instant::now() >= self.next_watch_check {
            self.next_watch_check = Instant::now() + std::time::Duration::from_millis(800);
            if self.workspace.read(cx).take_reload_events() {
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                ws.update(cx, |ws, cx| ws.reload_workspace_data(&state, cx));
            }
        }
        // Drain WebSocket / SSE events more frequently than workspace rescans.
        if Instant::now() >= self.next_realtime_check {
            self.next_realtime_check = Instant::now() + std::time::Duration::from_millis(100);
            let state = self.app_state.clone();
            state.update(cx, |s, cx| s.poll_realtime(cx));
        }

        let zen_mode = self.app_state.read(cx).zen_mode;
        let bottom_panel_visible = self.app_state.read(cx).bottom_panel_visible;
        let has_tabs = !self.tabs.read(cx).tabs().is_empty();
        let toast = self.app_state.read(cx).toast.clone();
        let toast_deadline = self.app_state.read(cx).toast_deadline;
        let overlay = self.app_state.read(cx).overlay.clone();

        if toast.is_some()
            && toast_deadline
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(false)
        {
            let state = self.app_state.clone();
            window.on_next_frame(move |_, cx| {
                state.update(cx, |s, cx| s.clear_toast(cx));
            });
        }

        let mut root = div()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .key_context("ApiArk")
            .on_action(cx.listener(|this, _: &SaveRequest, _, cx| {
                this.tab_bar.update(cx, |bar, cx| bar.save_active(cx));
            }))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| {
                this.command_palette_input.update(cx, |input, cx| input.focus(window, cx));
                this.app_state.update(cx, |state, cx| {
                    state.set_command_palette_open(true, cx);
                });
            }))
            .on_action(cx.listener(|this, _: &NewTab, _, cx| {
                this.tabs.update(cx, |state, cx| {
                    state.new_tab(cx);
                });
            }))
            .on_action(cx.listener(|this, _: &FocusEnvironment, _, cx| {
                this.app_state.update(cx, |state, cx| {
                    state.active_view = ActivityView::Environments;
                    state.side_panel_visible = true;
                    cx.notify();
                });
            }))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg_base())
            .rounded_lg()
            .text_color(theme::text_primary());

        let main_row = div()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .child(if zen_mode {
                div().w(px(0.0)).into_any_element()
            } else {
                self.activity_bar.clone().into_any_element()
            })
            .child(if zen_mode {
                div().w(px(0.0)).into_any_element()
            } else {
                self.side_panel.clone().into_any_element()
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(self.tab_bar.clone())
                    .child(if has_tabs {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .child(self.url_bar.clone())
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .gap_1()
                                    .px_1()
                                    .pb_1()
                                    .child(self.request_panel.clone())
                                    .child(div().w(px(1.0)).bg(theme::border()))
                                    .child(self.response_panel.clone()),
                            )
                            .into_any_element()
                    } else {
                        self.render_empty_state(cx)
                    }),
            );

        root = root.child(main_row);
        if bottom_panel_visible {
            root = root.child(self.render_bottom_console(cx));
        }
        root = root.child(self.status_bar.clone());

        if let Some((message, kind)) = toast {
            let color = match kind {
                ToastKind::Info => theme::accent(),
                ToastKind::Success => theme::success(),
                ToastKind::Error => theme::error(),
            };
            let app_state = self.app_state.clone();
            root = root.child(
                div()
                    .id("toast")
                    .absolute()
                    .bottom(px(36.0))
                    .right(px(20.0))
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .border_l_3()
                    .border_color(color)
                    .shadow_lg()
                    .max_w(px(420.0))
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(SharedString::from(message))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        app_state.update(cx, |s, cx| s.clear_toast(cx));
                    })),
            );
        }

        match overlay {
            Overlay::Method { x, y } => {
                let methods = [
                    HttpMethod::Get,
                    HttpMethod::Post,
                    HttpMethod::Put,
                    HttpMethod::Patch,
                    HttpMethod::Delete,
                    HttpMethod::Head,
                    HttpMethod::Options,
                ];
                let outside_state = self.app_state.clone();
                let mut menu = div()
                    .id("method-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(120.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));
                for method in methods {
                    let tabs = self.tabs.clone();
                    let state = self.app_state.clone();
                    menu = menu.child(overlay_menu_item(
                        format!("method-{:?}", method),
                        method.as_str(),
                        method_menu_color(method),
                        move |cx| {
                            tabs.update(cx, |s, cx| s.set_method(method, cx));
                            state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ));
                }
                root = root.child(menu);
            }
            Overlay::TreeContext {
                x,
                y,
                path,
                kind,
                name,
            } => {
                let outside_state = self.app_state.clone();
                let mut menu = div()
                    .id("tree-context-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(160.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));

                if matches!(kind, CollectionNodeKind::Collection | CollectionNodeKind::Folder) {
                    let state = self.app_state.clone();
                    let target = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "ctx-new-folder",
                        "New Folder",
                        theme::text_primary(),
                        move |cx| {
                            state.update(cx, |s, cx| {
                                s.set_overlay(
                                    Overlay::NewNode {
                                        parent_path: target.clone(),
                                        kind: CollectionNodeKind::Folder,
                                        name: "New Folder".to_string(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));

                    let state = self.app_state.clone();
                    let target = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "ctx-new-request",
                        "New Request",
                        theme::text_primary(),
                        move |cx| {
                            state.update(cx, |s, cx| {
                                s.set_overlay(
                                    Overlay::NewNode {
                                        parent_path: target.clone(),
                                        kind: CollectionNodeKind::Request,
                                        name: "New Request".to_string(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                }

                if kind == CollectionNodeKind::Request {
                    let ws = self.workspace.clone();
                    let state = self.app_state.clone();
                    let toast_state = self.app_state.clone();
                    let target = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "ctx-duplicate",
                        "Duplicate",
                        theme::text_primary(),
                        move |cx| {
                            let result =
                                ws.update(cx, |ws, cx| ws.duplicate_node(&target, &state, cx));
                            finish_overlay_action(&toast_state, cx, result, "Request duplicated");
                        },
                    ));
                }

                if matches!(kind, CollectionNodeKind::Request | CollectionNodeKind::Folder) {
                    let state = self.app_state.clone();
                    let target = path.clone();
                    let node_name = name.clone();
                    menu = menu.child(overlay_menu_item(
                        "ctx-move",
                        "Move to...",
                        theme::text_primary(),
                        move |cx| {
                            let initial_target = std::path::Path::new(&target)
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| target.clone());
                            state.update(cx, |s, cx| {
                                s.set_overlay(
                                    Overlay::MoveNode {
                                        path: target.clone(),
                                        kind,
                                        name: node_name.clone(),
                                        target: initial_target,
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                }

                let state = self.app_state.clone();
                let rename_path = path.clone();
                let rename_name = name.clone();
                menu = menu.child(overlay_menu_item(
                    "ctx-rename",
                    "Rename",
                    theme::text_primary(),
                    move |cx| {
                        state.update(cx, |s, cx| {
                            s.set_overlay(
                                Overlay::Rename {
                                    path: rename_path.clone(),
                                    kind,
                                    name: rename_name.clone(),
                                },
                                cx,
                            );
                        });
                    },
                ));

                let is_panel_root = self
                    .app_state
                    .read(cx)
                    .collections
                    .iter()
                    .any(|node| node.path() == path);
                if is_panel_root {
                    let state = self.app_state.clone();
                    let forget_ws = self.workspace.clone();
                    let target = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "ctx-remove-panel",
                        "Remove from Panel",
                        theme::text_primary(),
                        move |cx| {
                            forget_ws.update(cx, |ws, _| ws.forget_collection(&target));
                            state.update(cx, |s, cx| {
                                s.remove_collection_from_panel(&target, cx);
                                s.close_overlay(cx);
                            });
                        },
                    ));
                }

                let state = self.app_state.clone();
                let target = path.clone();
                let node_name = name.clone();
                menu = menu.child(overlay_menu_item(
                    "ctx-delete",
                    "Delete",
                    theme::error(),
                    move |cx| {
                        state.update(cx, |s, cx| {
                            s.set_overlay(
                                Overlay::DeleteNode {
                                    path: target.clone(),
                                    kind,
                                    name: node_name.clone(),
                                },
                                cx,
                            );
                        });
                    },
                ));

                root = root.child(menu);
            }
            Overlay::NewCollection { .. } => {
                let name_value = self.new_collection_name.read(cx).value().to_string();
                let location_value = self.new_collection_location.read(cx).value().to_string();
                let browse_dir = location_value.clone();
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let browse_input = self.new_collection_location.clone();
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();

                let dialog = div()
                    .id("new-collection-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("new-collection-dialog")
                            .w(px(440.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from("New Collection")),
                            )
                            .child(dialog_field("Name", Input::new(&self.new_collection_name)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from("Location")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(div().flex_1().child(Input::new(&self.new_collection_location)))
                                            .child(
                                                div()
                                                    .id("browse-location")
                                                    .px_3()
                                                    .py_2()
                                                    .rounded_lg()
                                                    .bg(theme::bg_panel())
                                                    .border_1()
                                                    .border_color(theme::border())
                                                    .text_xs()
                                                    .text_color(theme::text_primary())
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme::bg_hover()))
                                                    .child(SharedString::from("Browse"))
                                                    .on_click(cx.listener(move |_, _, window, cx| {
                                                        cx.stop_propagation();
                                                        let input = browse_input.clone();
                                                        let start = browse_dir.clone();
                                                        cx.spawn_in(window, async move |_, cx| {
                                                            let Some(dir) = rfd::AsyncFileDialog::new()
                                                                .set_directory(&start)
                                                                .pick_folder()
                                                                .await
                                                            else {
                                                                return;
                                                            };
                                                            let path = dir.path().to_string_lossy().to_string();
                                                            let _ = cx.update(|window, cx| {
                                                                input.update(cx, |state, cx| {
                                                                    state.set_value(path, window, cx);
                                                                });
                                                            });
                                                        })
                                                        .detach();
                                                    }))
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-new-collection",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "create-collection",
                                        "Create",
                                        true,
                                        move |cx| {
                                            let result = ws.update(cx, |ws, cx| {
                                                ws.create_collection_at(&location_value, &name_value, &state, cx)
                                            });
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Collection created",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::NewNode {
                parent_path,
                kind,
                name: default_name,
            } => {
                let name_value = self.new_node_name.read(cx).value().to_string();
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let expand_state = self.app_state.clone();
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let parent = parent_path.clone();
                let is_folder = kind == CollectionNodeKind::Folder;
                let title = if is_folder { "New Folder" } else { "New Request" };
                let success_message = if is_folder { "Folder created" } else { "Request created" };
                let _ = default_name;

                let dialog = div()
                    .id("new-node-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("new-node-dialog")
                            .w(px(360.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from(title)),
                            )
                            .child(dialog_field("Name", Input::new(&self.new_node_name)))
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-new-node",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "confirm-new-node",
                                        "Create",
                                        true,
                                        move |cx| {
                                            let result = if kind == CollectionNodeKind::Folder {
                                                ws.update(cx, |ws, cx| {
                                                    ws.create_folder_named(
                                                        &parent,
                                                        &name_value,
                                                        &state,
                                                        cx,
                                                    )
                                                })
                                            } else {
                                                ws.update(cx, |ws, cx| {
                                                    ws.create_request_named(
                                                        &parent,
                                                        &name_value,
                                                        &state,
                                                        cx,
                                                    )
                                                })
                                            };
                                            let succeeded = result.is_ok();
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                success_message,
                                            );
                                            if succeeded {
                                                expand_state.update(cx, |s, cx| {
                                                    s.expand_node(
                                                        format!("node-{}", parent),
                                                        cx,
                                                    );
                                                });
                                            }
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::Rename {
                path,
                kind,
                name: old_name,
            } => {
                let new_name = self.rename_input.read(cx).value().to_string();
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let target = path.clone();

                let dialog = div()
                    .id("rename-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("rename-dialog")
                            .w(px(360.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from("Rename")),
                            )
                            .child(dialog_field("New name", Input::new(&self.rename_input)))
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-rename",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "confirm-rename",
                                        "Rename",
                                        true,
                                        move |cx| {
                                            let result = ws.update(cx, |ws, cx| {
                                                ws.rename_node(
                                                    &target,
                                                    kind,
                                                    &old_name,
                                                    &new_name,
                                                    &state,
                                                    cx,
                                                )
                                            });
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Renamed",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::DeleteCollection { path, name } => {
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let remove_state = self.app_state.clone();
                let delete_ws = self.workspace.clone();
                let forget_ws = delete_ws.clone();
                let delete_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let remove_path = path.clone();
                let delete_path = path.clone();
                let delete_name = name.clone();

                let dialog = div()
                    .id("delete-collection-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("delete-collection-dialog")
                            .w(px(440.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from("Delete collection")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(format!(
                                        "What should happen to “{}”?",
                                        name
                                    ))),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-delete-collection",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "remove-collection-panel",
                                        "Remove from panel",
                                        false,
                                        move |cx| {
                                            forget_ws.update(cx, |ws, _| {
                                                ws.forget_collection(&remove_path)
                                            });
                                            remove_state.update(cx, |s, cx| {
                                                s.remove_collection_from_panel(&remove_path, cx);
                                                s.close_overlay(cx);
                                            });
                                        },
                                    ))
                                    .child(dialog_button(
                                        "delete-collection-files",
                                        "Delete files",
                                        true,
                                        move |cx| {
                                            let result = delete_ws.update(cx, |ws, cx| {
                                                ws.delete_node(
                                                    &delete_path,
                                                    CollectionNodeKind::Collection,
                                                    &delete_name,
                                                    &delete_state,
                                                    cx,
                                                )
                                            });
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Collection deleted",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::DeleteNode { path, kind, name } => {
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let delete_ws = self.workspace.clone();
                let delete_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let target = path.clone();
                let node_name = name.clone();
                let label = match kind {
                    CollectionNodeKind::Folder => "folder",
                    CollectionNodeKind::Request => "request",
                    CollectionNodeKind::Case => "case",
                    CollectionNodeKind::Collection => "collection",
                };

                let dialog = div()
                    .id("delete-node-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("delete-node-dialog")
                            .w(px(400.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from("Delete confirmation")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(format!(
                                        "Delete {label} “{}”?",
                                        name
                                    ))),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-delete-node",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "confirm-delete-node",
                                        "Delete",
                                        true,
                                        move |cx| {
                                            let result = delete_ws.update(cx, |ws, cx| {
                                                ws.delete_node(
                                                    &target,
                                                    kind,
                                                    &node_name,
                                                    &delete_state,
                                                    cx,
                                                )
                                            });
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Deleted",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::MoveNode {
                path,
                kind,
                name,
                target,
            } => {
                let close_state = self.app_state.clone();
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let tabs = self.tabs.clone();
                let source = path.clone();
                let target_path = target.clone();
                let move_name = name.clone();
                let collections = self.app_state.read(cx).collections.clone();

                let mut tree = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .h(px(220.0))
                    .overflow_hidden()
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme::border());
                for node in &collections {
                    tree = tree.child(self.render_move_target_node(
                        node,
                        0,
                        &source,
                        &source,
                        &move_name,
                        kind,
                        &target,
                        cx,
                    ));
                }

                let dialog = div()
                    .id("move-node-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("move-node-dialog")
                            .w(px(440.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |s, cx| s.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::text_primary())
                                            .child(SharedString::from("Move to Folder")),
                                    )
                                    .child(
                                        div()
                                            .id("close-move-node")
                                            .w(px(24.0))
                                            .h(px(24.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_lg()
                                            .text_sm()
                                            .text_color(theme::text_muted())
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::bg_hover()))
                                            .child(SharedString::from("×"))
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.stop_propagation();
                                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(format!("Moving: {}", name))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::accent())
                                    .child(SharedString::from(format!("Destination: {}", target))),
                            )
                            .child(tree)
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-move-node",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "confirm-move-node",
                                        "Move",
                                        true,
                                        move |cx| {
                                            let result = ws.update(cx, |ws, cx| {
                                                ws.move_node(
                                                    &source,
                                                    kind,
                                                    &target_path,
                                                    &tabs,
                                                    &toast_state,
                                                    cx,
                                                )
                                            });
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Moved",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::SaveAs { name: _, location } => {
                let name_value = self.save_as_name.read(cx).value().to_string();
                let location_value = location.clone();
                let close_state = self.app_state.clone();
                let cancel_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let refresh_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let tabs_update = self.tabs.clone();
                let tab = self.tabs.read(cx).active_tab().cloned();

                let dialog = div()
                    .id("save-as-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("save-as-dialog")
                            .w(px(440.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::text_primary())
                                            .child(SharedString::from("Save Request")),
                                    )
                                    .child(
                                        div()
                                            .id("close-save-as")
                                            .w(px(24.0))
                                            .h(px(24.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_lg()
                                            .text_sm()
                                            .text_color(theme::text_muted())
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::bg_hover()))
                                            .child(SharedString::from("×"))
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.stop_propagation();
                                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                                            })),
                                    ),
                            )
                            .child(dialog_field(
                                "Request Name",
                                Input::new(&self.save_as_name),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from("Save Location")),
                                    )
                                    .child({
                                        let collections = self.app_state.read(cx).collections.clone();
                                        let mut tree = div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .h(px(220.0))
                                            .overflow_hidden()
                                            .p_1()
                                            .rounded_lg()
                                            .border_1()
                                            .border_color(theme::border());
                                        for node in &collections {
                                            tree = tree.child(self.render_save_as_node(
                                                node,
                                                0,
                                                &location_value,
                                                &name_value,
                                                cx,
                                            ));
                                        }
                                        tree
                                    }),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-save-as",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            cancel_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "confirm-save-as",
                                        "Save",
                                        true,
                                        move |cx| {
                                            let result = (|| -> Result<(), String> {
                                                let Some(tab) = tab.as_ref() else {
                                                    return Err("No active request".into());
                                                };
                                                let dir = std::path::Path::new(&location_value);
                                                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                                                let safe = collection_service::sanitize_filename(&name_value);
                                                if safe.trim().is_empty() {
                                                    return Err("Request name cannot be empty".into());
                                                }
                                                let stem = collection_service::unique_stem(
                                                    dir, &safe, "yaml", None,
                                                );
                                                let path = dir.join(format!("{}.yaml", stem));
                                                let mut file = tab.to_request_file();
                                                file.name = name_value.clone();
                                                collection_service::write_request(&path, &file)?;
                                                collection_service::append_parent_order(dir, &stem)?;
                                                let path_string = path.to_string_lossy().to_string();
                                                tabs_update.update(cx, |state, cx| {
                                                    if let Some(active) = state.active_tab_mut() {
                                                        active.file_path = Some(path_string);
                                                        active.case_name = None;
                                                        active.collection_path = Some(location_value.clone());
                                                        active.name = name_value.clone();
                                                        active.is_dirty = false;
                                                        state.save();
                                                        cx.notify();
                                                    }
                                                });
                                                let _ = ws.update(cx, |ws, cx| {
                                                    ws.refresh_collections(&refresh_state, cx)
                                                });
                                                refresh_state.update(cx, |state, cx| {
                                                    let root = state
                                                        .collections
                                                        .iter()
                                                        .find(|node| {
                                                            std::path::Path::new(&location_value)
                                                                .starts_with(std::path::Path::new(node.path()))
                                                        })
                                                        .map(|node| node.path().to_string())
                                                        .or_else(|| state.collection_path.clone());
                                                    let mut current =
                                                        std::path::PathBuf::from(&location_value);
                                                    loop {
                                                        let text = current.to_string_lossy().to_string();
                                                        let is_root = root.as_deref() == Some(text.as_str());
                                                        state.expand_node(format!("node-{}", text), cx);
                                                        if is_root || !current.pop() {
                                                            break;
                                                        }
                                                    }
                                                });
                                                Ok(())
                                            })();
                                            finish_overlay_action(
                                                &toast_state,
                                                cx,
                                                result,
                                                "Request saved",
                                            );
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::TabContext { x, y, tab_id } => {
                let outside_state = self.app_state.clone();
                let pin_state = self.app_state.clone();
                let pin_tabs = self.tabs.clone();
                let pin_id = tab_id.clone();
                let dup_state = self.app_state.clone();
                let dup_tabs = self.tabs.clone();
                let dup_id = tab_id.clone();
                let close_state = self.app_state.clone();
                let close_tabs = self.tabs.clone();
                let close_id = tab_id.clone();
                let other_state = self.app_state.clone();
                let other_tabs = self.tabs.clone();
                let other_id = tab_id.clone();
                let all_state = self.app_state.clone();
                let all_tabs = self.tabs.clone();
                let move_state = self.app_state.clone();

                let mut menu = div()
                    .id("tab-context-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(210.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));

                menu = menu
                    .child(overlay_menu_item(
                        "tab-pin",
                        "Pin / Unpin Tab",
                        theme::text_primary(),
                        move |cx| {
                            pin_tabs.update(cx, |s, cx| s.toggle_pin(&pin_id, cx));
                            pin_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "tab-duplicate",
                        "Duplicate Tab",
                        theme::text_primary(),
                        move |cx| {
                            dup_tabs.update(cx, |s, cx| s.duplicate_tab(&dup_id, cx));
                            dup_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "tab-close",
                        "Close",
                        theme::text_primary(),
                        move |cx| {
                            close_tabs.update(cx, |s, cx| s.close_tab(&close_id, cx));
                            close_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "tab-close-others",
                        "Close Others",
                        theme::text_primary(),
                        move |cx| {
                            other_tabs.update(cx, |s, cx| s.close_other_tabs(&other_id, cx));
                            other_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "tab-close-all",
                        "Close All",
                        theme::error(),
                        move |cx| {
                            all_tabs.update(cx, |s, cx| s.close_all_tabs(cx));
                            all_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "tab-move-window",
                        "Move to New Window",
                        theme::text_muted(),
                        move |cx| {
                            move_state.update(cx, |s, cx| {
                                s.show_toast(
                                    "Move to New Window is not available in this build",
                                    ToastKind::Info,
                                    cx,
                                );
                                s.close_overlay(cx);
                            });
                        },
                    ));
                root = root.child(menu);
            }
            Overlay::RequestSettings {
                verify_ssl,
                follow_redirects,
            } => {
                let timeout_value = self.settings_timeout.read(cx).value().to_string();
                let proxy_value = self.settings_proxy.read(cx).value().to_string();
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let verify_state = self.app_state.clone();
                let follow_state = self.app_state.clone();
                let apply_state = self.app_state.clone();
                let verify_next = !verify_ssl;
                let follow_next = !follow_redirects;

                let checkbox = |label: &'static str, checked: bool, id: &'static str, toggle: bool, state: Entity<AppState>| {
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_2()
                        .rounded_lg()
                        .bg(if checked { theme::bg_hover() } else { theme::bg_panel() })
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from(if checked { "☑" } else { "☐" }))
                        .child(SharedString::from(label))
                        .on_mouse_down(
                            MouseButton::Left,
                            move |_, _, cx| {
                                state.update(cx, |s, cx| {
                                    s.set_overlay(
                                        Overlay::RequestSettings {
                                            verify_ssl: if id == "settings-verify-ssl" {
                                                toggle
                                            } else {
                                                verify_next
                                            },
                                            follow_redirects: if id == "settings-follow-redirects" {
                                                toggle
                                            } else {
                                                follow_next
                                            },
                                        },
                                        cx,
                                    );
                                });
                            },
                        )
                };

                let dialog = div()
                    .id("request-settings-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("request-settings-dialog")
                            .w(px(420.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Request Settings")),
                            )
                            .child(dialog_field(
                                "Timeout (ms)",
                                Input::new(&self.settings_timeout),
                            ))
                            .child(dialog_field(
                                "Proxy URL",
                                Input::new(&self.settings_proxy),
                            ))
                            .child(checkbox(
                                "Verify SSL",
                                verify_ssl,
                                "settings-verify-ssl",
                                verify_next,
                                verify_state,
                            ))
                            .child(checkbox(
                                "Follow Redirects",
                                follow_redirects,
                                "settings-follow-redirects",
                                follow_next,
                                follow_state,
                            ))
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-request-settings",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            close_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                                    .child(dialog_button(
                                        "apply-request-settings",
                                        "Apply",
                                        true,
                                        move |cx| {
                                            let timeout = timeout_value
                                                .parse::<u64>()
                                                .unwrap_or(30000)
                                                .max(1000);
                                            apply_state.update(cx, |s, cx| {
                                                s.timeout_ms = timeout;
                                                s.verify_ssl = verify_ssl;
                                                s.follow_redirects = follow_redirects;
                                                s.proxy_url = proxy_value.clone();
                                                s.close_overlay(cx);
                                            });
                                        },
                                    )),
                            ),
                    );
                root = root.child(dialog);
            }
            Overlay::ExportFormat { path: _ } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let mut dialog = div()
                    .id("export-format-dialog-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("export-format-dialog")
                            .w(px(360.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Export Collection")),
                            )
                        );
                for (format, label) in [
                    ("apiark", "ApiArk YAML"),
                    ("postman", "Postman v2.1"),
                    ("openapi", "OpenAPI 3.x"),
                    ("insomnia", "Insomnia"),
                    ("bruno", "Bruno"),
                ] {
                    let ws = ws.clone();
                    let state = state.clone();
                    let toast_state = toast_state.clone();
                    dialog = dialog.child(dialog_button(
                        id_from_label(label),
                        label,
                        false,
                        move |cx| {
                            let result = (|| -> Result<(), String> {
                                let (filename, content) = ws.update(cx, |ws, cx| {
                                    ws.export_collection_by_app(cx, &state, format)
                                })?;
                                let dir = rfd::FileDialog::new()
                                    .set_file_name(&filename)
                                    .save_file()
                                    .ok_or("Export cancelled")?;
                                std::fs::write(&dir, content)
                                    .map_err(|e| format!("Export failed: {e}"))?;
                                Ok(())
                            })();
                            finish_overlay_action(&toast_state, cx, result, "Collection exported");
                        },
                    ));
                }
                dialog = dialog.child(dialog_button(
                    "cancel-export-format",
                    "Cancel",
                    false,
                    move |cx| {
                        close_state.update(cx, |s, cx| s.close_overlay(cx));
                    },
                ));
                root = root.child(dialog);
            }
            Overlay::CodeGen { language } => {
                let tab = self.tabs.read(cx).active_tab().cloned();
                let code = tab
                    .as_ref()
                    .map(|tab| generate_request_code(tab, &language))
                    .unwrap_or_default();
                let copy_code = code.clone();
                let toast_state = self.app_state.clone();
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let mut row = div().flex().gap_2();
                for (id, label, target) in [
                    ("code-curl", "cURL", "curl"),
                    ("code-python", "Python", "python"),
                    ("code-javascript", "JavaScript", "javascript"),
                ] {
                    let state = self.app_state.clone();
                    row = row.child(dialog_button(
                        id,
                        label,
                        language == target,
                        move |cx| {
                            state.update(cx, |s, cx| {
                                s.set_overlay(
                                    Overlay::CodeGen {
                                        language: target.to_string(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                }
                root = root.child(
                    div()
                        .id("codegen-dialog-wrap")
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("codegen-dialog")
                                .w(px(640.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_4()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(SharedString::from("Generate Code")),
                                        )
                                        .child(row),
                                )
                                .child(
                                    div()
                                        .p_3()
                                        .rounded_lg()
                                        .bg(theme::bg_base())
                                        .font_family("monospace")
                                        .text_sm()
                                        .text_color(theme::text_primary())
                                        .child(SharedString::from(code.clone())),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(dialog_button(
                                            "close-codegen",
                                            "Close",
                                            false,
                                            move |cx| {
                                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                                            },
                                        ))
                                        .child(dialog_button(
                                            "copy-codegen",
                                            "Copy",
                                            true,
                                            move |cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(copy_code.clone()));
                                                toast_state.update(cx, |s, cx| {
                                                    s.show_toast("Code copied", ToastKind::Success, cx);
                                                });
                                            },
                                        )),
                                ),
                        ),
                );
            }
            Overlay::ResponseDiff => {
                let (snapshots, left_id, right_id, view) = {
                    let state = self.app_state.read(cx);
                    (
                        state.diff_snapshots.clone(),
                        state.diff_left.clone(),
                        state.diff_right.clone(),
                        state.diff_view,
                    )
                };
                let format_response =
                    |response: &crate::models::ResponseData, view: crate::models::DiffView| {
                        match view {
                            crate::models::DiffView::Body => {
                                crate::ui::json_view::try_format_json(&response.body)
                                    .unwrap_or_else(|| response.body.clone())
                            }
                            crate::models::DiffView::Headers => response
                                .headers
                                .iter()
                                .map(|header| format!("{}: {}", header.key, header.value))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        }
                    };
                let left = left_id
                    .as_ref()
                    .and_then(|id| snapshots.iter().find(|snapshot| &snapshot.id == id));
                let right = right_id
                    .as_ref()
                    .and_then(|id| snapshots.iter().find(|snapshot| &snapshot.id == id));
                let left_label = left
                    .map(|snapshot| snapshot.label.clone())
                    .unwrap_or_else(|| "Select snapshot...".to_string());
                let right_label = right
                    .map(|snapshot| snapshot.label.clone())
                    .unwrap_or_else(|| "Select snapshot...".to_string());
                let left_text = left
                    .map(|snapshot| format_response(&snapshot.response, view))
                    .unwrap_or_default();
                let right_text = right
                    .map(|snapshot| format_response(&snapshot.response, view))
                    .unwrap_or_default();

                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let clear_state = self.app_state.clone();
                let body_state = self.app_state.clone();
                let headers_state = self.app_state.clone();
                let left_state = self.app_state.clone();
                let right_state = self.app_state.clone();

                let selector = |id: &'static str,
                                label: String,
                                side: u8,
                                state: Entity<AppState>| {
                    div()
                        .id(id)
                        .flex_1()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::border())
                        .text_xs()
                        .text_color(theme::text_primary())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from(label))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                state.update(cx, |state, cx| {
                                    state.set_overlay(
                                        Overlay::DiffPicker {
                                            side,
                                            x: event.position.x.as_f32(),
                                            y: event.position.y.as_f32() + 24.0,
                                        },
                                        cx,
                                    );
                                });
                            }),
                        )
                };

                root = root.child(
                    div()
                        .id("diff-dialog-wrap")
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("diff-dialog")
                                .w(px(880.0))
                                .h(px(460.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_4()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(SharedString::from("Compare Responses")),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .gap_2()
                                                .child(dialog_button(
                                                    "diff-body",
                                                    "Body",
                                                    view == crate::models::DiffView::Body,
                                                    move |cx| {
                                                        body_state.update(cx, |state, cx| {
                                                            state.set_diff_view(
                                                                crate::models::DiffView::Body,
                                                                cx,
                                                            );
                                                        });
                                                    },
                                                ))
                                                .child(dialog_button(
                                                    "diff-headers",
                                                    "Headers",
                                                    view == crate::models::DiffView::Headers,
                                                    move |cx| {
                                                        headers_state.update(cx, |state, cx| {
                                                            state.set_diff_view(
                                                                crate::models::DiffView::Headers,
                                                                cx,
                                                            );
                                                        });
                                                    },
                                                ))
                                                .child(dialog_button(
                                                    "diff-clear",
                                                    "Clear",
                                                    false,
                                                    move |cx| {
                                                        let result = clear_state.update(cx, |state, cx| {
                                                            state.clear_diff_snapshots(cx)
                                                        });
                                                        let _ = result;
                                                    },
                                                ))
                                                .child(dialog_button(
                                                    "close-diff",
                                                    "Close",
                                                    false,
                                                    move |cx| {
                                                        close_state.update(cx, |s, cx| s.close_overlay(cx));
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .child(
                                            div()
                                                .w(px(40.0))
                                                .text_xs()
                                                .text_color(theme::text_muted())
                                                .child(SharedString::from("Left")),
                                        )
                                        .child(selector(
                                            "diff-left-select",
                                            left_label,
                                            0,
                                            left_state.clone(),
                                        ))
                                        .child(
                                            div()
                                                .w(px(40.0))
                                                .text_xs()
                                                .text_color(theme::text_muted())
                                                .child(SharedString::from("Right")),
                                        )
                                        .child(selector(
                                            "diff-right-select",
                                            right_label,
                                            1,
                                            right_state.clone(),
                                        )),
                                )
                                .child(if snapshots.is_empty() {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_sm()
                                        .text_color(theme::text_muted())
                                        .child(SharedString::from(
                                            "Save responses using Save for Diff, then select them above to compare.",
                                        ))
                                        .into_any_element()
                                } else {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .gap_3()
                                        .child(
                                            div()
                                                .flex_1()
                                                .p_3()
                                                .rounded_lg()
                                                .bg(theme::bg_base())
                                                .font_family("monospace")
                                                .text_xs()
                                                .child(SharedString::from(if left_text.is_empty() {
                                                    "Select a snapshot".to_string()
                                                } else {
                                                    left_text
                                                })),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .p_3()
                                                .rounded_lg()
                                                .bg(theme::bg_base())
                                                .font_family("monospace")
                                                .text_xs()
                                                .child(SharedString::from(if right_text.is_empty() {
                                                    "Select a snapshot".to_string()
                                                } else {
                                                    right_text
                                                })),
                                        )
                                        .into_any_element()
                                }),
                        ),
                );
            }
            Overlay::ImportPreview { message, is_error } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                root = root.child(
                    div()
                        .id("import-preview-wrap")
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("import-preview-dialog")
                                .w(px(460.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_4()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if is_error {
                                            theme::error()
                                        } else {
                                            theme::text_primary()
                                        })
                                        .child(SharedString::from(if is_error {
                                            "Import Failed"
                                        } else {
                                            "Import Preview"
                                        })),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_primary())
                                        .child(SharedString::from(message.clone())),
                                )
                                .child(dialog_button(
                                    "close-import-preview",
                                    "OK",
                                    true,
                                    move |cx| {
                                        close_state.update(cx, |s, cx| s.close_overlay(cx));
                                    },
                                )),
                        ),
                );
            }
            Overlay::EnvironmentEditor {
                original_name,
                name: _,
                scope,
                variables: _,
                secrets: _,
            } => {
                let name_value = self.env_name_input.read(cx).value().to_string();
                let variables_value = self.env_vars_editor.read(cx).value().to_string();
                let secrets_value = self.env_secrets_editor.read(cx).value().to_string();
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let save_state = self.app_state.clone();
                let delete_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let toggle_state = self.app_state.clone();
                let original_for_toggle = original_name.clone();
                let name_for_toggle = name_value.clone();
                let variables_for_toggle = variables_value.clone();
                let secrets_for_toggle = secrets_value.clone();
                let scope_next = match scope {
                    crate::models::EnvironmentScope::Shared => {
                        crate::models::EnvironmentScope::Personal
                    }
                    crate::models::EnvironmentScope::Personal => {
                        crate::models::EnvironmentScope::Shared
                    }
                };
                let scope_label = match scope {
                    crate::models::EnvironmentScope::Shared => "Shared",
                    crate::models::EnvironmentScope::Personal => "Personal",
                };
                let original_for_save = original_name.clone();
                let name_for_delete = name_value.clone();
                let original_for_delete = original_name.clone();

                let mut dialog = div()
                    .id("environment-editor-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("environment-editor")
                            .w(px(520.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(SharedString::from(if original_name.is_some() {
                                                "Edit Environment"
                                            } else {
                                                "New Environment"
                                            })),
                                    )
                                    .child(dialog_button(
                                        "env-scope-toggle",
                                        scope_label,
                                        false,
                                        move |cx| {
                                            toggle_state.update(cx, |state, cx| {
                                                state.set_overlay(
                                                    Overlay::EnvironmentEditor {
                                                        original_name: original_for_toggle.clone(),
                                                        name: name_for_toggle.clone(),
                                                        scope: scope_next,
                                                        variables: variables_for_toggle.clone(),
                                                        secrets: secrets_for_toggle.clone(),
                                                    },
                                                    cx,
                                                );
                                            });
                                        },
                                    )),
                            )
                            .child(dialog_field(
                                "Name",
                                Input::new(&self.env_name_input),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from("Variables (KEY=value)")),
                                    )
                                    .child(
                                        div()
                                            .h(px(160.0))
                                            .rounded_lg()
                                            .border_1()
                                            .border_color(theme::border())
                                            .p_2()
                                            .font_family("monospace")
                                            .child(
                                                Editor::new(&self.env_vars_editor)
                                                    .h_full()
                                                    .bordered(false),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from("Secret Keys (one per line)")),
                                    )
                                    .child(
                                        div()
                                            .h(px(100.0))
                                            .rounded_lg()
                                            .border_1()
                                            .border_color(theme::border())
                                            .p_2()
                                            .font_family("monospace")
                                            .child(
                                                Editor::new(&self.env_secrets_editor)
                                                    .h_full()
                                                    .bordered(false),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .justify_end()
                                    .gap_2()
                                    .child(dialog_button(
                                        "cancel-env-editor",
                                        "Cancel",
                                        false,
                                        move |cx| {
                                            close_state.update(cx, |s, cx| s.close_overlay(cx));
                                        },
                                    ))
                            )
                        );
                if let Some(original) = original_name {
                    let ws_delete = ws.clone();
                    let state_delete = save_state.clone();
                    let scope_delete = scope;
                    dialog = dialog.child(dialog_button(
                        "delete-env",
                        "Delete",
                        false,
                        move |cx| {
                            let result = ws_delete.update(cx, |ws, cx| {
                                ws.delete_environment_config(
                                    &name_for_delete,
                                    scope_delete,
                                    &delete_state,
                                    cx,
                                )
                            });
                            finish_overlay_action(&state_delete, cx, result, "Environment deleted");
                        },
                    ));
                    let _ = (original, original_for_delete);
                }
                dialog = dialog.child(dialog_button(
                    "save-env",
                    "Save",
                    true,
                    move |cx| {
                        let result = ws.update(cx, |ws, cx| {
                            ws.save_environment_config(
                                original_for_save.as_deref(),
                                &name_value,
                                scope,
                                &variables_value,
                                &secrets_value,
                                &save_state,
                                cx,
                            )
                        });
                        finish_overlay_action(&save_state, cx, result, "Environment saved");
                    },
                ));
                root = root.child(dialog);
            }
            Overlay::DeleteEnvironment { name, scope } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let delete_ws = self.workspace.clone();
                let delete_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let delete_name = name.clone();
                root = root.child(
                    div()
                        .id("delete-env-dialog-wrap")
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("delete-env-dialog")
                                .w(px(380.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_4()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from("Delete Environment")),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_muted())
                                        .child(SharedString::from(format!(
                                            "Delete environment “{}”?",
                                            name
                                        ))),
                                )
                                .child(
                                    div()
                                        .mt_2()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(dialog_button(
                                            "cancel-delete-env",
                                            "Cancel",
                                            false,
                                            move |cx| {
                                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                                            },
                                        ))
                                        .child(dialog_button(
                                            "confirm-delete-env",
                                            "Delete",
                                            true,
                                            move |cx| {
                                                let result = delete_ws.update(cx, |ws, cx| {
                                                    ws.delete_environment_config(
                                                        &delete_name,
                                                        scope,
                                                        &delete_state,
                                                        cx,
                                                    )
                                                });
                                                finish_overlay_action(
                                                    &toast_state,
                                                    cx,
                                                    result,
                                                    "Environment deleted",
                                                );
                                            },
                                        )),
                                ),
                        ),
                );
            }
            Overlay::CurlImport => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let tabs = self.tabs.clone();
                let curl_text = self.curl_input.read(cx).value().to_string();

                root = root.child(
                    div()
                        .id("curl-import-wrap")
                        .absolute()
                        .top(px(0.0))
                        .left(px(0.0))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("curl-import-dialog")
                                .w(px(620.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_4()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(SharedString::from("Import cURL")),
                                        )
                                        .child(dialog_button(
                                            "close-curl-import",
                                            "Close",
                                            false,
                                            move |cx| {
                                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                                            },
                                        )),
                                )
                                .child(
                                    div()
                                        .h(px(260.0))
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(theme::border())
                                        .p_3()
                                        .font_family("monospace")
                                        .child(
                                            Editor::new(&self.curl_input)
                                                .h_full()
                                                .bordered(false),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .child(dialog_button(
                                            "confirm-curl-import",
                                            "Import",
                                            true,
                                            move |cx| {
                                                let result = (|| -> Result<(), String> {
                                                    let parsed = crate::services::curl::parse_curl(&curl_text)?;
                                                    tabs.update(cx, |state, cx| {
                                                        state.apply_parsed_curl(&parsed, cx);
                                                    });
                                                    Ok(())
                                                })();
                                                finish_overlay_action(
                                                    &toast_state,
                                                    cx,
                                                    result,
                                                    "cURL imported",
                                                );
                                            },
                                        )),
                                ),
                        ),
                );
            }
            Overlay::CollectionDefaults {
                send_cookies,
                store_cookies,
                persist_cookies,
            } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let save_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let ws = self.workspace.clone();

                let mut dialog = div()
                    .id("collection-defaults-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("collection-defaults-dialog")
                            .w(px(380.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Collection Defaults")),
                            ),
                    );

                for (id, label, checked, target) in [
                    ("defaults-send-cookies", "Send cookies", send_cookies, 0u8),
                    ("defaults-store-cookies", "Store cookies", store_cookies, 1u8),
                    (
                        "defaults-persist-cookies",
                        "Persist cookies",
                        persist_cookies,
                        2u8,
                    ),
                ] {
                    let state = self.app_state.clone();
                    let next_send = if target == 0 { !send_cookies } else { send_cookies };
                    let next_store = if target == 1 { !store_cookies } else { store_cookies };
                    let next_persist = if target == 2 {
                        !persist_cookies
                    } else {
                        persist_cookies
                    };
                    dialog = dialog.child(dialog_button(id, label, checked, move |cx| {
                        state.update(cx, |s, cx| {
                            s.set_overlay(
                                Overlay::CollectionDefaults {
                                    send_cookies: next_send,
                                    store_cookies: next_store,
                                    persist_cookies: next_persist,
                                },
                                cx,
                            );
                        });
                    }));
                }

                dialog = dialog
                    .child(dialog_button(
                        "cancel-collection-defaults",
                        "Cancel",
                        false,
                        move |cx| {
                            close_state.update(cx, |s, cx| s.close_overlay(cx));
                        },
                    ))
                    .child(dialog_button(
                        "save-collection-defaults",
                        "Save",
                        true,
                        move |cx| {
                            let defaults = crate::models::CollectionDefaults {
                                auth: None,
                                send_cookies,
                                store_cookies,
                                persist_cookies,
                            };
                            let result = ws.update(cx, |ws, cx| {
                                ws.save_collection_defaults_config(defaults, &save_state, cx)
                            });
                            finish_overlay_action(
                                &toast_state,
                                cx,
                                result,
                                "Collection defaults saved",
                            );
                        },
                    ));
                root = root.child(dialog);
            }
            Overlay::SaveCase { name: _ } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let toast_state = self.app_state.clone();
                let refresh_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let tabs = self.tabs.clone();
                let name_value = self.case_name_input.read(cx).value().to_string();
                let mut dialog = div()
                    .id("save-case-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("save-case-dialog")
                            .w(px(360.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Save as Case")),
                            )
                            .child(dialog_field("Name", Input::new(&self.case_name_input))),
                    );
                dialog = dialog.child(
                    div()
                        .mt_2()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(dialog_button(
                            "cancel-save-case",
                            "Cancel",
                            false,
                            move |cx| {
                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                            },
                        ))
                        .child(dialog_button(
                            "confirm-save-case",
                            "Save",
                            true,
                            move |cx| {
                                let result =
                                    tabs.update(cx, |state, cx| state.save_case(&name_value, cx));
                                if result.is_ok() {
                                    let _ = ws.update(cx, |ws, cx| {
                                        ws.refresh_collections(&refresh_state, cx)
                                    });
                                }
                                finish_overlay_action(&toast_state, cx, result, "Case saved");
                            },
                        )),
                );
                root = root.child(dialog);
            }
            Overlay::CaseMenu { cases } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let tabs = self.tabs.clone();
                let mut list = div().flex().flex_col().gap_1();
                for (index, case) in cases.into_iter().enumerate() {
                    let tabs = tabs.clone();
                    let close_state = close_state.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("case-{index}")))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(case))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                tabs.update(cx, |state, cx| state.activate_case(index, cx));
                                close_state.update(cx, |s, cx| s.close_overlay(cx));
                            }),
                    );
                }
                root = root.child(
                    div()
                        .id("case-menu-wrap")
                        .absolute()
                        .top(px(120.0))
                        .right(px(24.0))
                        .child(
                            div()
                                .id("case-menu-dialog")
                                .w(px(220.0))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .p_2()
                                .rounded_lg()
                                .bg(theme::bg_surface())
                                .border_1()
                                .border_color(theme::border())
                                .shadow_lg()
                                .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    outside_state.update(cx, |state, cx| state.close_overlay(cx));
                                }))
                                .child(list),
                        ),
                );
            }
            Overlay::SaveMenu { x, y } => {
                let outside_state = self.app_state.clone();
                let close_state = self.app_state.clone();
                let save_tabs = self.tabs.clone();
                let save_state = self.app_state.clone();
                let case_state = self.app_state.clone();
                let mut menu = div()
                    .id("save-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(180.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));
                menu = menu
                    .child(overlay_menu_item(
                        "save-menu-request",
                        "Save Request",
                        theme::text_primary(),
                        move |cx| {
                            crate::ui::save_active_tab(&save_tabs, &save_state, cx);
                            close_state.update(cx, |state, cx| state.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "save-menu-case",
                        "Save as Case",
                        theme::text_primary(),
                        move |cx| {
                            case_state.update(cx, |state, cx| {
                                state.set_overlay(
                                    Overlay::SaveCase {
                                        name: "New Case".to_string(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                root = root.child(menu);
            }
            Overlay::UrlMoreMenu { x, y } => {
                let outside_state = self.app_state.clone();
                let url_bar = self.url_bar.clone();
                let close_state = self.app_state.clone();
                let import_state = self.app_state.clone();
                let settings_state = self.app_state.clone();
                let prefs_state = self.app_state.clone();
                let mut menu = div()
                    .id("url-more-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(180.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));
                menu = menu
                    .child(overlay_menu_item(
                        "url-more-curl",
                        "Copy as cURL",
                        theme::text_primary(),
                        move |cx| {
                            url_bar.update(cx, |bar, cx| bar.copy_as_curl(cx));
                            close_state.update(cx, |state, cx| state.close_overlay(cx));
                        },
                    ))
                    .child(overlay_menu_item(
                        "url-more-import",
                        "Import cURL",
                        theme::text_primary(),
                        move |cx| {
                            import_state.update(cx, |state, cx| {
                                state.set_overlay(Overlay::CurlImport, cx);
                            });
                        },
                    ))
                    .child(overlay_menu_item(
                        "url-more-settings",
                        "Request Settings",
                        theme::text_primary(),
                        move |cx| {
                            let (verify_ssl, follow_redirects) = {
                                let state = settings_state.read(cx);
                                (state.verify_ssl, state.follow_redirects)
                            };
                            settings_state.update(cx, |state, cx| {
                                state.set_overlay(
                                    Overlay::RequestSettings {
                                        verify_ssl,
                                        follow_redirects,
                                    },
                                    cx,
                                );
                            });
                        },
                    ))
                    .child(overlay_menu_item(
                        "url-more-preferences",
                        "Preferences",
                        theme::text_primary(),
                        {
                            move |cx| {
                                prefs_state.update(cx, |s, cx| {
                                    s.set_overlay(Overlay::Settings { section: 0 }, cx);
                                });
                            }
                        },
                    ));
                root = root.child(menu);
            }
            Overlay::CollectionsMoreMenu { x, y, path, name } => {
                let outside_state = self.app_state.clone();
                let close_state = self.app_state.clone();
                let ws = self.workspace.clone();
                let state = self.app_state.clone();
                let mut menu = div()
                    .id("collections-more-menu")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(190.0))
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        outside_state.update(cx, |state, cx| state.close_overlay(cx));
                    }));
                menu = menu
                    .child(overlay_menu_item(
                        "collections-backup",
                        "Backup State",
                        theme::text_primary(),
                        {
                            let ws = ws.clone();
                            let state = state.clone();
                            let close = close_state.clone();
                            move |cx| {
                                if let Some(file) = rfd::FileDialog::new()
                                    .set_file_name("apiark-backup.zip")
                                    .save_file()
                                {
                                    let result = ws.update(cx, |ws, _| ws.create_backup(&file));
                                    state.update(cx, |s, cx| {
                                        match result {
                                            Ok(()) => s.show_toast("Backup created", ToastKind::Success, cx),
                                            Err(e) => s.show_toast(format!("Backup failed: {e}"), ToastKind::Error, cx),
                                        }
                                    });
                                }
                                close.update(cx, |s, cx| s.close_overlay(cx));
                            }
                        },
                    ))
                    .child(overlay_menu_item(
                        "collections-restore",
                        "Restore State",
                        theme::text_primary(),
                        {
                            let ws = ws.clone();
                            let state = state.clone();
                            let close = close_state.clone();
                            move |cx| {
                                if let Some(file) = rfd::FileDialog::new()
                                    .add_filter("Backup", &["zip"])
                                    .pick_file()
                                {
                                    let result = ws.update(cx, |ws, cx| {
                                        ws.restore_backup(&file, &state, cx)
                                    });
                                    state.update(cx, |s, cx| {
                                        match result {
                                            Ok(()) => s.show_toast("Backup restored", ToastKind::Success, cx),
                                            Err(e) => s.show_toast(format!("Restore failed: {e}"), ToastKind::Error, cx),
                                        }
                                    });
                                }
                                close.update(cx, |s, cx| s.close_overlay(cx));
                            }
                        },
                    ));
                if let Some(ref path) = path {
                    let runner_state = state.clone();
                    let runner_path = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "collections-runner",
                        "Run Collection",
                        theme::text_primary(),
                        move |cx| {
                            runner_state.update(cx, |s, cx| {
                                s.set_runner_results(Vec::new(), cx);
                                s.set_overlay(
                                    Overlay::Runner {
                                        collection_path: runner_path.clone(),
                                        data_path: String::new(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                    let defaults_state = state.clone();
                    let defaults_path = path.clone();
                    let defaults_close = close_state.clone();
                    menu = menu.child(overlay_menu_item(
                        "collections-defaults",
                        "Collection Defaults",
                        theme::text_primary(),
                        move |cx| {
                            if let Ok(config) = crate::services::collection::load_collection_config(
                                std::path::Path::new(&defaults_path),
                            ) {
                                defaults_state.update(cx, |s, cx| {
                                    s.set_overlay(
                                        Overlay::CollectionDefaults {
                                            send_cookies: config.defaults.send_cookies,
                                            store_cookies: config.defaults.store_cookies,
                                            persist_cookies: config.defaults.persist_cookies,
                                        },
                                        cx,
                                    );
                                });
                            } else {
                                defaults_close.update(cx, |s, cx| s.close_overlay(cx));
                            }
                        },
                    ));
                    let export_state = state.clone();
                    let export_path = path.clone();
                    menu = menu.child(overlay_menu_item(
                        "collections-export",
                        "Export Collection",
                        theme::text_primary(),
                        move |cx| {
                            export_state.update(cx, |s, cx| {
                                s.set_overlay(
                                    Overlay::ExportFormat {
                                        path: export_path.clone(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                    if let Some(ref name) = name {
                        let delete_state = state.clone();
                        let delete_path = path.clone();
                        let delete_name = name.clone();
                        menu = menu.child(overlay_menu_item(
                            "collections-delete",
                            "Delete Collection",
                            theme::error(),
                            move |cx| {
                                delete_state.update(cx, |s, cx| {
                                    s.set_overlay(
                                        Overlay::DeleteCollection {
                                            path: delete_path.clone(),
                                            name: delete_name.clone(),
                                        },
                                        cx,
                                    );
                                });
                            },
                        ));
                    }
                }
                root = root.child(menu);
            }
            Overlay::Runner {
                collection_path,
                data_path: _,
            } => {
                let close_state = self.app_state.clone();
                let data_path = self.runner_data_input.read(cx).value().to_string();
                let running = self.app_state.read(cx).runner_running;
                let results = self.app_state.read(cx).runner_results.clone();
                let total = results.len();
                let passed = results.iter().filter(|result| result.passed).count();

                let mut dialog = div()
                    .id("runner-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("runner-dialog")
                            .w(px(720.0))
                            .h(px(560.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(SharedString::from("Run Collection")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(SharedString::from(collection_path.clone())),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(Input::new(&self.runner_data_input)),
                                    )
                                    .child({
                                        let input = self.runner_data_input.clone();
                                        div()
                                            .id("runner-browse")
                                            .px_3()
                                            .py_2()
                                            .rounded_lg()
                                            .bg(theme::bg_panel())
                                            .border_1()
                                            .border_color(theme::border())
                                            .text_xs()
                                            .text_color(theme::text_primary())
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::bg_hover()))
                                            .child(SharedString::from("Browse"))
                                            .on_click(cx.listener(move |_, _, window, cx| {
                                                cx.stop_propagation();
                                                let Some(file) = rfd::FileDialog::new()
                                                    .add_filter("CSV", &["csv"])
                                                    .pick_file()
                                                else {
                                                    return;
                                                };
                                                let path = file.to_string_lossy().to_string();
                                                input.update(cx, |state, cx| {
                                                    state.set_value(path, window, cx);
                                                });
                                            }))
                                    })
                                    .child({
                                        let run_collection_path = collection_path.clone();
                                        let run_data_path = data_path.clone();
                                        div()
                                            .id("runner-run")
                                            .px_3()
                                            .py_2()
                                            .rounded_lg()
                                            .bg(theme::accent())
                                            .text_xs()
                                            .text_color(white())
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme::accent_hover()))
                                            .child(SharedString::from(if running {
                                                "Running..."
                                            } else {
                                                "Run"
                                            }))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.start_runner(
                                                    window,
                                                    cx,
                                                    run_collection_path.clone(),
                                                    run_data_path.clone(),
                                                );
                                            }))
                                    }),
                            ),
                    );

                if !results.is_empty() {
                    let mut table = div().flex().flex_col().flex_1().overflow_hidden().gap_1();
                    table = table.child(
                        div()
                            .flex()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_muted())
                            .child(div().w(px(60.0)).child(SharedString::from("Status")))
                            .child(div().w(px(180.0)).child(SharedString::from("Name")))
                            .child(div().flex_1().child(SharedString::from("URL")))
                            .child(div().w(px(70.0)).child(SharedString::from("Time"))),
                    );
                    for (idx, result) in results.iter().enumerate() {
                        table = table.child(
                            div()
                                .id(SharedString::from(format!("runner-result-{}", idx)))
                                .flex()
                                .items_start()
                                .text_xs()
                                .child(
                                    div()
                                        .w(px(60.0))
                                        .text_color(if result.passed {
                                            theme::success()
                                        } else {
                                            theme::error()
                                        })
                                        .child(SharedString::from(if result.passed {
                                            "PASS"
                                        } else {
                                            "FAIL"
                                        })),
                                )
                                .child(
                                    div()
                                        .w(px(180.0))
                                        .child(SharedString::from(result.name.clone())),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(theme::text_muted())
                                        .child(SharedString::from(result.url.clone())),
                                )
                                .child(
                                    div()
                                        .w(px(70.0))
                                        .child(SharedString::from(format!("{}ms", result.time_ms))),
                                ),
                        );
                    }
                    dialog = dialog.child(
                        div()
                            .text_xs()
                            .text_color(if passed == total {
                                theme::success()
                            } else {
                                theme::error()
                            })
                            .child(SharedString::from(format!("{passed} passed"))),
                    );
                    dialog = dialog.child(table);
                } else {
                    dialog = dialog.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(theme::text_muted())
                            .child(SharedString::from(
                                "No runs yet. Select an optional CSV file and click Run.",
                            )),
                    );
                }

                dialog = dialog.child(dialog_button(
                    "runner-close",
                    "Close",
                    false,
                    move |cx| {
                        close_state.update(cx, |s, cx| s.close_overlay(cx));
                    },
                ));
                root = root.child(dialog);
            }
            Overlay::Settings { section } => {
                let close_state = self.app_state.clone();
                let outside_state = self.app_state.clone();
                let (timeout_value, proxy_value, verify_ssl, follow_redirects, dark_mode) = {
                    let state = self.app_state.read(cx);
                    (
                        state.timeout_ms.to_string(),
                        state.proxy_url.clone(),
                        state.verify_ssl,
                        state.follow_redirects,
                        state.dark_mode,
                    )
                };

                let mut dialog = div()
                    .id("settings-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("settings-dialog")
                            .w(px(620.0))
                            .h(px(480.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                outside_state.update(cx, |state, cx| state.close_overlay(cx));
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Preferences")),
                            ),
                    );

                let mut tabs = div().flex().items_center().gap_2();
                let section_state = self.app_state.clone();
                for (index, label) in ["General", "Requests", "Shortcuts", "Theme"].into_iter().enumerate() {
                    let state = section_state.clone();
                    tabs = tabs.child(dialog_button(
                        match index {
                            0 => "settings-general",
                            1 => "settings-requests",
                            2 => "settings-shortcuts",
                            _ => "settings-theme",
                        },
                        label,
                        section == index as u8,
                        move |cx| {
                            state.update(cx, |state, cx| {
                                state.set_overlay(
                                    Overlay::Settings {
                                        section: index as u8,
                                    },
                                    cx,
                                );
                            });
                        },
                    ));
                }
                dialog = dialog.child(tabs);

                let mut content = div().flex().flex_col().flex_1().gap_2();
                match section {
                    0 => {
                        content = content
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from("Workspace")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(
                                        "Requests, tabs, environments, and the last collection are restored automatically.",
                                    )),
                            );
                    }
                    1 => {
                        content = content
                            .child(dialog_field("Timeout (ms)", Input::new(&self.settings_timeout)))
                            .child(dialog_field("Proxy URL", Input::new(&self.settings_proxy)))
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from(format!(
                                        "Verify SSL: {}",
                                        if verify_ssl { "on" } else { "off" }
                                    )))
                                    .child(SharedString::from(format!(
                                        "Follow redirects: {}",
                                        if follow_redirects { "on" } else { "off" }
                                    ))),
                            );
                        let apply_state = self.app_state.clone();
                        content = content.child(dialog_button(
                            "settings-apply-requests",
                            "Apply Request Defaults",
                            true,
                            move |cx| {
                                let timeout = timeout_value.parse::<u64>().unwrap_or(30000);
                                apply_state.update(cx, |state, cx| {
                                    state.apply_request_settings(
                                        timeout,
                                        proxy_value.clone(),
                                        verify_ssl,
                                        follow_redirects,
                                        cx,
                                    );
                                    state.show_toast("Request defaults applied", ToastKind::Success, cx);
                                });
                            },
                        ));
                    }
                    2 => {
                        let conflicts = shortcut_conflicts();
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(if conflicts.is_empty() {
                                    theme::success()
                                } else {
                                    theme::error()
                                })
                                .child(SharedString::from(if conflicts.is_empty() {
                                    "No shortcut conflicts detected".to_string()
                                } else {
                                    format!("Conflicts: {}", conflicts.join(", "))
                                })),
                        );
                        for (label, key) in SETTINGS_SHORTCUTS {
                            content = content.child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .child(
                                        div().text_color(theme::text_primary()).child(
                                            SharedString::from((*label).to_string()),
                                        ),
                                    )
                                    .child(
                                        div().text_color(theme::text_muted()).child(
                                            SharedString::from((*key).to_string()),
                                        ),
                                    ),
                            );
                        }
                    }
                    _ => {
                        let light_state = self.app_state.clone();
                        let dark_state = self.app_state.clone();
                        content = content
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(dialog_button(
                                        "settings-theme-light",
                                        "Light",
                                        !dark_mode,
                                        move |cx| {
                                            light_state.update(cx, |state, cx| {
                                                state.set_dark_mode(false, cx);
                                            });
                                            Theme::change(ThemeMode::Light, None, cx);
                                        },
                                    ))
                                    .child(dialog_button(
                                        "settings-theme-dark",
                                        "Dark",
                                        dark_mode,
                                        move |cx| {
                                            dark_state.update(cx, |state, cx| {
                                                state.set_dark_mode(true, cx);
                                            });
                                            Theme::change(ThemeMode::Dark, None, cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(
                                        "System and black themes are not exposed by the current UI toolkit.",
                                    )),
                            );
                    }
                }
                dialog = dialog.child(content).child(dialog_button(
                    "settings-close",
                    "Close",
                    false,
                    move |cx| {
                        close_state.update(cx, |state, cx| state.close_overlay(cx));
                    },
                ));
                root = root.child(dialog);
            }
            Overlay::DiffPicker { side, x, y } => {
                let state = self.app_state.read(cx);
                let snapshots = state.diff_snapshots.clone();
                let selected = if side == 0 {
                    state.diff_left.clone()
                } else {
                    state.diff_right.clone()
                };
                let picker_state = self.app_state.clone();
                let mut menu = div()
                    .id("diff-picker")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(260.0))
                    .max_h(px(300.0))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_1()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        picker_state.update(cx, |state, cx| {
                            state.set_overlay(Overlay::ResponseDiff, cx);
                        });
                    }));
                if snapshots.is_empty() {
                    menu = menu.child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(theme::text_muted())
                            .child(SharedString::from("No saved responses")),
                    );
                }
                for snapshot in snapshots {
                    let state = self.app_state.clone();
                    let id = snapshot.id.clone();
                    let is_selected = selected.as_deref() == Some(snapshot.id.as_str());
                    menu = menu.child(
                        div()
                            .id(SharedString::from(format!("diff-pick-{}", snapshot.id)))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(if is_selected {
                                theme::bg_hover()
                            } else {
                                theme::bg_surface()
                            })
                            .text_sm()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from(snapshot.label))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                state.update(cx, |state, cx| {
                                    state.set_diff_side(side, id.clone(), cx);
                                    state.set_overlay(Overlay::ResponseDiff, cx);
                                });
                            }),
                    );
                }
                root = root.child(menu);
            }
            Overlay::None => {}
        }

        let command_palette_open = self.app_state.read(cx).command_palette_open;
        if command_palette_open {
            let close_state = self.app_state.clone();
            let query = self.command_palette_input.read(cx).value().to_string();
            let commands = [
                ("New Tab", "Create a new request tab"),
                ("New WebSocket", "Open a WebSocket connection tab"),
                ("New SSE", "Open an SSE stream tab"),
                ("Close Tab", "Close the active request tab"),
                ("Save Request", "Save or save-as the active request"),
                ("Toggle Zen Mode", "Show or hide panels"),
                ("Toggle Side Panel", "Collapse or expand the side panel"),
                ("Focus Collections", "Show the collection tree"),
                ("Focus History", "Show request history"),
                ("New Collection", "Create a new collection"),
                ("Open Collection Folder", "Open an existing folder as a collection"),
                ("Clear History", "Delete all request history"),
            ];
            let needle = query.trim().to_lowercase();
            let visible: Vec<(String, String)> = commands
                .iter()
                .filter(|(label, hint)| {
                    needle.is_empty()
                        || label.to_lowercase().contains(&needle)
                        || hint.to_lowercase().contains(&needle)
                })
                .map(|(label, hint)| (label.to_string(), hint.to_string()))
                .collect();

            let mut list = div()
                .flex()
                .flex_col()
                .gap_1()
                .max_h(px(320.0))
                .overflow_hidden();
            if visible.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme::text_muted())
                        .child(SharedString::from("No matching commands")),
                );
            }
            for (index, (label, hint)) in visible.into_iter().enumerate() {
                let command = label.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("command-palette-{}", index)))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child(SharedString::from(label)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(SharedString::from(hint)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.run_command(&command, window, cx);
                            }),
                        ),
                );
            }

            root = root.child(
                div()
                    .id("command-palette-wrap")
                    .absolute()
                    .top(px(80.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("command-palette")
                            .w(px(520.0))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                close_state.update(cx, |state, cx| {
                                    state.set_command_palette_open(false, cx)
                                });
                            }))
                            .child(Input::new(&self.command_palette_input))
                            .child(list),
                    ),
            );
        }

        if self.app_state.read(cx).show_onboarding {
            let start_state = self.app_state.clone();
            root = root.child(
                div()
                    .id("onboarding-wrap")
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("onboarding-dialog")
                            .w(px(520.0))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_5()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from("Welcome to ApiArk")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(
                                        "Create a collection or open an existing folder, then send your first request.",
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from(
                                        "1. Build requests in the tabs\n2. Inspect responses on the right\n3. Save everything inside your collection folder",
                                    )),
                            )
                            .child(dialog_button(
                                "start-onboarding",
                                "Get Started",
                                true,
                                move |cx| {
                                    start_state.update(cx, |state, cx| state.dismiss_onboarding(cx));
                                },
                            )),
                    ),
            );
        }

        root
    }
}

fn overlay_menu_item(
    id: impl Into<SharedString>,
    label: &'static str,
    color: Hsla,
    action: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .px_3()
        .py_2()
        .text_sm()
        .text_color(color)
        .cursor_pointer()
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from(label))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            action(cx);
        })
        .into_any_element()
}

pub(crate) fn generate_request_code(tab: &crate::models::Tab, language: &str) -> String {
    let url = crate::models::replace_url_path_variables(&tab.url, &tab.path_params);
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
    match language {
        "curl" => crate::services::curl::export_curl(
            tab.method.as_str(),
            &url,
            &headers,
            body,
            basic,
        ),
        "python" => {
            let mut lines = vec![
                "import requests".to_string(),
                String::new(),
                format!("url = {:?}", url),
            ];
            if !headers.is_empty() {
                lines.push(format!("headers = {:?}", headers));
            }
            if let Some((user, pass)) = basic {
                lines.push(format!("auth = ({:?}, {:?})", user, pass));
            }
            lines.push(String::new());
            let mut call = format!("requests.{}(", tab.method.as_str().to_lowercase());
            call.push_str("url");
            if !headers.is_empty() {
                call.push_str(", headers=headers");
            }
            if basic.is_some() {
                call.push_str(", auth=auth");
            }
            if let Some(body) = body {
                call.push_str(&format!(", data={body:?}"));
            }
            call.push(')');
            lines.push(call);
            lines.join("\n")
        }
        "javascript" => {
            let header_json = serde_json::to_string_pretty(&headers).unwrap_or_else(|_| "{}".into());
            let mut script = format!(
                "const headers = {};\nconst options = {{\n  method: {:?},\n  headers,\n",
                header_json,
                tab.method.as_str()
            );
            if let Some(body) = body {
                script.push_str(&format!("  body: {:?},\n", body));
            }
            script.push_str("};\n");
            script.push_str(&format!("fetch({:?}, options);", url));
            script
        }
        _ => String::new(),
    }
}

fn id_from_label(label: &str) -> &'static str {
    match label {
        "ApiArk YAML" => "export-apiark",
        "Postman v2.1" => "export-postman",
        "OpenAPI 3.x" => "export-openapi",
        "Insomnia" => "export-insomnia",
        "Bruno" => "export-bruno",
        _ => "export-format",
    }
}

fn dialog_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    action: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(if primary { theme::accent() } else { theme::bg_panel() })
        .border_1()
        .border_color(theme::border())
        .text_xs()
        .text_color(if primary { white() } else { theme::text_primary() })
        .cursor_pointer()
        .hover(|s| s.bg(if primary { theme::accent_hover() } else { theme::bg_hover() }))
        .child(SharedString::from(label))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            action(cx);
        })
        .into_any_element()
}

fn finish_overlay_action(
    app_state: &Entity<AppState>,
    cx: &mut App,
    result: Result<(), String>,
    success: &str,
) {
    app_state.update(cx, |state, cx| {
        match result {
            Ok(()) => state.show_toast(success, ToastKind::Success, cx),
            Err(e) => state.show_toast(format!("{success} failed: {e}"), ToastKind::Error, cx),
        }
        state.close_overlay(cx);
    });
}

fn dialog_field(label: &'static str, input: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(theme::text_muted())
                .child(SharedString::from(label)),
        )
        .child(input.into_any_element())
        .into_any_element()
}

fn method_menu_color(method: HttpMethod) -> Hsla {
    match method {
        HttpMethod::Get => theme::method::get(),
        HttpMethod::Post => theme::method::post(),
        HttpMethod::Put => theme::method::put(),
        HttpMethod::Patch => theme::method::patch(),
        HttpMethod::Delete => theme::method::delete(),
        _ => theme::text_muted(),
    }
}
