//! Collection import / export — ported from
//! `apps/desktop/src-tauri/src/{importer,exporter}`.
//!
//! Supported: Postman v2.1 (import+export), OpenAPI 3.x (import+export),
//! ApiArk native YAML (export). Runs fully in-process.

use crate::models::{
    AuthConfig, CollectionConfig, CollectionDefaults, HttpMethod, RequestFile,
};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Intermediate representation (mirrors importer::ImportData).
#[derive(Debug, Clone)]
pub struct ImportData {
    pub collection_name: String,
    pub items: Vec<ImportItem>,
    pub environments: Vec<(String, HashMap<String, String>)>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ImportItem {
    Folder { name: String, items: Vec<ImportItem> },
    Request {
        name: String,
        method: HttpMethod,
        url: String,
        headers: HashMap<String, String>,
        params: HashMap<String, String>,
        body_type: Option<String>,
        content: String,
        auth: AuthConfig,
    },
}

impl ImportData {
    pub fn preview(&self) -> String {
        fn counts(items: &[ImportItem]) -> (usize, usize) {
            items.iter().fold((0, 0), |(r, f), i| match i {
                ImportItem::Request { .. } => (r + 1, f),
                ImportItem::Folder { items, .. } => {
                    let (rr, ff) = counts(items);
                    (r + rr, f + 1 + ff)
                }
            })
        }
        let (requests, folders) = counts(&self.items);
        format!(
            "{}: {} requests, {} folders, {} environments",
            self.collection_name, requests, folders, self.environments.len()
        )
    }
}

/// Detect a file's import format by inspecting its contents.
pub fn detect_format(content: &str) -> Result<&'static str, String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("meta {")
        || trimmed.lines().any(|line| {
            let line = line.trim();
            ["GET ", "POST ", "PUT ", "PATCH ", "DELETE ", "HEAD ", "OPTIONS "]
                .iter()
                .any(|method| line.starts_with(method))
                && !line.starts_with('#')
        })
    {
        return Ok("bruno");
    }

    let value: Value = serde_json::from_str(content)
        .or_else(|_| serde_yaml::from_str(content).map_err(|e| e.to_string()))?;
    if value.get("openapi").and_then(|v| v.as_str()).is_some_and(|v| v.starts_with("3.")) {
        return Ok("openapi");
    }
    if value.get("swagger").is_some() {
        return Err("Swagger 2.0 is not supported; use OpenAPI 3.x".into());
    }
    if value.get("info").and_then(|i| i.get("name")).is_some()
        && value.get("item").is_some()
    {
        return Ok("postman");
    }
    if value.get("_type").and_then(|v| v.as_str()) == Some("workspace")
        || value.get("resources").and_then(|v| v.as_array()).is_some()
    {
        return Ok("insomnia");
    }
    if value.get("log").and_then(|v| v.get("entries")).and_then(|v| v.as_array()).is_some() {
        return Ok("har");
    }
    if value.get("collections").and_then(|v| v.as_array()).is_some()
        || value.get("requests").and_then(|v| v.as_array()).is_some()
    {
        return Ok("hoppscotch");
    }
    Err("Unrecognized import format".into())
}

/// Parse an import file given its detected format.
pub fn import_content(content: &str, format: &str) -> Result<ImportData, String> {
    match format {
        "postman" => parse_postman(content),
        "openapi" => parse_openapi(content),
        "insomnia" => parse_insomnia(content),
        "har" => parse_har(content),
        "hoppscotch" => parse_hoppscotch(content),
        "bruno" => parse_bruno(content),
        _ => Err(format!("Unsupported import format: {format}")),
    }
}

// ── Postman v2.1 ─────────────────────────────────────────────────────────

pub fn parse_postman(content: &str) -> Result<ImportData, String> {
    let root: Value = serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
    let info = root.get("info").ok_or("Missing 'info' field")?;
    let collection_name = info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported Collection")
        .to_string();

    let mut warnings = Vec::new();
    let items = root
        .get("item")
        .and_then(|v| v.as_array())
        .map(|arr| parse_postman_items(arr, &mut warnings))
        .unwrap_or_default();

    let mut environments = Vec::new();
    if let Some(vars) = root.get("variable").and_then(|v| v.as_array()) {
        let mut variables = HashMap::new();
        for var in vars {
            if let (Some(k), Some(v)) = (
                var.get("key").and_then(|v| v.as_str()),
                var.get("value").and_then(|v| v.as_str()),
            ) {
                variables.insert(k.to_string(), v.to_string());
            }
        }
        if !variables.is_empty() {
            environments.push(("Collection Variables".to_string(), variables));
        }
    }

    Ok(ImportData { collection_name, items, environments, warnings })
}

