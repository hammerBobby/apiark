//! Form-data editor — ported from `components/request/request-panel.tsx::FormDataEditor`.
//!
//! 5 columns: enabled checkbox / key / value / pick-file / delete.
//! Selecting a file stores its path in `value` and sets `value_type = "file"`,
//! matching the React implementation (the HTTP layer reads the path as text).

use crate::models::KeyValuePair;
use crate::state::TabsState;
use crate::theme;

use super::prelude::*;

struct Row {
    pair_id: String,
    key_input: Entity<InputState>,
    value_input: Entity<InputState>,
    enabled: bool,
    is_file: bool,
}

pub struct FormDataEditor {
    tabs: Entity<TabsState>,
    rows: Vec<Row>,
    fingerprint: String,
    _subscriptions: Vec<Subscription>,
}

impl FormDataEditor {
    pub fn new(tabs: Entity<TabsState>) -> Self {
        Self {
            tabs,
            rows: Vec::new(),
            fingerprint: String::new(),
            _subscriptions: Vec::new(),
        }
    }

    fn current_pairs(&self, cx: &App) -> Vec<KeyValuePair> {
        self.tabs
            .read(cx)
            .active_tab()
            .map(|t| t.body.form_data.clone())
            .unwrap_or_default()
    }

    fn write_pairs(&mut self, pairs: Vec<KeyValuePair>, cx: &mut App) {
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                tab.body.form_data = pairs;
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pairs = self.current_pairs(cx);
        let tab_id = self.tabs.read(cx).active_tab_id().unwrap_or("").to_string();
        let fp = format!("{}:{}", tab_id,
            pairs.iter().map(|p| format!("{}={}", p.id, p.value_type.as_deref().unwrap_or("")))
                .collect::<Vec<_>>().join(","));
        if self.fingerprint == fp {
            return;
        }
        self.fingerprint = fp;

        self._subscriptions.clear();
        self.rows.clear();
        for pair in &pairs {
            let is_file = pair.value_type.as_deref() == Some("file");
            self.add_row_inner(pair.clone(), is_file, window, cx);
        }
        if self.rows.is_empty() {
            self.add_empty_row(window, cx);
        }
    }

    fn add_row_inner(
        &mut self,
        pair: KeyValuePair,
        is_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pair_id = pair.id.clone();
        let key_val = pair.key.clone();
        let value_val = pair.value.clone();

        let key_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("Field name");
            s.set_value(key_val.clone(), window, cx);
            s
        });
        let value_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            if is_file {
                s = s.placeholder("No file selected");
            } else {
                s = s.placeholder("Value");
            }
            s.set_value(value_val.clone(), window, cx);
            s
        });

        let tabs = self.tabs.clone();
        let id = pair_id.clone();
        let sub_key = cx.subscribe_in(&key_input, window, move |_, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                tabs.update(cx, |state, cx| {
                    if let Some(tab) = state.active_tab_mut() {
                        if let Some(p) = tab.body.form_data.iter_mut().find(|p| p.id == id) {
                            p.key = value;
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    }
                });
            }
        });

        let tabs = self.tabs.clone();
        let id = pair_id.clone();
        let sub_value = cx.subscribe_in(&value_input, window, move |_, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                tabs.update(cx, |state, cx| {
                    if let Some(tab) = state.active_tab_mut() {
                        if let Some(p) = tab.body.form_data.iter_mut().find(|p| p.id == id) {
                            // Typing manually clears the file marker (React behavior).
                            p.value = value;
                            p.value_type = None;
                            tab.is_dirty = true;
                            cx.notify();
                        }
                    }
                });
            }
        });

        self._subscriptions.push(sub_key);
        self._subscriptions.push(sub_value);
        self.rows.push(Row {
            pair_id,
            key_input,
            value_input,
            enabled: pair.enabled,
            is_file,
        });
    }

    fn add_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pair = KeyValuePair::new("", "");
        let p = pair.clone();
        self.add_row_inner(pair, false, window, cx);
        let mut pairs = self.current_pairs(cx);
        pairs.push(p);
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
        if let Some(p) = pairs.iter_mut().find(|p| p.id == pair_id) {
            p.enabled = checked;
        }
        self.write_pairs(pairs, cx);
        cx.notify();
    }

    fn pick_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        if index >= self.rows.len() {
            return;
        }
        let pair_id = self.rows[index].pair_id.clone();
        self.rows[index].is_file = true;
        self.rows[index].value_input.update(cx, |state, cx| {
            state.set_value(path_str.clone(), window, cx);
        });
        let mut pairs = self.current_pairs(cx);
        if let Some(p) = pairs.iter_mut().find(|p| p.id == pair_id) {
            p.value = path_str;
            p.value_type = Some("file".into());
        }
        self.write_pairs(pairs, cx);
        self.fingerprint.clear();
        cx.notify();
    }
}

