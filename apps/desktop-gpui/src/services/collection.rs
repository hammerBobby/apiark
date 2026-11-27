//! Collection storage — filesystem + YAML, ported from
//! `apps/desktop/src-tauri/src/storage/collection.rs`.
//!
//! In the Tauri version these ran behind `tauri::command` + `invoke` IPC.
//! GPUI runs in-process, so the exact same logic is exposed as plain functions.

use crate::models::{
    CollectionConfig, CollectionDefaults, CollectionNode, CollectionNodeKind, FolderConfig,
    RequestFile,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_DIR: &str = ".apiark";
const CONFIG_FILE: &str = "apiark.yaml";
const FOLDER_FILE: &str = "_folder.yaml";

/// Load a collection tree from a directory containing `.apiark/apiark.yaml`.
pub fn load_collection_tree(collection_path: &Path) -> Result<CollectionNode, String> {
    let config = load_collection_config(collection_path)?;
    let children = scan_directory(collection_path)?;
    Ok(CollectionNode::Collection {
        name: config.name,
        path: collection_path.to_string_lossy().to_string(),
        children,
    })
}

/// Scan a directory for request YAML files and subfolders (folders first, then requests).
fn scan_directory(dir: &Path) -> Result<Vec<CollectionNode>, String> {
    let mut folders = Vec::new();
    let mut requests = Vec::new();

    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {e}", dir.display()))?;

    let folder_config = load_folder_config(dir);
    let order_map: HashMap<String, usize> = folder_config
        .as_ref()
        .map(|fc| {
            fc.order
                .iter()
                .enumerate()
                .map(|(i, name)| (name.clone(), i))
                .collect()
        })
        .unwrap_or_default();

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        if file_name.starts_with('.') || file_name == FOLDER_FILE {
            continue;
        }

        if path.is_dir() {
            let children = scan_directory(&path)?;
            folders.push((
                order_map.get(&file_name).copied().unwrap_or(usize::MAX),
                CollectionNode::Folder {
                    name: file_name,
                    path: path.to_string_lossy().to_string(),
                    children,
                },
            ));
        } else if file_name.ends_with(".yaml") || file_name.ends_with(".yml") {
            match read_request_meta(&path) {
                Ok(meta) => {
                    let stem = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let is_gql = meta.protocol.as_deref() == Some("graphql");
                    let file_path = path.to_string_lossy().to_string();
                    let cases = meta
                        .cases
                        .iter()
                        .map(|c| CollectionNode::Case {
                            name: c.name.clone(),
                            method: c.method,
                            path: file_path.clone(),
                            protocol: c.protocol.clone().or_else(|| meta.protocol.clone()),
                        })
                        .collect();
                    requests.push((
                        order_map.get(&stem).copied().unwrap_or(usize::MAX),
                        CollectionNode::Request {
                            name: meta.name,
                            is_graphql: is_gql,
                            method: meta.method,
                            path: file_path,
                            protocol: meta.protocol,
                            cases,
                        },
                    ));
                }
                Err(e) => {
                    tracing::warn!("Skipping invalid request file {}: {e}", path.display());
                }
            }
        }
    }

    folders.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name().cmp(b.1.name())));
    requests.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name().cmp(b.1.name())));

    let mut result: Vec<CollectionNode> = folders.into_iter().map(|(_, n)| n).collect();
    result.extend(requests.into_iter().map(|(_, n)| n));
    Ok(result)
}

/// Read just the metadata fields needed for the tree view.
fn read_request_meta(path: &Path) -> Result<RequestFile, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if content.contains("<<<<<<<") || content.contains(">>>>>>>") {
        return Err("merge conflict".into());
    }
    serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid request YAML {}: {e}", path.display()))
}

/// Read a full request file from disk.
pub fn read_request(path: &Path) -> Result<RequestFile, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if content.contains("<<<<<<<") || content.contains(">>>>>>>") {
        return Err(format!("MERGE_CONFLICT:{}", path.display()));
    }
    serde_yaml::from_str(&content).map_err(|e| format!("Invalid request YAML: {e}"))
}