fn parse_postman_items(items: &[Value], warnings: &mut Vec<String>) -> Vec<ImportItem> {
    items.iter().filter_map(|i| parse_postman_item(i, warnings)).collect()
}

fn parse_postman_item(item: &Value, warnings: &mut Vec<String>) -> Option<ImportItem> {
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
    if let Some(children) = item.get("item").and_then(|v| v.as_array()) {
        return Some(ImportItem::Folder {
            name,
            items: parse_postman_items(children, warnings),
        });
    }
    let req = item.get("request")?;
    let method = HttpMethod::parse(req.get("method").and_then(|v| v.as_str()).unwrap_or("GET"));
    let (url, params) = parse_postman_url(req.get("url"));
    let headers = parse_postman_headers(req.get("header"));
    let (body_type, content) = parse_postman_body(req.get("body"));
    let auth = parse_postman_auth(req.get("auth"));

    if let Some(events) = item.get("event").and_then(|v| v.as_array()) {
        for event in events {
            let script = event
                .get("script")
                .and_then(|s| s.get("exec"))
                .and_then(|e| {
                    if let Some(arr) = e.as_array() {
                        Some(arr.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("\n"))
                    } else {
                        e.as_str().map(|s| s.to_string())
                    }
                });
            if let Some(src) = script {
                if src.contains("pm.") {
                    warnings.push(format!("{name}: script uses Postman's pm.* API — replace with ark.*"));
                }
            }
        }
    }

    Some(ImportItem::Request {
        name, method, url, headers, params, body_type, content, auth,
    })
}

fn parse_postman_url(url_val: Option<&Value>) -> (String, HashMap<String, String>) {
    let empty = HashMap::new();
    match url_val {
        None => (String::new(), empty),
        Some(v) if v.is_string() => (v.as_str().unwrap_or("").to_string(), empty),
        Some(v) => {
            let params: HashMap<String, String> = v
                .get("variable")
                .and_then(|vars| vars.as_array())
                .map(|arr| {
                    arr.iter().filter_map(|var| {
                        let k = var.get("key").and_then(|k| k.as_str())?;
                        let val = var.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        Some((k.to_string(), val))
                    }).collect()
                })
                .unwrap_or_default();
            let url = if let Some(raw) = v.get("raw").and_then(|r| r.as_str()) {
                raw.to_string()
            } else {
                let host = v.get("host").and_then(|h| h.as_array())
                    .map(|p| p.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join("."))
                    .unwrap_or_default();
                let path = v.get("path").and_then(|p| p.as_array())
                    .map(|p| p.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join("/"))
                    .unwrap_or_default();
                let protocol = v.get("protocol").and_then(|p| p.as_str()).unwrap_or("https");
                format!("{protocol}://{host}/{path}")
            };
            (url, params)
        }
    }
}

