//! Shared data models for the GPUI frontend.
//! These mirror the TypeScript types in `packages/types/src/index.ts`
//! and the Rust models in `src-tauri/src/models/`.

use gpui_kit::SharedString;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── HTTP Methods ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

impl Default for HttpMethod {
    fn default() -> Self {
        Self::Get
    }
}

impl HttpMethod {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Get,
        }
    }
}

// ── Key-Value Pair ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePair {
    pub id: String,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
}

impl KeyValuePair {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            key: key.into(),
            value: value.into(),
            enabled: true,
            value_type: None,
        }
    }
}

// ── Request Body ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BodyType {
    Json,
    Xml,
    FormData,
    Urlencoded,
    Raw,
    Binary,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    #[serde(rename = "type")]
    pub body_type: BodyType,
    pub content: String,
    pub form_data: Vec<KeyValuePair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
}

impl Default for RequestBody {
    fn default() -> Self {
        Self {
            body_type: BodyType::None,
            content: String::new(),
            form_data: Vec::new(),
            binary_path: None,
        }
    }
}

// ── Auth Config ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AuthConfig {
    None,
    Bearer { token: String },
    Basic { username: String, password: String },
    ApiKey {
        key: String,
        value: String,
        #[serde(default = "default_add_to", rename = "addTo")]
        add_to: String,
    },
    Oauth2 {
        #[serde(default, rename = "grantType")]
        grant_type: String,
        #[serde(default, rename = "authUrl")]
        auth_url: String,
        #[serde(default, rename = "tokenUrl")]
        token_url: String,
        #[serde(default, rename = "clientId")]
        client_id: String,
        #[serde(default, rename = "clientSecret")]
        client_secret: String,
        #[serde(default)]
        scope: String,
        #[serde(default, rename = "callbackUrl")]
        callback_url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default, rename = "usePkce")]
        use_pkce: bool,
    },
    Digest { username: String, password: String },
    AwsV4 {
        #[serde(default, rename = "accessKey")]
        access_key: String,
        #[serde(default, rename = "secretKey")]
        secret_key: String,
        #[serde(default)]
        region: String,
        #[serde(default)]
        service: String,
        #[serde(default, rename = "sessionToken")]
        session_token: String,
    },
    Ntlm {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default)]
        domain: String,
        #[serde(default)]
        workstation: String,
    },
    JwtBearer {
        #[serde(default)]
        secret: String,
        #[serde(default = "default_hs256")]
        algorithm: String,
        #[serde(default)]
        payload: String,
        #[serde(default = "default_bearer_prefix", rename = "headerPrefix")]
        header_prefix: String,
    },
    Saml {
        #[serde(default, rename = "idpUrl")]
        idp_url: String,
        #[serde(default, rename = "entityId")]
        entity_id: String,
        #[serde(default, rename = "assertionConsumerUrl")]
        assertion_consumer_url: String,
        #[serde(default)]
        certificate: String,
        #[serde(default, rename = "nameIdFormat")]
        name_id_format: String,
        #[serde(default, rename = "samlToken")]
        saml_token: String,
    },
}

fn default_add_to() -> String { "header".into() }
fn default_hs256() -> String { "HS256".into() }
fn default_bearer_prefix() -> String { "Bearer".into() }

impl Default for AuthConfig {
    fn default() -> Self {
        Self::None
    }
}

impl AuthConfig {
    /// Stable type key for the auth-type selector.
    pub fn type_key(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bearer { .. } => "bearer",
            Self::Basic { .. } => "basic",
            Self::ApiKey { .. } => "api-key",
            Self::Oauth2 { .. } => "oauth2",
            Self::Digest { .. } => "digest",
            Self::AwsV4 { .. } => "aws-v4",
            Self::Ntlm { .. } => "ntlm",
            Self::JwtBearer { .. } => "jwt-bearer",
            Self::Saml { .. } => "saml",
        }
    }

    /// Build a default config for a given type key (mirrors React `AuthEditor`).
    pub fn for_type(key: &str) -> Self {
        match key {
            "bearer" => Self::Bearer { token: String::new() },
            "basic" => Self::Basic { username: String::new(), password: String::new() },
            "api-key" => Self::ApiKey {
                key: String::new(),
                value: String::new(),
                add_to: "header".into(),
            },
            "oauth2" => Self::Oauth2 {
                grant_type: "authorization_code".into(),
                auth_url: String::new(),
                token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                callback_url: "http://localhost:9876/callback".into(),
                username: String::new(),
                password: String::new(),
                use_pkce: true,
            },
            "digest" => Self::Digest { username: String::new(), password: String::new() },
            "aws-v4" => Self::AwsV4 {
                access_key: String::new(),
                secret_key: String::new(),
                region: String::new(),
                service: String::new(),
                session_token: String::new(),
            },
            "ntlm" => Self::Ntlm {
                username: String::new(),
                password: String::new(),
                domain: String::new(),
                workstation: String::new(),
            },
            "jwt-bearer" => Self::JwtBearer {
                secret: String::new(),
                algorithm: "HS256".into(),
                payload: "{\n  \"sub\": \"1234567890\",\n  \"name\": \"ApiArk\"\n}".into(),
                header_prefix: "Bearer".into(),
            },
            "saml" => Self::Saml {
                idp_url: String::new(),
                entity_id: String::new(),
                assertion_consumer_url: String::new(),
                certificate: String::new(),
                name_id_format: "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress".into(),
                saml_token: String::new(),
            },
            _ => Self::None,
        }
    }
}

