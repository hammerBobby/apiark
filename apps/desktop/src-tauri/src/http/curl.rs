use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

/// A single multipart/form-data field parsed from `-F` / `--form`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDataField {
    pub key: String,
    pub value: String,
    #[serde(rename = "valueType")]
    pub value_type: Option<String>,
}

/// Parse a cURL command string into structured request data.
pub fn parse_curl(input: &str) -> Result<ParsedCurlRequest, String> {
    let tokens = tokenize(input)?;

    let mut method = None;
    let mut url = None;
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut data: Option<String> = None;
    let mut form_data: Vec<FormDataField> = Vec::new();
    let mut auth_basic: Option<(String, String)> = None;
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
                        headers.insert(key.trim().to_string(), value.trim().to_string());
                    } else if let Some(key) = header.strip_suffix(';') {
                        // Chrome emits `-H 'Key;'` for headers with an empty value
                        headers.insert(key.trim().to_string(), String::new());
                    } else {
                        // A bare token with no colon/semicolon — header with empty value
                        headers.insert(header.trim().to_string(), String::new());
                    }
                }
            }
            "-b" | "--cookie" | "--cookie-jar" => {
                i += 1;
                if let Some(cookie) = tokens.get(i) {
                    // Chrome uses `-b` for cookies; surface them as a Cookie header
                    let entry = headers
                        .entry("Cookie".to_string())
                        .or_insert_with(String::new);
                    if entry.is_empty() {
                        *entry = cookie.clone();
                    } else {
                        entry.push_str("; ");
                        entry.push_str(cookie);
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                i += 1;
                data = tokens.get(i).cloned();
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
            "-k" | "--insecure" => {
                verify_ssl = false;
            }
            "-L" | "--location" => {
                follow_redirects = true;
            }
            "--compressed" | "-s" | "--silent" | "-S" | "--show-error" | "-v" | "--verbose" => {
                // Ignored flags
            }
            _ => {
                // If it starts with -, it's an unknown flag; skip its value if needed
                if token.starts_with('-') {
                    // Unknown flag, skip
                } else if url.is_none() {
                    // First non-flag token after 'curl' is the URL
                    url = Some(token.clone());
                }
            }
        }
        i += 1;
    }

    let url = url.ok_or("No URL found in cURL command")?;

    // Infer method from data presence
    let method = method.unwrap_or_else(|| {
        if data.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        }
    });

    // Infer body type from content-type header
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());

    let body_type = content_type.as_deref().map(|ct| {
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

    // Presence of -F / --form wins over content-type for body typing.
    let body_type = if !form_data.is_empty() {
        Some("form-data".to_string())
    } else {
        body_type
    };

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

/// Parse a single `-F 'key=value'` (or `-F 'key=@path'`) form field.
/// Supports `;type=...` / `;filename=...` modifiers, which are ignored for the
/// plain key/value that ApiArk's form-data table stores.
fn parse_form_field(field: &str, out: &mut Vec<FormDataField>) {
    // Strip leading "=" if present (curl allows `-F '=value'` for unnamed parts).
    let field = field.strip_prefix('=').unwrap_or(field);

    // Split on the first '=' to get key and value.
    let (key, value_part) = match field.split_once('=') {
        Some((k, v)) => (k, v),
        None => (field, ""),
    };
    let key = key.trim();
    if key.is_empty() {
        return;
    }

    // value may carry `;type=...` / `;filename=...` / `;headers=...` suffixes.
    let value = value_part.split(';').next().unwrap_or(value_part);

    // A leading '@' marks a file upload.
    let (value, is_file) = if let Some(p) = value.strip_prefix('@') {
        (p.to_string(), true)
    } else {
        (value.to_string(), false)
    };

    out.push(FormDataField {
        key: key.to_string(),
        value,
        value_type: if is_file {
            Some("file".to_string())
        } else {
            Some("text".to_string())
        },
    });
}

/// Export request parameters to a cURL command string.
/// Output follows Chrome DevTools "Copy as cURL" conventions:
/// `curl --url '<URL>'` plus `-H`, `--request`, `--data-raw` etc.
pub fn export_curl(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    auth_basic: Option<(&str, &str)>,
) -> String {
    let mut parts = vec![format!("curl --url '{url}'")];

    // Like Chrome, only emit --request for non-default (non-GET/HEAD) methods.
    let m = method.to_uppercase();
    if m != "GET" && m != "HEAD" {
        parts.push(format!("--request {m}"));
    }

    for (key, value) in headers {
        // Chrome emits `-H 'Key;'` for empty-value headers.
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

/// Tokenize a cURL command, handling single and double quotes.
///
/// Also handles shell line-continuation (`\` immediately followed by a newline),
/// which must be dropped rather than emitted as a literal `\n` token — otherwise
/// the stray token breaks URL detection on multi-line Chrome DevTools copies.
fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    while let Some(ch) = chars.next() {
        if escape_next {
            // A backslash-escaped newline is a line continuation and is dropped.
            // Any other escaped char is kept literally.
            if ch != '\n' && ch != '\r' {
                current.push(ch);
            }
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quote => {
                // Line continuation: `\` + newline is skipped entirely.
                if let Some(&next) = chars.peek() {
                    if next == '\n' {
                        chars.next();
                        continue;
                    }
                    if next == '\r' {
                        chars.next();
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        continue;
                    }
                }
                escape_next = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' | '\n' | '\r' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    if in_single_quote || in_double_quote {
        return Err("Unterminated quote in cURL command".to_string());
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_get() {
        let result = parse_curl("curl https://api.example.com/users").unwrap();
        assert_eq!(result.method, "GET");
        assert_eq!(result.url, "https://api.example.com/users");
    }

    #[test]
    fn test_post_with_data() {
        let result = parse_curl(
            r#"curl -X POST https://api.example.com/users -H 'Content-Type: application/json' -d '{"name": "test"}'"#,
        )
        .unwrap();
        assert_eq!(result.method, "POST");
        assert_eq!(result.body.as_deref(), Some(r#"{"name": "test"}"#));
        assert_eq!(result.body_type.as_deref(), Some("json"));
    }

    #[test]
    fn test_basic_auth() {
        let result = parse_curl("curl -u user:pass https://example.com").unwrap();
        assert_eq!(
            result.auth_basic,
            Some(("user".to_string(), "pass".to_string()))
        );
    }

    #[test]
    fn test_insecure_flag() {
        let result = parse_curl("curl -k https://example.com").unwrap();
        assert!(!result.verify_ssl);
    }

    #[test]
    fn test_chrome_get_format() {
        // Chrome DevTools "Copy as cURL" — GET with query params + --url
        let curl = r#"curl --url 'http://172.19.80.10:31003/mediinfo-merc-hulianfw/api/v1/peizhi/getPeiZhi?zuZhiJGID=47142000133068311A1001&yeWuLX=37' \
  -H 'Accept: */*' \
  -H 'Authorization: Bearer abc123' \
  -b '_c_WBKFRo=n4VnVvQ2NduIvTuSh4QNjuvhRyLYIdqiMvj6mndh' \
  -H 'yuanQuID;' \
  -H 'zhiGongGH: jkdba' \
  --insecure"#;
        let result = parse_curl(curl).unwrap();
        assert_eq!(result.method, "GET");
        assert_eq!(
            result.url,
            "http://172.19.80.10:31003/mediinfo-merc-hulianfw/api/v1/peizhi/getPeiZhi?zuZhiJGID=47142000133068311A1001&yeWuLX=37"
        );
        assert!(result.headers.contains_key("Accept"));
        assert!(result.headers.contains_key("zhiGongGH"));
        assert!(!result.verify_ssl);
        println!("Chrome GET format: url={}", result.url);
    }

    #[test]
    fn test_curl_x_get_with_query() {
        // First reported case: -X GET with URL containing query params + -g flag
        let curl = r#"curl -X GET \
  'http://172.19.80.10:31003/mediinfo-merc-yidongys/api/v1/shenPi/getTeShuSPList?Type=5&ShenHeZT=0&kaiShiSJ=2026-08-11&jieShuSJ=2026-08-18&Row=10&Page=1' \
  -H 'zhiGongGH: jkdba' \
  -g"#;
        let result = parse_curl(curl).unwrap();
        assert_eq!(result.method, "GET");
        assert_eq!(
            result.url,
            "http://172.19.80.10:31003/mediinfo-merc-yidongys/api/v1/shenPi/getTeShuSPList?Type=5&ShenHeZT=0&kaiShiSJ=2026-08-11&jieShuSJ=2026-08-18&Row=10&Page=1"
        );
        println!("-X GET format: url={}", result.url);
    }

    #[test]
    fn test_form_data_parsing() {
        // Chrome DevTools "Copy as cURL" for a multipart/form-data POST.
        let curl = r#"curl --url 'http://172.19.80.10:31003/mediinfo-lyra-authserver/connect/token' \
  --request POST \
  -F 'client_id=lyra.saas' \
  -F 'client_secret=49C1A7E1-0C79-4A89-A3D6-A37998FB86B0' \
  -F 'grant_type=client_credentials'"#;
        let result = parse_curl(curl).unwrap();
        assert_eq!(result.method, "POST");
        assert_eq!(result.body_type.as_deref(), Some("form-data"));
        assert_eq!(
            result.url,
            "http://172.19.80.10:31003/mediinfo-lyra-authserver/connect/token"
        );
        assert_eq!(result.form_data.len(), 3);
        assert_eq!(result.form_data[0].key, "client_id");
        assert_eq!(result.form_data[0].value, "lyra.saas");
        assert_eq!(result.form_data[1].key, "client_secret");
        assert_eq!(result.form_data[1].value, "49C1A7E1-0C79-4A89-A3D6-A37998FB86B0");
        assert_eq!(result.form_data[2].key, "grant_type");
        assert_eq!(result.form_data[2].value, "client_credentials");
        println!("Form-data parsing: {} fields parsed", result.form_data.len());
    }

    #[test]
    fn test_form_file_parsing() {
        // A -F field whose value starts with '@' denotes a file upload.
        let result =
            parse_curl("curl -X POST https://api.example.com/upload -F 'file=@/tmp/photo.png'")
                .unwrap();
        assert_eq!(result.body_type.as_deref(), Some("form-data"));
        assert_eq!(result.form_data.len(), 1);
        assert_eq!(result.form_data[0].key, "file");
        assert_eq!(result.form_data[0].value, "/tmp/photo.png");
        assert_eq!(result.form_data[0].value_type.as_deref(), Some("file"));
        println!("Form file parsing: value_type=file");
    }

    #[test]
    fn test_export_chrome_style() {
        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Empty".to_string(), String::new());
        let out = export_curl("POST", "https://api.example.com/token", &headers, Some("a=b"), None);
        assert!(out.starts_with("curl --url 'https://api.example.com/token'"));
        assert!(out.contains("--request POST"));
        assert!(out.contains("-H 'Accept: application/json'"));
        assert!(out.contains("-H 'Empty;'"));
        assert!(out.contains("--data-raw 'a=b'"));
        println!("Export (Chrome style):\n{out}");
    }
}
