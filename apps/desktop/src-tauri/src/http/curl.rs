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
    let mut data_parts: Vec<String> = Vec::new();
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
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                i += 1;
                // cURL concatenates repeated `-d` fragments with '&'.
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

    let raw_url = url.ok_or("No URL found in cURL command")?;
    let url = normalize_url(&raw_url);

    let data = if data_parts.is_empty() {
        None
    } else {
        Some(data_parts.join("&"))
    };

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

    // An urlencoded body (`-d 'a=1&b=2'`) must be surfaced as a key/value
    // table: the request is built from `form_data`, so leaving it empty would
    // send nothing at all.
    if form_data.is_empty() {
        if let (Some("urlencoded"), Some(body)) = (body_type.as_deref(), data.as_deref()) {
            parse_urlencoded(body, &mut form_data);
        }
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

/// Whether `s` already carries a URL scheme such as `http://` or `ws://`.
fn has_scheme(s: &str) -> bool {
    match s.find("://") {
        Some(0) => false,
        Some(idx) => {
            let scheme = &s[..idx];
            scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// Prefix a scheme-less URL with `http://`.
///
/// cURL itself defaults to HTTP when the scheme is omitted (`curl example.com/x`
/// means `http://example.com/x`), and browser/WeChat copies sometimes leave it
/// out. Protocol-relative URLs (`//host/x`) only need the scheme, while
/// anything already carrying a scheme is left untouched.
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

/// Split an `application/x-www-form-urlencoded` body into key/value fields.
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

/// Percent-decode a form value (also maps `+` back to a space).
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
    fn test_miniprogram_urlencoded_body() {
        let curl = r#"curl 'https://cloud-lyra-jmssjhbyy.mediinfo.cn/mediinfo-merc-authserver-java/connect/token' \
  -H 'authority: cloud-lyra-jmssjhbyy.mediinfo.cn' \
  -H 'accept: */*' \
  -H 'accept-language: zh-CN' \
  -H 'content-type: application/x-www-form-urlencoded' \
  -H 'sec-ch-ua: "Not.A/Brand";v="99", "Chromium";v="136"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'user-agent: Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.0 Mobile/15E148 Safari/604.1 wechatdevtools/2.02.2608080 MicroMessenger/8.0.5 Language/zh_CN webview/ hash/722597064 sid/s0 winId/s0 token/e78be9ec0533c21ca9113e8312b41631' \
  --data-raw 'client_id=mercdiandianjy-xcx&client_secret=49C1A7E1-0C79-4A89-A3D6-A37998FB86B0&grant_type=miniprogram&idp=weixinmp&code=0a1DZP000Gs4gX1L5O200n8qnp3DZP0Y&scope=openid&jiGouID=1223080041428113X1' \
  --compressed"#;
        let result = parse_curl(curl).unwrap();
        assert_eq!(
            result.url,
            "https://cloud-lyra-jmssjhbyy.mediinfo.cn/mediinfo-merc-authserver-java/connect/token"
        );
        assert_eq!(result.method, "POST");
        assert_eq!(result.body_type.as_deref(), Some("urlencoded"));
        // The whole --data-raw string is still available verbatim…
        assert!(result
            .body
            .as_deref()
            .unwrap()
            .starts_with("client_id=mercdiandianjy-xcx"));
        // …and the pairs are exposed as a table so the request can be sent.
        assert_eq!(result.form_data.len(), 7);
        assert_eq!(result.form_data[0].key, "client_id");
        assert_eq!(result.form_data[0].value, "mercdiandianjy-xcx");
        assert_eq!(result.form_data[1].key, "client_secret");
        assert_eq!(
            result.form_data[1].value,
            "49C1A7E1-0C79-4A89-A3D6-A37998FB86B0"
        );
        assert_eq!(result.form_data[6].key, "jiGouID");
        assert_eq!(result.form_data[6].value, "1223080041428113X1");
        // Headers with double quotes / semicolons inside must survive intact.
        assert_eq!(
            result.headers.get("sec-ch-ua").map(String::as_str),
            Some("\"Not.A/Brand\";v=\"99\", \"Chromium\";v=\"136\"")
        );
        assert_eq!(result.headers.get("sec-ch-ua-mobile").map(String::as_str), Some("?0"));
    }

    #[test]
    fn test_urlencoded_values_are_decoded() {
        let curl = "curl -X POST https://example.com/token \
            -H 'Content-Type: application/x-www-form-urlencoded' \
            --data-raw 'name=John+Doe&note=hello%20world%26more&empty='";
        let result = parse_curl(curl).unwrap();
        assert_eq!(result.form_data.len(), 3);
        assert_eq!(result.form_data[0].key, "name");
        assert_eq!(result.form_data[0].value, "John Doe");
        assert_eq!(result.form_data[1].value, "hello world&more");
        assert_eq!(result.form_data[2].key, "empty");
        assert_eq!(result.form_data[2].value, "");
    }

    #[test]
    fn test_repeated_data_flags_are_merged() {
        // cURL joins consecutive -d fragments with '&'
        let result = parse_curl(
            "curl -X POST https://example.com/api -d 'a=1' -d 'b=2' --data-raw 'c=3'",
        )
        .unwrap();
        assert_eq!(result.body.as_deref(), Some("a=1&b=2&c=3"));
    }

    #[test]
    fn test_url_without_scheme_gets_http() {
        assert_eq!(normalize_url("example.com/api/users"), "http://example.com/api/users");
        assert_eq!(normalize_url("localhost:3000/health"), "http://localhost:3000/health");
        assert_eq!(normalize_url("//cdn.example.com/a.png"), "http://cdn.example.com/a.png");
        // Already-schemed URLs are untouched
        assert_eq!(normalize_url("https://api.example.com/x"), "https://api.example.com/x");
        assert_eq!(normalize_url("wss://ws.example.com/stream"), "wss://ws.example.com/stream");
    }

    #[test]
    fn test_parse_curl_without_scheme() {
        let result = parse_curl("curl example.com/api -H 'Accept: */*'").unwrap();
        assert_eq!(result.url, "http://example.com/api");
        assert!(result.headers.contains_key("Accept"));
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