// ── Response ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseData {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<KeyValuePair>,
    #[serde(default)]
    pub cookies: Vec<KeyValuePair>,
    pub body: String,
    pub time_ms: u64,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_size: Option<u64>,
    /// Full body spooled to disk for "load full response".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_body_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffView {
    Body,
    Headers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSnapshot {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub response: ResponseData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerResult {
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub time_ms: u64,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Tab Protocol ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabProtocol {
    Http,
    GraphQL,
    WebSocket,
    Sse,
    Grpc,
    Mqtt,
    SocketIo,
}

impl Default for TabProtocol {
    fn default() -> Self {
        Self::Http
    }
}

// ── Tab ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    pub name: String,
    pub file_path: Option<String>,
    /// Saved cases live inside the parent request file; this identifies which
    /// case a tab represents so it cannot collide with the parent request tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_name: Option<String>,
    pub collection_path: Option<String>,
    pub is_dirty: bool,
    pub protocol: TabProtocol,

    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<KeyValuePair>,
    pub params: Vec<KeyValuePair>,
    #[serde(default)]
    pub path_params: Vec<KeyValuePair>,
    pub body: RequestBody,
    pub auth: AuthConfig,

    pub pre_request_script: Option<String>,
    pub post_response_script: Option<String>,
    pub test_script: Option<String>,

    pub response: Option<ResponseData>,
    pub loading: bool,
    #[serde(default)]
    pub test_results: Vec<TestResult>,
    #[serde(default)]
    pub cases: Vec<RequestFile>,
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Untitled Request".into(),
            file_path: None,
            case_name: None,
            collection_path: None,
            is_dirty: false,
            protocol: TabProtocol::Http,
            method: HttpMethod::Get,
            url: String::new(),
            headers: vec![KeyValuePair::new("Accept", "application/json")],
            params: Vec::new(),
            path_params: Vec::new(),
            body: RequestBody::default(),
            auth: AuthConfig::None,
            pre_request_script: None,
            post_response_script: None,
            test_script: None,
            response: None,
            loading: false,
            test_results: Vec::new(),
            cases: Vec::new(),
        }
    }
}

/// Extract `:name` segments from a URL path, preserving existing values.
pub fn parse_url_path_variables(url: &str, existing: &[KeyValuePair]) -> Vec<KeyValuePair> {
    let base = url.split('?').next().unwrap_or(url);
    base.split('/')
        .filter_map(|segment| segment.strip_prefix(':'))
        .filter(|key| !key.is_empty())
        .map(|key| {
            let value = existing
                .iter()
                .find(|pair| pair.key == key)
                .map(|pair| pair.value.clone())
                .unwrap_or_default();
            KeyValuePair::new(key, value)
        })
        .collect()
}