/// Write a request file, preserving existing `cases` if the caller omitted them.
pub fn write_request(path: &Path, request: &RequestFile) -> Result<(), String> {
    let cases = if request.cases.is_empty() {
        read_request(path).map(|r| r.cases).unwrap_or_default()
    } else {
        request.cases.clone()
    };
    let mut to_write = request.clone();
    to_write.cases = cases;
    let yaml = serde_yaml::to_string(&to_write).map_err(|e| e.to_string())?;
    atomic_write(path, &yaml)
}

/// Upsert a saved case into its parent request YAML. The parent request itself
/// (method/url/headers/body) is left untouched.
pub fn save_request_case(
    path: &Path,
    case_name: &str,
    case: &RequestFile,
) -> Result<RequestFile, String> {
    let mut parent = read_request(path)?;
    let mut stored = case.clone();
    stored.name = case_name.to_string();
    stored.cases.clear();
    parent.cases.retain(|existing| existing.name != case_name);
    parent.cases.push(stored);
    write_request(path, &parent)?;
    Ok(parent)
}

/// Migrate older collection configs to the current schema version.
pub fn migrate_collection(collection_path: &Path) -> Result<(), String> {
    let config_path = collection_path.join(CONFIG_DIR).join(CONFIG_FILE);
    if !config_path.exists() {
        return Ok(());
    }
    let mut config = load_collection_config(collection_path)?;
    if config.version < 2 {
        config.version = 2;
        let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
        atomic_write(&config_path, &yaml)?;
    }
    Ok(())
}

/// Load the collection config from `.apiark/apiark.yaml`.
pub fn load_collection_config(collection_path: &Path) -> Result<CollectionConfig, String> {
    let config_path = collection_path.join(CONFIG_DIR).join(CONFIG_FILE);
    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read collection config: {e}"))?;
    serde_yaml::from_str(&content).map_err(|e| format!("Invalid collection config YAML: {e}"))
}

pub fn load_collection_defaults(collection_path: &Path) -> Result<CollectionDefaults, String> {
    Ok(load_collection_config(collection_path)?.defaults)
}

/// Load folder config (`_folder.yaml`), if present.
pub fn load_folder_config(dir: &Path) -> Option<FolderConfig> {
    let path = dir.join(FOLDER_FILE);
    if !path.exists() {
        return None;
    }
    fs::read_to_string(&path).ok()?;
    serde_yaml::from_str(&fs::read_to_string(&path).ok()?).ok()
}

/// Save folder ordering to `_folder.yaml`.
pub fn save_folder_order(dir: &Path, order: &[String]) -> Result<(), String> {
    let mut config = load_folder_config(dir).unwrap_or_default();
    config.order = order.to_vec();
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&dir.join(FOLDER_FILE), &yaml)
}

/// Create a new request YAML file. Returns the created path.
pub fn create_request(
    dir: &Path,
    filename: &str,
    name: &str,
    method: crate::models::HttpMethod,
    url: &str,
) -> Result<String, String> {
    let safe = sanitize_filename(filename);
    let path = dir.join(format!("{}.yaml", safe));
    if path.exists() {
        return Err(format!("File already exists: {}", path.display()));
    }
    let request = RequestFile {
        name: name.to_string(),
        method,
        url: url.to_string(),
        ..Default::default()
    };
    let yaml = serde_yaml::to_string(&request).map_err(|e| e.to_string())?;
    atomic_write(&path, &yaml)?;
    Ok(path.to_string_lossy().to_string())
}

/// Create a request whose filename never collides with existing entries.
pub fn create_request_unique(
    dir: &Path,
    name: &str,
    method: crate::models::HttpMethod,
    url: &str,
) -> Result<String, String> {
    let stem = unique_stem(dir, &sanitize_filename(name), "yaml", None);
    create_request(dir, &stem, name, method, url)
}

