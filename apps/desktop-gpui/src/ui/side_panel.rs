//! Side panel — replaces `components/layout/side-panel.tsx`.
//! Now renders the real collection tree, environments, and history.

use crate::models::{ActivityView, CollectionNode, HistoryEntry, HttpMethod, KeyValuePair};
use crate::services::{collection as collection_service, WorkspaceService};
use crate::state::{AppState, Overlay, TabsState};
use crate::theme;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::prelude::*;

pub struct SidePanel {
    app_state: Entity<AppState>,
    tabs: Entity<TabsState>,
    workspace: Entity<WorkspaceService>,
    search_input: Entity<InputState>,
    search_query: String,
    folder_picker_open: Arc<AtomicBool>,
    panel_scroll_handle: ScrollHandle,
    last_view: Option<ActivityView>,
    _subscriptions: Vec<Subscription>,
}

impl SidePanel {
    pub fn new(
        app_state: Entity<AppState>,
        tabs: Entity<TabsState>,
        workspace: Entity<WorkspaceService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search requests...")
        });
        let search_sub = cx.subscribe_in(&search_input, window, move |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change)
                && this.app_state.read(cx).active_view == ActivityView::History
            {
                let query = input.read(cx).value().to_string();
                let app_state = this.app_state.clone();
                this.workspace.update(cx, |ws, cx| {
                    ws.search_history(&query, &app_state, cx);
                });
            }
        });
        Self {
            app_state,
            tabs,
            workspace,
            search_input,
            search_query: String::new(),
            folder_picker_open: Arc::new(AtomicBool::new(false)),
            panel_scroll_handle: ScrollHandle::new(),
            last_view: None,
            _subscriptions: vec![search_sub],
        }
    }

    /// Recursively render a tree node with expand/collapse + request opening.
    fn render_node(
        &mut self,
        node: &CollectionNode,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_folder = matches!(node, CollectionNode::Collection { .. } | CollectionNode::Folder { .. });
        let has_children = !node.children().is_empty();
        let node_id = node.element_id();
        let is_expanded = self.app_state.read(cx).is_node_expanded(&node_id);
        let indent = px(8.0 + depth as f32 * 14.0);
        let is_top_level = depth == 0 && is_folder;

        let mut row = div()
            .id(SharedString::from(node_id.clone()))
            .flex()
            .items_center()
            .gap_1()
            .h(px(if is_top_level { 30.0 } else { 26.0 }))
            .pl(indent)
            .pr_2()
            .rounded_lg()
            .bg(if is_top_level { theme::bg_hover() } else { transparent_black() })
            .cursor_pointer()
            .hover(|s| s.bg(theme::bg_hover()));

        if is_folder {
            let toggle_id = node_id.clone();
            let expanded_state = self.app_state.clone();
            row = row
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(SharedString::from(if is_expanded { "▾" } else { "▸" })),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(if is_top_level { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(theme::text_primary())
                        .truncate()
                        .child(SharedString::from(format!(
                            "{}{}",
                            if depth > 0 { "📁 " } else { "📦 " },
                            node.name()
                        )))
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    expanded_state.update(cx, |state, cx| {
                        state.toggle_node(toggle_id.clone(), cx);
                    });
                }));
        } else {
            let method = match node {
                CollectionNode::Request { method, .. } | CollectionNode::Case { method, .. } => *method,
                _ => HttpMethod::Get,
            };
            let path = node.path().to_string();
            let app_state = self.app_state.clone();
            let tabs = self.tabs.clone();
            let workspace = self.workspace.clone();
            let name = node.name().to_string();
            let is_case = matches!(node, CollectionNode::Case { .. });
            let case_name = if is_case { Some(name.clone()) } else { None };
            let case_count = match node {
                CollectionNode::Request { cases, .. } => cases.len(),
                _ => 0,
            };
            let label = name;
            let indent = if is_case {
                px(8.0 + depth as f32 * 14.0 + 12.0)
            } else {
                indent
            };
            row = row.pl(indent);

            if has_children {
                let toggle_id = node_id.clone();
                let expanded_state = self.app_state.clone();
                row = row.child(
                    div()
                        .id(SharedString::from(format!("toggle-{}", node_id)))
                        .w(px(12.0))
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(SharedString::from(if is_expanded { "▾" } else { "▸" }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                expanded_state.update(cx, |state, cx| {
                                    state.toggle_node(toggle_id.clone(), cx);
                                });
                            }),
                        ),
                );
            }

            if !is_case {
                row = row.child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .w(px(40.0))
                        .text_color(method_color(method))
                        .child(SharedString::from(method.as_str())),
                );
            } else {
                row = row
                    .child(div().w(px(1.0)).h(px(18.0)).bg(theme::border()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(SharedString::from("└")),
                    )
                    .child(
                        div()
                            .w(px(5.0))
                            .h(px(5.0))
                            .rounded_lg()
                            .bg(theme::accent()),
                    );
            }
            row = row.child(
                div()
                    .text_sm()
                    .font_weight(if is_case {
                        FontWeight::NORMAL
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_case {
                        theme::text_muted()
                    } else {
                        theme::text_primary()
                    })
                    .truncate()
                    .child(SharedString::from(label)),
            );
            if !is_case && case_count > 0 {
                row = row.child(
                    div()
                        .px_2()
                        .rounded_lg()
                        .bg(theme::bg_hover())
                        .text_xs()
                        .text_color(theme::accent())
                        .child(SharedString::from(format!("{} 用例", case_count))),
                );
            }
            row = row
                .on_click(cx.listener(move |_, _, _, cx| {
                    open_request_file(
                        &path,
                        case_name.as_deref(),
                        &app_state,
                        &tabs,
                        &workspace,
                        cx,
                    );
                }));
        }

        let node_kind = node.kind();
        let node_path = node.path().to_string();
        let node_name = node.name().to_string();
        let overlay_state = self.app_state.clone();
        row = row.on_mouse_down(MouseButton::Right, cx.listener(move |_, event: &MouseDownEvent, _, cx| {
            overlay_state.update(cx, |state, cx| {
                state.set_overlay(
                    Overlay::TreeContext {
                        x: event.position.x.as_f32(),
                        y: event.position.y.as_f32(),
                        path: node_path.clone(),
                        kind: node_kind,
                        name: node_name.clone(),
                    },
                    cx,
                );
            });
        }));

        let mut result = div().flex().flex_col().child(row);
        if is_expanded {
            for child in node.children() {
                result = result.child(self.render_node(child, depth + 1, cx));
            }
        }
        result.into_any_element()
    }

    /// Collect all request nodes whose name contains `query`.
    fn matching_requests<'a>(node: &'a CollectionNode, query: &str, out: &mut Vec<&'a CollectionNode>) {
        match node {
            CollectionNode::Request { name, .. } | CollectionNode::Case { name, .. } => {
                if name.to_lowercase().contains(&query.to_lowercase()) {
                    out.push(node);
                }
                for child in node.children() {
                    Self::matching_requests(child, query, out);
                }
            }
            _ => {
                for child in node.children() {
                    Self::matching_requests(child, query, out);
                }
            }
        }
    }

    fn render_collections(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (collections, active_collection) = {
            let state = self.app_state.read(cx);
            (state.collections.clone(), state.collection.clone())
        };
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(theme::border())
            .child(import_button(cx, self.workspace.clone(), self.app_state.clone()))
            .child(open_collection_button(
                cx,
                self.workspace.clone(),
                self.app_state.clone(),
                self.folder_picker_open.clone(),
            ))
            .child(new_collection_button(cx, self.app_state.clone()))
            .child(collections_more_button(
                cx,
                self.app_state.clone(),
                active_collection.as_ref().map(|node| node.path().to_string()),
                active_collection.as_ref().map(|node| node.name().to_string()),
            ));

        if collections.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(
                    div()
                        .flex_1()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child(SharedString::from("No collection loaded.")),
                        )
                        .child(create_sample_button(cx, self.workspace.clone(), self.app_state.clone()))
                )
                .into_any_element();
        }

        let query = self.search_input.read(cx).value().to_string();

        let tree_content: AnyElement = if query.trim().is_empty() {
            let mut list = div().flex().flex_col();
            for tree in &collections {
                list = list.child(self.render_node(tree, 0, cx));
            }
            list.into_any_element()
        } else {
            let mut matches = Vec::new();
            for tree in &collections {
                Self::matching_requests(tree, query.trim(), &mut matches);
            }
            if matches.is_empty() {
                div().px_3().py_2().text_sm().text_color(theme::text_muted())
                    .child(SharedString::from("No matching requests"))
                    .into_any_element()
            } else {
                let mut list = div().flex().flex_col();
                for node in matches {
                    list = list.child(self.render_node(node, 0, cx));
                }
                list.into_any_element()
            }
        };

        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    
                    .py_2()
                    .child(tree_content)
            )
            .into_any_element()
    }

    fn render_trash(&self, cx: &mut Context<Self>) -> AnyElement {
        let items = self.app_state.read(cx).trash_items.clone();
        let mut col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .gap_1()
            ;
        let empty_state = self.app_state.clone();
        let empty_ws = self.workspace.clone();
        col = col.child(
            div()
                .id("empty-trash")
                .mb_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .text_sm()
                .text_color(theme::text_primary())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("Empty Trash"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let result = empty_ws.update(cx, |ws, cx| ws.empty_trash(&empty_state, cx));
                    if let Err(e) = result {
                        empty_state.update(cx, |state, cx| {
                            state.show_toast(
                                format!("Empty trash failed: {e}"),
                                crate::state::ToastKind::Error,
                                cx,
                            );
                        });
                    }
                })),
        );
        if items.is_empty() {
            col = col.child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child(SharedString::from("Trash is empty.")),
            );
        }
        for item in items {
            let restore_state = self.app_state.clone();
            let restore_ws = self.workspace.clone();
            let id = item.id.clone();
            let name = item.name.clone();
            col = col.child(
                div()
                    .id(SharedString::from(format!("trash-{}", item.id)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        item.kind, item.path
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("restore-{}", item.id)))
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_xs()
                            .text_color(theme::accent())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("Restore"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                let result = restore_ws.update(cx, |ws, cx| {
                                    ws.restore_trash(&id, &restore_state, cx)
                                });
                                if let Err(e) = result {
                                    restore_state.update(cx, |state, cx| {
                                        state.show_toast(
                                            format!("Restore failed: {e}"),
                                            crate::state::ToastKind::Error,
                                            cx,
                                        );
                                    });
                                }
                            })),
                    ),
            );
        }
        col.into_any_element()
    }

    fn render_environments(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.app_state.read(cx);
        let envs = state.environments.clone();
        let active = state.environment.clone();
        let app_state = self.app_state.clone();
        let collection_path = state.collection_path.clone();

        let mut col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .gap_1()
            ;
        let new_state = app_state.clone();
        let import_ws = self.workspace.clone();
        let import_state = app_state.clone();
        col = col.child(
            div()
                .id("import-environment-variables")
                .mb_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .text_sm()
                .text_color(theme::text_primary())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("Import .env"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let Some(path) = rfd::FileDialog::new()
                        .add_filter("dotenv", &["env"])
                        .pick_file()
                    else {
                        return;
                    };
                    let result = import_ws.update(cx, |ws, cx| {
                        ws.import_environment_file(&path, &import_state, cx)
                    });
                    match result {
                        Ok(message) => import_state.update(cx, |state, cx| {
                            state.show_toast(message, crate::state::ToastKind::Success, cx);
                        }),
                        Err(e) => import_state.update(cx, |state, cx| {
                            state.show_toast(
                                format!("Import failed: {e}"),
                                crate::state::ToastKind::Error,
                                cx,
                            );
                        }),
                    }
                })),
        );
        col = col.child(
            div()
                .id("new-environment")
                .mb_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .text_sm()
                .text_color(theme::text_primary())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("+ Environment"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    new_state.update(cx, |state, cx| {
                        state.set_overlay(
                            Overlay::EnvironmentEditor {
                                original_name: None,
                                name: "New Environment".to_string(),
                                scope: crate::models::EnvironmentScope::Shared,
                                variables: String::new(),
                                secrets: String::new(),
                            },
                            cx,
                        );
                    });
                })),
        );

        if envs.is_empty() {
            col = col.child(
                div().text_sm().text_color(theme::text_muted())
                    .child(SharedString::from("No environments found.")),
            );
        }

        for env in envs {
            let is_active = active.as_deref() == Some(env.name.as_str());
            let name = env.name.clone();
            let state_clone = app_state.clone();
            let path_clone = collection_path.clone();
            let var_count = env.variables.len();
            let edit_state = app_state.clone();
            let delete_state = app_state.clone();
            let edit_name = env.name.clone();
            let delete_name = env.name.clone();
            let edit_scope = env.scope;
            let edit_variables = env
                .variables
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("\n");
            let edit_secrets = env.secrets.join("\n");

            col = col.child(
                div()
                    .id(SharedString::from(format!("env-{}", env.name)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(if is_active { theme::bg_hover() } else { theme::bg_panel() })
                    .border_l_2()
                    .border_color(if is_active { theme::accent() } else { transparent_black() })
                    .child(
                        div().flex().items_center().justify_between()
                            .child(
                                div().text_sm().text_color(theme::text_primary())
                                    .child(SharedString::from(env.name.clone())),
                            )
                            .child(
                                div().text_xs().text_color(theme::text_muted())
                                    .child(SharedString::from(format!("{} vars", var_count))),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("env-edit-{}", env.name)))
                                    .px_1()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::accent()))
                                    .child(SharedString::from("✎"))
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        cx.stop_propagation();
                                        edit_state.update(cx, |state, cx| {
                                            state.set_overlay(
                                                Overlay::EnvironmentEditor {
                                                    original_name: Some(edit_name.clone()),
                                                    name: edit_name.clone(),
                                                    scope: edit_scope,
                                                    variables: edit_variables.clone(),
                                                    secrets: edit_secrets.clone(),
                                                },
                                                cx,
                                            );
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("env-delete-{}", env.name)))
                                    .px_1()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::error()))
                                    .child(SharedString::from("×"))
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        cx.stop_propagation();
                                        delete_state.update(cx, |state, cx| {
                                            state.set_overlay(
                                                Overlay::DeleteEnvironment {
                                                    name: delete_name.clone(),
                                                    scope: env.scope,
                                                },
                                                cx,
                                            );
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div().text_xs().text_color(theme::text_muted())
                            .child(SharedString::from(format!(
                                "{:?} · {}", env.scope,
                                path_clone.as_deref().unwrap_or("no collection")
                            ))),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        state_clone.update(cx, |s, cx| s.set_environment(Some(name.clone()), cx));
                    })),
            );
        }

        col.into_any_element()
    }

    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.app_state.read(cx);
        let entries = state.history_entries.clone();
        let app_state = self.app_state.clone();
        let workspace = self.workspace.clone();
        let tabs = self.tabs.clone();

        let mut col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .gap_1()
            ;

        if entries.is_empty() {
            col = col.child(
                div().text_sm().text_color(theme::text_muted())
                    .child(SharedString::from("No requests sent yet.")),
            );
        }

        for entry in entries.iter() {
            let entry = entry.clone();
            let app_state = app_state.clone();
            let delete_ws = workspace.clone();
            let delete_state = app_state.clone();
            let open_tabs = tabs.clone();
            let delete_id = entry.id;
            let status = entry.status.unwrap_or(0);
            let status_color = if (200..300).contains(&status) {
                theme::success()
            } else if status >= 400 {
                theme::error()
            } else {
                theme::text_muted()
            };

            col = col.child(
                div()
                    .id(SharedString::from(format!("history-{}", entry.id)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::bg_hover()))
                    .child(
                        div().flex().items_center().gap_2()
                            .child(
                                div().text_xs().font_weight(FontWeight::SEMIBOLD)
                                    .text_color(method_color_from_str(&entry.method))
                                    .child(SharedString::from(entry.method.clone())),
                            )
                            .child(
                                div().flex_1().text_sm().text_color(theme::text_primary()).truncate()
                                    .child(SharedString::from(entry.url.clone())),
                            )
                            .child(
                                div().text_xs().text_color(status_color)
                                    .child(SharedString::from(status.to_string())),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("history-delete-{}", delete_id)))
                                    .w(px(18.0))
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::error()))
                                    .child(SharedString::from("×"))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ = delete_ws.update(cx, |ws, cx| {
                                            ws.delete_history(delete_id, &delete_state, cx)
                                        });
                                    })),
                            ),
                    )
                    .child(
                        div().text_xs().text_color(theme::text_muted())
                            .child(SharedString::from(format!(
                                "{} · {}ms", entry.timestamp, entry.time_ms.unwrap_or(0)
                            ))),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        open_history_entry(&entry, &app_state, &open_tabs, cx);
                    })),
            );
        }

        let more_ws = workspace.clone();
        let more_state = app_state.clone();

        col = col.child(
            div()
                .id("load-more-history")
                .mt_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .text_sm()
                .text_color(theme::text_primary())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("Load More"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    more_ws.update(cx, |ws, cx| ws.load_more_history(&more_state, cx));
                })),
        );

        // Clear history button
        col = col.child(
            div()
                .id("clear-history")
                .mt_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .text_sm()
                .text_color(theme::error())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("Clear History"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let _ = workspace.update(cx, |ws, cx| {
                        ws.clear_history(&app_state, cx)
                    });
                })),
        );

        col.into_any_element()
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, active_view) = {
            let app = self.app_state.read(cx);
            if !app.side_panel_visible {
                return div()
                    .id("side-panel-hidden")
                    .w(px(0.0))
                    .h_full()
                    .bg(theme::bg_panel());
            }
            (app.active_view.label(), app.active_view)
        };
        if self.last_view != Some(active_view) {
            self.last_view = Some(active_view);
            self.panel_scroll_handle
                .set_offset(point(px(0.0), px(0.0)));
        }
        let scroll_handle = self.panel_scroll_handle.clone();

        div()
            .id("side-panel")
            .flex()
            .flex_col()
            .w(px(280.0))
            .min_w(px(200.0))
            .h_full()
            .bg(theme::bg_panel())
            .rounded_lg()
            .border_r_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(38.0))
                    .px_3()
                    .border_b_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(SharedString::from(title)),
                    ),
            )
            // Search box (shown for collections view)
            .child(if matches!(active_view, ActivityView::Collections | ActivityView::History) {
                div()
                    .px_2()
                    .py_2()
                    .child(Input::new(&self.search_input))
            } else {
                div()
            })
            .child(
                div()
                    .id("side-panel-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .track_scroll(&scroll_handle)
                    .child(match active_view {
                        ActivityView::Collections => self.render_collections(cx),
                        ActivityView::Environments => self.render_environments(cx),
                        ActivityView::Trash => self.render_trash(cx),
                        ActivityView::History => self.render_history(cx),
                        _ => div()
                            .flex_1()
                            .p_3()
                            .child(
                                div().text_sm().text_color(theme::text_muted())
                                    .child(SharedString::from("Coming soon")),
                            )
                            .into_any_element(),
                    }),
            )
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn method_color(method: HttpMethod) -> Hsla {
    match method {
        HttpMethod::Get => theme::method::get(),
        HttpMethod::Post => theme::method::post(),
        HttpMethod::Put => theme::method::put(),
        HttpMethod::Patch => theme::method::patch(),
        HttpMethod::Delete => theme::method::delete(),
        _ => theme::text_muted(),
    }
}

fn method_color_from_str(method: &str) -> Hsla {
    match method {
        "GET" => theme::method::get(),
        "POST" => theme::method::post(),
        "PUT" => theme::method::put(),
        "PATCH" => theme::method::patch(),
        "DELETE" => theme::method::delete(),
        _ => theme::text_muted(),
    }
}

fn import_button(
    cx: &mut Context<SidePanel>,
    workspace: Entity<WorkspaceService>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("import-collection")
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_1()
        .px_1()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("↓ Import"))
        .on_click(cx.listener(move |_, _, _, cx| {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Collection", &["json", "yaml", "yml"])
                .pick_file()
            else {
                return;
            };
            let result = workspace.update(cx, |ws, cx| ws.import_file(&path, &app_state, cx));
            app_state.update(cx, |s, cx| {
                match result {
                    Ok(preview) => s.set_overlay(
                        Overlay::ImportPreview {
                            message: preview,
                            is_error: false,
                        },
                        cx,
                    ),
                    Err(e) => s.set_overlay(
                        Overlay::ImportPreview {
                            message: format!("Import failed: {e}"),
                            is_error: true,
                        },
                        cx,
                    ),
                }
            });
        }))
        .into_any_element()
}

