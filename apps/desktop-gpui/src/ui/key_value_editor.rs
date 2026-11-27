//! Reusable key-value editor (Params / Headers / urlencoded body).
//! Ported from `apps/desktop/src/components/request/key-value-editor.tsx`.
//!
//! Each row owns two `InputState` entities. A `Change` subscription writes the
//! edited value back into the active tab's `Vec<KeyValuePair>`, just like the
//! React `onChange` handler.

use crate::models::KeyValuePair;
use crate::state::TabsState;
use crate::theme;

use super::prelude::*;

/// Which list in the active tab this editor is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvTarget {
    Params,
    Headers,
    PathParams,
}

struct Row {
    pair_id: String,
    key_input: Entity<InputState>,
    value_input: Entity<InputState>,
    enabled: bool,
}

pub struct KeyValueEditor {
    tabs: Entity<TabsState>,
    target: KvTarget,
    rows: Vec<Row>,
    fingerprint: String,
    key_placeholder: String,
    value_placeholder: String,
    /// Common key suggestions (e.g. common HTTP headers).
    suggestions: Vec<String>,
    show_suggestions_for: Option<usize>,
    selected_suggestion: usize,
    _subscriptions: Vec<Subscription>,
}

impl KeyValueEditor {
    pub fn new(tabs: Entity<TabsState>, target: KvTarget) -> Self {
        let (key_ph, value_ph) = match target {
            KvTarget::Params => ("Parameter", "Value"),
            KvTarget::Headers => ("Header", "Value"),
            KvTarget::PathParams => ("Path variable", "Value"),
        };
        Self {
            tabs,
            target,
            rows: Vec::new(),
            fingerprint: String::new(),
            key_placeholder: key_ph.into(),
            value_placeholder: value_ph.into(),
            suggestions: match target {
                KvTarget::Headers => COMMON_HEADERS.iter().map(|s| s.to_string()).collect(),
                KvTarget::Params | KvTarget::PathParams => Vec::new(),
            },
            show_suggestions_for: None,
            selected_suggestion: 0,
            _subscriptions: Vec::new(),
        }
    }

    /// Read the current list from the active tab.
    fn current_pairs(&self, cx: &App) -> Vec<KeyValuePair> {
        let tabs = self.tabs.read(cx);
        let Some(tab) = tabs.active_tab() else {
            return Vec::new();
        };
        match self.target {
            KvTarget::Params => tab.params.clone(),
            KvTarget::Headers => tab.headers.clone(),
            KvTarget::PathParams => tab.path_params.clone(),
        }
    }

    fn write_pairs(&mut self, pairs: Vec<KeyValuePair>, cx: &mut App) {
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                match self.target {
                    KvTarget::Params => tab.params = pairs,
                    KvTarget::Headers => tab.headers = pairs,
                    KvTarget::PathParams => tab.path_params = pairs,
                }
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    /// Rebuild row editors when the active tab or its pair ids change.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pairs = self.current_pairs(cx);
        let tab_id = self.tabs.read(cx).active_tab_id().unwrap_or("").to_string();
        let fp = format!("{}:{}", tab_id,
            pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(","));

        if self.fingerprint == fp {
            return;
        }
        self.fingerprint = fp;

        self._subscriptions.clear();
        self.rows.clear();

        for pair in &pairs {
            let pair_id = pair.id.clone();
            let key_val = pair.key.clone();
            let value_val = pair.value.clone();
            let enabled = pair.enabled;
            self.add_row_inner(pair_id, key_val, value_val, enabled, window, cx);
        }

        // Always keep at least one empty row (like the React editor).
        if self.rows.is_empty() {
            self.add_empty_row(window, cx);
        }
    }

