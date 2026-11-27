//! Recycle bin for deleted collections, folders and requests.

use std::fs;
use std::path::{Path, PathBuf};

use crate::models::TrashItem;

fn root() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".apiark").join("trash"))
        .ok_or_else(|| "Could not determine home directory".to_string())
}

fn index_file() -> Result<PathBuf, String> {
    Ok(root()?.join("index.json"))
}

fn load_index() -> Vec<TrashItem> {
    let Ok(path) = index_file() else {
        return Vec::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_index(items: &[TrashItem]) -> Result<(), String> {
    let file = index_file()?;
    fs::create_dir_all(file.parent().ok_or("Invalid trash path")?)
        .map_err(|e| e.to_string())?;
    fs::write(file, serde_json::to_string_pretty(items).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn copy_recursive(source: &Path, target: &Path) -> Result<(), String> {
    if source.is_dir() {
        fs::create_dir_all(target).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            copy_recursive(&entry.path(), &target.join(name))?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(source, target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn list() -> Vec<TrashItem> {
    load_index()
}

pub fn move_to_trash(path: &Path, name: &str, kind: &str) -> Result<TrashItem, String> {
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    let trash_root = root()?;
    fs::create_dir_all(&trash_root).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "item".to_string());
    let target = trash_root.join(format!("{id}-{file_name}"));
    if fs::rename(path, &target).is_err() {
        copy_recursive(path, &target)?;
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    let item = TrashItem {
        id: id.clone(),
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        trash_path: target.to_string_lossy().to_string(),
        kind: kind.to_string(),
        deleted_at: chrono::Utc::now().to_rfc3339(),
    };
    let mut items = load_index();
    items.push(item.clone());
    save_index(&items)?;
    Ok(item)
}

pub fn restore(id: &str) -> Result<(), String> {
    let mut items = load_index();
    let Some(index) = items.iter().position(|item| item.id == id) else {
        return Err(format!("Trash item not found: {id}"));
    };
    let item = items[index].clone();
    let source = Path::new(&item.trash_path);
    let mut target = PathBuf::from(&item.path);
    if target.exists() {
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "restored".to_string());
        target.set_file_name(format!("{name}.restored"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(source, &target).map_err(|e| e.to_string())?;
    items.remove(index);
    save_index(&items)
}

pub fn empty() -> Result<(), String> {
    let items = load_index();
    for item in items {
        let path = Path::new(&item.trash_path);
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
    save_index(&[])
}