fn open_collection_button(
    cx: &mut Context<SidePanel>,
    workspace: Entity<WorkspaceService>,
    app_state: Entity<AppState>,
    guard: Arc<AtomicBool>,
) -> AnyElement {
    div()
        .id("open-collection")
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_1()
        .px_1()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("↑ Open"))
        .on_click(cx.listener(move |_, _, window, cx| {
            if guard.swap(true, Ordering::SeqCst) {
                return;
            }
            let guard = guard.clone();
            let ws = workspace.clone();
            let state = app_state.clone();
            let dialog = rfd::AsyncFileDialog::new()
                .set_parent(window)
                .set_directory(WorkspaceService::collections_root());
            cx.spawn_in(window, async move |_, cx| {
                let picked = dialog.pick_folder().await;
                guard.store(false, Ordering::SeqCst);
                let Some(dir) = picked else {
                    return;
                };
                let path = dir.path().to_string_lossy().to_string();
                let _ = cx.update(|_, cx| {
                    let result = ws.update(cx, |ws, cx| ws.open_collection(&path, &state, cx));
                    state.update(cx, |s, cx| match result {
                        Ok(()) => s.show_toast(
                            format!("Opened {}", dir_name(&path)),
                            crate::state::ToastKind::Success,
                            cx,
                        ),
                        Err(e) => s.show_toast(
                            format!("Open failed: {e}"),
                            crate::state::ToastKind::Error,
                            cx,
                        ),
                    });
                });
            })
            .detach();
        }))
        .into_any_element()
}