impl Render for FormDataEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);

        let this_weak = cx.weak_entity();
        let mut col = div().flex().flex_col().flex_1().min_h(px(0.0)).overflow_scrollbar();

        // Header row (5 columns).
        col = col.child(
            div().flex().items_center().gap_2().px_3().py_1()
                .border_b_1().border_color(theme::border())
                .child(div().w(px(20.0)))
                .child(div().w(px(160.0)).text_xs().font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted()).child(SharedString::from("Key")))
                .child(div().flex_1().text_xs().font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted()).child(SharedString::from("Value")))
                .child(div().w(px(28.0)))
                .child(div().w(px(24.0))),
        );

        let row_count = self.rows.len();
        for i in 0..row_count {
            let enabled = self.rows[i].enabled;
            let is_file = self.rows[i].is_file;
            let key_input = self.rows[i].key_input.clone();
            let value_input = self.rows[i].value_input.clone();

            let weak = this_weak.clone();
            let mut row = div().flex().items_start().gap_2().px_3().py_1()
                .border_b_1().border_color(theme::border_subtle())
                .child(
                    div().w(px(20.0)).pt_1().child(
                        Checkbox::new(SharedString::from(format!("fd-enabled-{}", i)))
                            .checked(enabled)
                            .on_click(move |checked, _, cx| {
                                let checked = *checked;
                                let _ = weak.update(cx, |this, cx| this.toggle_row(i, checked, cx));
                            }),
                    ),
                )
                .child(div().w(px(160.0)).child(Input::new(&key_input)))
                .child(
                    div().flex_1().child(
                        Input::new(&value_input)
                            .disabled(is_file),
                    ),
                );

            // File picker button.
            row = row.child(
                div()
                    .id(SharedString::from(format!("fd-file-{}", i)))
                    .w(px(28.0)).h(px(28.0)).flex().items_center().justify_center()
                    .text_sm()
                    .text_color(if is_file { theme::accent() } else { theme::text_muted() })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::bg_hover()))
                    .child(SharedString::from("📁"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_file(i, window, cx);
                    })),
            );

            // Delete button.
            row = row.child(
                div()
                    .id(SharedString::from(format!("fd-delete-{}", i)))
                    .w(px(24.0)).h(px(28.0)).flex().items_center().justify_center()
                    .text_sm().text_color(theme::text_muted()).cursor_pointer()
                    .hover(|s| s.text_color(theme::error()))
                    .child(SharedString::from("×"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_row(i, cx);
                    })),
            );

            col = col.child(row);
        }

        let _ = this_weak;
        col = col.child(
            div().id("fd-add-row").flex().items_center().gap_1().px_3().py_2()
                .text_sm().text_color(theme::accent()).cursor_pointer()
                .hover(|s| s.bg(theme::bg_hover()))
                .child(SharedString::from("+ Add"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.add_empty_row(window, cx);
                })),
        );

        col
    }
}
