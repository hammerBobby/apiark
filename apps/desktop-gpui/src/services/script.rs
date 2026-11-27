//! Lightweight JavaScript scripting support (ported from `BE/scripting/engine.rs`).
//!
//! Scripts are evaluated in an isolated QuickJS runtime. The `ark` global exposes
//! the small API surface used by ApiArk scripts: `ark.env`, `ark.request`,
//! `ark.response`, `ark.test`, and `ark.expect`.

use std::collections::HashMap;
use std::sync::Mutex;

use rquickjs::{CaughtError, Context, Function, Runtime};

use crate::models::{ConsoleLevel, ResponseData, TestResult};

#[derive(Debug, Clone, Default)]
pub struct ScriptChanges {
    pub variables: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub console: Vec<(ConsoleLevel, String)>,
}

static RSA_LAST_ERROR: Mutex<String> = Mutex::new(String::new());

fn native_rsa_sign_sha256(pem: String, data: String) -> Option<String> {
    match do_rsa_sign_sha256(&pem, &data) {
        Ok(signature) => Some(signature),
        Err(err) => {
            if let Ok(mut guard) = RSA_LAST_ERROR.lock() {
                *guard = err;
            }
            None
        }
    }
}

fn native_rsa_last_error() -> String {
    RSA_LAST_ERROR
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

fn do_rsa_sign_sha256(pem: &str, data: &str) -> Result<String, String> {
    use base64::Engine as _;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::RsaPrivateKey;
    use sha2::Sha256;

    let der = extract_key_der(pem)?;
    let private_key = RsaPrivateKey::from_pkcs8_der(&der).or_else(|e8| {
        RsaPrivateKey::from_pkcs1_der(&der).map_err(|e1| {
            format!(
                "invalid private key DER (tried PKCS#8, then PKCS#1): pkcs8: {e8}; pkcs1: {e1}"
            )
        })
    })?;
    let signing_key = SigningKey::<Sha256>::new(private_key);
    let signature = signing_key.sign(data.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()))
}

/// Accepts full PEM, bare base64 body, or a key pasted without its end marker.
fn extract_key_der(key: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;

    let mut body = String::new();
    let mut in_body = false;
    let mut saw_end = false;
    for line in key.lines() {
        let t = line.trim();
        if t.starts_with("-----BEGIN") {
            in_body = true;
            continue;
        }
        if t.starts_with("-----END") {
            in_body = false;
            saw_end = true;
            continue;
        }
        if in_body {
            body.push_str(t.trim());
        }
    }
    let body = if body.is_empty() && !saw_end {
        key.trim()
    } else {
        &body
    };
    let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Err("invalid private key: key body is empty".to_string());
    }
    if let Some((pos, ch)) = cleaned
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '='))
    {
        return Err(format!(
            "invalid private key: illegal character {ch:?} (U+{:04X}) at position {pos} of the key body",
            ch as u32
        ));
    }
    let unpadded = cleaned.trim_end_matches('=');
    let der = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(unpadded)
        .map_err(|e| {
            format!(
                "invalid private key: base64 decode failed ({e}); body length is {} chars",
                unpadded.len()
            )
        })?;

    if der.len() >= 2 && der[0] == 0x30 {
        let declared = if der[1] & 0x80 == 0 {
            Some((der[1] as usize, 2usize))
        } else {
            let n = (der[1] & 0x7f) as usize;
            if (1..=4).contains(&n) && der.len() >= 2 + n {
                let len = der[2..2 + n]
                    .iter()
                    .fold(0usize, |acc, &b| (acc << 8) | b as usize);
                Some((len, 2 + n))
            } else {
                None
            }
        };
        if let Some((content_len, header_len)) = declared {
            if content_len + header_len > der.len() {
                return Err(format!(
                    "invalid private key: DER sequence declares {} bytes but only {} are present; the pasted key is likely truncated",
                    content_len + header_len,
                    der.len()
                ));
            }
        }
    }
    Ok(der)
}

fn native_console(level: String, message: String) {
    tracing::info!(target: "apiark::script", "[script {level}] {message}");
}

const CONSOLE_SHIM: &str = r#"
  const __console_entries = [];
  function __pushConsole(level, args) {
    var message = Array.prototype.map.call(args, String).join(' ');
    __console_entries.push({ level: level, message: message });
    __arkConsole(level, message);
  }
  const console = {
    log: function() { __pushConsole('log', arguments); },
    info: function() { __pushConsole('info', arguments); },
    warn: function() { __pushConsole('warn', arguments); },
    error: function() { __pushConsole('error', arguments); }
  };
"#;