/// Move a request or folder into another collection/folder directory.
/// Returns the final path (a name collision is resolved with a numeric suffix).
pub fn move_node(
    path: &str,
    kind: CollectionNodeKind,
    target_dir: &str,
) -> Result<String, String> {
    if !matches!(kind, CollectionNodeKind::Request | CollectionNodeKind::Folder) {
        return Err("Only requests and folders can be moved".into());
    }
    let source = PathBuf::from(path);
    let target = PathBuf::from(target_dir);
    if !target.is_dir() {
        return Err(format!(
            "Destination folder does not exist: {}",
            target.display()
        ));
    }
    if kind == CollectionNodeKind::Folder && (target == source || target.starts_with(&source)) {
        return Err("Cannot move a folder into itself".into());
    }
    let source_parent = source
        .parent()
        .ok_or("Node has no parent directory")?
        .to_path_buf();
    if target == source_parent {
        return Ok(path.to_string());
    }

    let (old_name, new_path) = match kind {
        CollectionNodeKind::Request => {
            let stem = source
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .ok_or("Request has no filename")?;
            let new_stem = unique_stem(&target, &sanitize_filename(&stem), "yaml", Some(&source));
            (stem, target.join(format!("{}.yaml", new_stem)))
        }
        _ => {
            let name = source
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .ok_or("Folder has no name")?;
            let new_dir = unique_dir_path(&target, &sanitize_filename(&name), Some(&source));
            (name, new_dir)
        }
    };

    if new_path != source {
        fs::rename(&source, &new_path).map_err(|e| format!("Failed to move: {e}"))?;
    }
    let new_name = match kind {
        CollectionNodeKind::Request => new_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        _ => new_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
    };
    remove_parent_order(&source_parent, &old_name)?;
    append_parent_order(&target, &new_name)?;
    Ok(new_path.to_string_lossy().to_string())
}

/// Duplicate a request file next to the original, with a unique filename.
pub fn duplicate_request(path: &str) -> Result<String, String> {
    let source = PathBuf::from(path);
    let parent = source.parent().ok_or("Request has no parent directory")?;
    let mut request = read_request(&source)?;
    let base_name = if request.name.trim().is_empty() {
        "Untitled Request".to_string()
    } else {
        request.name.clone()
    };
    request.name = format!("{} Copy", base_name);
    let stem = unique_stem(parent, &sanitize_filename(&request.name), "yaml", None);
    let target = parent.join(format!("{}.yaml", stem));
    write_request(&target, &request)?;
    append_parent_order(parent, &stem)?;
    Ok(target.to_string_lossy().to_string())
}

/// Create a folder (with `_folder.yaml`) inside a collection or another folder.
pub fn create_folder(parent_dir: &Path, name: &str) -> Result<String, String> {
    let path = unique_dir_path(parent_dir, &sanitize_filename(name), None);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    update_folder_name(&path, name)?;
    Ok(path.to_string_lossy().to_string())
}

/// Update cookie-related collection defaults in `.apiark/apiark.yaml`.
pub fn save_collection_defaults(
    collection_path: &Path,
    defaults: crate::models::CollectionDefaults,
) -> Result<(), String> {
    let mut config = load_collection_config(collection_path)?;
    config.defaults = defaults;
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&collection_path.join(CONFIG_DIR).join(CONFIG_FILE), &yaml)
}

/// Rename a collection directory and keep `.apiark/apiark.yaml` in sync.
pub fn rename_collection(path: &str, new_name: &str) -> Result<String, String> {
    let old = PathBuf::from(path);
    let parent = old.parent().ok_or("Collection has no parent directory")?;
    let new = unique_dir_path(parent, &sanitize_filename(new_name), Some(&old));
    if new != old {
        fs::rename(&old, &new).map_err(|e| e.to_string())?;
    }
    let mut config = load_collection_config(&new)?;
    config.name = new_name.to_string();
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&new.join(CONFIG_DIR).join(CONFIG_FILE), &yaml)?;
    Ok(new.to_string_lossy().to_string())
}