/// Replace `:name` path segments with values from the path variable table.
pub fn replace_url_path_variables(url: &str, pairs: &[KeyValuePair]) -> String {
    let (base, suffix) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let replaced = base
        .split('/')
        .map(|segment| {
            if let Some(key) = segment.strip_prefix(':') {
                match pairs.iter().find(|p| p.enabled && p.key == key) {
                    Some(pair) if !pair.value.is_empty() => pair.value.as_str(),
                    _ => segment,
                }
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    match suffix {
        Some(query) => format!("{replaced}?{query}"),
        None => replaced,
    }
}

impl Tab {
    /// Display label used in tab bar.
    pub fn label(&self) -> SharedString {
        let marker = if self.is_dirty { "• " } else { "" };
        SharedString::from(format!("{}{}", marker, self.name))
    }

    /// Build a Tab from a request file on disk.
    pub fn from_request_file(file_path: &str, collection_path: &str, file: &RequestFile) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: file.name.clone(),
            file_path: Some(file_path.to_string()),
            case_name: None,
            collection_path: Some(collection_path.to_string()),
            is_dirty: false,
            protocol: if file.is_graphql.unwrap_or(false) {
                TabProtocol::GraphQL
            } else {
                TabProtocol::Http
            },
            method: file.method,
            url: file.url.clone(),
            headers: from_ordered_map(&file.headers, &file.header_order),
            params: from_ordered_map(
                file.params.as_ref().unwrap_or(&HashMap::new()),
                &file.param_order,
            ),
            path_params: parse_url_path_variables(
                &file.url,
                &from_ordered_map(
                    file.path_params.as_ref().unwrap_or(&HashMap::new()),
                    &file.path_param_order,
                ),
            ),
            body: file.body.clone().map(|b| RequestBody {
                body_type: parse_body_type(&b.body_type),
                content: b.content,
                form_data: b.form_data,
                binary_path: b.binary_path,
            }).unwrap_or_default(),
            auth: file.auth.clone().unwrap_or_default(),
            pre_request_script: file.pre_request_script.clone(),
            post_response_script: file.post_response_script.clone(),
            test_script: file.tests.clone(),
            response: None,
            loading: false,
            test_results: Vec::new(),
            cases: file.cases.clone(),
        }
    }

    /// Apply a saved case to this tab.
    pub fn apply_case(&mut self, case: &RequestFile) {
        self.method = case.method;
        self.url = case.url.clone();
        self.headers = from_ordered_map(&case.headers, &case.header_order);
        self.params = from_ordered_map(
            case.params.as_ref().unwrap_or(&HashMap::new()),
            &case.param_order,
        );
        self.path_params = parse_url_path_variables(&self.url, &self.path_params);
        self.body = case.body.clone().map(|b| RequestBody {
            body_type: parse_body_type(&b.body_type),
            content: b.content,
            form_data: b.form_data,
            binary_path: b.binary_path,
        }).unwrap_or_default();
        self.auth = case.auth.clone().unwrap_or_default();
        self.pre_request_script = case.pre_request_script.clone();
        self.post_response_script = case.post_response_script.clone();
        self.test_script = case.tests.clone();
        self.is_dirty = true;
    }

    /// Convert this tab back to a request file for saving.
    pub fn to_request_file(&self) -> RequestFile {
        let (headers, header_order) = ordered_map(&self.headers);
        let (params, param_order) = ordered_map(&self.params);
        let (path_params, path_param_order) = ordered_map(&self.path_params);
        RequestFile {
            name: self.name.clone(),
            method: self.method,
            url: self.url.clone(),
            protocol: None,
            description: None,
            headers,
            header_order,
            auth: Some(self.auth.clone()),
            body: Some(RequestBodyFile {
                body_type: self.body.body_type.to_string(),
                content: self.body.content.clone(),
                form_data: self.body.form_data.clone(),
                binary_path: self.body.binary_path.clone(),
            }),
            params: Some(params),
            param_order,
            path_params: Some(path_params),
            path_param_order,
            pre_request_script: self.pre_request_script.clone(),
            post_response_script: self.post_response_script.clone(),
            tests: self.test_script.clone(),
            cases: self.cases.clone(),
            ..Default::default()
        }
    }
}

fn ordered_map(pairs: &[KeyValuePair]) -> (HashMap<String, String>, Vec<String>) {
    let mut map = HashMap::new();
    let mut order = Vec::new();
    for pair in pairs.iter().filter(|p| p.enabled && !p.key.trim().is_empty()) {
        if map.insert(pair.key.clone(), pair.value.clone()).is_none() {
            order.push(pair.key.clone());
        }
    }
    (map, order)
}

fn from_ordered_map(map: &HashMap<String, String>, order: &[String]) -> Vec<KeyValuePair> {
    let mut pairs = Vec::new();
    for key in order {
        if let Some(value) = map.get(key) {
            pairs.push(KeyValuePair::new(key, value));
        }
    }
    let mut rest: Vec<_> = map
        .iter()
        .filter(|(key, _)| !order.iter().any(|k| k == *key))
        .collect();
    rest.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in rest {
        pairs.push(KeyValuePair::new(key, value));
    }
    pairs
}

pub fn parse_body_type(s: &str) -> BodyType {
    match s.to_lowercase().as_str() {
        "json" => BodyType::Json,
        "xml" => BodyType::Xml,
        "form-data" | "formdata" => BodyType::FormData,
        "urlencoded" | "x-www-form-urlencoded" => BodyType::Urlencoded,
        "raw" | "text" => BodyType::Raw,
        "binary" => BodyType::Binary,
        _ => BodyType::None,
    }
}

impl std::fmt::Display for BodyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Json => "json",
            Self::Xml => "xml",
            Self::FormData => "form-data",
            Self::Urlencoded => "urlencoded",
            Self::Raw => "raw",
            Self::Binary => "binary",
            Self::None => "none",
        };
        write!(f, "{}", s)
    }
}

