//! OAuth 2.0 token acquisition — ported from
//! `apps/desktop/src-tauri/src/oauth/{mod,flow,callback}.rs`.
//!
//! In-process: no Tauri IPC. Supported grants:
//!   - client_credentials / password : direct token request, no browser
//!   - authorization_code           : local callback server + system browser
//!   - implicit                     : local callback server (fragment via POST)
//!
//! Tokens are cached in memory, keyed by "{client_id}:{auth_url}" (same as Tauri).

use crate::models::AuthConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Cached token, mirrors Tauri `OAuthToken`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
}

impl OAuthToken {
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => chrono::Utc::now().timestamp() >= exp,
            None => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokenStatus {
    pub has_token: bool,
    pub is_expired: bool,
    pub expires_at: Option<i64>,
    pub token_type: Option<String>,
}

pub struct OAuthTokenStore {
    tokens: Mutex<HashMap<String, OAuthToken>>,
}

impl OAuthTokenStore {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn cache_key(client_id: &str, auth_url: &str) -> String {
        format!("{}:{}", client_id, auth_url)
    }

    pub fn get(&self, key: &str) -> Option<OAuthToken> {
        self.tokens.lock().ok()?.get(key).cloned()
    }

    pub fn set(&self, key: String, token: OAuthToken) {
        if let Ok(mut map) = self.tokens.lock() {
            map.insert(key, token);
        }
    }

    pub fn remove(&self, key: &str) {
        if let Ok(mut map) = self.tokens.lock() {
            map.remove(key);
        }
    }

    /// Return a cached, non-expired access token without starting any flow.
    pub fn cached_access_token(&self, config: &AuthConfig) -> Option<String> {
        match config {
            AuthConfig::Oauth2 { client_id, auth_url, .. } => {
                let key = Self::cache_key(client_id, auth_url);
                self.get(&key)
                    .filter(|t| !t.is_expired())
                    .map(|t| t.access_token)
            }
            _ => None,
        }
    }

    pub fn status(&self, key: &str) -> OAuthTokenStatus {
        match self.get(key) {
            Some(token) => OAuthTokenStatus {
                has_token: true,
                is_expired: token.is_expired(),
                expires_at: token.expires_at,
                token_type: Some(token.token_type),
            },
            None => OAuthTokenStatus {
                has_token: false,
                is_expired: false,
                expires_at: None,
                token_type: None,
            },
        }
    }

    /// Resolve a usable access token: cached & valid, refresh if possible,
    /// otherwise start a new flow. Returns the access token string.
    pub async fn access_token(&self, config: &AuthConfig) -> Result<String, String> {
        let AuthConfig::Oauth2 {
            grant_type, auth_url, token_url, client_id, client_secret, scope,
            callback_url, username, password, use_pkce,
        } = config else {
            return Err("Auth config is not OAuth2".into());
        };

        let key = Self::cache_key(client_id, auth_url);
        if let Some(token) = self.get(&key) {
            if !token.is_expired() {
                return Ok(token.access_token);
            }
            if let Some(refresh) = token.refresh_token.as_ref().filter(|_| !token_url.is_empty()) {
                if let Ok(new_token) = refresh_token_request(token_url, client_id, client_secret, refresh).await {
                    self.set(key.clone(), new_token.clone());
                    return Ok(new_token.access_token);
                }
            }
        }

        let token = match grant_type.as_str() {
            "client_credentials" => {
                client_credentials_flow(token_url, client_id, client_secret, scope).await?
            }
            "password" => password_flow(token_url, client_id, client_secret, username, password, scope).await?,
            "authorization_code" => {
                authorization_code_flow(auth_url, token_url, client_id, client_secret, scope, callback_url, *use_pkce).await?
            }
            "implicit" => implicit_flow(auth_url, client_id, scope, callback_url).await?,
            other => return Err(format!("Unsupported grant type: {other}")),
        };

        let access = token.access_token.clone();
        self.set(key, token);
        Ok(access)
    }
}

impl Default for OAuthTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Token response parsing ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_expires_in")]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

fn deserialize_expires_in<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum E {
        Num(u64),
        Str(String),
        Null,
    }
    match E::deserialize(d) {
        Ok(E::Num(n)) => Ok(Some(n)),
        Ok(E::Str(s)) => Ok(s.parse().ok()),
        Ok(E::Null) | Err(_) => Ok(None),
    }
}

fn token_from_response(resp: TokenResponse) -> OAuthToken {
    OAuthToken {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at: resp.expires_in.map(|s| chrono::Utc::now().timestamp() + s as i64),
        token_type: resp.token_type.unwrap_or_else(|| "Bearer".into()),
    }
}

async fn post_token(params: &[(&str, &str)], token_url: &str) -> Result<OAuthToken, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token request failed: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Token endpoint returned {status}: {body}"));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Invalid token response: {e}"))?;
    Ok(token_from_response(parsed))
}

// ── Direct grants ─────────────────────────────────────────────────────────

async fn client_credentials_flow(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    scope: &str,
) -> Result<OAuthToken, String> {
    let mut params = vec![
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    if !scope.is_empty() {
        params.push(("scope", scope));
    }
    post_token(&params, token_url).await
}

async fn password_flow(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    username: &str,
    password: &str,
    scope: &str,
) -> Result<OAuthToken, String> {
    let mut params = vec![
        ("grant_type", "password"),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("username", username),
        ("password", password),
    ];
    if !scope.is_empty() {
        params.push(("scope", scope));
    }
    post_token(&params, token_url).await
}

async fn refresh_token_request(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh: &str,
) -> Result<OAuthToken, String> {
    let params = vec![
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh),
    ];
    post_token(&params, token_url).await
}

