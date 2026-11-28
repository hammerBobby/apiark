//! HTTP request service — direct in-process calls, replacing `invoke("send_request")`.
//!
//! In Tauri the frontend called `invoke("send_request", ...)` which crossed an IPC
//! boundary. With GPUI everything runs in one Rust process, so the UI calls this
//! service directly (typically via `cx.spawn` to keep the UI responsive).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use futures_util::StreamExt;

use crate::models::{AuthConfig, HttpMethod, KeyValuePair, RequestBody, ResponseData};

const MAX_RESPONSE_PREVIEW: usize = 256 * 1024;

/// Convert bytes into a String without an extra allocation on the common
/// (valid UTF-8) path; only fall back to lossy conversion for invalid bytes.
fn bytes_to_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn response_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".apiark")
        .join("response-cache")
}

fn write_full_body(bytes: &[u8]) -> Result<String, String> {
    let dir = response_cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

pub fn prune_response_cache() {
    let Ok(dir) = std::fs::read_dir(response_cache_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in dir.flatten() {
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if let Ok(age) = now.duration_since(modified) {
            if age.as_secs() > 7 * 24 * 60 * 60 {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Cache clients keyed by TLS / redirect / proxy settings. Rebuilding a
/// `reqwest::Client` per request re-creates the TLS configuration and connection
/// pool every time, which is both slow and memory-churny.
fn cached_client(
    verify_ssl: bool,
    follow_redirects: bool,
    proxy_url: &str,
    timeout_ms: u64,
) -> Result<reqwest::Client, String> {
    static CLIENTS: OnceLock<Mutex<HashMap<String, reqwest::Client>>> = OnceLock::new();
    let key = format!("{verify_ssl}|{follow_redirects}|{}|{timeout_ms}", proxy_url.trim());
    let cache = CLIENTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(client) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(client);
    }

    let mut builder = reqwest::Client::builder()
        .cookie_store(true)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .timeout(std::time::Duration::from_millis(timeout_ms));
    if !verify_ssl {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }
    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    if !proxy_url.trim().is_empty() {
        let proxy = reqwest::Proxy::all(proxy_url.trim())
            .map_err(|e| format!("Invalid proxy: {e}"))?;
        builder = builder.proxy(proxy);
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, client.clone());
    }
    Ok(client)
}

/// Parameters for an HTTP request (mirrors `SendRequestParams`).
#[derive(Debug, Clone)]
pub struct SendParams {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<KeyValuePair>,
    pub params: Vec<KeyValuePair>,
    pub body: RequestBody,
    pub auth: AuthConfig,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub verify_ssl: bool,
    pub proxy_url: String,
}

/// In-process HTTP engine.
pub struct HttpService {
    client: reqwest::Client,
}

impl HttpService {
    pub fn new() -> Self {
        prune_response_cache();
        Self {
            client: reqwest::Client::builder()
                .cookie_store(true)
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Clone the underlying client so async tasks can own it (keeps futures 'static).
    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// Send a request. Builds the client from params so `verify_ssl` /
    /// `follow_redirects` (from cURL `--insecure` / `-L`) are honored.
    pub async fn send_owned(params: SendParams) -> Result<ResponseData, String> {
        let (headers, query) = apply_auth_transport(&params);
        let url = append_params(&params.url, &query)?;

        let client = cached_client(
            params.verify_ssl,
            params.follow_redirects,
            &params.proxy_url,
            params.timeout_ms,
        )?;

        let mut builder = client.request(
            reqwest::Method::from_bytes(params.method.as_str().as_bytes())
                .map_err(|e| e.to_string())?,
            &url,
        );

        // Ask for an uncompressed body by default. Some servers return a broken
        // Content-Encoding when a browser-style Accept-Encoding is sent.
        let has_accept_encoding = headers.iter().any(|h| {
            h.enabled && !h.key.is_empty() && h.key.eq_ignore_ascii_case("accept-encoding")
        });
        if !has_accept_encoding {
            builder = builder.header(reqwest::header::ACCEPT_ENCODING, "identity");
        }

        // Headers
        for h in headers.iter().filter(|h| h.enabled && !h.key.is_empty()) {
            builder = builder.header(&h.key, &h.value);
        }

        // Auth
        match &params.auth {
            AuthConfig::Bearer { token } => {
                builder = builder.bearer_auth(token);
            }
            AuthConfig::Basic { username, password } => {
                builder = builder.basic_auth(username, Some(password));
            }
            AuthConfig::ApiKey { key, value, add_to } => {
                if add_to == "header" {
                    builder = builder.header(key, value);
                }
            }
            _ => {}
        }

        // Body
        if !matches!(params.body.body_type, crate::models::BodyType::None) {
            let content = params.body.content.clone();
            builder = match params.body.body_type {
                crate::models::BodyType::Json => builder
                    .header("Content-Type", "application/json")
                    .body(content),
                crate::models::BodyType::Xml => builder
                    .header("Content-Type", "application/xml")
                    .body(content),
                crate::models::BodyType::Raw => builder.body(content),
                crate::models::BodyType::Urlencoded => {
                    let pairs: Vec<(String, String)> = params
                        .body
                        .form_data
                        .iter()
                        .filter(|p| p.enabled)
                        .map(|p| (p.key.clone(), p.value.clone()))
                        .collect();
                    builder.form(&pairs)
                }
                crate::models::BodyType::FormData => {
                    let mut form = reqwest::multipart::Form::new();
                    for field in params
                        .body
                        .form_data
                        .iter()
                        .filter(|p| p.enabled && !p.key.is_empty())
                    {
                        if field.value_type.as_deref() == Some("file") {
                            let bytes = std::fs::read(&field.value).map_err(|e| {
                                format!("Failed to read file '{}': {}", field.value, e)
                            })?;
                            let file_name = std::path::Path::new(&field.value)
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_else(|| "file".to_string());
                            form = form.part(
                                field.key.clone(),
                                reqwest::multipart::Part::bytes(bytes).file_name(file_name),
                            );
                        } else {
                            form = form.text(field.key.clone(), field.value.clone());
                        }
                    }
                    builder.multipart(form)
                }
                crate::models::BodyType::Binary => {
                    let path = params.body.binary_path.as_ref().ok_or("No binary file selected")?;
                    let bytes = std::fs::read(path).map_err(|e| {
                        format!("Failed to read binary file '{}': {}", path, e)
                    })?;
                    builder
                        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                        .body(bytes)
                }
                _ => builder.body(content),
            };
        }

        let start = Instant::now();
        let response = builder.send().await.map_err(|e| format!("{}", e))?;
        let elapsed = start.elapsed().as_millis() as u64;

        let status = response.status();
        let status_text = status.canonical_reason().unwrap_or("").to_string();

        let headers: Vec<KeyValuePair> = response
            .headers()
            .iter()
            .map(|(k, v)| KeyValuePair {
                id: uuid::Uuid::new_v4().to_string(),
                key: k.to_string(),
                value: v.to_str().unwrap_or_default().to_string(),
                enabled: true,
                value_type: None,
            })
            .collect();

        let cookies = headers
            .iter()
            .filter(|h| h.key.eq_ignore_ascii_case("set-cookie"))
            .map(|h| {
                let (name, rest) = h.value.split_once('=').unwrap_or((&h.value, ""));
                let value = rest.split(';').next().unwrap_or("").trim();
                KeyValuePair::new(name.trim(), value)
            })
            .collect();

        // Some servers terminate the chunked stream incorrectly after sending a
        // complete body. `text()` fails with "error decoding response body" in
        // that case and throws away already-received bytes. Collect chunks
        // manually and keep what we got so the response is still inspectable.
        let mut body_bytes: Vec<u8> = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => body_bytes.extend_from_slice(bytes.as_ref()),
                Err(e) => {
                    tracing::warn!("Response body stream ended with error: {e}");
                    break;
                }
            }
        }
        let size_bytes = body_bytes.len() as u64;
        let (body, truncated, full_size, full_body_path) = if body_bytes.len() > MAX_RESPONSE_PREVIEW {
            let preview = bytes_to_string(body_bytes[..MAX_RESPONSE_PREVIEW].to_vec());
            let path = write_full_body(&body_bytes).ok();
            (preview, Some(true), Some(size_bytes), path)
        } else {
            // Reuse the collected buffer when the bytes are valid UTF-8 instead
            // of allocating a second copy through from_utf8_lossy().
            (bytes_to_string(body_bytes), None, None, None)
        };

        Ok(ResponseData {
            status: status.as_u16(),
            status_text,
            headers,
            cookies,
            body,
            time_ms: elapsed,
            size_bytes,
            truncated,
            full_size,
            full_body_path,
        })
    }
}

impl Default for HttpService {
    fn default() -> Self {
        Self::new()
    }
}

fn dedupe_headers(headers: Vec<KeyValuePair>) -> Vec<KeyValuePair> {
    let mut result: Vec<KeyValuePair> = Vec::new();
    for header in headers {
        if !header.enabled || header.key.is_empty() {
            result.push(header);
            continue;
        }
        if let Some(existing) = result
            .iter_mut()
            .find(|item| item.enabled && item.key.eq_ignore_ascii_case(&header.key))
        {
            if existing.key.eq_ignore_ascii_case("cookie") {
                if existing.value.is_empty() {
                    existing.value = header.value;
                } else if !header.value.is_empty() {
                    existing.value.push_str("; ");
                    existing.value.push_str(&header.value);
                }
            } else {
                *existing = header;
            }
        } else {
            result.push(header);
        }
    }
    result
}

/// Apply auth choices that are represented as ordinary headers/query params.
/// Returns the final header and query lists used to build the request.
fn apply_auth_transport(params: &SendParams) -> (Vec<KeyValuePair>, Vec<KeyValuePair>) {
    let mut headers = params.headers.clone();
    let mut query = params.params.clone();

    if let AuthConfig::ApiKey { key, value, add_to } = &params.auth {
        match add_to.as_str() {
            "query" => query.push(KeyValuePair::new(key, value)),
            "cookie" => {
                let cookie = format!("{}={}", key, value);
                if let Some(existing) = headers.iter_mut().find(|h| {
                    h.enabled && h.key.eq_ignore_ascii_case("cookie")
                }) {
                    if !existing.value.is_empty() {
                        existing.value.push_str("; ");
                    }
                    existing.value.push_str(&cookie);
                } else {
                    headers.push(KeyValuePair::new("Cookie", cookie));
                }
            }
            _ => {}
        }
    }

    (dedupe_headers(headers), query)
}

/// Append query parameters to a URL.
fn append_params(url: &str, params: &[KeyValuePair]) -> Result<String, String> {
    let enabled: Vec<_> = params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();
    if enabled.is_empty() {
        return Ok(url.to_string());
    }

    let mut parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    // Replace, don't append: the URL typically already carries the same query
    // params (synced from the table). Appending duplicated keys makes servers
    // return unexpected results (e.g. `data: null`).
    {
        let mut query = parsed.query_pairs_mut();
        query.clear();
        for p in enabled {
            query.append_pair(&p.key, &p.value);
        }
    }
    Ok(parsed.to_string())
}

/// Helper to build a variables map (for future environment interpolation).
#[allow(dead_code)]
fn build_variables(vars: &HashMap<String, String>) -> HashMap<String, String> {
    vars.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_headers_are_deduplicated() {
        let params = SendParams {
            method: crate::models::HttpMethod::Post,
            url: "http://x/api".into(),
            headers: vec![
                KeyValuePair::new("Content-Type", "application/json"),
                KeyValuePair::new("content-type", "application/json"),
                KeyValuePair::new("Cookie", "a=1"),
                KeyValuePair::new("cookie", "b=2"),
            ],
            params: Vec::new(),
            body: Default::default(),
            auth: AuthConfig::None,
            timeout_ms: 1000,
            follow_redirects: true,
            verify_ssl: true,
            proxy_url: String::new(),
        };
        let (headers, _) = apply_auth_transport(&params);
        assert_eq!(
            headers.iter().filter(|h| h.key.eq_ignore_ascii_case("content-type")).count(),
            1
        );
        let cookies: Vec<_> = headers
            .iter()
            .filter(|h| h.key.eq_ignore_ascii_case("cookie"))
            .collect();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].value, "a=1; b=2");
    }

    #[test]
    fn api_key_can_go_to_query_or_cookie() {
        let base = SendParams {
            method: crate::models::HttpMethod::Get,
            url: "http://x/api".into(),
            headers: vec![KeyValuePair::new("X-Test", "1")],
            params: vec![KeyValuePair::new("a", "1")],
            body: Default::default(),
            auth: AuthConfig::ApiKey {
                key: "token".into(),
                value: "abc".into(),
                add_to: "query".into(),
            },
            timeout_ms: 1000,
            follow_redirects: true,
            verify_ssl: true,
            proxy_url: String::new(),
        };
        let (headers, query) = apply_auth_transport(&base);
        assert!(headers.iter().all(|h| h.key != "token"));
        assert!(query.iter().any(|p| p.key == "token" && p.value == "abc"));

        let cookie = SendParams {
            auth: AuthConfig::ApiKey {
                key: "token".into(),
                value: "abc".into(),
                add_to: "cookie".into(),
            },
            ..base
        };
        let (headers, query) = apply_auth_transport(&cookie);
        assert!(query.iter().all(|p| p.key != "token"));
        let cookie_header = headers
            .iter()
            .find(|h| h.key.eq_ignore_ascii_case("cookie"))
            .expect("cookie header");
        assert_eq!(cookie_header.value, "token=abc");
    }

    #[test]
    fn append_params_replaces_existing_query() {
        let params = vec![
            KeyValuePair::new("a", "1"),
            KeyValuePair::new("b", "2"),
        ];
        // URL already has the same query — must not duplicate keys.
        let url = append_params("http://x/api?a=1&b=2", &params).unwrap();
        assert_eq!(url.matches("a=").count(), 1);
        assert_eq!(url.matches("b=").count(), 1);

        // Table edit should replace the old values.
        let edited = vec![KeyValuePair::new("a", "999")];
        let url = append_params("http://x/api?a=1&b=2", &edited).unwrap();
        assert!(url.contains("a=999"));
        assert!(!url.contains("b=2"));
    }
}