// ── Collection tree ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionConfig {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub defaults: CollectionDefaults,
}

fn default_version() -> u32 { 1 }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default = "default_true")]
    pub send_cookies: bool,
    #[serde(default = "default_true")]
    pub store_cookies: bool,
    #[serde(default)]
    pub persist_cookies: bool,
}

fn default_true() -> bool { true }

/// On-disk YAML representation of a request (`simple-get.yaml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFile {
    pub name: String,
    pub method: HttpMethod,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBodyFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_params: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_param_order: Vec<String>,
    #[serde(default, rename = "isGraphql", skip_serializing_if = "Option::is_none")]
    pub is_graphql: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_response_script: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<RequestFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestBodyFile {
    #[serde(rename = "type")]
    pub body_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub form_data: Vec<KeyValuePair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub order: Vec<String>,
}

/// A node in the collection tree (collection → folder → request → case).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CollectionNode {
    Collection {
        name: String,
        path: String,
        children: Vec<CollectionNode>,
    },
    Folder {
        name: String,
        path: String,
        children: Vec<CollectionNode>,
    },
    Request {
        name: String,
        method: HttpMethod,
        path: String,
        #[serde(default, rename = "isGraphql")]
        is_graphql: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<String>,
        #[serde(default)]
        cases: Vec<CollectionNode>,
    },
    Case {
        name: String,
        method: HttpMethod,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<String>,
    },
}

/// Kind of a node in the collection tree (for context menus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionNodeKind {
    Collection,
    Folder,
    Request,
    Case,
}

impl CollectionNode {
    pub fn kind(&self) -> CollectionNodeKind {
        match self {
            Self::Collection { .. } => CollectionNodeKind::Collection,
            Self::Folder { .. } => CollectionNodeKind::Folder,
            Self::Request { .. } => CollectionNodeKind::Request,
            Self::Case { .. } => CollectionNodeKind::Case,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Collection { name, .. }
            | Self::Folder { name, .. }
            | Self::Request { name, .. }
            | Self::Case { name, .. } => name,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Collection { path, .. }
            | Self::Folder { path, .. }
            | Self::Request { path, .. }
            | Self::Case { path, .. } => path,
        }
    }

    pub fn children(&self) -> &[CollectionNode] {
        match self {
            Self::Collection { children, .. } | Self::Folder { children, .. } => children,
            Self::Request { cases, .. } => cases,
            Self::Case { .. } => &[],
        }
    }

    /// Unique UI element id. Cases share their parent request's path, so the
    /// case name must be part of the id or clicks land on the parent request row.
    pub fn element_id(&self) -> String {
        match self {
            Self::Case { name, .. } => format!("node-{}#case={}", self.path(), name),
            _ => format!("node-{}", self.path()),
        }
    }
}

// ── Environment ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentFile {
    pub name: String,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub scope: EnvironmentScope,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentScope {
    #[default]
    Shared,
    Personal,
}

// ── History ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub method: String,
    pub url: String,
    pub status: Option<i32>,
    pub status_text: Option<String>,
    pub time_ms: Option<i64>,
    pub size_bytes: Option<i64>,
    pub timestamp: String,
    pub collection_path: Option<String>,
    pub request_name: Option<String>,
    pub request_json: String,
    pub response_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub id: String,
    pub level: ConsoleLevel,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub trash_path: String,
    pub kind: String,
    pub deleted_at: String,
}

// ── Activity view (left activity bar) ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityView {
    Collections,
    History,
    Environments,
    Trash,
    Search,
    Extensions,
}