    fn add_row_inner(
        &mut self,
        pair_id: String,
        key: String,
        value: String,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_index = self.rows.len();
        let key_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(self.key_placeholder.clone());
            s.set_value(key.clone(), window, cx);
            s
        });
        let value_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(self.value_placeholder.clone());
            s.set_value(value.clone(), window, cx);
            s
        });

        // Subscribe key input -> write back to the matching pair.
        let tabs = self.tabs.clone();
        let target = self.target;
        let id = pair_id.clone();
        let sub_key = cx.subscribe_in(&key_input, window, move |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                tabs.update(cx, |state, cx| {
                    if let Some(tab) = state.active_tab_mut() {
                        let list = match target {
                            KvTarget::Params => &mut tab.params,
                            KvTarget::Headers => &mut tab.headers,
                            KvTarget::PathParams => &mut tab.path_params,
                        };
                        if let Some(pair) = list.iter_mut().find(|p| p.id == id) {
                            pair.key = value;
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    }
                });
                this.show_suggestions_for = Some(row_index);
                this.selected_suggestion = 0;
                cx.notify();
            }
        });

        let tabs = self.tabs.clone();
        let id = pair_id.clone();
        let sub_value = cx.subscribe_in(
            &value_input,
            window,
            move |this, input, event, window, cx| {
                match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        tabs.update(cx, |state, cx| {
                            if let Some(tab) = state.active_tab_mut() {
                                let list = match target {
                                    KvTarget::Params => &mut tab.params,
                                    KvTarget::Headers => &mut tab.headers,
                                    KvTarget::PathParams => &mut tab.path_params,
                                };
                                if let Some(pair) = list.iter_mut().find(|p| p.id == id) {
                                    pair.value = value;
                                    tab.is_dirty = true;
                                    cx.notify();
                                }
                            }
                        });
                    }
                    InputEvent::Blur => {
                        let filled = !input.read(cx).value().trim().is_empty();
                        let is_last = this
                            .current_pairs(cx)
                            .last()
                            .is_some_and(|pair| pair.id == id);
                        if filled && is_last {
                            this.add_empty_row(window, cx);
                        }
                    }
                    _ => {}
                }
            },
        );

        self._subscriptions.push(sub_key);
        self._subscriptions.push(sub_value);
        self.rows.push(Row {
            pair_id,
            key_input,
            value_input,
            enabled,
        });
    }

    fn add_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pair = KeyValuePair::new("", "");
        let pair_id = pair.id.clone();
        self.add_row_inner(pair_id.clone(), String::new(), String::new(), true, window, cx);
        let mut pairs = self.current_pairs(cx);
        pairs.push(pair);
        self.write_pairs(pairs, cx);
    }

    fn remove_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.rows.len() {
            return;
        }
        let pair_id = self.rows[index].pair_id.clone();
        self.rows.remove(index);
        let mut pairs = self.current_pairs(cx);
        pairs.retain(|p| p.id != pair_id);
        if pairs.is_empty() {
            pairs.push(KeyValuePair::new("", ""));
        }
        self.write_pairs(pairs, cx);
        self.fingerprint.clear();
        cx.notify();
    }

    fn toggle_row(&mut self, index: usize, checked: bool, cx: &mut Context<Self>) {
        if index >= self.rows.len() {
            return;
        }
        self.rows[index].enabled = checked;
        let pair_id = self.rows[index].pair_id.clone();
        let mut pairs = self.current_pairs(cx);
        if let Some(pair) = pairs.iter_mut().find(|p| p.id == pair_id) {
            pair.enabled = checked;
        }
        self.write_pairs(pairs, cx);
        cx.notify();
    }

    fn filtered_suggestions(&self, query: &str) -> Vec<String> {
        self.suggestions
            .iter()
            .filter(|suggestion| suggestion.to_lowercase().starts_with(query))
            .take(8)
            .cloned()
            .collect()
    }

    fn move_suggestion(&mut self, delta: isize, count: usize) {
        if count == 0 {
            self.selected_suggestion = 0;
            return;
        }
        let current = self.selected_suggestion as isize;
        let next = current + delta;
        self.selected_suggestion = if next < 0 {
            count - 1
        } else if next >= count as isize {
            0
        } else {
            next as usize
        };
    }

    fn choose_suggestion(&mut self, row: usize, key: String, window: &mut Window, cx: &mut Context<Self>) {
        if row >= self.rows.len() {
            return;
        }
        let pair_id = self.rows[row].pair_id.clone();
        let input = self.rows[row].key_input.clone();
        input.update(cx, |state, cx| state.set_value(key.clone(), window, cx));
        let mut pairs = self.current_pairs(cx);
        if let Some(pair) = pairs.iter_mut().find(|p| p.id == pair_id) {
            pair.key = key;
        }
        self.write_pairs(pairs, cx);
        self.show_suggestions_for = None;
        cx.notify();
    }
}

