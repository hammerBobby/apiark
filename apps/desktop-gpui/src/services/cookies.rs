//! Cookie jar persistence — migrated from the Tauri `cookies` commands.
//!
//! The GPUI UI keeps the active jar as `Vec<KeyValuePair>` on `AppState`.
//! This module simply persists that jar to `~/.apiark/cookies.json` so cookies
//! survive restarts (the Tauri version used per-collection files; the GPUI
//! version currently uses one global jar).

use std::path::PathBuf;

use crate::models::KeyValuePair;

fn jar_file() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".apiark")
        .join("cookies.json")
}

/// Load the persisted cookie jar. Missing or corrupt files yield an empty jar.
pub fn load() -> Vec<KeyValuePair> {
    let Ok(json) = std::fs::read_to_string(jar_file()) else {
        return Vec::new();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Persist the cookie jar. Runs on a background thread so cookie merges after
/// a response never block the UI.
pub fn save(cookies: Vec<KeyValuePair>) {
    std::thread::spawn(move || {
        let path = jar_file();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&cookies) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("Failed to persist cookies: {e}");
                }
            }
            Err(e) => tracing::warn!("Failed to serialize cookies: {e}"),
        }
    });
}