fn parse_postman_headers(val: Option<&Value>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(arr) = val.and_then(|v| v.as_array()) {
        for h in arr {
            if h.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            if let (Some(k), Some(v)) = (
                h.get("key").and_then(|v| v.as_str()),
                h.get("value").and_then(|v| v.as_str()),
            ) {
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    map
}

fn parse_postman_body(val: Option<&Value>) -> (Option<String>, String) {
    let Some(body) = val else { return (None, String::new()) };
    match body.get("mode").and_then(|v| v.as_str()).unwrap_or("") {
        "raw" => {
            let content = body.get("raw").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let lang = body.get("options").and_then(|o| o.get("raw"))
                .and_then(|r| r.get("language")).and_then(|l| l.as_str()).unwrap_or("json");
            let bt = match lang { "json" => "json", "xml" => "xml", _ => "raw" };
            (Some(bt.to_string()), content)
        }
        "urlencoded" => {
            let content = body.get("urlencoded").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|p| {
                    let k = p.get("key").and_then(|v| v.as_str())?;
                    let v = p.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    Some(format!("{k}={v}"))
                }).collect::<Vec<_>>().join("&"))
                .unwrap_or_default();
            (Some("urlencoded".into()), content)
        }
        "formdata" => {
            let content = body.get("formdata").and_then(|v| v.as_array())
                .map(|arr| {
                    let items: Vec<Value> = arr.iter().filter_map(|p| {
                        let k = p.get("key").and_then(|v| v.as_str())?;
                        let v = p.get("value").and_then(|v| v.as_str()).unwrap_or("");
                        Some(serde_json::json!({"key": k, "value": v}))
                    }).collect();
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                })
                .unwrap_or_default();
            (Some("form-data".into()), content)
        }
        _ => (None, String::new()),
    }
}

fn parse_postman_auth(val: Option<&Value>) -> AuthConfig {
    let Some(auth) = val else { return AuthConfig::None };
    match auth.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "bearer" => {
            let token = auth.get("bearer").and_then(|b| b.as_array())
                .and_then(|arr| arr.iter().find_map(|item| {
                    if item.get("key").and_then(|v| v.as_str()) == Some("token") {
                        item.get("value").and_then(|v| v.as_str()).map(|s| s.to_string())
                    } else { None }
                }))
                .unwrap_or_default();
            AuthConfig::Bearer { token }
        }
        "basic" => {
            let get = |field: &str| -> String {
                auth.get("basic").and_then(|b| b.as_array())
                    .and_then(|arr| arr.iter().find_map(|item| {
                        if item.get("key").and_then(|v| v.as_str()) == Some(field) {
                            item.get("value").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else { None }
                    }))
                    .unwrap_or_default()
            };
            AuthConfig::Basic { username: get("username"), password: get("password") }
        }
        _ => AuthConfig::None,
    }
}

// ── OpenAPI 3.x ──────────────────────────────────────────────────────────

pub fn parse_openapi(content: &str) -> Result<ImportData, String> {
    let root: Value = serde_yaml::from_str(content)
        .or_else(|_| serde_json::from_str(content).map_err(|e| e.to_string()))?;
    let version = root.get("openapi").and_then(|v| v.as_str())
        .ok_or("Missing 'openapi' field")?;
    if !version.starts_with("3.") {
        return Err(format!("Unsupported OpenAPI version: {version}"));
    }
    let name = root.get("info").and_then(|i| i.get("title")).and_then(|v| v.as_str())
        .unwrap_or("Imported API").to_string();
    let base_url = root.get("servers").and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("url")).and_then(|v| v.as_str())
        .unwrap_or("{{baseUrl}}").to_string();

    let warnings = Vec::new();
    let mut items = Vec::new();
    let methods = ["get", "post", "put", "patch", "delete", "head", "options"];

    if let Some(paths) = root.get("paths").and_then(|p| p.as_object()) {
        for (path, methods_val) in paths {
            let Some(methods_obj) = methods_val.as_object() else { continue };
            for method in methods {
                let Some(op) = methods_obj.get(method) else { continue };
                let summary = op.get("summary").or_else(|| op.get("operationId"))
                    .and_then(|v| v.as_str()).unwrap_or(path.as_str());

                let mut headers = HashMap::new();
                let mut params = HashMap::new();
                if let Some(ps) = op.get("parameters").and_then(|v| v.as_array()) {
                    for p in ps {
                        let Some(pname) = p.get("name").and_then(|v| v.as_str()) else { continue };
                        let example = p.get("example").and_then(|v| v.as_str()).unwrap_or("");
                        match p.get("in").and_then(|v| v.as_str()).unwrap_or("") {
                            "header" => { headers.insert(pname.to_string(), example.to_string()); }
                            "query" => { params.insert(pname.to_string(), example.to_string()); }
                            _ => {}
                        }
                    }
                }

                let (body_type, content) = if let Some(rb) = op.get("requestBody") {
                    let content_type = rb.get("content").and_then(|c| c.as_object())
                        .and_then(|m| m.keys().next()).map(|s| s.as_str()).unwrap_or("");
                    let example = rb.get("content").and_then(|c| c.as_object())
                        .and_then(|m| m.values().next())
                        .and_then(|v| v.get("example"))
                        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
                        .unwrap_or_default();
                    let bt = if content_type.contains("json") { Some("json".to_string()) }
                             else if content_type.contains("xml") { Some("xml".to_string()) }
                             else { Some("raw".to_string()) };
                    (bt, example)
                } else {
                    (None, String::new())
                };

                let url = format!("{}{}", base_url.trim_end_matches('/'), path);
                let folder_name = op.get("tags").and_then(|t| t.as_array())
                    .and_then(|a| a.first()).and_then(|v| v.as_str())
                    .unwrap_or("Default").to_string();

                let request = ImportItem::Request {
                    name: summary.to_string(),
                    method: HttpMethod::parse(method),
                    url,
                    headers,
                    params,
                    body_type,
                    content,
                    auth: AuthConfig::None,
                };

                // Group by first tag into folders.
                if let Some(ImportItem::Folder { items, .. }) =
                    items.iter_mut().find(|i| matches!(i, ImportItem::Folder { name, .. } if name == &folder_name))
                {
                    items.push(request);
                } else {
                    items.push(ImportItem::Folder {
                        name: folder_name,
                        items: vec![request],
                    });
                }
            }
        }
    }

    let mut environments = Vec::new();
    if base_url.contains("{{") {
        let mut variables = HashMap::new();
        variables.insert("baseUrl".to_string(), "{{baseUrl}}".to_string());
        environments.push(("OpenAPI Base URL".to_string(), variables));
    }
    Ok(ImportData { collection_name: name, items, environments, warnings })
}