fn dir_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn delete_collection_button(
    cx: &mut Context<SidePanel>,
    app_state: Entity<AppState>,
    path: String,
    name: String,
) -> AnyElement {
    div()
        .id("delete-collection")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("× Collection"))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            app_state.update(cx, |state, cx| {
                state.set_overlay(
                    Overlay::DeleteCollection {
                        path: path.clone(),
                        name: name.clone(),
                    },
                    cx,
                );
            });
        }))
        .into_any_element()
}

fn collections_more_button(
    cx: &mut Context<SidePanel>,
    app_state: Entity<AppState>,
    path: Option<String>,
    name: Option<String>,
) -> AnyElement {
    div()
        .id("collections-more")
        .flex()
        .items_center()
        .justify_center()
        .w(px(28.0))
        .px_2()
        .py_1()
        .rounded_lg()
        .text_sm()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("⋯"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let x = event.position.x.as_f32() - 100.0;
                let y = event.position.y.as_f32() + 24.0;
                app_state.update(cx, |state, cx| {
                    state.set_overlay(
                        Overlay::CollectionsMoreMenu {
                            x,
                            y,
                            path: path.clone(),
                            name: name.clone(),
                        },
                        cx,
                    );
                });
            }),
        )
        .into_any_element()
}

fn backup_button(
    cx: &mut Context<SidePanel>,
    workspace: Entity<WorkspaceService>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("backup-state")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("↓ Backup"))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            let Some(path) = rfd::FileDialog::new()
                .set_file_name("apiark-backup.zip")
                .save_file()
            else {
                return;
            };
            let result = workspace.update(cx, |ws, _| ws.create_backup(&path));
            app_state.update(cx, |state, cx| match result {
                Ok(()) => state.show_toast("Backup created", crate::state::ToastKind::Success, cx),
                Err(e) => state.show_toast(
                    format!("Backup failed: {e}"),
                    crate::state::ToastKind::Error,
                    cx,
                ),
            });
        }))
        .into_any_element()
}