// ── Browser-based grants ──────────────────────────────────────────────────

fn extract_port(callback_url: &str) -> Result<u16, String> {
    let url = url::Url::parse(callback_url).map_err(|e| e.to_string())?;
    url.port().ok_or_else(|| "Callback URL must include a port".into())
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Minimal callback listener using axum, adapted from Tauri `oauth/callback.rs`.
async fn wait_for_callback(port: u16, path: &str, state: Option<&str>) -> Result<String, String> {
    use axum::extract::Query;
    use axum::response::Html;
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashMap as StdHashMap;

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let expected_state = state.map(|s| s.to_string());
    let callback_path = path.to_string();

    let app = Router::new().route(
        &format!("/{}", callback_path.trim_start_matches('/')),
        get(move |Query(params): Query<StdHashMap<String, String>>| {
            let tx = tx.clone();
            let expected_state = expected_state.clone();
            async move {
                if let Some(ref expected) = expected_state {
                    if params.get("state").map(|s| s.as_str()) != Some(expected.as_str()) {
                        return Html("<h1>State mismatch</h1>".to_string());
                    }
                }
                let code = params.get("code").cloned();
                if let Some(sender) = tx.lock().ok().and_then(|mut g| g.take()) {
                    let _ = sender.send(code.ok_or_else(|| "No code in callback".to_string()));
                }
                Html("<h1>Authentication complete</h1><p>You can close this window.</p>".to_string())
            }
        }),
    );

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind callback server on {addr}: {e}"))?;

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    let result = tokio::time::timeout(Duration::from_secs(120), rx)
        .await
        .map_err(|_| "OAuth callback timed out (120s)".to_string())?
        .map_err(|_| "Callback channel closed".to_string())?;
    server.abort();
    result
}

async fn authorization_code_flow(
    auth_url: &str,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    scope: &str,
    callback_url: &str,
    use_pkce: bool,
) -> Result<OAuthToken, String> {
    use sha2::{Digest, Sha256};

    let port = extract_port(callback_url)?;
    let state = uuid::Uuid::new_v4().to_string();
    let (verifier, challenge) = if use_pkce {
        let v = uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string();
        let digest = Sha256::digest(v.as_bytes());
        let c = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, digest);
        (Some(v), Some(c))
    } else {
        (None, None)
    };

    let mut params = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", callback_url),
        ("scope", scope),
        ("state", state.as_str()),
    ];
    if let Some(ref c) = challenge {
        params.push(("code_challenge", c));
        params.push(("code_challenge_method", "S256"));
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    let separator = if auth_url.contains('?') { "&" } else { "?" };
    let auth_full = format!("{auth_url}{separator}{query}");

    // Start listener before opening the browser; pass expected state for CSRF check.
    let expected_state = state.clone();
    let callback_fut = tokio::spawn(async move {
        wait_for_callback(port, "/callback", Some(expected_state.as_str())).await
    });
    // Give the listener a moment to bind before launching the browser.
    tokio::time::sleep(Duration::from_millis(200)).await;
    open_browser(&auth_full)?;

    let code = callback_fut
        .await
        .map_err(|e| format!("Callback task failed: {e}"))??;

    let mut token_params = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", callback_url),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    if let Some(ref v) = verifier {
        token_params.push(("code_verifier", v));
    }
    post_token(&token_params, token_url).await
}

async fn implicit_flow(
    auth_url: &str,
    client_id: &str,
    scope: &str,
    callback_url: &str,
) -> Result<OAuthToken, String> {
    let port = extract_port(callback_url)?;
    let params = vec![
        ("response_type", "token"),
        ("client_id", client_id),
        ("redirect_uri", callback_url),
        ("scope", scope),
    ];
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    let separator = if auth_url.contains('?') { "&" } else { "?" };
    open_browser(&format!("{auth_url}{separator}{query}"))?;

    // Implicit returns the token in the URL fragment; browsers don't send it to
    // servers. The Tauri app has a JS page that POSTs it back to /callback/token.
    // For this MVP we surface a clear limitation instead of silently failing.
    Err(format!(
        "Implicit grant requires the callback HTML page to relay the URL fragment. \
         Please use Authorization Code with PKCE instead (callback port {port})."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_store_keys_and_status() {
        let store = OAuthTokenStore::new();
        let key = OAuthTokenStore::cache_key("cid", "https://auth.example.com");
        assert_eq!(key, "cid:https://auth.example.com");

        let status = store.status(&key);
        assert!(!status.has_token);

        store.set(
            key.clone(),
            OAuthToken {
                access_token: "abc".into(),
                refresh_token: None,
                expires_at: None,
                token_type: "Bearer".into(),
            },
        );
        let status = store.status(&key);
        assert!(status.has_token);
        assert!(!status.is_expired);
    }

    #[test]
    fn expires_in_accepts_string_or_number() {
        let from_num: TokenResponse = serde_json::from_str(
            r#"{"access_token":"a","expires_in":3600}"#,
        ).unwrap();
        assert_eq!(from_num.expires_in, Some(3600));

        let from_str: TokenResponse = serde_json::from_str(
            r#"{"access_token":"a","expires_in":"7200"}"#,
        ).unwrap();
        assert_eq!(from_str.expires_in, Some(7200));
    }
}