impl ActivityView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Collections => "Collections",
            Self::History => "History",
            Self::Environments => "Environments",
            Self::Trash => "Trash",
            Self::Search => "Search",
            Self::Extensions => "Extensions",
        }
    }
}

// ── Response view tabs ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseView {
    Body,
    Headers,
    Cookies,
    Tests,
    Timing,
    Console,
    Code,
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_nodes_have_unique_element_ids() {
        let request = CollectionNode::Request {
            name: "主动查询".into(),
            method: HttpMethod::Post,
            path: "/collections/merc/req.yaml".into(),
            is_graphql: false,
            protocol: None,
            cases: vec![],
        };
        let case = CollectionNode::Case {
            name: "成功".into(),
            method: HttpMethod::Post,
            path: "/collections/merc/req.yaml".into(),
            protocol: None,
        };
        assert!(request.element_id() != case.element_id());
        assert!(case.element_id().contains("成功"));
    }

    #[test]
    fn url_path_variables_roundtrip() {
        let pairs = parse_url_path_variables(
            "https://api.test/users/:id/posts/:postId?q=1",
            &[],
        );
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].key, "id");
        assert_eq!(pairs[1].key, "postId");
        let values = vec![
            KeyValuePair::new("id", "42"),
            KeyValuePair::new("postId", "7"),
        ];
        let url = replace_url_path_variables(
            "https://api.test/users/:id/posts/:postId?q=1",
            &values,
        );
        assert_eq!(url, "https://api.test/users/42/posts/7?q=1");
    }

    #[test]
    fn auth_yaml_compatible_with_tauri() {
        // Existing Tauri request YAML files use these exact field names.
        let cases = [
            ("type: none\n", AuthConfig::None),
            ("type: bearer\ntoken: abc\n", AuthConfig::Bearer { token: "abc".into() }),
            (
                "type: api-key\nkey: X-Key\nvalue: secret\naddTo: query\n",
                AuthConfig::ApiKey { key: "X-Key".into(), value: "secret".into(), add_to: "query".into() },
            ),
            (
                "type: jwt-bearer\nsecret: s\nalgorithm: RS256\npayload: '{}'\nheaderPrefix: Token\n",
                AuthConfig::JwtBearer {
                    secret: "s".into(), algorithm: "RS256".into(),
                    payload: "{}".into(), header_prefix: "Token".into(),
                },
            ),
            (
                "type: oauth2\ngrantType: client_credentials\ntokenUrl: https://x/token\nclientId: id\nclientSecret: sec\nusePkce: false\n",
                AuthConfig::for_type("oauth2"),
            ),
        ];

        for (yaml, expected) in cases {
            let parsed: AuthConfig = serde_yaml::from_str(yaml).unwrap_or_else(|e| {
                panic!("failed to parse {yaml:?}: {e}")
            });
            assert_eq!(parsed.type_key(), expected.type_key(), "type mismatch for {yaml:?}");
            // Re-serialization must keep the tag field.
            let out = serde_yaml::to_string(&parsed).unwrap();
            assert!(out.contains("type:"), "missing type tag: {out}");
        }
    }

    #[test]
    fn preserves_param_order_on_save_reload() {
        let mut params = HashMap::new();
        params.insert("a".to_string(), "1".to_string());
        params.insert("b".to_string(), "2".to_string());
        let file = RequestFile {
            params: Some(params),
            param_order: vec!["b".to_string(), "a".to_string()],
            ..Default::default()
        };

        let tab = Tab::from_request_file("req.yaml", "col", &file);
        assert_eq!(tab.params[0].key, "b");
        assert_eq!(tab.params[1].key, "a");

        let saved = tab.to_request_file();
        assert_eq!(saved.param_order, vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn auth_for_type_builds_defaults() {
        assert_eq!(AuthConfig::for_type("nope").type_key(), "none");
        if let AuthConfig::JwtBearer { algorithm, header_prefix, .. } = AuthConfig::for_type("jwt-bearer") {
            assert_eq!(algorithm, "HS256");
            assert_eq!(header_prefix, "Bearer");
        } else {
            panic!("expected jwt-bearer");
        }
        if let AuthConfig::Oauth2 { callback_url, use_pkce, .. } = AuthConfig::for_type("oauth2") {
            assert_eq!(callback_url, "http://localhost:9876/callback");
            assert!(use_pkce);
        } else {
            panic!("expected oauth2");
        }
    }
}