/// Rename any collection tree node. Returns the (possibly new) path.
pub fn rename_node(
    path: &str,
    kind: CollectionNodeKind,
    old_name: &str,
    new_name: &str,
) -> Result<String, String> {
    match kind {
        CollectionNodeKind::Collection => rename_collection(path, new_name),
        CollectionNodeKind::Folder => {
            let old = PathBuf::from(path);
            let parent = old.parent().ok_or("Folder has no parent directory")?;
            let old_file_name = old
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let new = unique_dir_path(parent, &sanitize_filename(new_name), Some(&old));
            let new_file_name = new
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if new != old {
                fs::rename(&old, &new).map_err(|e| e.to_string())?;
            }
            update_folder_name(&new, new_name)?;
            replace_parent_order(parent, &old_file_name, &new_file_name)?;
            Ok(new.to_string_lossy().to_string())
        }
        CollectionNodeKind::Request => {
            let old = PathBuf::from(path);
            let parent = old.parent().ok_or("Request has no parent directory")?;
            let old_stem = old
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = unique_stem(parent, &sanitize_filename(new_name), "yaml", Some(&old));
            let new = parent.join(format!("{}.yaml", stem));
            if new != old {
                fs::rename(&old, &new).map_err(|e| e.to_string())?;
            }
            let mut request = read_request(&new)?;
            request.name = new_name.to_string();
            write_request(&new, &request)?;
            replace_parent_order(parent, &old_stem, &stem)?;
            Ok(new.to_string_lossy().to_string())
        }
        CollectionNodeKind::Case => {
            let request_path = PathBuf::from(path);
            let mut request = read_request(&request_path)?;
            let Some(case) = request.cases.iter_mut()
                .find(|c| c.name.as_str() == old_name)
            else {
                return Err(format!("Case not found: {old_name}"));
            };
            case.name = new_name.to_string();
            write_request(&request_path, &request)?;
            Ok(path.to_string())
        }
    }
}

