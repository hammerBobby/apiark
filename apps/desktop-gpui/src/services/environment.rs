//! Environment storage — filesystem + YAML, ported from
//! `apps/desktop/src-tauri/src/storage/environment.rs`.

use crate::models::{EnvironmentFile, EnvironmentScope};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Load all environments from both shared and personal directories.
pub fn load_environments(collection_path: &Path) -> Result<Vec<EnvironmentFile>, String> {
    let mut envs = Vec::new();

    load_envs_from_dir(
        &collection_path.join(".apiark").join("environments"),
        EnvironmentScope::Shared,
        &mut envs,
    )?;
    load_envs_from_dir(
        &collection_path.join(".apiark").join("environments.local"),
        EnvironmentScope::Personal,
        &mut envs,
    )?;

    envs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(envs)
}

fn load_envs_from_dir(
    dir: &Path,
    scope: EnvironmentScope,
    envs: &mut Vec<EnvironmentFile>,
) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)
        .map_err(|e| format!("Failed to read environments dir: {e}"))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            let mut env: EnvironmentFile = serde_yaml::from_str(&content)
                .map_err(|e| format!("Invalid environment YAML {}: {e}", path.display()))?;
            env.scope = scope;
            envs.push(env);
        }
    }
    Ok(())
}

/// Parse a `.env` file into a map (comments/blank lines ignored, quotes stripped).
pub fn parse_dotenv(path: &Path) -> HashMap<String, String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    parse_dotenv_text(&content)
}

/// Parse `KEY=value` lines from an in-memory text buffer.
pub fn parse_dotenv_text(content: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value.to_string()
            };
            vars.insert(key.trim().to_string(), value);
        }
    }
    vars
}

pub fn load_root_dotenv(collection_path: &Path) -> HashMap<String, String> {
    parse_dotenv(&collection_path.join(".env"))
}

pub fn load_dotenv_secrets(collection_path: &Path) -> HashMap<String, String> {
    parse_dotenv(&collection_path.join(".apiark").join(".env"))
}

/// Resolve variables: root `.env` < environment YAML < `.apiark/.env` secrets.
pub fn get_resolved_variables(
    collection_path: &Path,
    environment_name: &str,
) -> Result<HashMap<String, String>, String> {
    let envs = load_environments(collection_path)?;
    let env = envs
        .iter()
        .find(|e| e.name == environment_name)
        .ok_or_else(|| format!("Environment '{}' not found", environment_name))?;

    let mut variables = load_root_dotenv(collection_path);
    variables.extend(env.variables.clone());

    let secrets = load_dotenv_secrets(collection_path);
    for secret_key in &env.secrets {
        if let Some(value) = secrets.get(secret_key) {
            variables.insert(secret_key.clone(), value.clone());
        }
    }
    Ok(variables)
}

/// Save an environment file, honoring shared vs personal scope.
pub fn save_environment(collection_path: &Path, env: &EnvironmentFile) -> Result<(), String> {
    let subdir = match env.scope {
        EnvironmentScope::Personal => "environments.local",
        EnvironmentScope::Shared => "environments",
    };
    let env_dir = collection_path.join(".apiark").join(subdir);
    fs::create_dir_all(&env_dir).map_err(|e| format!("Failed to create environments dir: {e}"))?;

    if matches!(env.scope, EnvironmentScope::Personal) {
        let gitignore = env_dir.join(".gitignore");
        if !gitignore.exists() {
            let _ = fs::write(&gitignore, "*\n!.gitignore\n");
        }
    }

    let filename = env.name.to_lowercase().replace(' ', "-");
    let file_path = env_dir.join(format!("{filename}.yaml"));
    let yaml = serde_yaml::to_string(env).map_err(|e| e.to_string())?;

    let tmp = file_path.with_extension("apiark.tmp");
    fs::write(&tmp, &yaml).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &file_path)
        .map_err(|e| format!("Failed to write environment file: {e}"))
}

/// Delete an environment file for the given scope.
pub fn delete_environment(
    collection_path: &Path,
    name: &str,
    scope: EnvironmentScope,
) -> Result<(), String> {
    let subdir = match scope {
        EnvironmentScope::Personal => "environments.local",
        EnvironmentScope::Shared => "environments",
    };
    let file_path = collection_path
        .join(".apiark")
        .join(subdir)
        .join(format!("{}.yaml", name.to_lowercase().replace(' ', "-")));
    if file_path.exists() {
        fs::remove_file(&file_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_variable_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(".env"), "HOST=root\nSHARED=from-dotenv\n").unwrap();
        let env_dir = root.join(".apiark").join("environments");
        fs::create_dir_all(&env_dir).unwrap();
        fs::write(
            env_dir.join("dev.yaml"),
            "name: dev\nvariables:\n  HOST: yaml\nsecrets: [TOKEN]\nscope: shared\n",
        )
        .unwrap();
        fs::write(root.join(".apiark").join(".env"), "TOKEN=secret-value\n").unwrap();

        let vars = get_resolved_variables(root, "dev").unwrap();
        assert_eq!(vars.get("HOST").unwrap(), "yaml");
        assert_eq!(vars.get("SHARED").unwrap(), "from-dotenv");
        assert_eq!(vars.get("TOKEN").unwrap(), "secret-value");
    }
}