fn restore_button(
    cx: &mut Context<SidePanel>,
    workspace: Entity<WorkspaceService>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("restore-state")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("↑ Restore"))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Backup", &["zip"])
                .pick_file()
            else {
                return;
            };
            let result = workspace.update(cx, |ws, cx| {
                ws.restore_backup(&path, &app_state, cx)
            });
            app_state.update(cx, |state, cx| match result {
                Ok(()) => state.show_toast("Backup restored", crate::state::ToastKind::Success, cx),
                Err(e) => state.show_toast(
                    format!("Restore failed: {e}"),
                    crate::state::ToastKind::Error,
                    cx,
                ),
            });
        }))
        .into_any_element()
}

fn collection_defaults_button(
    cx: &mut Context<SidePanel>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("collection-defaults")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("⚙ Defaults"))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            let Some(path) = app_state.read(cx).collection_path.clone() else {
                return;
            };
            let Ok(config) =
                crate::services::collection::load_collection_config(std::path::Path::new(&path))
            else {
                return;
            };
            app_state.update(cx, |state, cx| {
                state.set_overlay(
                    Overlay::CollectionDefaults {
                        send_cookies: config.defaults.send_cookies,
                        store_cookies: config.defaults.store_cookies,
                        persist_cookies: config.defaults.persist_cookies,
                    },
                    cx,
                );
            });
        }))
        .into_any_element()
}

