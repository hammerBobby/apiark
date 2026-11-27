//! Global state ZIP backup/restore for `~/.apiark`.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn apiark_root() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".apiark"))
        .ok_or_else(|| "Could not determine home directory".to_string())
}

/// Write the whole `.apiark` directory (except logs) into a ZIP archive.
pub fn create_backup(output: &Path) -> Result<(), String> {
    let root = apiark_root()?;
    let parent = root.parent().ok_or("Invalid backup root")?;
    let file = File::create(output).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for entry in WalkDir::new(&root).into_iter().filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.starts_with(root.join("logs")) {
            continue;
        }
        let relative = path
            .strip_prefix(parent)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(relative, options).map_err(|e| e.to_string())?;
        let mut source = File::open(path).map_err(|e| e.to_string())?;
        io::copy(&mut source, &mut zip).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Restore a previously created backup into the home directory.
pub fn restore_backup(input: &Path) -> Result<(), String> {
    let target = dirs::home_dir().ok_or("Could not determine home directory")?;
    let file = File::open(input).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let path = target.join(&name);
        if !path.starts_with(&target) {
            return Err(format!("Unsafe backup entry: {name}"));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = File::create(&path).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
    }
    Ok(())
}
