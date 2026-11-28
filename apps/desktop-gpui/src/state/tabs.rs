//! Tab state management — GPUI Entity replacing Zustand `tab-store.ts`.

use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::models::{parse_url_path_variables, HttpMethod, Tab};
use crate::services::collection as collection_service;

/// Central tab state, stored as a GPUI `Entity<TabsState>`.
/// GPUI entities provide fine-grained change notification via `cx.notify()`,
/// replacing Zustand's selector-based subscriptions.
#[derive(Default)]
pub struct TabsState {
    tabs: Vec<Tab>,
    active_tab_id: Option<String>,
    next_tab_index: usize,
    pinned_ids: HashSet<String>,
}

#[derive(Serialize, Deserialize)]
struct TabsSnapshot {
    tabs: Vec<Tab>,
    active_tab_id: Option<String>,
    next_tab_index: usize,
}

/// Rewrite a path that points inside a moved file/folder.
fn retarget_path(old: &Path, new: &Path, value: &str) -> Option<String> {
    let current = Path::new(value);
    if current == old || current.starts_with(old) {
        let suffix = current.strip_prefix(old).unwrap_or(Path::new(""));
        Some(new.join(suffix).to_string_lossy().to_string())
    } else {
        None
    }
}

/// Case tabs share the parent request's file path, so case identity must be
/// part of the tab key; otherwise opening a case just activates the parent tab.
fn same_open_target(a: &Tab, b: &Tab) -> bool {
    match (a.file_path.as_deref(), b.file_path.as_deref()) {
        (Some(a_path), Some(b_path)) => a_path == b_path && a.case_name == b.case_name,
        _ => false,
    }
}