fn export_button(
    cx: &mut Context<SidePanel>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("export-collection")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("↑ Export"))
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.stop_propagation();
            let Some(path) = app_state.read(cx).collection_path.clone() else {
                return;
            };
            let app_state = app_state.clone();
            app_state.update(cx, |state, cx| {
                state.set_overlay(Overlay::ExportFormat { path }, cx);
            });
        }))
        .into_any_element()
}

fn new_collection_button(
    cx: &mut Context<SidePanel>,
    app_state: Entity<AppState>,
) -> AnyElement {
    div()
        .id("new-collection")
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_1()
        .px_1()
        .py_1()
        .rounded_lg()
        .text_xs()
        .cursor_pointer()
        .bg(theme::bg_surface())
        .text_color(theme::text_primary())
        .hover(|s| s.bg(theme::bg_hover()))
        .child(SharedString::from("+ New"))
        .on_click(cx.listener(move |_, _, _, cx| {
            app_state.update(cx, |state, cx| {
                state.set_overlay(
                    Overlay::NewCollection {
                        name: "New Collection".to_string(),
                        location: crate::services::WorkspaceService::collections_root(),
                    },
                    cx,
                );
            });
        }))
        .into_any_element()
}

fn create_sample_button(cx: &mut Context<SidePanel>, workspace: Entity<WorkspaceService>, app_state: Entity<AppState>) -> AnyElement {
    div()
        .id("create-sample")
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(theme::accent())
        .text_sm()
        .text_color(gpui_kit::white())
        .cursor_pointer()
        .child(SharedString::from("Create Sample Collection"))
        .on_click(cx.listener(move |_, _, _, cx| {
            let _ = workspace.update(cx, |ws, cx| ws.create_sample(&app_state, cx));
        }))
        .into_any_element()
}

