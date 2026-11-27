//! Global application/UI state — replaces the various Zustand UI stores.

use gpui_kit::*;
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::models::{
    ActivityView, CollectionNode, ConsoleEntry, ConsoleLevel, DiffSnapshot, DiffView,
    EnvironmentFile, HistoryEntry, KeyValuePair, ResponseView, RunnerResult, TrashItem,
};

/// Application-wide UI state that isn't tied to a specific view.
pub struct AppState {
    pub active_view: ActivityView,
    pub side_panel_visible: bool,
    pub bottom_panel_visible: bool,
    pub zen_mode: bool,
    pub response_view: ResponseView,
    pub environment: Option<String>,
    pub toast: Option<(String, ToastKind)>,
    pub toast_deadline: Option<Instant>,

    // ── Workspace data (migrated from Tauri IPC state) ──
    /// All collections shown as sibling roots in the side panel.
    pub collections: Vec<CollectionNode>,
    /// Currently active collection (export/env/settings).
    pub collection: Option<CollectionNode>,
    pub collection_path: Option<String>,
    pub environments: Vec<EnvironmentFile>,
    pub history_entries: Vec<HistoryEntry>,
    pub history_query: String,
    pub history_limit: usize,
    pub cookie_jar: Vec<KeyValuePair>,

    pub timeout_ms: u64,
    pub verify_ssl: bool,
    pub follow_redirects: bool,
    pub proxy_url: String,

    pub runner_running: bool,
    pub runner_results: Vec<RunnerResult>,
    pub diff_snapshots: Vec<DiffSnapshot>,
    pub diff_left: Option<String>,
    pub diff_right: Option<String>,
    pub diff_view: DiffView,
    pub console_entries: Vec<ConsoleEntry>,
    pub trash_items: Vec<TrashItem>,

    pub command_palette_open: bool,
    pub show_onboarding: bool,
    pub dark_mode: bool,

    // ── Overlay menus / dialogs (rendered last in AppView) ──
    pub overlay: Overlay,

    /// Expanded node ids (`node-<path>`) in the collection tree.
    pub expanded_nodes: HashSet<String>,
}

/// Context menu or modal currently shown on top of everything else.
#[derive(Clone, Debug)]
pub enum Overlay {
    None,
    /// HTTP method dropdown at the given window coordinates.
    Method { x: f32, y: f32 },
    /// Right-click menu on a collection-tree node.
    TreeContext {
        x: f32,
        y: f32,
        path: String,
        kind: crate::models::CollectionNodeKind,
        name: String,
    },
    /// "New Collection" dialog.
    NewCollection {
        name: String,
        location: String,
    },
    /// "New Folder" / "New Request" dialog under a tree node.
    NewNode {
        parent_path: String,
        kind: crate::models::CollectionNodeKind,
        name: String,
    },
    /// Rename dialog for a tree node.
    Rename {
        path: String,
        kind: crate::models::CollectionNodeKind,
        name: String,
    },
    /// Ask whether to remove a collection from the panel or delete its files.
    DeleteCollection {
        path: String,
        name: String,
    },
    /// Confirm deletion of a folder/request/case node.
    DeleteNode {
        path: String,
        kind: crate::models::CollectionNodeKind,
        name: String,
    },
    /// Move a request/folder into another collection/folder.
    MoveNode {
        path: String,
        kind: crate::models::CollectionNodeKind,
        name: String,
        target: String,
    },
    /// Save the active request to a chosen folder (Save As).
    SaveAs {
        name: String,
        location: String,
    },
    /// Right-click menu on a request tab.
    TabContext {
        x: f32,
        y: f32,
        tab_id: String,
    },
    /// Request transport settings dialog.
    RequestSettings {
        verify_ssl: bool,
        follow_redirects: bool,
    },
    /// Export format picker for the active collection.
    ExportFormat {
        path: String,
    },
    /// Generated code preview for the active request.
    CodeGen {
        language: String,
    },
    /// Response diff against the most recent history entry.
    ResponseDiff,
    /// Import preview and warnings.
    ImportPreview {
        message: String,
        is_error: bool,
    },
    /// Create or edit an environment file.
    EnvironmentEditor {
        original_name: Option<String>,
        name: String,
        scope: crate::models::EnvironmentScope,
        variables: String,
        secrets: String,
    },
    /// Confirm environment deletion.
    DeleteEnvironment {
        name: String,
        scope: crate::models::EnvironmentScope,
    },
    /// Dedicated cURL paste/import dialog.
    CurlImport,
    /// Collection cookie defaults editor.
    CollectionDefaults {
        send_cookies: bool,
        store_cookies: bool,
        persist_cookies: bool,
    },
    /// Save current request as a named case.
    SaveCase {
        name: String,
    },
    /// Pick one of the saved cases on the active request.
    CaseMenu {
        cases: Vec<String>,
    },
    /// Combined Save / Save as Case menu.
    SaveMenu {
        x: f32,
        y: f32,
    },
    /// Secondary request actions.
    UrlMoreMenu {
        x: f32,
        y: f32,
    },
    /// Secondary collection actions.
    CollectionsMoreMenu {
        x: f32,
        y: f32,
        path: Option<String>,
        name: Option<String>,
    },
    /// Collection runner dialog.
    Runner {
        collection_path: String,
        data_path: String,
    },
    /// Settings dialog with a selected section index.
    Settings {
        section: u8,
    },
    /// Snapshot picker for one side of the Compare dialog.
    DiffPicker {
        side: u8,
        x: f32,
        y: f32,
    },
}

