//! Workspace coordinator — loads collections/environments/history into GPUI state.
//! Replaces the async `invoke` calls in the Tauri React frontend.

use crate::models::{
    AuthConfig, CollectionNodeKind, EnvironmentFile, EnvironmentScope, HistoryEntry, HttpMethod,
    ResponseData,
};
use crate::services::{
    backup, collection, environment, trash,
    import_export::{self, ImportData},
    oauth::{OAuthTokenStatus, OAuthTokenStore},
    HistoryStore,
};
use crate::state::{AppState, TabsState};
use gpui_kit::*;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub struct WorkspaceService {
    history: HistoryStore,
    oauth: std::sync::Arc<OAuthTokenStore>,
    watcher: RefCell<Option<RecommendedWatcher>>,
    reload_rx: RefCell<Option<Receiver<notify::Result<notify::Event>>>>,
}

impl WorkspaceService {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            history: HistoryStore::open_default()?,
            oauth: std::sync::Arc::new(OAuthTokenStore::new()),
            watcher: RefCell::new(None),
            reload_rx: RefCell::new(None),
        })
    }

    /// Clone the OAuth store handle for use in 'static async tasks.
    pub fn oauth_store(&self) -> std::sync::Arc<OAuthTokenStore> {
        self.oauth.clone()
    }

    fn start_watching(&self, path: &Path) {
        let (tx, rx) = channel();
        let watcher = notify::recommended_watcher(move |event| {
            if let Ok(event) = event {
                let _ = tx.send(Ok(event));
            }
        });
        if let Ok(mut watcher) = watcher {
            let _ = watcher.watch(path, RecursiveMode::Recursive);
            *self.watcher.borrow_mut() = Some(watcher);
            *self.reload_rx.borrow_mut() = Some(rx);
        }
    }

    /// Drain pending file-change events. Returns true when a reload is needed.
    pub fn take_reload_events(&self) -> bool {
        let binding = self.reload_rx.borrow();
        let Some(rx) = binding.as_ref() else {
            return false;
        };
        let mut changed = false;
        while let Ok(event) = rx.try_recv() {
            if event.is_ok() {
                changed = true;
            }
        }
        changed
    }

    /// Reload collections and environments from disk.
    pub fn reload_workspace_data(&self, app_state: &Entity<AppState>, cx: &mut App) {
        let _ = self.refresh_collections(app_state, cx);
        let has_collection = app_state.read(cx).collection_path.is_some();
        if has_collection {
            let _ = self.reload_environments(app_state, cx);
        }
    }

    // ── OAuth2 ─────────────────────────────────────────────────────────────

    pub fn oauth_status(&self, config: &AuthConfig) -> OAuthTokenStatus {
        match config {
            AuthConfig::Oauth2 { client_id, auth_url, .. } => {
                self.oauth.status(&OAuthTokenStore::cache_key(client_id, auth_url))
            }
            _ => OAuthTokenStatus {
                has_token: false,
                is_expired: false,
                expires_at: None,
                token_type: None,
            },
        }
    }

    /// Return a cached, valid OAuth access token (no flow started).
    pub fn oauth_cached_token(&self, config: &AuthConfig) -> Option<String> {
        self.oauth.cached_access_token(config)
    }

    pub fn oauth_clear(&self, config: &AuthConfig) {
        if let AuthConfig::Oauth2 { client_id, auth_url, .. } = config {
            self.oauth.remove(&OAuthTokenStore::cache_key(client_id, auth_url));
        }
    }

    /// Obtain an access token (cached/refresh/new flow) for the given config.
    pub async fn oauth_access_token(&self, config: &AuthConfig) -> Result<String, String> {
        self.oauth.access_token(config).await
    }

    /// Open a collection folder and load everything into state (synchronous, in-process).
    pub fn open_collection(
        &self,
        path: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let dir = Path::new(path);
        let path = collection::ensure_collection(dir)?;
        collection::migrate_collection(Path::new(&path))?;
        self.save_last_collection_path(Path::new(&path));
        let tree = collection::load_collection_tree(Path::new(&path))?;
        let envs = environment::load_environments(dir)?;
        app_state.update(cx, |state, cx| {
            state.set_collection(tree, path.clone(), envs, cx);
            state.expand_node(format!("node-{}", path), cx);
        });
        self.start_watching(Path::new(&path));
        self.refresh_collections(app_state, cx)
    }

    /// Reload every collection so the side panel shows them side by side.
    pub fn refresh_collections(
        &self,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let root = PathBuf::from(Self::collections_root());
        let mut collections = Vec::new();
        if root.is_dir() {
            for entry in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.is_dir() && path.join(".apiark").join("apiark.yaml").exists() {
                    collections.push(collection::load_collection_tree(&path)?);
                }
            }
        }
        // Reload collections opened from arbitrary folders outside the default
        // root instead of reusing a stale in-memory tree.
        let existing = app_state.read(cx).collections.clone();
        for node in existing {
            let path = PathBuf::from(node.path());
            if path.is_dir()
                && !path.starts_with(&root)
                && !collections.iter().any(|n| n.path() == node.path())
                && path.join(".apiark").join("apiark.yaml").exists()
            {
                if let Ok(tree) = collection::load_collection_tree(&path) {
                    collections.push(tree);
                }
            }
        }
        collections.sort_by(|a, b| a.name().cmp(b.name()));
        app_state.update(cx, |state, cx| state.set_collections(collections, cx));
        Ok(())
    }

    /// Open the last-used / sample collection, if any.
    pub fn open_default_collection(
        &self,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let path = Self::default_collection_path();
        if path.join(".apiark").join("apiark.yaml").exists() {
            self.open_collection(&path.to_string_lossy(), app_state, cx)
        } else {
            Ok(())
        }
    }

    /// Create and open a sample collection for first-run.
    pub fn create_sample(&self, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        let path = collection::create_sample_collection()?;
        self.open_collection(&path, app_state, cx)
    }

    /// Default parent directory used by the "New Collection" dialog.
    pub fn collections_root() -> String {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".apiark")
            .join("collections")
            .to_string_lossy()
            .to_string()
    }

    /// Create a collection under `parent_dir` and open it.
    pub fn create_collection_at(
        &self,
        parent_dir: &str,
        name: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let parent = Path::new(parent_dir);
        let already_named = parent
            .file_name()
            .map(|component| component.to_string_lossy().eq_ignore_ascii_case(name.trim()))
            .unwrap_or(false);

        // If the user already typed/selected the collection folder itself,
        // initialize that folder directly instead of nesting name/name.
        let path = if already_named {
            collection::ensure_collection(parent)?
        } else {
            collection::create_collection(parent, name)?
        };
        self.open_collection(&path, app_state, cx)
    }

    /// Create a folder with a user-provided name and reload the tree.
    pub fn create_folder_named(
        &self,
        parent_path: &str,
        name: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Folder name cannot be empty".into());
        }
        collection::create_folder(Path::new(parent_path), name)?;
        self.reload_collection_for_path(parent_path, app_state, cx)
    }

    /// Create a folder with the default name.
    pub fn create_folder(
        &self,
        parent_path: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        self.create_folder_named(parent_path, "New Folder", app_state, cx)
    }

    /// Create a request with a user-provided name and reload the tree.
    pub fn create_request_named(
        &self,
        parent_path: &str,
        name: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Request name cannot be empty".into());
        }
        collection::create_request_unique(
            Path::new(parent_path),
            name,
            HttpMethod::Get,
            "https://example.com",
        )?;
        self.reload_collection_for_path(parent_path, app_state, cx)
    }

    /// Duplicate a request and reload its owning collection.
    pub fn duplicate_node(
        &self,
        path: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        collection::duplicate_request(path)?;
        self.reload_collection_for_path(path, app_state, cx)
    }

    /// Move a request/folder into another directory and keep open tabs valid.
    pub fn move_node(
        &self,
        path: &str,
        kind: CollectionNodeKind,
        target_dir: &str,
        tabs: &Entity<TabsState>,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let new_path = collection::move_node(path, kind, target_dir)?;
        tabs.update(cx, |state, cx| state.retarget_moved_paths(path, &new_path, cx));
        self.reload_collection_for_path(&new_path, app_state, cx)?;
        app_state.update(cx, |state, cx| {
            state.expand_node(format!("node-{}", target_dir), cx)
        });
        Ok(())
    }

    /// Create a request with the default name.
    pub fn create_request(
        &self,
        parent_path: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        self.create_request_named(parent_path, "New Request", app_state, cx)
    }

    /// Rename a collection/folder/request/case and reload the tree.
    pub fn rename_node(
        &self,
        path: &str,
        kind: CollectionNodeKind,
        old_name: &str,
        new_name: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        if new_name.trim().is_empty() {
            return Err("Name cannot be empty".into());
        }
        if kind == CollectionNodeKind::Collection {
            let new_path = collection::rename_collection(path, new_name)?;
            return self.open_collection(&new_path, app_state, cx);
        }
        collection::rename_node(path, kind, old_name, new_name)?;
        self.reload_collection_for_path(path, app_state, cx)
    }

    /// Delete a collection/folder/request/case and reload the tree.
    pub fn delete_node(
        &self,
        path: &str,
        kind: CollectionNodeKind,
        name: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let current_collection = app_state.read(cx).collection_path.clone();
        if kind == CollectionNodeKind::Case {
            collection::delete_node(path, kind, name)?;
        } else {
            let kind_label = match kind {
                CollectionNodeKind::Collection => "collection",
                CollectionNodeKind::Folder => "folder",
                CollectionNodeKind::Request => "request",
                CollectionNodeKind::Case => "case",
            };
            trash::move_to_trash(Path::new(path), name, kind_label)?;
        }
        self.refresh_trash(app_state, cx);
        if kind == CollectionNodeKind::Collection {
            app_state.update(cx, |state, cx| state.clear_collection(cx));
            self.refresh_collections(app_state, cx)
        } else if let Some(collection_path) = current_collection {
            self.open_collection(&collection_path, app_state, cx)
        } else {
            self.refresh_collections(app_state, cx)
        }
    }

    pub fn refresh_trash(&self, app_state: &Entity<AppState>, cx: &mut App) {
        app_state.update(cx, |state, cx| state.set_trash_items(trash::list(), cx));
    }

    pub fn restore_trash(&self, id: &str, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        trash::restore(id)?;
        self.refresh_collections(app_state, cx)?;
        self.refresh_trash(app_state, cx);
        Ok(())
    }

    pub fn empty_trash(&self, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        trash::empty()?;
        self.refresh_trash(app_state, cx);
        Ok(())
    }

    /// Reload the collection that owns `node_path`, independent of which
    /// collection is currently active.
    fn reload_collection_for_path(
        &self,
        node_path: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let root = {
            let state = app_state.read(cx);
            state
                .collections
                .iter()
                .find(|node| Path::new(node_path).starts_with(Path::new(node.path())))
                .map(|node| node.path().to_string())
                .or_else(|| state.collection_path.clone())
        };
        match root {
            Some(path) => self.open_collection(&path, app_state, cx),
            None => Err("No collection is open".into()),
        }
    }

    // ── Import / Export ────────────────────────────────────────────────────

    /// Parse an import file and write it as a new collection under ~/.apiark/collections.
    /// Returns the preview text on success.
    pub fn import_file(&self, path: &Path, app_state: &Entity<AppState>, cx: &mut App) -> Result<String, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read import file: {e}"))?;
        let format = import_export::detect_format(&content)?;
        let data: ImportData = import_export::import_content(&content, format)?;
        let target = dirs::home_dir()
            .ok_or("Could not determine home directory")?
            .join(".apiark")
            .join("collections");
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let collection_path = import_export::write_import(&data, &target)?;
        self.open_collection(&collection_path, app_state, cx)?;
        let mut preview = data.preview();
        for w in &data.warnings {
            preview.push_str(&format!("\n⚠ {w}"));
        }
        Ok(preview)
    }

    /// Export the currently open collection. Returns (filename, content).
    pub fn export_collection_by_app(
        &self,
        app: &App,
        app_state: &Entity<AppState>,
        format: &str,
    ) -> Result<(String, String), String> {
        let state = app_state.read(app);
        let path = state.collection_path.as_ref().ok_or("No collection is open")?;
        let name = state.collection.as_ref()
            .map(|n| n.name().to_string())
            .unwrap_or_else(|| "collection".to_string());
        let content = import_export::export_collection(Path::new(path), format)?;
        let ext = match format {
            "postman" => "postman_collection.json",
            "openapi" => "openapi.yaml",
            "insomnia" => "insomnia.yaml",
            "bruno" => "bruno.bru",
            _ => "yaml",
        };
        Ok((format!("{}.{}", name, ext), content))
    }

    /// Resolve variables for the currently active environment.
    pub fn resolved_variables(&self, app: &App, app_state: &Entity<AppState>) -> std::collections::HashMap<String, String> {
        let state = app_state.read(app);
        let (Some(path), Some(env)) = (&state.collection_path, &state.environment) else {
            return std::collections::HashMap::new();
        };
        environment::get_resolved_variables(Path::new(path), env).unwrap_or_default()
    }

    pub fn create_backup(&self, output: &Path) -> Result<(), String> {
        backup::create_backup(output)
    }

    pub fn restore_backup(
        &self,
        input: &Path,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        backup::restore_backup(input)?;
        self.reload_workspace_data(app_state, cx);
        Ok(())
    }

    pub fn save_collection_defaults_config(
        &self,
        defaults: crate::models::CollectionDefaults,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let path = app_state
            .read(cx)
            .collection_path
            .clone()
            .ok_or("No collection is open")?;
        collection::save_collection_defaults(Path::new(&path), defaults)?;
        self.refresh_collections(app_state, cx)
    }

    // ── Environments ───────────────────────────────────────────────────────

    fn reload_environments(&self, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        let path = app_state
            .read(cx)
            .collection_path
            .clone()
            .ok_or("No collection is open")?;
        let envs = environment::load_environments(Path::new(&path))?;
        app_state.update(cx, |state, cx| state.set_environments(envs, cx));
        Ok(())
    }

    pub fn save_environment_config(
        &self,
        original_name: Option<&str>,
        name: &str,
        scope: EnvironmentScope,
        variables_text: &str,
        secrets_text: &str,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Environment name cannot be empty".into());
        }
        let (collection_path, original_scope) = {
            let state = app_state.read(cx);
            let original_scope = original_name.and_then(|original| {
                state
                    .environments
                    .iter()
                    .find(|env| env.name == original)
                    .map(|env| env.scope)
            });
            (state.collection_path.clone(), original_scope)
        };
        let collection_path =
            collection_path.ok_or_else(|| "No collection is open".to_string())?;
        if let (Some(original), Some(original_scope)) = (original_name, original_scope) {
            if original != name || original_scope != scope {
                environment::delete_environment(
                    Path::new(&collection_path),
                    original,
                    original_scope,
                )?;
            }
        }
        let variables = environment::parse_dotenv_text(variables_text);
        let secrets: Vec<String> = environment::parse_dotenv_text(secrets_text)
            .keys()
            .cloned()
            .collect();
        environment::save_environment(
            Path::new(&collection_path),
            &EnvironmentFile {
                name: name.to_string(),
                variables,
                secrets,
                scope,
            },
        )?;
        self.reload_environments(app_state, cx)
    }

    pub fn import_environment_file(
        &self,
        path: &Path,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<String, String> {
        let (collection_path, existing) = {
            let state = app_state.read(cx);
            let active = state.environment.as_ref();
            let existing = active.and_then(|name| {
                state
                    .environments
                    .iter()
                    .find(|env| &env.name == name)
                    .cloned()
            });
            (state.collection_path.clone(), existing)
        };
        let collection_path = collection_path.ok_or("No collection is open")?;
        let variables = environment::parse_dotenv(path);
        let imported = variables.len();
        let mut env = existing.unwrap_or_else(|| EnvironmentFile {
            name: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| "Imported".to_string()),
            scope: EnvironmentScope::Shared,
            ..Default::default()
        });
        env.variables.extend(variables);
        environment::save_environment(Path::new(&collection_path), &env)?;
        self.reload_environments(app_state, cx)?;
        Ok(format!("Imported {imported} variables into {}", env.name))
    }

    pub fn delete_environment_config(
        &self,
        name: &str,
        scope: EnvironmentScope,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let collection_path = app_state
            .read(cx)
            .collection_path
            .clone()
            .ok_or("No collection is open")?;
        environment::delete_environment(Path::new(&collection_path), name, scope)?;
        self.reload_environments(app_state, cx)
    }

    // ── History ────────────────────────────────────────────────────────────

    pub fn refresh_history(&self, app_state: &Entity<AppState>, cx: &mut App) {
        let limit = app_state.read(cx).history_limit as i64;
        match self.history.list(limit, 0) {
            Ok(entries) => app_state.update(cx, |s, cx| s.refresh_history(entries, cx)),
            Err(e) => tracing::warn!("Failed to load history: {e}"),
        }
    }

    pub fn load_more_history(&self, app_state: &Entity<AppState>, cx: &mut App) {
        app_state.update(cx, |s, cx| {
            s.history_limit = s.history_limit.saturating_add(50);
            cx.notify();
        });
        self.refresh_history(app_state, cx);
    }

    pub fn search_history(&self, query: &str, app_state: &Entity<AppState>, cx: &mut App) {
        let entries = if query.trim().is_empty() {
            self.history.list(100, 0)
        } else {
            self.history.search(query, 100)
        };
        if let Ok(entries) = entries {
            app_state.update(cx, |s, cx| s.refresh_history(entries, cx));
        }
    }

    /// Most recent response for the same method and URL, restored when a
    /// request is opened again.
    pub fn latest_response(&self, method: &str, url: &str) -> Option<ResponseData> {
        self.history
            .list(200, 0)
            .ok()?
            .into_iter()
            .find(|entry| {
                entry.url == url && entry.method.eq_ignore_ascii_case(method)
            })
            .and_then(|entry| entry.response_json)
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn latest_response_body(&self) -> String {
        self.history
            .list(1, 0)
            .ok()
            .and_then(|entries| entries.into_iter().next())
            .and_then(|entry| entry.response_json)
            .and_then(|json| serde_json::from_str::<ResponseData>(&json).ok())
            .map(|response| response.body)
            .unwrap_or_default()
    }

    /// Most recent response for the same request that differs from `current`.
    pub fn previous_response_body(&self, method: &str, url: &str, current: &str) -> String {
        self.history
            .list(200, 0)
            .ok()
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.url == url && entry.method.eq_ignore_ascii_case(method)
            })
            .filter_map(|entry| {
                entry
                    .response_json
                    .and_then(|json| serde_json::from_str::<ResponseData>(&json).ok())
            })
            .map(|response| response.body)
            .find(|body| body != current)
            .unwrap_or_default()
    }

    pub fn insert_history(&self, entry: &HistoryEntry) -> Result<(), String> {
        self.history.insert(entry)?;
        Ok(())
    }

    pub fn clear_history(&self, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        self.history.clear()?;
        app_state.update(cx, |s, cx| s.refresh_history(Vec::new(), cx));
        Ok(())
    }

    pub fn delete_history(&self, id: i64, app_state: &Entity<AppState>, cx: &mut App) -> Result<(), String> {
        self.history.delete(id)?;
        self.refresh_history(app_state, cx);
        Ok(())
    }

    fn default_collection_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".apiark")
            .join("collections")
            .join("My Collection")
    }

    fn state_file() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".apiark")
            .join("state.json")
    }

    fn read_last_collection_path() -> Option<String> {
        let json = std::fs::read_to_string(Self::state_file()).ok()?;
        let value: serde_json::Value = serde_json::from_str(&json).ok()?;
        value
            .get("lastCollectionPath")
            .or_else(|| value.get("last_collection_path"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    }

    /// Every collection currently shown in the panel, including ones opened
    /// from arbitrary folders outside the default root.
    fn read_open_collection_paths() -> Vec<String> {
        let Ok(json) = std::fs::read_to_string(Self::state_file()) else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return Vec::new();
        };
        let mut paths: Vec<String> = value
            .get("openCollections")
            .and_then(|items| items.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(path) = Self::read_last_collection_path() {
            paths.push(path);
        }
        paths
    }

    fn save_last_collection_path(&self, path: &Path) {
        let file = Self::state_file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut value: serde_json::Value = std::fs::read_to_string(&file)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .filter(|value: &serde_json::Value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({}));

        let path_string = path.to_string_lossy().to_string();
        let mut paths = Self::read_open_collection_paths();
        if !paths.iter().any(|item| item == &path_string) {
            paths.push(path_string.clone());
        }
        paths.retain(|item| {
            Path::new(item)
                .join(".apiark")
                .join("apiark.yaml")
                .exists()
        });

        if let Some(object) = value.as_object_mut() {
            object.insert(
                "lastCollectionPath".into(),
                serde_json::Value::String(path_string),
            );
            object.insert(
                "openCollections".into(),
                serde_json::Value::Array(
                    paths.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }
        let _ = std::fs::write(file, serde_json::to_string_pretty(&value).unwrap_or_default());
    }

    /// Remove a collection from the persisted panel list without touching files.
    pub fn forget_collection(&self, path: &str) {
        let file = Self::state_file();
        let Ok(json) = std::fs::read_to_string(&file) else {
            return;
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return;
        };
        let Some(object) = value.as_object_mut() else {
            return;
        };
        if let Some(items) = object
            .get_mut("openCollections")
            .and_then(|items| items.as_array_mut())
        {
            items.retain(|item| item.as_str() != Some(path));
        }
        if object.get("lastCollectionPath").and_then(|v| v.as_str()) == Some(path) {
            object.remove("lastCollectionPath");
        }
        let _ = std::fs::write(file, serde_json::to_string_pretty(&value).unwrap_or_default());
    }

    /// Restore every collection that was open before the app was closed.
    pub fn restore_last_collection(
        &self,
        tabs: &Entity<TabsState>,
        app_state: &Entity<AppState>,
        cx: &mut App,
    ) -> Result<(), String> {
        let (active_from_tab, tab_paths) = {
            let state = tabs.read(cx);
            (
                state
                    .active_tab()
                    .and_then(|tab| tab.collection_path.clone()),
                state
                    .tabs()
                    .iter()
                    .filter_map(|tab| tab.collection_path.clone())
                    .collect::<Vec<_>>(),
            )
        };
        let mut paths = tab_paths;
        paths.extend(Self::read_open_collection_paths());

        let mut seen = std::collections::HashSet::new();
        let valid: Vec<String> = paths
            .into_iter()
            .filter(|path| seen.insert(path.clone()))
            .filter(|path| {
                Path::new(path)
                    .join(".apiark")
                    .join("apiark.yaml")
                    .exists()
            })
            .collect();

        for path in &valid {
            if let Ok(tree) = collection::load_collection_tree(Path::new(path)) {
                app_state.update(cx, |state, cx| state.add_collection_tree(tree, cx));
            }
        }

        match active_from_tab
            .or_else(Self::read_last_collection_path)
            .filter(|path| valid.contains(path))
        {
            Some(path) => self.open_collection(&path, app_state, cx),
            None => self.open_default_collection(app_state, cx),
        }
    }
}
