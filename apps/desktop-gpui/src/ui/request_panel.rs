//! Request panel — replaces `components/request/request-panel.tsx`.
//! Now includes a native multi-line body editor (Textarea) synced to the tab.

use crate::models::BodyType;
use crate::services::{collection as collection_service, WorkspaceService};
use crate::state::{AppState, Overlay, TabsState, ToastKind};
use crate::theme;
use crate::ui::{AuthEditor, FormDataEditor, KeyValueEditor, KvTarget};

use super::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestTab {
    Params,
    PathVars,
    Headers,
    Body,
    Auth,
    Scripts,
}

impl RequestTab {
    fn label(&self) -> &'static str {
        match self {
            Self::Params => "Params",
            Self::PathVars => "Path",
            Self::Headers => "Headers",
            Self::Body => "Body",
            Self::Auth => "Auth",
            Self::Scripts => "Scripts",
        }
    }
}

pub struct RequestPanel {
    tabs: Entity<TabsState>,
    app_state: Entity<AppState>,
    active: RequestTab,
    body_editor: Entity<EditorState>,
    params_editor: Entity<KeyValueEditor>,
    path_vars_editor: Entity<KeyValueEditor>,
    headers_editor: Entity<KeyValueEditor>,
    form_data_editor: Entity<FormDataEditor>,
    auth_editor: Entity<AuthEditor>,
    last_sync: Option<(String, String)>,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
    body_value: String,
    suppress_history: bool,
    pre_request_editor: Entity<EditorState>,
    post_response_editor: Entity<EditorState>,
    test_editor: Entity<EditorState>,
    script_tab: ScriptTab,
    last_script_key: Option<String>,
    suppress_script_events: bool,
    _subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptTab {
    PreRequest,
    PostResponse,
    Tests,
}

impl ScriptTab {
    fn label(self) -> &'static str {
        match self {
            Self::PreRequest => "Pre-request",
            Self::PostResponse => "Post-response",
            Self::Tests => "Tests",
        }
    }
}