fn expand_path_ancestors(node: &CollectionNode, target: &std::path::Path, ids: &mut Vec<String>) {
    if !target.starts_with(std::path::Path::new(node.path())) {
        return;
    }
    ids.push(format!("node-{}", node.path()));
    for child in node.children() {
        expand_path_ancestors(child, target, ids);
    }
}

fn json_value_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => String::new(),
        value => value.to_string(),
    }
}

fn history_pairs(
    value: &serde_json::Value,
    key: &str,
    order_key: &str,
) -> Option<Vec<KeyValuePair>> {
    let raw = value.get(key)?;
    match raw {
        serde_json::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| {
                    let key = item.get("key").and_then(serde_json::Value::as_str)?;
                    Some(KeyValuePair {
                        id: item
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        key: key.to_string(),
                        value: item
                            .get("value")
                            .map(json_value_string)
                            .unwrap_or_default(),
                        enabled: item
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true),
                        value_type: item
                            .get("valueType")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = value
                .get(order_key)
                .and_then(serde_json::Value::as_array)
                .map(|order| {
                    order
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .filter(|key| map.contains_key(*key))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            for key in map.keys() {
                if !keys.contains(key) {
                    keys.push(key.clone());
                }
            }
            Some(
                keys.into_iter()
                    .map(|key| {
                        KeyValuePair::new(&key, &json_value_string(&map[&key]))
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

fn params_from_url(url: &str) -> Vec<KeyValuePair> {
    if let Ok(parsed) = url::Url::parse(url) {
        return parsed
            .query_pairs()
            .map(|(key, value)| KeyValuePair::new(key.as_ref(), value.as_ref()))
            .collect();
    }
    let Some(query) = url.split_once('?').map(|(_, query)| query) else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some(KeyValuePair::new(key, value))
        })
        .collect()
}

fn pairs_to_map(pairs: &[KeyValuePair]) -> (std::collections::HashMap<String, String>, Vec<String>) {
    let mut map = std::collections::HashMap::new();
    let mut order = Vec::new();
    for pair in pairs {
        map.insert(pair.key.clone(), pair.value.clone());
        if !order.contains(&pair.key) {
            order.push(pair.key.clone());
        }
    }
    (map, order)
}

fn open_history_entry(
    entry: &HistoryEntry,
    app_state: &Entity<AppState>,
    tabs: &Entity<TabsState>,
    cx: &mut App,
) {
    let value: serde_json::Value =
        serde_json::from_str(&entry.request_json).unwrap_or(serde_json::Value::Null);
    let url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
        .unwrap_or(&entry.url)
        .to_string();
    let headers = history_pairs(&value, "headers", "headerOrder");
    let mut params = history_pairs(&value, "params", "paramOrder")
        .filter(|pairs| !pairs.is_empty());
    if params.is_none() {
        params = Some(params_from_url(&url));
    }
    let path_params = history_pairs(&value, "pathParams", "pathParamOrder")
        .or_else(|| history_pairs(&value, "pathVariables", "pathVariableOrder"));
    let (header_map, header_order) = pairs_to_map(headers.as_deref().unwrap_or(&[]));
    let (param_map, param_order) = pairs_to_map(params.as_deref().unwrap_or(&[]));
    let (path_param_map, path_param_order) = pairs_to_map(path_params.as_deref().unwrap_or(&[]));

    let file = crate::models::RequestFile {
        name: value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        method: serde_json::from_value(
            value
                .get("method")
                .cloned()
                .unwrap_or_else(|| serde_json::json!(entry.method)),
        )
        .unwrap_or_default(),
        url: url.clone(),
        protocol: value
            .get("protocol")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        description: value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        headers: header_map,
        header_order,
        auth: value
            .get("auth")
            .cloned()
            .and_then(|auth| serde_json::from_value(auth).ok()),
        body: value
            .get("body")
            .cloned()
            .and_then(|body| serde_json::from_value(body).ok()),
        params: Some(param_map),
        param_order,
        path_params: Some(path_param_map),
        path_param_order,
        is_graphql: value
            .get("isGraphql")
            .and_then(serde_json::Value::as_bool),
        tests: value
            .get("tests")
            .or_else(|| value.get("testScript"))
            .map(json_value_string)
            .filter(|script| !script.is_empty()),
        pre_request_script: value
            .get("preRequestScript")
            .map(json_value_string)
            .filter(|script| !script.is_empty()),
        post_response_script: value
            .get("postResponseScript")
            .map(json_value_string)
            .filter(|script| !script.is_empty()),
        cases: value
            .get("cases")
            .cloned()
            .and_then(|cases| serde_json::from_value(cases).ok())
            .unwrap_or_default(),
    };

    let collection_path = entry.collection_path.clone().unwrap_or_default();
    let mut tab = crate::models::Tab::from_request_file("", &collection_path, &file);
    let name = entry
        .request_name
        .clone()
        .unwrap_or_else(|| file.name.clone());
    tab.name = if name.trim().is_empty() {
        format!("{} {}", entry.method, entry.url)
    } else {
        name
    };
    tab.url = url;
    tab.headers = headers.unwrap_or_default();
    tab.params = params.unwrap_or_default();
    tab.path_params = crate::models::parse_url_path_variables(&tab.url, &tab.path_params);
    tab.file_path = None;
    tab.collection_path = entry.collection_path.clone();
    tab.response = entry
        .response_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok());
    tab.is_dirty = false;
    tab.test_results.clear();

    tabs.update(cx, |state, cx| {
        state.open_or_activate(tab, cx);
    });
    app_state.update(cx, |state, cx| {
        state.set_response_view(crate::models::ResponseView::Body, cx);
    });
}

fn open_request_file(
    path: &str,
    case_name: Option<&str>,
    app_state: &Entity<AppState>,
    tabs: &Entity<TabsState>,
    workspace: &Entity<WorkspaceService>,
    cx: &mut App,
) {
    // Resolve variables for the active environment.
    let variables = workspace.read(cx).resolved_variables(cx, app_state);
    let (collections, active_path) = {
        let state = app_state.read(cx);
        (state.collections.clone(), state.collection_path.clone())
    };
    let collection_path = collections
        .iter()
        .find(|node| std::path::Path::new(path).starts_with(std::path::Path::new(node.path())))
        .map(|node| node.path().to_string())
        .or(active_path)
        .unwrap_or_default();

    let mut expand_ids = Vec::new();
    for node in &collections {
        expand_path_ancestors(node, std::path::Path::new(path), &mut expand_ids);
    }
    app_state.update(cx, |state, cx| {
        for id in expand_ids {
            state.expand_node(id, cx);
        }
    });

    match collection_service::read_request(std::path::Path::new(path)) {
        Ok(mut file) => {
            if let Some(case) = case_name {
                let Some(case_file) = file
                    .cases
                    .iter()
                    .find(|candidate| candidate.name == case)
                    .cloned()
                else {
                    app_state.update(cx, |s, cx| {
                        s.show_toast(
                            format!("Case not found: {case}"),
                            crate::state::ToastKind::Error,
                            cx,
                        );
                    });
                    return;
                };
                file = case_file;
                file.cases.clear();
            }
            let history_url = file.url.clone();
            let history_method = file.method.as_str().to_string();

            // Substitute {{var}} placeholders in the URL.
            for (key, value) in &variables {
                file.url = file.url.replace(&format!("{{{{{}}}}}", key), value);
            }

            let mut tab = crate::models::Tab::from_request_file(path, &collection_path, &file);
            tab.case_name = case_name.map(str::to_string);
            if tab.response.is_none() {
                tab.response = workspace
                    .read(cx)
                    .latest_response(&history_method, &history_url);
            }
            tabs.update(cx, |state, cx| {
                state.open_or_activate(tab, cx);
            });
        }
        Err(e) => {
            app_state.update(cx, |s, cx| {
                s.show_toast(format!("Failed to open request: {e}"), crate::state::ToastKind::Error, cx);
            });
        }
    }
}
