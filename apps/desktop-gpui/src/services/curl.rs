//! cURL import/export — ported from `apps/desktop/src-tauri/src/http/curl.rs`.
//! In GPUI this runs in-process; no `invoke` round-trip.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCurlRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub body_type: Option<String>,
    pub form_data: Vec<FormDataField>,
    pub auth_basic: Option<(String, String)>,
    pub verify_ssl: bool,
    pub follow_redirects: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDataField {
    pub key: String,
    pub value: String,
    #[serde(rename = "valueType")]
    pub value_type: Option<String>,
}

fn insert_header(headers: &mut HashMap<String, String>, key: &str, value: &str) {
    if let Some(existing) = headers
        .keys()
        .find(|existing| existing.eq_ignore_ascii_case(key))
        .cloned()
    {
        headers.remove(&existing);
    }
    headers.insert(key.to_string(), value.to_string());
}

/// Parse a cURL command string into structured request data.
pub fn parse_curl(input: &str) -> Result<ParsedCurlRequest, String> {
    let tokens = tokenize(input)?;

    let mut method = None;
    let mut url = None;
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut data_parts: Vec<String> = Vec::new();
    let mut form_data: Vec<FormDataField> = Vec::new();
    let mut auth_basic = None;
    let mut verify_ssl = true;
    let mut follow_redirects = false;

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        match token.as_str() {
            "curl" => {}
            "-X" | "--request" => {
                i += 1;
                method = tokens.get(i).cloned();
            }
            "--url" => {
                i += 1;
                url = tokens.get(i).cloned();
            }
            "-H" | "--header" => {
                i += 1;
                if let Some(header) = tokens.get(i) {
                    if let Some((key, value)) = header.split_once(':') {
                        insert_header(&mut headers, key.trim(), value.trim());
                    } else if let Some(key) = header.strip_suffix(';') {
                        insert_header(&mut headers, key.trim(), "");
                    } else {
                        insert_header(&mut headers, header.trim(), "");
                    }
                }
            }
            "-b" | "--cookie" | "--cookie-jar" => {
                i += 1;
                if let Some(cookie) = tokens.get(i) {
                    let existing = headers
                        .keys()
                        .find(|key| key.eq_ignore_ascii_case("cookie"))
                        .cloned();
                    let mut value = existing
                        .as_ref()
                        .and_then(|key| headers.get(key))
                        .cloned()
                        .unwrap_or_default();
                    if !value.is_empty() {
                        value.push_str("; ");
                    }
                    value.push_str(cookie);
                    insert_header(&mut headers, "Cookie", &value);
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                i += 1;
                if let Some(part) = tokens.get(i) {
                    data_parts.push(part.clone());
                }
            }
            "-F" | "--form" | "--form-string" => {
                i += 1;
                if let Some(field) = tokens.get(i) {
                    parse_form_field(field, &mut form_data);
                }
            }
            "-u" | "--user" => {
                i += 1;
                if let Some(creds) = tokens.get(i) {
                    let parts: Vec<&str> = creds.splitn(2, ':').collect();
                    auth_basic = Some((
                        parts[0].to_string(),
                        parts.get(1).unwrap_or(&"").to_string(),
                    ));
                }
            }
            "-k" | "--insecure" => verify_ssl = false,
            "-L" | "--location" => follow_redirects = true,
            "--compressed" | "-s" | "--silent" | "-S" | "--show-error" | "-v" | "--verbose" => {}
            _ => {
                if !token.starts_with('-') && url.is_none() {
                    url = Some(token.clone());
                }
            }
        }
        i += 1;
    }

    let raw_url = url.ok_or("No URL found in cURL command")?;
    let url = normalize_url(&raw_url);
    let data = if data_parts.is_empty() {
        None
    } else {
        Some(data_parts.join("&"))
    };
    let method = method.unwrap_or_else(|| {
        if data.is_some() { "POST".to_string() } else { "GET".to_string() }
    });

    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());

    let mut body_type = content_type.as_deref().map(|ct| {
        if ct.contains("application/json") {
            "json"
        } else if ct.contains("application/xml") || ct.contains("text/xml") {
            "xml"
        } else if ct.contains("application/x-www-form-urlencoded") {
            "urlencoded"
        } else if ct.contains("multipart/form-data") {
            "form-data"
        } else {
            "raw"
        }
        .to_string()
    });

    if !form_data.is_empty() {
        body_type = Some("form-data".to_string());
    } else if let (Some("urlencoded"), Some(body)) = (body_type.as_deref(), data.as_deref()) {
        parse_urlencoded(body, &mut form_data);
    }

    Ok(ParsedCurlRequest {
        method: method.to_uppercase(),
        url,
        headers,
        body: data,
        body_type,
        form_data,
        auth_basic,
        verify_ssl,
        follow_redirects,
    })
}

fn parse_form_field(field: &str, out: &mut Vec<FormDataField>) {
    let field = field.strip_prefix('=').unwrap_or(field);
    let (key, value_part) = match field.split_once('=') {
        Some((k, v)) => (k, v),
        None => (field, ""),
    };
    let key = key.trim();
    if key.is_empty() {
        return;
    }
    let value = value_part.split(';').next().unwrap_or(value_part);
    let (value, is_file) = if let Some(p) = value.strip_prefix('@') {
        (p.to_string(), true)
    } else {
        (value.to_string(), false)
    };
    out.push(FormDataField {
        key: key.to_string(),
        value,
        value_type: Some(if is_file { "file" } else { "text" }.to_string()),
    });
}