impl Default for Overlay {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_view: ActivityView::Collections,
            side_panel_visible: true,
            bottom_panel_visible: false,
            zen_mode: false,
            response_view: ResponseView::Body,
            environment: None,
            toast: None,
            toast_deadline: None,
            collections: Vec::new(),
            collection: None,
            collection_path: None,
            environments: Vec::new(),
            history_entries: Vec::new(),
            history_query: String::new(),
            history_limit: 100,
            cookie_jar: Vec::new(),
            timeout_ms: 30000,
            verify_ssl: true,
            follow_redirects: true,
            proxy_url: String::new(),
            runner_running: false,
            runner_results: Vec::new(),
            diff_snapshots: Vec::new(),
            diff_left: None,
            diff_right: None,
            diff_view: DiffView::Body,
            console_entries: Vec::new(),
            trash_items: Vec::new(),
            command_palette_open: false,
            show_onboarding: true,
            dark_mode: false,
            overlay: Overlay::None,
            expanded_nodes: HashSet::new(),
        }
    }
}

impl AppState {
    pub fn create(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self::default())
    }

    pub fn set_view(&mut self, view: ActivityView, cx: &mut Context<Self>) {
        if self.active_view == view && self.side_panel_visible {
            self.side_panel_visible = false;
        } else {
            self.active_view = view;
            self.side_panel_visible = true;
        }
        cx.notify();
    }

    pub fn toggle_side_panel(&mut self, cx: &mut Context<Self>) {
        self.side_panel_visible = !self.side_panel_visible;
        cx.notify();
    }

    pub fn toggle_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.bottom_panel_visible = !self.bottom_panel_visible;
        cx.notify();
    }

    pub fn log_console(&mut self, level: ConsoleLevel, message: impl Into<String>, cx: &mut Context<Self>) {
        self.console_entries.push(ConsoleEntry {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            message: message.into(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
        if self.console_entries.len() > 200 {
            let excess = self.console_entries.len() - 200;
            self.console_entries.drain(0..excess);
        }
        cx.notify();
    }

    pub fn clear_console(&mut self, cx: &mut Context<Self>) {
        self.console_entries.clear();
        cx.notify();
    }

    pub fn toggle_zen_mode(&mut self, cx: &mut Context<Self>) {
        self.zen_mode = !self.zen_mode;
        cx.notify();
    }

    pub fn set_response_view(&mut self, view: ResponseView, cx: &mut Context<Self>) {
        self.response_view = view;
        cx.notify();
    }

    pub fn set_environment(&mut self, env: Option<String>, cx: &mut Context<Self>) {
        self.environment = env;
        cx.notify();
    }

    pub fn show_toast(&mut self, message: impl Into<String>, kind: ToastKind, cx: &mut Context<Self>) {
        self.toast = Some((message.into(), kind));
        self.toast_deadline = Some(Instant::now() + Duration::from_secs(4));
        cx.notify();
    }

    pub fn clear_toast(&mut self, cx: &mut Context<Self>) {
        self.toast = None;
        self.toast_deadline = None;
        cx.notify();
    }

    pub fn expire_toast_if_due(&mut self, cx: &mut Context<Self>) {
        let due = self
            .toast_deadline
            .map(|deadline| Instant::now() >= deadline)
            .unwrap_or(false);
        if due {
            self.toast = None;
            self.toast_deadline = None;
            cx.notify();
        }
    }

    pub fn set_overlay(&mut self, overlay: Overlay, cx: &mut Context<Self>) {
        self.overlay = overlay;
        cx.notify();
    }

    pub fn merge_cookies(&mut self, cookies: Vec<KeyValuePair>, cx: &mut Context<Self>) {
        for cookie in cookies {
            self.cookie_jar.retain(|item| item.key != cookie.key);
            self.cookie_jar.push(cookie);
        }
        cx.notify();
    }

    pub fn remove_cookie(&mut self, key: &str, cx: &mut Context<Self>) {
        self.cookie_jar.retain(|item| item.key != key);
        cx.notify();
    }

    pub fn clear_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookie_jar.clear();
        cx.notify();
    }

    pub fn set_command_palette_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.command_palette_open = open;
        cx.notify();
    }

    pub fn refresh_diff_snapshots(&mut self, cx: &mut Context<Self>) {
        self.diff_snapshots = crate::services::diff::list();
        self.ensure_diff_selection();
        cx.notify();
    }

    fn ensure_diff_selection(&mut self) {
        let ids: Vec<String> = self
            .diff_snapshots
            .iter()
            .rev()
            .take(2)
            .map(|snapshot| snapshot.id.clone())
            .collect();
        if self
            .diff_left
            .as_ref()
            .is_none_or(|id| !self.diff_snapshots.iter().any(|s| &s.id == id))
        {
            self.diff_left = ids.first().cloned();
        }
        if self
            .diff_right
            .as_ref()
            .is_none_or(|id| !self.diff_snapshots.iter().any(|s| &s.id == id))
        {
            self.diff_right = ids.get(1).or(ids.first()).cloned();
        }
    }

    pub fn save_diff_snapshot(
        &mut self,
        label: String,
        response: &crate::models::ResponseData,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut response = response.clone();
        response.full_body_path = None;
        let id = uuid::Uuid::new_v4().to_string();
        let snapshot = DiffSnapshot {
            id: id.clone(),
            label,
            created_at: chrono::Utc::now().to_rfc3339(),
            response,
        };
        self.diff_snapshots = crate::services::diff::save(snapshot)?;
        self.diff_left = self.diff_right.take().or_else(|| {
            self.diff_snapshots
                .iter()
                .rev()
                .nth(1)
                .map(|snapshot| snapshot.id.clone())
        });
        self.diff_right = Some(id);
        self.ensure_diff_selection();
        cx.notify();
        Ok(())
    }

    pub fn set_diff_side(&mut self, side: u8, id: String, cx: &mut Context<Self>) {
        if side == 0 {
            self.diff_left = Some(id);
        } else {
            self.diff_right = Some(id);
        }
        cx.notify();
    }

    pub fn set_diff_view(&mut self, view: DiffView, cx: &mut Context<Self>) {
        self.diff_view = view;
        cx.notify();
    }

    pub fn clear_diff_snapshots(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        crate::services::diff::clear()?;
        self.diff_snapshots.clear();
        self.diff_left = None;
        self.diff_right = None;
        cx.notify();
        Ok(())
    }

    pub fn set_dark_mode(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.dark_mode = dark;
        cx.notify();
    }

    pub fn apply_request_settings(
        &mut self,
        timeout_ms: u64,
        proxy_url: String,
        verify_ssl: bool,
        follow_redirects: bool,
        cx: &mut Context<Self>,
    ) {
        self.timeout_ms = timeout_ms;
        self.proxy_url = proxy_url;
        self.verify_ssl = verify_ssl;
        self.follow_redirects = follow_redirects;
        cx.notify();
    }

    pub fn set_trash_items(&mut self, items: Vec<TrashItem>, cx: &mut Context<Self>) {
        self.trash_items = items;
        cx.notify();
    }

    pub fn set_runner_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.runner_running = running;
        cx.notify();
    }

    pub fn set_runner_results(&mut self, results: Vec<RunnerResult>, cx: &mut Context<Self>) {
        self.runner_results = results;
        self.runner_running = false;
        cx.notify();
    }

    pub fn dismiss_onboarding(&mut self, cx: &mut Context<Self>) {
        self.show_onboarding = false;
        cx.notify();
    }

    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        cx.notify();
    }

    // ── Collection / environment / history state ──────────────────────────

    pub fn set_collections(&mut self, collections: Vec<CollectionNode>, cx: &mut Context<Self>) {
        let has_collection_config = |path: &str| {
            Path::new(path)
                .join(".apiark")
                .join("apiark.yaml")
                .exists()
        };
        let mut flattened = Vec::new();
        for node in collections {
            let is_collection_root = matches!(node, CollectionNode::Collection { .. });
            let children_are_collections = !node.children().is_empty()
                && node
                    .children()
                    .iter()
                    .all(|child| has_collection_config(child.path()));
            if is_collection_root
                && !has_collection_config(node.path())
                && children_are_collections
            {
                flattened.extend(node.children().iter().cloned());
            } else {
                flattened.push(node);
            }
        }
        let mut seen = HashSet::new();
        flattened.retain(|node| seen.insert(node.path().to_string()));
        self.collections = flattened;
        cx.notify();
    }

    pub fn set_environments(&mut self, envs: Vec<EnvironmentFile>, cx: &mut Context<Self>) {
        self.environments = envs;
        if self
            .environment
            .as_ref()
            .is_none_or(|name| !self.environments.iter().any(|env| &env.name == name))
        {
            self.environment = self.environments.first().map(|env| env.name.clone());
        }
        cx.notify();
    }

    pub fn set_collection(
        &mut self,
        tree: CollectionNode,
        path: String,
        envs: Vec<EnvironmentFile>,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.collections.iter_mut().find(|n| n.path() == path) {
            *existing = tree.clone();
        } else {
            self.collections.push(tree.clone());
        }
        self.collection = Some(tree);
        self.collection_path = Some(path);
        self.environments = envs;
        if self.environment.is_none() {
            self.environment = self.environments.first().map(|e| e.name.clone());
        }
        cx.notify();
    }

    /// Add a collection root to the side panel without changing the active one.
    pub fn add_collection_tree(&mut self, tree: CollectionNode, cx: &mut Context<Self>) {
        if !self.collections.iter().any(|node| node.path() == tree.path()) {
            self.collections.push(tree);
        }
        cx.notify();
    }

    pub fn clear_collection(&mut self, cx: &mut Context<Self>) {
        self.collection = None;
        self.collection_path = None;
        self.environments.clear();
        self.environment = None;
        cx.notify();
    }

    /// Remove a collection from the side panel without touching its files.
    pub fn remove_collection_from_panel(&mut self, path: &str, cx: &mut Context<Self>) {
        self.collections.retain(|node| node.path() != path);
        if self.collection_path.as_deref() == Some(path) {
            self.collection = None;
            self.collection_path = None;
            self.environments.clear();
            self.environment = None;
        }
        cx.notify();
    }

    pub fn is_node_expanded(&self, node_id: &str) -> bool {
        self.expanded_nodes.contains(node_id)
    }

    pub fn toggle_node(&mut self, node_id: String, cx: &mut Context<Self>) {
        if !self.expanded_nodes.remove(&node_id) {
            self.expanded_nodes.insert(node_id);
        }
        cx.notify();
    }

    pub fn expand_node(&mut self, node_id: String, cx: &mut Context<Self>) {
        if self.expanded_nodes.insert(node_id) {
            cx.notify();
        }
    }

    pub fn refresh_history(&mut self, entries: Vec<HistoryEntry>, cx: &mut Context<Self>) {
        self.history_entries = entries;
        cx.notify();
    }

    pub fn set_history_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.history_query = query;
        cx.notify();
    }
}