impl Render for KeyValueEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);

        let id_scope = format!("{:?}", self.target);
        let mut col = div().flex().flex_col().flex_1().min_h(px(0.0)).overflow_scrollbar();

        // Header row.
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(theme::border())
                .child(div().w(px(20.0)))
                .child(
                    div()
                        .w(px(180.0))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_muted())
                        .child(SharedString::from("Key")),
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_muted())
                        .child(SharedString::from("Value")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("kv-header-add-{}", id_scope)))
                        .w(px(24.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .text_sm()
                        .text_color(theme::accent())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_hover()))
                        .child(SharedString::from("+"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_empty_row(window, cx);
                        })),
                ),
        );

        let this_weak = cx.weak_entity();
        let row_count = self.rows.len();
        for i in 0..row_count {
            let enabled = self.rows[i].enabled;
            let key_input = self.rows[i].key_input.clone();
            let value_input = self.rows[i].value_input.clone();
            let key_query = self.rows[i].key_input.read(cx).value().to_string().to_lowercase();
            let suggestion_items = self.filtered_suggestions(&key_query);
            let show_suggestions = self.show_suggestions_for == Some(i)
                && !suggestion_items.is_empty();
            let is_last = i + 1 == row_count;

            let weak = this_weak.clone();
            let mut row = div()
                .flex()
                .items_start()
                .gap_2()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(theme::border_subtle())
                .child(
                    div().w(px(20.0)).pt_1().child(
                        Checkbox::new(SharedString::from(format!("kv-enabled-{}-{}", id_scope, i)))
                            .checked(enabled)
                            .on_click(move |checked, _, cx| {
                                let checked = *checked;
                                let _ = weak.update(cx, |this, cx| {
                                    this.toggle_row(i, checked, cx);
                                });
                            }),
                    ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("kv-key-wrap-{}-{}", id_scope, i)))
                        .w(px(180.0))
                        .rounded_lg()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.show_suggestions_for = Some(i);
                                this.selected_suggestion = 0;
                                cx.notify();
                            }),
                        )
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if this.show_suggestions_for != Some(i) {
                                return;
                            }
                            let query = this.rows[i].key_input.read(cx).value().to_string().to_lowercase();
                            let items = this.filtered_suggestions(&query);
                            if items.is_empty() {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "down" | "arrowdown" => {
                                    cx.stop_propagation();
                                    this.move_suggestion(1, items.len());
                                    cx.notify();
                                }
                                "up" | "arrowup" => {
                                    cx.stop_propagation();
                                    this.move_suggestion(-1, items.len());
                                    cx.notify();
                                }
                                "enter" | "tab" => {
                                    cx.stop_propagation();
                                    let index = this.selected_suggestion.min(items.len() - 1);
                                    let value = items[index].clone();
                                    this.choose_suggestion(i, value, window, cx);
                                }
                                "escape" => {
                                    cx.stop_propagation();
                                    this.show_suggestions_for = None;
                                    cx.notify();
                                }
                                _ => {}
                            }
                        }))
                        .child(Input::new(&key_input)),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("kv-value-wrap-{}-{}", id_scope, i)))
                        .flex_1()
                        .rounded_lg()
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            let m = &event.keystroke.modifiers;
                            if is_last
                                && event.keystroke.key == "tab"
                                && !m.control
                                && !m.alt
                                && !m.shift
                                && !m.platform
                                && !m.function
                            {
                                this.add_empty_row(window, cx);
                            }
                        }))
                        .child(Input::new(&value_input)),
                );

            // Delete button.
            row = row.child(
                div()
                    .id(SharedString::from(format!("kv-delete-{}-{}", id_scope, i)))
                    .w(px(24.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::error()))
                    .child(SharedString::from("×"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_row(i, cx);
                    })),
            );

            col = col.child(row);

            // Suggestion dropdown for the focused key field.
            if show_suggestions {
                let mut list = div()
                    .flex()
                    .flex_col()
                    .mx_3()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg();
                let selected = self.selected_suggestion.min(suggestion_items.len().saturating_sub(1));
                for (index, suggestion) in suggestion_items.iter().enumerate() {
                    let s = suggestion.clone();
                    let value = s.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("kv-sugg-{}-{}-{}", id_scope, i, s)))
                            .px_3()
                            .py_1()
                            .rounded_lg()
                            .bg(if index == selected {
                                theme::bg_hover()
                            } else {
                                theme::bg_surface()
                            })
                            .text_sm()
                            .text_color(theme::text_primary())
                            .cursor_pointer()
                            .hover(|st| st.bg(theme::bg_hover()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.choose_suggestion(i, value.clone(), window, cx);
                                }),
                            )
                            .child(SharedString::from(s)),
                    );
                }
                col = col.child(list);
            }
        }

        // Add row button.
        col = col.child(
            div()
                .id(SharedString::from(format!("kv-add-row-{}", id_scope)))
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(theme::accent())
                .cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("+ Add"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.add_empty_row(window, cx);
                })),
        );

        col
    }
}

/// Common HTTP header names (from React `key-value-editor.tsx`).
const COMMON_HEADERS: &[&str] = &[
    "Authorization",
    "Content-Type",
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Cache-Control",
    "Connection",
    "Cookie",
    "Host",
    "Origin",
    "Referer",
    "User-Agent",
    "X-Requested-With",
    "X-API-Key",
];