/// "Save as Case" always creates a new case, never overwrites an existing one.
fn unique_case_name(base: &str, existing: &[String]) -> String {
    let taken: HashSet<&str> = existing.iter().map(String::as_str).collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{} {}", base, suffix);
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// A duplicated tab must not keep the source request's file path, otherwise
/// Save would overwrite the original. The source folder is kept only as the
/// default location for Save As.
fn duplicate_payload(tab: &Tab) -> Tab {
    let mut copy = tab.clone();
    copy.id = uuid::Uuid::new_v4().to_string();
    copy.name = format!("{} Copy", tab.name);
    copy.case_name = None;
    copy.is_dirty = true;
    if let Some(path) = tab.file_path.as_deref() {
        if let Some(parent) = std::path::Path::new(path).parent() {
            copy.collection_path = Some(parent.to_string_lossy().to_string());
        }
    }
    copy.file_path = None;
    copy
}

impl TabsState {
    fn tabs_file() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".apiark")
            .join("tabs.json")
    }

    pub fn load() -> Option<Self> {
        let json = std::fs::read_to_string(Self::tabs_file()).ok()?;
        let snapshot: TabsSnapshot = serde_json::from_str(&json).ok()?;
        if snapshot.tabs.is_empty() {
            return None;
        }
        let active_tab_id = snapshot
            .active_tab_id
            .filter(|id| snapshot.tabs.iter().any(|tab| &tab.id == id))
            .or_else(|| snapshot.tabs.first().map(|tab| tab.id.clone()));
        Some(Self {
            tabs: snapshot.tabs,
            active_tab_id,
            next_tab_index: snapshot.next_tab_index,
            pinned_ids: HashSet::new(),
        })
    }

    pub fn save(&self) {
        let persisted: Vec<Tab> = self
            .tabs
            .iter()
            .map(|tab| {
                let mut tab = tab.clone();
                tab.response = None;
                tab.test_results.clear();
                tab.loading = false;
                tab
            })
            .collect();
        let snapshot = TabsSnapshot {
            tabs: persisted,
            active_tab_id: self.active_tab_id.clone(),
            next_tab_index: self.next_tab_index,
        };
        let Ok(json) = serde_json::to_string_pretty(&snapshot) else {
            return;
        };
        let path = Self::tabs_file();
        // Persist on a single background writer so tab operations never block
        // the UI thread. A single writer keeps writes ordered (latest wins).
        static WRITER: OnceLock<Arc<Mutex<std::sync::mpsc::Sender<(PathBuf, String)>>>> =
            OnceLock::new();
        let sender = WRITER.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, String)>();
            std::thread::Builder::new()
                .name("apiark-tabs-writer".to_string())
                .spawn(move || {
                    while let Ok((path, json)) = rx.recv() {
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::write(&path, json) {
                            tracing::warn!("Failed to persist tabs: {e}");
                        }
                    }
                })
                .expect("failed to spawn tabs writer thread");
            Arc::new(Mutex::new(tx))
        });
        if let Ok(tx) = sender.lock() {
            let _ = tx.send((path, json));
        }
    }

    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab_id: None,
            next_tab_index: 0,
            pinned_ids: HashSet::new(),
        }
    }

    /// Create the entity and open an initial tab.
    pub fn create(cx: &mut App) -> Entity<Self> {
        let restored = Self::load().unwrap_or_else(Self::new);
        if restored.tabs.is_empty() {
            let entity = cx.new(|_| restored);
            entity.update(cx, |this, cx| {
                this.new_tab(cx);
            });
            entity
        } else {
            cx.new(|_| restored)
        }
    }

    // ── Queries ────────────────────────────────────────────────────────────

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Mutable access to all tabs (used by the HTTP service callback).
    pub fn tabs_mut(&mut self) -> &mut Vec<Tab> {
        &mut self.tabs
    }

    pub fn active_tab_id(&self) -> Option<&str> {
        self.active_tab_id.as_deref()
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active_tab_id.clone()?;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned_ids.contains(id)
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.pinned_ids.remove(id) {
            self.pinned_ids.insert(id.to_string());
        }
        self.save();
        cx.notify();
    }

    pub fn duplicate_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(pos) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let tab = duplicate_payload(&self.tabs[pos]);
        let new_id = tab.id.clone();
        self.tabs.insert(pos + 1, tab);
        self.active_tab_id = Some(new_id);
        self.save();
        cx.notify();
    }

    // ── Tab lifecycle ──────────────────────────────────────────────────────

    pub fn new_tab(&mut self, cx: &mut Context<Self>) -> String {
        self.next_tab_index += 1;
        let tab = Tab {
            name: format!("Untitled Request {}", self.next_tab_index),
            ..Default::default()
        };
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.active_tab_id = Some(id.clone());
        self.save();
        cx.notify();
        id
    }

    pub fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_tab_id.as_deref() == Some(id) {
                self.active_tab_id = self.tabs.last().map(|t| t.id.clone());
            }
            self.save();
            cx.notify();
        }
    }

    pub fn close_other_tabs(&mut self, id: &str, cx: &mut Context<Self>) {
        self.tabs.retain(|t| t.id == id);
        self.active_tab_id = Some(id.to_string());
        self.save();
        cx.notify();
    }

    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.active_tab_id = None;
        self.save();
        cx.notify();
    }

    pub fn set_active_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_tab_id = Some(id.to_string());
            self.save();
            cx.notify();
        }
    }

    pub fn activate_prev_tab(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self
            .active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|t| &t.id == id))
        else {
            return;
        };
        if current == 0 {
            return;
        }
        self.active_tab_id = Some(self.tabs[current - 1].id.clone());
        self.save();
        cx.notify();
    }

    pub fn activate_next_tab(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self
            .active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|t| &t.id == id))
        else {
            return;
        };
        if current + 1 >= self.tabs.len() {
            return;
        }
        self.active_tab_id = Some(self.tabs[current + 1].id.clone());
        self.save();
        cx.notify();
    }

    /// Open a request file, reusing an existing tab for the same path instead
    /// of creating a duplicate tab on every click.
    pub fn open_or_activate(&mut self, tab: Tab, cx: &mut Context<Self>) -> String {
        if tab.file_path.is_some() {
            if let Some(existing) = self.tabs.iter_mut().find(|t| same_open_target(t, &tab)) {
                let id = existing.id.clone();
                if existing.response.is_none() && tab.response.is_some() {
                    existing.response = tab.response.clone();
                }
                self.active_tab_id = Some(id.clone());
                self.save();
                cx.notify();
                return id;
            }
        }
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.active_tab_id = Some(id.clone());
        self.save();
        cx.notify();
        id
    }

    /// Keep tabs pointing at the right file after a request/folder was moved.
    pub fn retarget_moved_paths(
        &mut self,
        old_prefix: &str,
        new_prefix: &str,
        cx: &mut Context<Self>,
    ) {
        let old = Path::new(old_prefix);
        let new = Path::new(new_prefix);
        for tab in &mut self.tabs {
            if let Some(path) = tab.file_path.as_deref() {
                if let Some(updated) = retarget_path(old, new, path) {
                    tab.file_path = Some(updated);
                }
            }
            if let Some(path) = tab.collection_path.as_deref() {
                if let Some(updated) = retarget_path(old, new, path) {
                    tab.collection_path = Some(updated);
                }
            }
        }
        self.save();
        cx.notify();
    }

    pub fn select_next(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self
            .active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|t| &t.id == id))
        else {
            return;
        };
        if idx + 1 >= self.tabs.len() {
            return;
        }
        self.active_tab_id = Some(self.tabs[idx + 1].id.clone());
        self.save();
        cx.notify();
    }

    pub fn select_prev(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self
            .active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().position(|t| &t.id == id))
        else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.active_tab_id = Some(self.tabs[idx - 1].id.clone());
        cx.notify();
    }

    // ── Active tab mutations ───────────────────────────────────────────────

    pub fn set_method(&mut self, method: HttpMethod, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.method = method;
            tab.is_dirty = true;
            cx.notify();
        }
    }

    pub fn set_url(&mut self, url: String, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.url = url.clone();
            // URL query string is the source of truth — sync into the params table
            // like the React `setUrl` store action does.
            tab.params = parse_url_params(&url);
            tab.path_params = parse_url_path_variables(&url, &tab.path_params);
            tab.is_dirty = true;
            cx.notify();
        }
    }

    pub fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.is_dirty = true;
            cx.notify();
        }
    }

    pub fn mark_clean(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.is_dirty = false;
            cx.notify();
        }
    }

    pub fn save_case(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(tab) = self.active_tab_mut() else {
            return Err("No active request".into());
        };
        let path = tab.file_path.clone().ok_or("Save the request before adding a case")?;
        if name.trim().is_empty() {
            return Err("Case name cannot be empty".into());
        }
        let mut file = collection_service::read_request(std::path::Path::new(&path))?;
        let existing_names: Vec<String> = file.cases.iter().map(|c| c.name.clone()).collect();
        let unique_name = unique_case_name(name, &existing_names);
        let mut case = tab.to_request_file();
        case.name = unique_name.clone();
        case.cases.clear();
        file.cases.retain(|existing| existing.name != unique_name);
        file.cases.push(case);
        collection_service::write_request(std::path::Path::new(&path), &file)?;
        tab.cases = file.cases;
        cx.notify();
        Ok(())
    }

    pub fn activate_case(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(case) = self
            .active_tab()
            .and_then(|tab| tab.cases.get(index).cloned())
        else {
            return;
        };
        if let Some(tab) = self.active_tab_mut() {
            tab.apply_case(&case);
            cx.notify();
        }
    }

    pub fn load_full_response(&mut self, cx: &mut Context<Self>) {
        if let Some(response) = self.active_tab_mut().and_then(|tab| tab.response.as_mut()) {
            if let Some(path) = response.full_body_path.clone() {
                if let Ok(full) = std::fs::read_to_string(&path) {
                    response.body = full;
                    response.truncated = None;
                    cx.notify();
                }
            }
        }
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.loading = loading;
            cx.notify();
        }
    }

    /// Apply a parsed cURL command to the active tab (ported from
    /// `lib/apply-parsed-curl.ts::applyParsedCurlToCurrentTab`).
    pub fn apply_parsed_curl(
        &mut self,
        parsed: &crate::services::curl::ParsedCurlRequest,
        cx: &mut Context<Self>,
    ) {
        use crate::models::{parse_body_type, AuthConfig, HttpMethod, KeyValuePair, RequestBody};

        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.method = HttpMethod::parse(&parsed.method);
        tab.url = parsed.url.clone();
        tab.params = parse_url_params(&parsed.url);
        tab.path_params = parse_url_path_variables(&parsed.url, &tab.path_params);
        tab.headers = parsed
            .headers
            .iter()
            .map(|(k, v)| KeyValuePair::new(k, v))
            .collect();
        tab.body = RequestBody {
            body_type: parsed
                .body_type
                .as_deref()
                .map(parse_body_type)
                .unwrap_or(crate::models::BodyType::None),
            content: parsed.body.clone().unwrap_or_default(),
            form_data: parsed
                .form_data
                .iter()
                .map(|f| {
                    let mut p = KeyValuePair::new(&f.key, &f.value);
                    p.value_type = f.value_type.clone();
                    p
                })
                .collect(),
            binary_path: None,
        };
        if let Some((user, pass)) = &parsed.auth_basic {
            tab.auth = AuthConfig::Basic {
                username: user.clone(),
                password: pass.clone(),
            };
        }
        tab.is_dirty = true;
        cx.notify();
    }
}