impl RequestPanel {
    pub fn new(
        tabs: Entity<TabsState>,
        app_state: Entity<AppState>,
        workspace: Entity<WorkspaceService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let body_editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).language("json");
            state.set_highlighter_factory(crate::ui::json_view::JsonHighlighter::factory(), cx);
            state
        });

        let tabs_clone = tabs.clone();
        let subscription = cx.subscribe_in(
            &body_editor,
            window,
            move |this, editor, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = editor.read(cx).value().to_string();
                    if !this.suppress_history && value != this.body_value {
                        this.undo_stack.push(this.body_value.clone());
                        if this.undo_stack.len() > 100 {
                            this.undo_stack.remove(0);
                        }
                        this.redo_stack.clear();
                        this.body_value = value.clone();
                    }
                    tabs_clone.update(cx, |s, cx| {
                        if let Some(tab) = s.active_tab_mut() {
                            if tab.body.content != value {
                                tab.body.content = value;
                                tab.is_dirty = true;
                                cx.notify();
                            }
                        }
                    });
                }
            },
        );

        let pre_request_editor = cx.new(|cx| EditorState::new(window, cx));
        let post_response_editor = cx.new(|cx| EditorState::new(window, cx));
        let test_editor = cx.new(|cx| EditorState::new(window, cx));

        let pre_tabs = tabs.clone();
        let pre_sub = cx.subscribe_in(
            &pre_request_editor,
            window,
            move |this, editor, event, _, cx| {
                if matches!(event, InputEvent::Change) && !this.suppress_script_events {
                    let value = editor.read(cx).value().to_string();
                    pre_tabs.update(cx, |state, cx| {
                        if let Some(tab) = state.active_tab_mut() {
                            tab.pre_request_script = Some(value);
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    });
                }
            },
        );
        let post_tabs = tabs.clone();
        let post_sub = cx.subscribe_in(
            &post_response_editor,
            window,
            move |this, editor, event, _, cx| {
                if matches!(event, InputEvent::Change) && !this.suppress_script_events {
                    let value = editor.read(cx).value().to_string();
                    post_tabs.update(cx, |state, cx| {
                        if let Some(tab) = state.active_tab_mut() {
                            tab.post_response_script = Some(value);
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    });
                }
            },
        );
        let test_tabs = tabs.clone();
        let test_sub = cx.subscribe_in(
            &test_editor,
            window,
            move |this, editor, event, _, cx| {
                if matches!(event, InputEvent::Change) && !this.suppress_script_events {
                    let value = editor.read(cx).value().to_string();
                    test_tabs.update(cx, |state, cx| {
                        if let Some(tab) = state.active_tab_mut() {
                            tab.test_script = Some(value);
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    });
                }
            },
        );

        let params_editor = cx.new(|_| KeyValueEditor::new(tabs.clone(), KvTarget::Params));
        let path_vars_editor = cx.new(|_| KeyValueEditor::new(tabs.clone(), KvTarget::PathParams));
        let headers_editor = cx.new(|_| KeyValueEditor::new(tabs.clone(), KvTarget::Headers));
        let form_data_editor = cx.new(|_| FormDataEditor::new(tabs.clone()));
        let auth_editor = cx.new(|_| AuthEditor::new(
            tabs.clone(),
            app_state.clone(),
            workspace,
        ));

        Self {
            tabs,
            app_state,
            active: RequestTab::Body,
            body_editor,
            params_editor,
            path_vars_editor,
            headers_editor,
            form_data_editor,
            auth_editor,
            last_sync: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            body_value: String::new(),
            suppress_history: false,
            pre_request_editor,
            post_response_editor,
            test_editor,
            script_tab: ScriptTab::PreRequest,
            last_script_key: None,
            suppress_script_events: false,
            _subscriptions: vec![subscription, pre_sub, post_sub, test_sub],
        }
    }

    /// Sync editor content from the active tab when the tab changes.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.read(cx).active_tab() else {
            return;
        };
        let key = (tab.id.clone(), tab.body.content.clone());
        if self.last_sync.as_ref() == Some(&key) {
            return;
        }
        let content = tab.body.content.clone();
        let editor = self.body_editor.clone();
        self.suppress_history = true;
        editor.update(cx, |state, cx| {
            if state.value().to_string() != content {
                state.set_value(content.clone(), window, cx);
            }
        });
        self.suppress_history = false;
        self.body_value = content;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_sync = Some(key);
    }

    fn set_body_type(&mut self, body_type: BodyType, cx: &mut Context<Self>) {
        self.tabs.update(cx, |s, cx| {
            if let Some(tab) = s.active_tab_mut() {
                tab.body.body_type = body_type;
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    fn pick_binary_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self.tabs.clone();
        window
            .spawn(cx, async move |cx| {
                let Some(file) = rfd::AsyncFileDialog::new().pick_file().await else {
                    return;
                };
                let path = file.path().to_string_lossy().to_string();
                let _ = cx.update(|_, cx| {
                    tabs.update(cx, |state, cx| {
                        if let Some(tab) = state.active_tab_mut() {
                            tab.body.binary_path = Some(path);
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    });
                });
            })
            .detach();
    }

    fn apply_body_text(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        record_history: bool,
    ) {
        if record_history && text != self.body_value {
            self.undo_stack.push(self.body_value.clone());
            if self.undo_stack.len() > 100 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
        self.body_value = text.clone();
        self.suppress_history = true;
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                tab.body.content = text.clone();
                tab.is_dirty = true;
                cx.notify();
            }
        });
        self.body_editor.update(cx, |state, cx| {
            state.set_value(text, window, cx);
        });
        self.suppress_history = false;
        self.last_sync = None;
    }

    fn set_body_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_body_text(text, window, cx, true);
    }

    fn undo_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.body_value.clone());
        self.apply_body_text(previous, window, cx, false);
    }

    fn redo_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.body_value.clone());
        self.apply_body_text(next, window, cx, false);
    }

    fn format_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.body_editor.read(cx).value().to_string();
        let Some(value) = serde_json::from_str::<serde_json::Value>(&content).ok() else {
            self.app_state.update(cx, |s, cx| {
                s.show_toast("Body is not valid JSON", ToastKind::Info, cx);
            });
            return;
        };
        let formatted = serde_json::to_string_pretty(&value).unwrap_or(content);
        self.set_body_text(formatted, window, cx);
    }

    fn minify_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.body_editor.read(cx).value().to_string();
        let Some(value) = serde_json::from_str::<serde_json::Value>(&content).ok() else {
            self.app_state.update(cx, |s, cx| {
                s.show_toast("Body is not valid JSON", ToastKind::Info, cx);
            });
            return;
        };
        let minified = serde_json::to_string(&value).unwrap_or(content);
        self.set_body_text(minified, window, cx);
    }

    fn save_request(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.read(cx).active_tab().cloned() else {
            return;
        };
        let (Some(path), Some(collection)) = (&tab.file_path, &tab.collection_path) else {
            self.app_state.update(cx, |s, cx| {
                s.show_toast("This request is not saved to a collection", ToastKind::Info, cx);
            });
            return;
        };

        match collection_service::write_request(
            std::path::Path::new(path),
            &tab.to_request_file(),
        ) {
            Ok(()) => {
                self.tabs.update(cx, |s, cx| s.mark_clean(cx));
                self.app_state.update(cx, |s, cx| {
                    s.show_toast(
                        format!("Saved {}", path.rsplit(['/', '\\']).next().unwrap_or(path)),
                        ToastKind::Success,
                        cx,
                    );
                });
                let _ = collection;
            }
            Err(e) => {
                self.app_state.update(cx, |s, cx| {
                    s.show_toast(format!("Save failed: {e}"), ToastKind::Error, cx);
                });
            }
        }
    }

    fn sync_scripts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.read(cx).active_tab().cloned() else {
            return;
        };
        if self.last_script_key.as_deref() == Some(tab.id.as_str()) {
            return;
        }
        let values = [
            tab.pre_request_script.clone().unwrap_or_default(),
            tab.post_response_script.clone().unwrap_or_default(),
            tab.test_script.clone().unwrap_or_default(),
        ];
        let editors = [
            self.pre_request_editor.clone(),
            self.post_response_editor.clone(),
            self.test_editor.clone(),
        ];
        self.suppress_script_events = true;
        for (editor, value) in editors.iter().zip(values) {
            editor.update(cx, |state, cx| {
                if state.value().to_string() != value {
                    state.set_value(value, window, cx);
                }
            });
        }
        self.suppress_script_events = false;
        self.last_script_key = Some(tab.id);
    }
}