fn eval_json(source: String) -> Result<String, String> {
    let runtime = Runtime::new().map_err(|e| format!("Failed to create JS runtime: {e}"))?;
    let context = Context::full(&runtime).map_err(|e| format!("Failed to create JS context: {e}"))?;
    context.with(|ctx| {
        let globals = ctx.globals();
        globals
            .set("__arkRsaSignSha256", Function::new(ctx.clone(), native_rsa_sign_sha256))
            .map_err(|e| format!("Failed to register rsa sign fn: {e}"))?;
        globals
            .set("__arkRsaLastError", Function::new(ctx.clone(), native_rsa_last_error))
            .map_err(|e| format!("Failed to register rsa last-error fn: {e}"))?;
        globals
            .set("__arkConsole", Function::new(ctx.clone(), native_console))
            .map_err(|e| format!("Failed to register console fn: {e}"))?;
        CaughtError::catch(&ctx, ctx.eval::<String, _>(source)).map_err(|e| e.to_string())
    })
}

pub fn run_pre_request(
    script: &str,
    variables: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<ScriptChanges, String> {
    if script.trim().is_empty() {
        return Ok(ScriptChanges::default());
    }
    let body_json = serde_json::to_string(body.unwrap_or(""))
        .unwrap_or_else(|_| "\"\"".to_string());
    let source = format!(
        r#"(function() {{
  const __env = {};
  const __headers = {};
  let __body = {};
{}
  const ark = {{
    env: {{
      get(name) {{ return __env[String(name)]; }},
      set(name, value) {{ __env[String(name)] = String(value); }},
      unset(name) {{ delete __env[String(name)]; }}
    }},
    variables: {{
      get(name) {{ return __env[String(name)]; }},
      set(name, value) {{ __env[String(name)] = String(value); }},
      unset(name) {{ delete __env[String(name)]; }}
    }},
    request: {{
      get body() {{ return __body; }},
      setBody(b) {{ __body = (b === null || b === undefined) ? null : String(b); }},
      setHeader(name, value) {{ __headers[String(name)] = String(value); }},
      removeHeader(name) {{ delete __headers[String(name)]; }}
    }},
    crypto: {{
      rsaSignSha256(pem, data) {{
        const result = __arkRsaSignSha256(String(pem), String(data));
        if (result === null || result === undefined) {{
          throw new Error('RSA signing failed: ' + __arkRsaLastError());
        }}
        return result;
      }}
    }}
  }};
  try {{
    {}
  }} catch (error) {{
    throw new Error('Pre-request: ' + ((error && error.message) || error));
  }}
  return JSON.stringify({{ env: __env, headers: __headers, body: __body, console: __console_entries }});
}})()"#,
        serde_json::to_string(variables).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string(headers).unwrap_or_else(|_| "{}".to_string()),
        body_json,
        CONSOLE_SHIM,
        script,
    );
    let json = eval_json(source)?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("Invalid pre-request result: {e}"))?;
    let to_map = |key: &str| -> HashMap<String, String> {
        value
            .get(key)
            .and_then(|value| value.as_object())
            .map(|object| {
                object
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let console = value
        .get("console")
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let level = match entry.get("level").and_then(|v| v.as_str()).unwrap_or("log") {
                        "warn" => ConsoleLevel::Warning,
                        "error" => ConsoleLevel::Error,
                        _ => ConsoleLevel::Info,
                    };
                    let message = entry
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    (level, message)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ScriptChanges {
        variables: to_map("env"),
        headers: to_map("headers"),
        body: value.get("body").and_then(|v| v.as_str()).map(str::to_string),
        console,
    })
}

pub fn run_post_response(script: &str, response: &ResponseData) -> Result<(), String> {
    if script.trim().is_empty() {
        return Ok(());
    }
    let response_json = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string());
    let source = format!(
        r#"(function() {{
  const response = {};
{}
  const ark = {{ response }};
  try {{
    {script}
  }} catch (error) {{
    throw new Error('Post-response: ' + ((error && error.message) || error));
  }}
  return 0;
}})()"#,
        response_json,
        CONSOLE_SHIM,
    );
    eval_json(source).map(|_| ())
}