fn has_scheme(s: &str) -> bool {
    match s.find("://") {
        Some(0) => false,
        Some(idx) => {
            let scheme = &s[..idx];
            scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

fn normalize_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || has_scheme(trimmed) {
        return trimmed.to_string();
    }
    if trimmed.starts_with("//") {
        return format!("http:{trimmed}");
    }
    format!("http://{trimmed}")
}

fn parse_urlencoded(body: &str, out: &mut Vec<FormDataField>) {
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key = form_decode(key);
        if key.is_empty() {
            continue;
        }
        out.push(FormDataField {
            key,
            value: form_decode(value),
            value_type: Some("text".to_string()),
        });
    }
}

fn form_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    out.push(hi * 16 + lo);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

/// Export a request to a cURL command (Chrome DevTools style).
pub fn export_curl(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    auth_basic: Option<(&str, &str)>,
) -> String {
    let mut parts = vec![format!("curl --url '{url}'")];
    let m = method.to_uppercase();
    if m != "GET" && m != "HEAD" {
        parts.push(format!("--request {m}"));
    }
    for (key, value) in headers {
        if value.trim().is_empty() {
            parts.push(format!("-H '{key};'"));
        } else {
            parts.push(format!("-H '{key}: {value}'"));
        }
    }
    if let Some(body) = body {
        let escaped = body.replace('\'', "'\\''");
        parts.push(format!("--data-raw '{escaped}'"));
    }
    if let Some((user, pass)) = auth_basic {
        parts.push(format!("-u '{user}:{pass}'"));
    }
    parts.join(" \\\n  ")
}

/// Heuristic detection of a pasted cURL command.
pub fn looks_like_curl(input: &str) -> bool {
    let s = input.trim_start();
    if !s.starts_with("curl") {
        return false;
    }
    ["-H", "-d", "-X", "-u", "-k", "-L", "--url", "--data", "--form", "--header"]
        .iter()
        .any(|flag| s.contains(flag))
}

/// Tokenize a cURL command, handling quotes and shell line continuations.
fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;
    let mut has_token = false;

    while let Some(ch) = chars.next() {
        if escape_next {
            if ch != '\n' && ch != '\r' {
                current.push(ch);
            }
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if !in_single_quote => {
                if let Some(&next) = chars.peek() {
                    if next == '\n' {
                        chars.next();
                        continue;
                    }
                }
                escape_next = true;
                has_token = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                has_token = true;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                has_token = true;
            }
            c if c.is_whitespace() && !in_single_quote && !in_double_quote => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if in_single_quote || in_double_quote {
        return Err("Unterminated quote in cURL command".into());
    }
    if has_token {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_headers_case_insensitively() {
        let input = "curl 'https://api.example.com' -H 'Content-Type: application/json' -H 'content-type: application/json'";
        let parsed = parse_curl(input).unwrap();
        assert_eq!(parsed.headers.len(), 1);
        assert_eq!(
            parsed.headers.values().next().map(|v| v.as_str()),
            Some("application/json")
        );
    }

    #[test]
    fn parses_chrome_style_curl() {
        let input = "curl 'https://api.example.com/users?x=1' \\\n  -H 'Authorization: Bearer abc' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"name\":\"test\"}'";
        let parsed = parse_curl(input).unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "https://api.example.com/users?x=1");
        assert_eq!(parsed.headers.get("Authorization").unwrap(), "Bearer abc");
        assert_eq!(parsed.body_type.as_deref(), Some("json"));
    }

    #[test]
    fn parses_form_data_with_file() {
        let parsed = parse_curl("curl -X POST http://x -F name=test -F 'file=@/tmp/a.txt;type=text/plain'").unwrap();
        assert_eq!(parsed.body_type.as_deref(), Some("form-data"));
        assert_eq!(parsed.form_data.len(), 2);
        assert_eq!(parsed.form_data[1].value, "/tmp/a.txt");
        assert_eq!(parsed.form_data[1].value_type.as_deref(), Some("file"));
    }

    #[test]
    fn infers_url_and_auth() {
        let parsed = parse_curl("curl -u user:pass example.com/path").unwrap();
        assert_eq!(parsed.url, "http://example.com/path");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.auth_basic, Some(("user".into(), "pass".into())));
    }

    #[test]
    fn parses_urlencoded_body_into_fields() {
        let parsed = parse_curl("curl -H 'Content-Type: application/x-www-form-urlencoded' -d 'a=1&b=hello+world' http://x").unwrap();
        assert_eq!(parsed.body_type.as_deref(), Some("urlencoded"));
        assert_eq!(parsed.form_data.len(), 2);
        assert_eq!(parsed.form_data[1].value, "hello world");
    }

    #[test]
    fn detects_curl_paste() {
        assert!(looks_like_curl("curl -H 'X: 1' https://x"));
        assert!(!looks_like_curl("https://x"));
    }
}