// ── Write imported data to disk ──────────────────────────────────────────

/// Write an ImportData to a collection directory (mirrors importer::writer).
pub fn write_import(data: &ImportData, target_dir: &Path) -> Result<String, String> {
    fn sanitize(s: &str) -> String {
        s.chars().map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        }).collect()
    }

    let collection_dir = target_dir.join(sanitize(&data.collection_name));
    if collection_dir.exists() {
        return Err(format!("Collection already exists: {}", collection_dir.display()));
    }
    let apiark_dir = collection_dir.join(".apiark");
    fs::create_dir_all(apiark_dir.join("environments")).map_err(|e| e.to_string())?;
    fs::write(apiark_dir.join(".gitignore"), "*\n!.gitignore\n").ok();

    let config = CollectionConfig {
        name: data.collection_name.clone(),
        version: 1,
        defaults: CollectionDefaults::default(),
    };
    fs::write(
        apiark_dir.join("apiark.yaml"),
        serde_yaml::to_string(&config).map_err(|e| e.to_string())?,
    ).map_err(|e| e.to_string())?;

    fn write_items(items: &[ImportItem], dir: &Path, used: &mut HashMap<String, usize>) -> Result<(), String> {
        for item in items {
            match item {
                ImportItem::Folder { name, items } => {
                    let folder_dir = dir.join(sanitize(name));
                    fs::create_dir_all(&folder_dir).map_err(|e| e.to_string())?;
                    write_items(items, &folder_dir, used)?;
                }
                ImportItem::Request { name, method, url, headers, params, body_type, content, auth } => {
                    let base = sanitize(name);
                    let count = used.entry(base.clone()).or_insert(0);
                    *count += 1;
                    let filename = if *count == 1 { base.clone() } else { format!("{base}-{}", *count) };
                    let request = RequestFile {
                        name: name.clone(),
                        method: *method,
                        url: url.clone(),
                        headers: headers.clone(),
                        params: if params.is_empty() { None } else { Some(params.clone()) },
                        auth: Some(auth.clone()),
                        body: body_type.as_ref().map(|bt| crate::models::RequestBodyFile {
                            body_type: bt.clone(),
                            content: content.clone(),
                            form_data: Vec::new(),
                            binary_path: None,
                        }),
                        ..Default::default()
                    };
                    fs::write(
                        dir.join(format!("{filename}.yaml")),
                        serde_yaml::to_string(&request).map_err(|e| e.to_string())?,
                    ).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    let mut used = HashMap::new();
    write_items(&data.items, &collection_dir, &mut used)?;

    for (name, vars) in &data.environments {
        let env = crate::models::EnvironmentFile {
            name: name.clone(),
            variables: vars.clone(),
            ..Default::default()
        };
        fs::write(
            apiark_dir.join("environments").join(format!("{}.yaml", sanitize(name))),
            serde_yaml::to_string(&env).map_err(|e| e.to_string())?,
        ).map_err(|e| e.to_string())?;
    }

    Ok(collection_dir.to_string_lossy().to_string())
}

// ── Additional import formats ────────────────────────────────────────────

fn kv_from_value(val: Option<&Value>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(arr) = val.and_then(|v| v.as_array()) {
        for item in arr {
            if item.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let key = item
                .get("key")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str());
            let value = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(key) = key {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }
    map
}

fn split_url_query(url: &str) -> (String, HashMap<String, String>) {
    if let Ok(mut parsed) = url::Url::parse(url) {
        let params: HashMap<String, String> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        parsed.set_query(None);
        return (parsed.to_string(), params);
    }
    if let Some((base, query)) = url.split_once('?') {
        let params = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        (base.to_string(), params)
    } else {
        (url.to_string(), HashMap::new())
    }
}

fn body_type_from_mime(mime: &str) -> String {
    if mime.contains("json") {
        "json".into()
    } else if mime.contains("xml") {
        "xml".into()
    } else if mime.contains("form-urlencoded") {
        "urlencoded".into()
    } else if mime.contains("multipart/form-data") {
        "form-data".into()
    } else {
        "raw".into()
    }
}

pub fn parse_insomnia(content: &str) -> Result<ImportData, String> {
    let root: Value = serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
    let resources: Vec<Value> = if let Some(items) = root.get("resources").and_then(|v| v.as_array()) {
        items.clone()
    } else {
        vec![root.clone()]
    };
    let collection_name = resources
        .iter()
        .find(|r| r.get("_type").and_then(|v| v.as_str()) == Some("workspace"))
        .and_then(|r| r.get("name").and_then(|v| v.as_str()))
        .or_else(|| root.get("name").and_then(|v| v.as_str()))
        .unwrap_or("Imported Collection")
        .to_string();

    let mut warnings = Vec::new();
    let items = insomnia_items(&resources, None, &mut warnings);
    Ok(ImportData {
        collection_name,
        items,
        environments: Vec::new(),
        warnings,
    })
}

fn insomnia_items(
    resources: &[Value],
    parent: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<ImportItem> {
    let mut items = Vec::new();
    for resource in resources {
        let resource_parent = resource.get("parentId").and_then(|v| v.as_str()).unwrap_or("");
        if resource_parent != parent.unwrap_or("") {
            continue;
        }
        match resource.get("_type").and_then(|v| v.as_str()).unwrap_or("") {
            "request_group" => {
                let id = resource.get("_id").and_then(|v| v.as_str());
                items.push(ImportItem::Folder {
                    name: resource
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Folder")
                        .to_string(),
                    items: insomnia_items(resources, id, warnings),
                });
            }
            "request" => {
                let name = resource
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let method = HttpMethod::parse(
                    resource.get("method").and_then(|v| v.as_str()).unwrap_or("GET"),
                );
                let (url, mut params) = split_url_query(
                    resource.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                );
                params.extend(kv_from_value(resource.get("params")));
                let headers = kv_from_value(resource.get("headers"));
                let body_val = resource.get("body");
                let content = body_val
                    .and_then(|b| b.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body_type = body_val
                    .and_then(|b| b.get("mimeType"))
                    .and_then(|v| v.as_str())
                    .map(body_type_from_mime);
                let auth = match resource
                    .get("authentication")
                    .and_then(|a| a.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                {
                    "bearer" => AuthConfig::Bearer {
                        token: resource
                            .get("authentication")
                            .and_then(|a| a.get("token"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    _ => AuthConfig::None,
                };
                if body_val.is_some() && body_type.is_none() {
                    warnings.push(format!("{name}: unsupported Insomnia body type"));
                }
                items.push(ImportItem::Request {
                    name,
                    method,
                    url,
                    headers,
                    params,
                    body_type,
                    content,
                    auth,
                });
            }
            _ => {}
        }
    }
    items
}

pub fn parse_har(content: &str) -> Result<ImportData, String> {
    let root: Value = serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
    let log = root.get("log").ok_or("Missing 'log' field")?;
    let collection_name = log
        .get("creator")
        .and_then(|c| c.get("name"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("HAR Import")
        .to_string();
    let mut warnings = Vec::new();
    let mut items = Vec::new();
    if let Some(entries) = log.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let req = match entry.get("request") {
                Some(req) => req,
                None => continue,
            };
            let name = entry
                .get("comment")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    req.get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });
            let method = HttpMethod::parse(
                req.get("method").and_then(|v| v.as_str()).unwrap_or("GET"),
            );
            let (url, mut params) =
                split_url_query(req.get("url").and_then(|v| v.as_str()).unwrap_or(""));
            params.extend(kv_from_value(req.get("queryString")));
            let headers = kv_from_value(req.get("headers"));
            let (body_type, content) = req
                .get("postData")
                .map(|body| {
                    let mime = body
                        .get("mimeType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("text/plain");
                    (
                        Some(body_type_from_mime(mime)),
                        body.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    )
                })
                .unwrap_or((None, String::new()));
            if body_type.is_none() {
                warnings.push(format!("{name}: unsupported HAR body type"));
            }
            items.push(ImportItem::Request {
                name,
                method,
                url,
                headers,
                params,
                body_type,
                content,
                auth: AuthConfig::None,
            });
        }
    }
    Ok(ImportData {
        collection_name,
        items,
        environments: Vec::new(),
        warnings,
    })
}

pub fn parse_hoppscotch(content: &str) -> Result<ImportData, String> {
    let root: Value = serde_json::from_str(content).map_err(|e| format!("Invalid JSON: {e}"))?;
    let collections = root
        .get("collections")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| root.get("requests").and_then(|v| v.as_array()).cloned())
        .unwrap_or_else(|| vec![root.clone()]);
    let collection_name = root
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Hoppscotch Import")
        .to_string();
    let warnings = Vec::new();
    let items = hoppscotch_items(&collections);
    Ok(ImportData {
        collection_name,
        items,
        environments: Vec::new(),
        warnings,
    })
}

fn hoppscotch_items(items: &[Value]) -> Vec<ImportItem> {
    let mut out = Vec::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string();
        let children = item
            .get("children")
            .or_else(|| item.get("requests"))
            .or_else(|| item.get("folders"))
            .and_then(|v| v.as_array());
        if let Some(children) = children {
            out.push(ImportItem::Folder {
                name,
                items: hoppscotch_items(children),
            });
            continue;
        }
        if item.get("method").is_none() && item.get("endpoint").is_none() {
            continue;
        }
        let method = HttpMethod::parse(
            item.get("method").and_then(|v| v.as_str()).unwrap_or("GET"),
        );
        let raw_url = item
            .get("endpoint")
            .or_else(|| item.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (url, url_params) = split_url_query(raw_url);
        let mut headers = HashMap::new();
        let mut params = url_params;
        if let Some(values) = item.get("params").and_then(|v| v.as_array()) {
            for value in values {
                let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("query");
                let key = match value.get("key").and_then(|v| v.as_str()) {
                    Some(key) => key.to_string(),
                    None => continue,
                };
                let val = value.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if kind == "header" {
                    headers.insert(key, val.to_string());
                } else if kind == "query" {
                    params.insert(key, val.to_string());
                }
            }
        }
        let (body_type, content) = match item.get("body") {
            Some(Value::String(body)) => (Some("raw".to_string()), body.clone()),
            Some(body) if body.is_object() => (
                body.get("contentType")
                    .and_then(|v| v.as_str())
                    .map(body_type_from_mime),
                body.get("body")
                    .or_else(|| body.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => (None, String::new()),
        };
        out.push(ImportItem::Request {
            name,
            method,
            url,
            headers,
            params,
            body_type,
            content,
            auth: AuthConfig::None,
        });
    }
    out
}

fn bruno_block(content: &str, name: &str) -> String {
    let mut collecting = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == format!("{name} {{") {
            collecting = true;
            continue;
        }
        if collecting && trimmed == "}" {
            break;
        }
        if collecting {
            lines.push(trimmed.to_string());
        }
    }
    lines.join("\n")
}

fn bruno_kv(block: &str) -> HashMap<String, String> {
    block
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

pub fn parse_bruno(content: &str) -> Result<ImportData, String> {
    let meta = bruno_block(content, "meta");
    let collection_name = bruno_kv(&meta)
        .get("name")
        .cloned()
        .unwrap_or_else(|| "Bruno Import".to_string());
    let request_line = content
        .lines()
        .map(|line| line.trim())
        .find(|line| {
            ["GET ", "POST ", "PUT ", "PATCH ", "DELETE ", "HEAD ", "OPTIONS "]
                .iter()
                .any(|method| line.starts_with(method))
        })
        .ok_or("No Bruno request line found")?;
    let (method, raw_url) = request_line
        .split_once(' ')
        .ok_or("Invalid Bruno request line")?;
    let (url, mut params) = split_url_query(raw_url.trim());
    params.extend(bruno_kv(&bruno_block(content, "query")));
    let headers = bruno_kv(&bruno_block(content, "headers"));
    let body = bruno_block(content, "body");
    let auth_block = bruno_kv(&bruno_block(content, "auth"));
    let auth = match auth_block.get("type").map(|s| s.as_str()).unwrap_or("") {
        "bearer" => AuthConfig::Bearer {
            token: auth_block.get("token").cloned().unwrap_or_default(),
        },
        "basic" => AuthConfig::Basic {
            username: auth_block.get("username").cloned().unwrap_or_default(),
            password: auth_block.get("password").cloned().unwrap_or_default(),
        },
        _ => AuthConfig::None,
    };
    let body_type = if body.trim().is_empty() {
        None
    } else {
        Some(if body.trim_start().starts_with('{') {
            "json".to_string()
        } else {
            "raw".to_string()
        })
    };
    let items = vec![ImportItem::Request {
        name: request_line.to_string(),
        method: HttpMethod::parse(method),
        url,
        headers,
        params,
        body_type,
        content: body,
        auth,
    }];
    Ok(ImportData {
        collection_name,
        items,
        environments: Vec::new(),
        warnings: Vec::new(),
    })
}

// ── Export ───────────────────────────────────────────────────────────────

/// Export a collection directory to a given format string.
pub fn export_collection(collection_path: &Path, format: &str) -> Result<String, String> {
    let config = crate::services::collection::load_collection_config(collection_path)?;
    match format {
        "apiark" => export_apiark_yaml(&config.name),
        "postman" => export_postman(collection_path, &config.name),
        "openapi" => export_openapi(collection_path, &config.name),
        "insomnia" => export_insomnia(collection_path, &config.name),
        "bruno" => export_bruno(collection_path, &config.name),
        _ => Err(format!("Unsupported export format: {format}")),
    }
}

fn export_apiark_yaml(name: &str) -> Result<String, String> {
    Ok(format!("# ApiArk collection: {name}\n# Native YAML format\nversion: 1\n"))
}

fn export_postman(collection_path: &Path, name: &str) -> Result<String, String> {
    fn collect(dir: &Path) -> Vec<Value> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(dir) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || fname == "_folder.yaml" { continue; }
            if path.is_dir() {
                out.push(serde_json::json!({
                    "name": fname,
                    "item": collect(&path),
                }));
            } else if fname.ends_with(".yaml") || fname.ends_with(".yml") {
                if let Ok(req) = crate::services::collection::read_request(&path) {
                    out.push(serde_json::json!({
                        "name": req.name,
                        "request": {
                            "method": req.method.as_str(),
                            "url": req.url,
                            "header": req.headers.iter().map(|(k, v)| {
                                serde_json::json!({"key": k, "value": v, "type": "text"})
                            }).collect::<Vec<_>>(),
                        }
                    }));
                }
            }
        }
        out
    }
    let items = collect(collection_path);
    let doc = serde_json::json!({
        "info": { "name": name, "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
        "item": items,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn export_openapi(collection_path: &Path, name: &str) -> Result<String, String> {
    let mut paths = serde_json::Map::new();
    fn collect(dir: &Path, paths: &mut serde_json::Map<String, Value>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || fname == "_folder.yaml" { continue; }
            if path.is_dir() {
                collect(&path, paths);
            } else if fname.ends_with(".yaml") || fname.ends_with(".yml") {
                if let Ok(req) = crate::services::collection::read_request(&path) {
                    let url = url::Url::parse(&req.url).ok();
                    let path_str = url.as_ref().map(|u| u.path().to_string())
                        .unwrap_or_else(|| req.url.clone());
                    let op = serde_json::json!({
                        "summary": req.name,
                        "responses": { "200": { "description": "OK" } }
                    });
                    let method = req.method.as_str().to_lowercase();
                    paths.entry(path_str).or_insert_with(|| serde_json::json!({}))
                        .as_object_mut().unwrap().insert(method, op);
                }
            }
        }
    }
    collect(collection_path, &mut paths);
    let doc = serde_json::json!({
        "openapi": "3.0.0",
        "info": { "title": name, "version": "1.0.0" },
        "paths": paths,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn collect_requests(dir: &Path, out: &mut Vec<RequestFile>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.starts_with('.') || fname == "_folder.yaml" {
            continue;
        }
        if path.is_dir() {
            collect_requests(&path, out);
        } else if fname.ends_with(".yaml") || fname.ends_with(".yml") {
            if let Ok(req) = crate::services::collection::read_request(&path) {
                out.push(req);
            }
        }
    }
}

fn export_insomnia(collection_path: &Path, name: &str) -> Result<String, String> {
    let mut requests = Vec::new();
    collect_requests(collection_path, &mut requests);
    let resources: Vec<Value> = requests
        .into_iter()
        .enumerate()
        .map(|(index, req)| {
            let body = req.body.unwrap_or_default();
            let mime = match body.body_type.as_str() {
                "json" => "application/json",
                "xml" => "application/xml",
                _ => "text/plain",
            };
            serde_json::json!({
                "_type": "request",
                "_id": format!("req-{index}"),
                "name": req.name,
                "method": req.method.as_str(),
                "url": req.url,
                "headers": req.headers.iter().map(|(k, v)| {
                    serde_json::json!({"name": k, "value": v})
                }).collect::<Vec<_>>(),
                "body": { "mimeType": mime, "text": body.content },
            })
        })
        .collect();
    let doc = serde_json::json!({
        "_type": "workspace",
        "name": name,
        "resources": resources,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn export_bruno(collection_path: &Path, name: &str) -> Result<String, String> {
    let mut requests = Vec::new();
    collect_requests(collection_path, &mut requests);
    let mut out = format!("meta {{\n  name: {name}\n  type: http\n}}\n\n");
    for (index, req) in requests.into_iter().enumerate() {
        let body = req.body.unwrap_or_default();
        out.push_str(&format!("### request {}\n{}\n{}\n", index + 1, req.method.as_str(), req.url));
        if !req.headers.is_empty() {
            out.push_str("headers {\n");
            for (key, value) in req.headers {
                out.push_str(&format!("  {key}: {value}\n"));
            }
            out.push_str("}\n");
        }
        if !body.content.trim().is_empty() {
            out.push_str("body {\n");
            out.push_str(&body.content);
            out.push_str("\n}\n");
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_postman_collection() {
        let json = r#"{
            "info": {"name": "My API"},
            "item": [
                {"name": "Get Users", "request": {"method": "GET", "url": "https://api.test/users"}},
                {"name": "Folder", "item": [
                    {"name": "Create", "request": {"method": "POST", "url": "https://api.test/users",
                     "header": [{"key": "Content-Type", "value": "application/json"}],
                     "body": {"mode": "raw", "raw": "{\"a\":1}"}}}
                ]}
            ],
            "variable": [{"key": "baseUrl", "value": "https://api.test"}]
        }"#;
        let data = parse_postman(json).unwrap();
        assert_eq!(data.collection_name, "My API");
        assert_eq!(data.items.len(), 2);
        assert!(matches!(data.items[1], ImportItem::Folder { .. }));
        assert_eq!(data.environments.len(), 1);
    }

    #[test]
    fn imports_openapi_collection() {
        let yaml = "openapi: 3.0.0\ninfo:\n  title: Pet API\n  version: '1'\nservers:\n  - url: https://api.test\npaths:\n  /pets:\n    get:\n      summary: List pets\n";
        let data = parse_openapi(yaml).unwrap();
        assert_eq!(data.collection_name, "Pet API");
        assert_eq!(data.items.len(), 1);
    }

    #[test]
    fn detects_formats() {
        assert_eq!(detect_format(r#"{"info":{"name":"x"},"item":[]}"#).unwrap(), "postman");
        assert_eq!(detect_format("openapi: 3.0.0\ninfo: {}").unwrap(), "openapi");
        assert_eq!(detect_format(r#"{"_type":"workspace","resources":[]}"#).unwrap(), "insomnia");
        assert_eq!(detect_format(r#"{"log":{"entries":[]}}"#).unwrap(), "har");
        assert_eq!(detect_format(r#"{"collections":[]}"#).unwrap(), "hoppscotch");
        assert_eq!(detect_format("meta {\n  name: Test\n}\nGET https://x.test").unwrap(), "bruno");
    }

    #[test]
    fn imports_additional_formats() {
        let insomnia = r#"{
            "_type":"workspace","name":"Insomnia API",
            "resources":[
                {"_type":"request","_id":"r1","name":"List","method":"GET","url":"https://api.test/users?a=1","headers":[{"name":"X-Key","value":"v"}]}
            ]
        }"#;
        let data = import_content(insomnia, "insomnia").unwrap();
        assert_eq!(data.collection_name, "Insomnia API");
        assert!(matches!(data.items[0], ImportItem::Request { .. }));

        let har = r#"{"log":{"creator":{"name":"HAR API"},"entries":[
            {"request":{"method":"POST","url":"https://api.test/users","headers":[{"name":"Content-Type","value":"application/json"}],"postData":{"mimeType":"application/json","text":"{}"}}}
        ]}}"#;
        let data = import_content(har, "har").unwrap();
        assert_eq!(data.collection_name, "HAR API");
        assert!(matches!(data.items[0], ImportItem::Request { .. }));

        let hoppscotch = r#"{"name":"Hoppscotch API","collections":[
            {"name":"Users","requests":[{"name":"Get","method":"GET","endpoint":"https://api.test/users","params":[{"key":"q","value":"1","type":"query"}]}]}
        ]}"#;
        let data = import_content(hoppscotch, "hoppscotch").unwrap();
        assert_eq!(data.collection_name, "Hoppscotch API");
        assert!(matches!(data.items[0], ImportItem::Folder { .. }));

        let bruno = "meta {\n  name: Bruno API\n}\nGET https://api.test/users\nheaders {\n  Accept: application/json\n}\n";
        let data = import_content(bruno, "bruno").unwrap();
        assert_eq!(data.collection_name, "Bruno API");
        assert!(matches!(data.items[0], ImportItem::Request { .. }));
    }
}