pub fn run_tests(script: &str, response: &ResponseData) -> Result<Vec<TestResult>, String> {
    if script.trim().is_empty() {
        return Ok(Vec::new());
    }
    let response_json = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string());
    let source = format!(
        r#"(function() {{
  const __results = [];
  const response = {};
{}
  const fail = (message) => {{ throw new Error(message); }};
  const ark = {{
    response,
    test(name, fn) {{
      try {{
        fn();
        __results.push({{ name: String(name), passed: true, error: null }});
      }} catch (error) {{
        __results.push({{ name: String(name), passed: false,
          error: String((error && error.message) || error) }});
      }}
    }},
    expect(actual) {{
      return {{
        toBe(expected) {{
          if (actual !== expected) fail('expected ' + JSON.stringify(actual) + ' to be ' + JSON.stringify(expected));
        }},
        toEqual(expected) {{
          if (JSON.stringify(actual) !== JSON.stringify(expected)) fail('expected ' + JSON.stringify(actual) + ' to equal ' + JSON.stringify(expected));
        }},
        toContain(expected) {{
          if (actual == null || actual.indexOf(expected) === -1) fail('expected ' + JSON.stringify(actual) + ' to contain ' + JSON.stringify(expected));
        }},
        toBeTruthy() {{
          if (!actual) fail('expected truthy value');
        }}
      }};
    }}
  }};
  try {{
    {script}
  }} catch (error) {{
    __results.push({{ name: 'Script error', passed: false,
      error: String((error && error.message) || error) }});
  }}
  return JSON.stringify(__results);
}})()"#,
        response_json,
        CONSOLE_SHIM,
    );
    let json = eval_json(source)?;
    let values: Vec<serde_json::Value> =
        serde_json::from_str(&json).map_err(|e| format!("Invalid test result: {e}"))?;
    Ok(values
        .into_iter()
        .map(|value| TestResult {
            name: value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Test")
                .to_string(),
            passed: value.get("passed").and_then(|v| v.as_bool()).unwrap_or(false),
            error: value
                .get("error")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string()),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> ResponseData {
        ResponseData {
            status: 200,
            status_text: "OK".into(),
            headers: Vec::new(),
            cookies: Vec::new(),
            body: "{\"code\":0}".into(),
            time_ms: 12,
            size_bytes: 10,
            truncated: None,
            full_size: None,
            full_body_path: None,
        }
    }

    #[test]
    fn pre_request_script_can_set_variables_and_headers() {
        let changes = run_pre_request(
            "ark.env.set('token','abc'); ark.request.setHeader('X-Token', ark.env.get('token'));",
            &HashMap::new(),
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(changes.variables.get("token").map(String::as_str), Some("abc"));
        assert_eq!(changes.headers.get("X-Token").map(String::as_str), Some("abc"));
    }

    #[test]
    fn pre_request_can_modify_body_and_rsa_sign() {
        use base64::Engine as _;
        use rsa::pkcs1v15::{Signature, VerifyingKey};
        use rsa::pkcs8::EncodePrivateKey;
        use rsa::signature::Verifier;
        use rsa::RsaPrivateKey;
        use sha2::Sha256;

        let private_key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
        let pem = private_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();
        let script = format!(
            r#"
              var body = JSON.parse(ark.request.body || '{{}}');
              body.timestamp = 1700000000000;
              delete body.sign;
              var keys = Object.keys(body).sort();
              var data = keys.map(function (k) {{ return k + '=' + body[k]; }}).join('&');
              console.log('keyValueString: ' + data);
              body.sign = ark.crypto.rsaSignSha256({}, data);
              ark.request.setBody(JSON.stringify(body));
            "#,
            serde_json::to_string(&pem).unwrap(),
        );

        let changes = run_pre_request(&script, &HashMap::new(), &HashMap::new(), Some(r#"{"a":1}"#))
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_str(changes.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["timestamp"], 1700000000000i64);

        let data = "a=1&timestamp=1700000000000";
        let signature = base64::engine::general_purpose::STANDARD
            .decode(body["sign"].as_str().unwrap())
            .unwrap();
        let verifying_key = VerifyingKey::<Sha256>::new(private_key.to_public_key());
        verifying_key
            .verify(data.as_bytes(), &Signature::try_from(signature.as_slice()).unwrap())
            .unwrap();
        assert!(changes
            .console
            .iter()
            .any(|(_, message)| message.contains("keyValueString")));
    }

    #[test]
    fn truncated_private_key_reports_clear_error() {
        let error = run_pre_request(
            "ark.crypto.rsaSignSha256('MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSj', 'data');",
            &HashMap::new(),
            &HashMap::new(),
            None,
        )
        .unwrap_err();
        assert!(error.contains("truncated"), "{error}");
    }

    #[test]
    fn pre_request_reports_real_js_error() {
        let error = run_pre_request(
            "ark.request.body.notAFunction();",
            &HashMap::new(),
            &HashMap::new(),
            Some("{}"),
        )
        .unwrap_err();
        assert!(error.contains("Pre-request:"), "{error}");
        assert!(!error.contains("Exception generated by QuickJS"), "{error}");
    }

    #[test]
    fn test_script_collects_pass_and_fail() {
        let results = run_tests(
            "ark.test('ok', () => { ark.expect(response.status).toBe(200); }); ark.test('bad', () => { ark.expect(1).toBe(2); });",
            &response(),
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(!results[1].passed);
        assert!(results[1].error.is_some());
    }
}