impl Render for RequestPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_editor(window, cx);
        self.sync_scripts(window, cx);

        let tab = self.tabs.read(cx).active_tab().cloned();

        let all_tabs = [
            RequestTab::Params,
            RequestTab::PathVars,
            RequestTab::Headers,
            RequestTab::Body,
            RequestTab::Auth,
            RequestTab::Scripts,
        ];

        let mut header_row = div()
            .flex()
            .items_center()
            .h(px(36.0))
            .border_b_1()
            .border_color(theme::border());

        let param_count = tab.as_ref().map(|t| t.params.iter().filter(|p| !p.key.is_empty()).count()).unwrap_or(0);
        let path_count = tab.as_ref().map(|t| t.path_params.iter().filter(|p| !p.key.is_empty()).count()).unwrap_or(0);
        let header_count = tab.as_ref().map(|t| t.headers.iter().filter(|p| !p.key.is_empty()).count()).unwrap_or(0);

        for t in all_tabs {
            let is_active = self.active == t;
            let t_val = t;
            let label = match t {
                RequestTab::Params => format!("Params ({})", param_count),
                RequestTab::PathVars => format!("Path ({})", path_count),
                RequestTab::Headers => format!("Headers ({})", header_count),
                _ => t.label().to_string(),
            };
            header_row = header_row.child(
                div()
                    .id(SharedString::from(format!("req-tab-{:?}", t)))
                    .px_3()
                    .h_full()
                    .flex()
                    .items_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .text_color(if is_active { theme::accent() } else { theme::text_muted() })
                    .border_b_2()
                    .border_color(if is_active { theme::accent() } else { transparent_black() })
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.active = t_val;
                        cx.notify();
                    })),
            );
        }

        let cases: Vec<String> = tab
            .as_ref()
            .map(|tab| tab.cases.iter().map(|case| case.name.clone()).collect())
            .unwrap_or_default();
        let case_menu_state = self.app_state.clone();
        header_row = header_row
            .child(div().flex_1())
            .child(
                div()
                    .id("case-menu")
                    .px_2()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::accent()))
                    .child(SharedString::from(format!("Cases ({})", cases.len())))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        case_menu_state.update(cx, |state, cx| {
                            state.set_overlay(Overlay::CaseMenu { cases: cases.clone() }, cx);
                        });
                    })),
            );

        let content: AnyElement = match self.active {
            RequestTab::Params => self.params_editor.clone().into_any_element(),
            RequestTab::PathVars => self.path_vars_editor.clone().into_any_element(),
            RequestTab::Headers => self.headers_editor.clone().into_any_element(),
            RequestTab::Body => {
                let body_types = [
                    BodyType::None,
                    BodyType::Json,
                    BodyType::Xml,
                    BodyType::FormData,
                    BodyType::Urlencoded,
                    BodyType::Raw,
                    BodyType::Binary,
                ];
                let current_type = tab.as_ref().map(|t| t.body.body_type).unwrap_or(BodyType::None);

                let mut type_row = div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(34.0))
                    .px_3();
                for bt in body_types {
                    let bt_val = bt;
                    let active = current_type == bt;
                    type_row = type_row.child(
                        div()
                            .id(SharedString::from(format!("body-type-{:?}", bt)))
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_xs()
                            .cursor_pointer()
                            .bg(if active { theme::bg_surface() } else { transparent_black() })
                            .text_color(if active { theme::accent() } else { theme::text_muted() })
                            .child(SharedString::from(bt.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_body_type(bt_val, cx);
                            })),
                    );
                }

                type_row = type_row
                    .child(
                        div()
                            .id("body-undo")
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .text_color(theme::text_primary())
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("↶"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.undo_body(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("body-redo")
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .text_color(theme::text_primary())
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("↷"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.redo_body(window, cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("body-format-json")
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_xs()
                            .cursor_pointer()
                            .text_color(theme::text_primary())
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("Format"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.format_json(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("body-minify-json")
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .text_xs()
                            .cursor_pointer()
                            .text_color(theme::text_primary())
                            .hover(|s| s.bg(theme::bg_hover()))
                            .child(SharedString::from("Minify"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.minify_json(window, cx);
                            })),
                    );

                let is_kv_body = matches!(current_type, BodyType::FormData | BodyType::Urlencoded);
                let binary_path = tab
                    .as_ref()
                    .and_then(|t| t.body.binary_path.clone())
                    .unwrap_or_default();

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(type_row)
                    .child(if current_type == BodyType::Binary {
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .child(
                                div()
                                    .id("pick-binary-body")
                                    .w(px(120.0))
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(theme::bg_surface())
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::bg_hover()))
                                    .child(SharedString::from("Choose file"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.pick_binary_file(window, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .font_family("monospace")
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(if binary_path.is_empty() {
                                        "No file selected".to_string()
                                    } else {
                                        binary_path
                                    })),
                            )
                            .into_any_element()
                    } else if is_kv_body {
                        self.form_data_editor.clone().into_any_element()
                    } else {
                        div()
                            .flex_1()
                            .px_3()
                            .pb_2()
                            .min_h(px(80.0))
                            .font_family("monospace")
                            .child(Editor::new(&self.body_editor).h_full().bordered(false))
                            .into_any_element()
                    })
                    .into_any_element()
            }
            RequestTab::Auth => self.auth_editor.clone().into_any_element(),
            RequestTab::Scripts => {
                let mut script_row = div().flex().items_center().gap_2().px_3().py_2();
                for script_tab in [
                    ScriptTab::PreRequest,
                    ScriptTab::PostResponse,
                    ScriptTab::Tests,
                ] {
                    let active = self.script_tab == script_tab;
                    script_row = script_row.child(
                        div()
                            .id(SharedString::from(format!("script-tab-{:?}", script_tab)))
                            .px_2()
                            .py_1()
                            .rounded_lg()
                            .bg(if active {
                                theme::bg_surface()
                            } else {
                                transparent_black()
                            })
                            .text_xs()
                            .text_color(if active {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                            .cursor_pointer()
                            .child(SharedString::from(script_tab.label()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.script_tab = script_tab;
                                cx.notify();
                            })),
                    );
                }
                let editor = match self.script_tab {
                    ScriptTab::PreRequest => self.pre_request_editor.clone(),
                    ScriptTab::PostResponse => self.post_response_editor.clone(),
                    ScriptTab::Tests => self.test_editor.clone(),
                };
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(script_row)
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .pb_2()
                            .min_h(px(80.0))
                            .font_family("monospace")
                            .child(Editor::new(&editor).h_full().bordered(false)),
                    )
                    .into_any_element()
            }
        };

        let breadcrumb = tab
            .as_ref()
            .map(|t| {
                let collection = t
                    .collection_path
                    .as_deref()
                    .and_then(|path| std::path::Path::new(path).file_name())
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Collection".into());
                let folder = t
                    .file_path
                    .as_deref()
                    .and_then(|path| std::path::Path::new(path).parent())
                    .and_then(|parent| parent.file_name())
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                let folder = if folder == collection {
                    String::new()
                } else {
                    folder
                };
                let request = if t.file_path.is_some() {
                    t.name.clone()
                } else {
                    format!("{} (unsaved)", t.name)
                };
                if folder.is_empty() {
                    format!("{} / {}", collection, request)
                } else {
                    format!("{} / {} / {}", collection, folder, request)
                }
            })
            .unwrap_or_default();

        div()
            .id("request-panel")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(150.0))
            .bg(theme::bg_base())
            .rounded_lg()
            .child(
                div()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .px_3()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(breadcrumb)),
            )
            .child(header_row)
            .child(content)
    }
}