/// Delete a collection tree node. Cases are removed from their parent request
/// file; everything else is removed from disk.
pub fn delete_node(path: &str, kind: CollectionNodeKind, name: &str) -> Result<(), String> {
    let node_path = PathBuf::from(path);
    if kind == CollectionNodeKind::Case {
        let mut request = read_request(&node_path)?;
        let before = request.cases.len();
        request.cases.retain(|c| c.name.as_str() != name);
        if request.cases.len() == before {
            return Err(format!("Case not found: {name}"));
        }
        return write_request(&node_path, &request);
    }

    delete_item(&node_path)?;

    if let Some(parent) = node_path.parent() {
        match kind {
            CollectionNodeKind::Folder => {
                if let Some(file_name) = node_path.file_name() {
                    remove_parent_order(parent, &file_name.to_string_lossy())?;
                }
            }
            CollectionNodeKind::Request => {
                if let Some(stem) = node_path.file_stem() {
                    remove_parent_order(parent, &stem.to_string_lossy())?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn unique_dir_path(parent: &Path, base: &str, exclude: Option<&Path>) -> PathBuf {
    let base = if base.trim().is_empty() { "Untitled" } else { base };
    let mut candidate = base.to_string();
    let mut index = 1;
    loop {
        let path = parent.join(&candidate);
        if exclude.map(|p| p == path.as_path()).unwrap_or(false) || !path.exists() {
            return path;
        }
        candidate = format!("{}-{}", base, index);
        index += 1;
    }
}

pub fn unique_stem(parent: &Path, base: &str, ext: &str, exclude: Option<&Path>) -> String {
    let base = if base.trim().is_empty() { "Untitled" } else { base };
    let mut candidate = base.to_string();
    let mut index = 1;
    loop {
        let path = parent.join(format!("{}.{}", candidate, ext));
        if exclude.map(|p| p == path.as_path()).unwrap_or(false) || !path.exists() {
            return candidate;
        }
        candidate = format!("{}-{}", base, index);
        index += 1;
    }
}

fn update_folder_name(path: &Path, name: &str) -> Result<(), String> {
    let mut config = load_folder_config(path).unwrap_or_default();
    config.name = Some(name.to_string());
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&path.join(FOLDER_FILE), &yaml)
}

fn replace_parent_order(parent: &Path, old: &str, new: &str) -> Result<(), String> {
    let config_path = parent.join(FOLDER_FILE);
    if !config_path.exists() {
        return Ok(());
    }
    let mut config = load_folder_config(parent).unwrap_or_default();
    let Some(position) = config.order.iter().position(|value| value == old) else {
        return Ok(());
    };
    config.order[position] = new.to_string();
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&config_path, &yaml)
}

pub fn append_parent_order(parent: &Path, name: &str) -> Result<(), String> {
    let config_path = parent.join(FOLDER_FILE);
    if !config_path.exists() {
        return Ok(());
    }
    let mut config = load_folder_config(parent).unwrap_or_default();
    if !config.order.iter().any(|value| value == name) {
        config.order.push(name.to_string());
        let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
        atomic_write(&config_path, &yaml)?;
    }
    Ok(())
}

fn remove_parent_order(parent: &Path, old: &str) -> Result<(), String> {
    let config_path = parent.join(FOLDER_FILE);
    if !config_path.exists() {
        return Ok(());
    }
    let mut config = load_folder_config(parent).unwrap_or_default();
    let before = config.order.len();
    config.order.retain(|value| value != old);
    if config.order.len() == before {
        return Ok(());
    }
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&config_path, &yaml)
}

/// Create a new collection directory with `.apiark/apiark.yaml`.
pub fn create_collection(parent_dir: &Path, name: &str) -> Result<String, String> {
    let safe = sanitize_filename(name);
    let path = parent_dir.join(&safe);
    if path.exists() {
        return Err(format!("Collection already exists: {}", path.display()));
    }
    fs::create_dir_all(path.join(CONFIG_DIR)).map_err(|e| e.to_string())?;
    let config = CollectionConfig {
        name: name.to_string(),
        version: 1,
        defaults: CollectionDefaults::default(),
    };
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    atomic_write(&path.join(CONFIG_DIR).join(CONFIG_FILE), &yaml)?;
    Ok(path.to_string_lossy().to_string())
}

/// Open an existing folder as a collection, initializing `.apiark/apiark.yaml`
/// when the folder does not have one yet.
pub fn ensure_collection(path: &Path) -> Result<String, String> {
    if !path.is_dir() {
        return Err(format!("Not a folder: {}", path.display()));
    }
    let config_path = path.join(CONFIG_DIR).join(CONFIG_FILE);
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Collection".to_string());
        let config = CollectionConfig {
            name,
            version: 1,
            defaults: CollectionDefaults::default(),
        };
        let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
        atomic_write(&config_path, &yaml)?;
    }
    Ok(path.to_string_lossy().to_string())
}

/// Create a sample collection (useful for first-run onboarding).
pub fn create_sample_collection() -> Result<String, String> {
    let base = dirs::home_dir()
        .ok_or("Could not determine home directory")?
        .join(".apiark")
        .join("collections");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = create_collection(&base, "My Collection")?;
    let dir = PathBuf::from(&path);
    create_request(
        &dir,
        "example-get",
        "Example GET",
        crate::models::HttpMethod::Get,
        "https://httpbin.org/get",
    )?;
    Ok(path)
}

/// Delete a file or directory (collection, folder or request).
pub fn delete_item(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// Atomic file write (temp file + rename) to avoid partial writes.
fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_old_collection_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = create_collection(tmp.path(), "Legacy").unwrap();
        let config_path = Path::new(&path).join(CONFIG_DIR).join(CONFIG_FILE);
        let yaml = fs::read_to_string(&config_path).unwrap();
        let mut config: CollectionConfig = serde_yaml::from_str(&yaml).unwrap();
        config.version = 1;
        fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
        migrate_collection(Path::new(&path)).unwrap();
        assert_eq!(load_collection_config(Path::new(&path)).unwrap().version, 2);
    }

    #[test]
    fn creates_and_loads_collection() {
        let tmp = tempfile::tempdir().unwrap();
        let path = create_collection(tmp.path(), "Test").unwrap();
        let tree = load_collection_tree(Path::new(&path)).unwrap();
        assert_eq!(tree.name(), "Test");
        assert!(tree.children().is_empty());

        create_request(
            Path::new(&path),
            "sample",
            "Sample",
            crate::models::HttpMethod::Get,
            "https://example.com",
        )
        .unwrap();
        let tree = load_collection_tree(Path::new(&path)).unwrap();
        assert_eq!(tree.children().len(), 1);
        assert_eq!(tree.children()[0].name(), "Sample");
    }

    #[test]
    fn renames_folders_and_requests() {
        let tmp = tempfile::tempdir().unwrap();
        let collection = create_collection(tmp.path(), "Test").unwrap();

        let folder = create_folder(Path::new(&collection), "Old Folder").unwrap();
        let renamed_folder = rename_node(
            &folder,
            CollectionNodeKind::Folder,
            "Old Folder",
            "New Folder",
        )
        .unwrap();
        let tree = load_collection_tree(Path::new(&collection)).unwrap();
        assert!(tree.children().iter().any(|n| n.name() == "New Folder"));

        let request = create_request_unique(
            Path::new(&renamed_folder),
            "Old Request",
            crate::models::HttpMethod::Get,
            "https://example.com",
        )
        .unwrap();
        let renamed_request = rename_node(
            &request,
            CollectionNodeKind::Request,
            "Old Request",
            "Renamed Request",
        )
        .unwrap();
        let file = read_request(Path::new(&renamed_request)).unwrap();
        assert_eq!(file.name, "Renamed Request");
    }

    #[test]
    fn moves_request_between_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let collection = create_collection(tmp.path(), "Test").unwrap();
        let folder_a = create_folder(Path::new(&collection), "A").unwrap();
        let folder_b = create_folder(Path::new(&collection), "B").unwrap();
        let request = create_request(
            Path::new(&folder_a),
            "sample",
            "Sample",
            crate::models::HttpMethod::Get,
            "https://example.com",
        )
        .unwrap();

        let moved = move_node(&request, CollectionNodeKind::Request, &folder_b).unwrap();
        assert!(Path::new(&moved).exists());
        assert!(!Path::new(&request).exists());
        assert!(Path::new(&moved).starts_with(&folder_b));
        assert_eq!(read_request(Path::new(&moved)).unwrap().name, "Sample");
        assert!(move_node(&folder_b, CollectionNodeKind::Folder, &folder_b).is_err());
    }

    #[test]
    fn saving_case_upserts_without_touching_parent_request() {
        let tmp = tempfile::tempdir().unwrap();
        let path = create_request(
            tmp.path(),
            "sample",
            "Sample",
            crate::models::HttpMethod::Get,
            "https://example.com",
        )
        .unwrap();
        let mut case = RequestFile {
            name: "ignored".into(),
            url: "https://case-1".into(),
            ..Default::default()
        };
        save_request_case(Path::new(&path), "成功", &case).unwrap();
        case.url = "https://case-2".into();
        let parent = save_request_case(Path::new(&path), "成功", &case).unwrap();
        assert_eq!(parent.url, "https://example.com");
        assert_eq!(parent.cases.len(), 1);
        assert_eq!(parent.cases[0].name, "成功");
        assert_eq!(parent.cases[0].url, "https://case-2");
    }
}