/// Parse query params from a URL into KeyValuePair rows.
/// Only real query parameters are returned; the editor adds an empty row when needed.
fn parse_url_params(url: &str) -> Vec<crate::models::KeyValuePair> {
    use crate::models::KeyValuePair;

    let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut rows: Vec<KeyValuePair> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key);
        if key.is_empty() {
            continue;
        }
        rows.push(KeyValuePair::new(key, percent_decode(value)));
    }
    rows
}

/// Minimal percent-decoding (`%XX` and `+` → space).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| -> Option<u8> {
                    match b {
                        b'0'..=b'9' => Some(b - b'0'),
                        b'a'..=b'f' => Some(b - b'a' + 10),
                        b'A'..=b'F' => Some(b - b'A' + 10),
                        _ => None,
                    }
                };
                if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(hi * 16 + lo);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{duplicate_payload, retarget_path, same_open_target, unique_case_name};
    use crate::models::{RequestFile, Tab};

    #[test]
    fn duplicated_tab_has_no_source_file_path() {
        let file = RequestFile {
            name: "source".into(),
            ..Default::default()
        };
        let source =
            Tab::from_request_file("/collections/merc/source.yaml", "/collections/merc", &file);
        let copy = duplicate_payload(&source);
        assert!(copy.id != source.id);
        assert!(copy.name == "source Copy");
        assert!(copy.is_dirty);
        assert!(copy.file_path.is_none());
        assert!(copy.collection_path.as_deref() == Some("/collections/merc"));
    }

    #[test]
    fn case_tab_is_distinct_from_parent_request_tab() {
        let file = RequestFile {
            name: "source".into(),
            ..Default::default()
        };
        let parent =
            Tab::from_request_file("/collections/merc/source.yaml", "/collections/merc", &file);
        let mut case =
            Tab::from_request_file("/collections/merc/source.yaml", "/collections/merc", &file);
        case.case_name = Some("成功".into());
        assert!(same_open_target(&parent, &parent));
        assert!(same_open_target(&case, &case.clone()));
        assert!(!same_open_target(&parent, &case));
    }

    #[test]
    fn duplicated_case_tab_drops_case_identity() {
        let file = RequestFile {
            name: "source".into(),
            ..Default::default()
        };
        let mut source =
            Tab::from_request_file("/collections/merc/source.yaml", "/collections/merc", &file);
        source.case_name = Some("成功".into());
        let copy = duplicate_payload(&source);
        assert!(copy.case_name.is_none());
        assert!(copy.file_path.is_none());
    }

    #[test]
    fn retargets_paths_after_move() {
        let old_file = std::path::Path::new("/c/a/req.yaml");
        let new_file = std::path::Path::new("/c/b/req.yaml");
        let moved = retarget_path(old_file, new_file, "/c/a/req.yaml").unwrap();
        assert_eq!(std::path::Path::new(&moved), new_file);

        let nested = retarget_path(
            std::path::Path::new("/c/a"),
            std::path::Path::new("/c/b"),
            "/c/a/nested/req.yaml",
        )
        .unwrap();
        assert_eq!(
            std::path::Path::new(&nested),
            std::path::Path::new("/c/b/nested/req.yaml")
        );
        assert!(retarget_path(old_file, new_file, "/c/other/req.yaml").is_none());
    }

    #[test]
    fn save_as_case_never_reuses_a_name() {
        let existing = vec!["成功".to_string(), "成功 2".to_string()];
        assert!(unique_case_name("成功", &existing) == "成功 3");
        assert!(unique_case_name("失败", &existing) == "失败");
    }
}


