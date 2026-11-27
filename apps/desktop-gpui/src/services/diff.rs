//! Saved response snapshots used by the Compare dialog.

use crate::models::DiffSnapshot;

fn file_path() -> Result<std::path::PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".apiark").join("diff-snapshots.json"))
        .ok_or_else(|| "Could not determine home directory".to_string())
}

pub fn list() -> Vec<DiffSnapshot> {
    let Ok(path) = file_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn write(items: &[DiffSnapshot]) -> Result<(), String> {
    let path = file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(items).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn save(snapshot: DiffSnapshot) -> Result<Vec<DiffSnapshot>, String> {
    let mut items = list();
    items.push(snapshot);
    if items.len() > 20 {
        let excess = items.len() - 20;
        items.drain(0..excess);
    }
    write(&items)?;
    Ok(items)
}

pub fn clear() -> Result<(), String> {
    write(&[])
}
