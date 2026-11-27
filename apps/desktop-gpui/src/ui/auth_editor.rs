//! Auth editor — ported from `components/request/request-panel.tsx::AuthEditor`.
//!
//! Supports all 11 auth types. Each field is an `InputState` that writes back to
//! the active tab's `AuthConfig` via a `set_field(key, value)` helper, mirroring
//! the React editor's field-level `onChange`.

use crate::models::AuthConfig;
use crate::services::WorkspaceService;
use crate::state::{AppState, TabsState, ToastKind};
use crate::theme;

use super::prelude::*;

const AUTH_TYPES: &[(&str, &str)] = &[
    ("none", "No Auth"),
    ("bearer", "Bearer Token"),
    ("basic", "Basic Auth"),
    ("api-key", "API Key"),
    ("oauth2", "OAuth 2.0"),
    ("digest", "Digest Auth"),
    ("aws-v4", "AWS Signature V4"),
    ("ntlm", "NTLM"),
    ("jwt-bearer", "JWT Bearer"),
    ("saml", "SAML Bearer"),
];

pub struct AuthEditor {
    tabs: Entity<TabsState>,
    app_state: Entity<AppState>,
    workspace: Entity<WorkspaceService>,
    token_status: Option<crate::services::oauth::OAuthTokenStatus>,
    subs: std::cell::RefCell<Vec<Subscription>>,
}

impl AuthEditor {
    pub fn new(
        tabs: Entity<TabsState>,
        app_state: Entity<AppState>,
        workspace: Entity<WorkspaceService>,
    ) -> Self {
        Self {
            tabs,
            app_state,
            workspace,
            token_status: None,
            subs: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Kick off token acquisition in the background and report via toast.
    fn get_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.tabs.read(cx).active_tab().map(|t| t.auth.clone()) else {
            return;
        };
        let oauth_store = self.workspace.read(cx).oauth_store();
        let app_state = self.app_state.clone();

        cx.spawn_in(window, async move |_, cx| {
            let token = crate::runtime::run_on_tokio(async move {
                oauth_store.access_token(&config).await
            })
            .await
            .unwrap_or_else(|e| Err(e));
            cx.update(|_window, cx| {
                match token {
                    Ok(access) => {
                        // Show a truncated token in the toast; do not log the full value.
                        let preview = if access.len() > 16 {
                            format!("{}…{}", &access[..8], &access[access.len() - 4..])
                        } else {
                            access.clone()
                        };
                        app_state.update(cx, |s, cx| {
                            s.show_toast(format!("OAuth token acquired: {preview}"), ToastKind::Success, cx);
                        });
                    }
                    Err(e) => {
                        app_state.update(cx, |s, cx| {
                            s.show_toast(format!("OAuth error: {e}"), ToastKind::Error, cx);
                        });
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn clear_token(&mut self, cx: &mut Context<Self>) {
        let Some(config) = self.tabs.read(cx).active_tab().map(|t| t.auth.clone()) else {
            return;
        };
        self.workspace.update(cx, |ws, cx| {
            ws.oauth_clear(&config);
            let _ = cx;
        });
        self.token_status = None;
        cx.notify();
    }

    fn set_type(&mut self, key: &str, cx: &mut Context<Self>) {
        let auth = AuthConfig::for_type(key);
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                tab.auth = auth;
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    /// Set a named string field on the current auth variant.
    fn set_field(&mut self, field: &str, value: String, cx: &mut Context<Self>) {
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                set_auth_field(&mut tab.auth, field, value);
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    /// Set a named boolean field on the current auth variant.
    fn set_bool_field(&mut self, field: &str, value: bool, cx: &mut Context<Self>) {
        self.tabs.update(cx, |state, cx| {
            if let Some(tab) = state.active_tab_mut() {
                set_auth_bool(&mut tab.auth, field, value);
                tab.is_dirty = true;
                cx.notify();
            }
        });
    }

    /// Build a labeled input row bound to a field. The input writes changes back
    /// to the tab through a weak reference to this editor.
    fn field_row(
        &mut self,
        label: &'static str,
        field: &'static str,
        value: String,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(value, window, cx);
            if secret {
                s = s.placeholder("••••••••");
            }
            s
        });

        let weak = cx.weak_entity();
        let _sub = cx.subscribe_in(&input, window, move |_, editor, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let v = editor.read(cx).value().to_string();
                let _ = weak.update(cx, |this, cx| this.set_field(field, v, cx));
            }
        });
        // Keep the subscription alive for the lifetime of the view.
        self.push_subscription(_sub);

        div()
            .flex()
            .flex_col()
            .gap_1()
            .mb_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(label)),
            )
            .child(div().child(Input::new(&input)))
            .into_any_element()
    }

    fn bool_row(
        &mut self,
        label: &'static str,
        field: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        div()
            .flex()
            .items_center()
            .gap_2()
            .mb_2()
            .child(
                Checkbox::new(SharedString::from(format!("auth-bool-{}", field)))
                    .checked(checked)
                    .on_click(move |checked, _, cx| {
                        let checked = *checked;
                        let _ = weak.update(cx, |this, cx| this.set_bool_field(field, checked, cx));
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(SharedString::from(label)),
            )
            .into_any_element()
    }

    fn push_subscription(&self, sub: Subscription) {
        self.subs.borrow_mut().push(sub);
    }
}

impl Render for AuthEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let auth = self
            .tabs
            .read(cx)
            .active_tab()
            .map(|t| t.auth.clone())
            .unwrap_or_default();

        let mut col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .p_3()
            .gap_1()
            .overflow_scrollbar();

        // Auth type selector (buttons, like the body-type selector).
        let current_type = auth.type_key();
        let mut type_row = div().flex().flex_wrap().gap_1().mb_3();
        for (key, label) in AUTH_TYPES {
            let is_active = *key == current_type;
            let key_val = *key;
            type_row = type_row.child(
                div()
                    .id(SharedString::from(format!("auth-type-{}", key)))
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .text_xs()
                    .cursor_pointer()
                    .bg(if is_active { theme::accent() } else { theme::bg_surface() })
                    .text_color(if is_active { gpui_kit::white() } else { theme::text_muted() })
                    .child(SharedString::from(*label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_type(key_val, cx);
                    })),
            );
        }
        col = col.child(type_row);

        match &auth {
            AuthConfig::None => {
                col = col.child(
                    div().text_sm().text_color(theme::text_muted())
                        .child(SharedString::from("This request does not use authorization.")),
                );
            }
            AuthConfig::Bearer { token } => {
                col = col.child(self.field_row("Token", "token", token.clone(), true, _window, cx));
            }
            AuthConfig::Basic { username, password } => {
                col = col.child(self.field_row("Username", "username", username.clone(), false, _window, cx));
                col = col.child(self.field_row("Password", "password", password.clone(), true, _window, cx));
            }
            AuthConfig::ApiKey { key, value, add_to } => {
                col = col.child(self.field_row("Key", "key", key.clone(), false, _window, cx));
                col = col.child(self.field_row("Value", "value", value.clone(), true, _window, cx));
                col = col.child(self.enum_row("Add to", "addTo", add_to.clone(), &["header", "query", "cookie"], cx));
            }
            AuthConfig::Oauth2 {
                grant_type, auth_url, token_url, client_id, client_secret, scope,
                callback_url, username, password, use_pkce,
            } => {
                col = col.child(self.enum_row(
                    "Grant Type", "grantType", grant_type.clone(),
                    &["authorization_code", "client_credentials", "implicit", "password"], cx,
                ));
                if grant_type == "authorization_code" || grant_type == "implicit" {
                    col = col.child(self.field_row("Auth URL", "authUrl", auth_url.clone(), false, _window, cx));
                }
                if grant_type != "implicit" {
                    col = col.child(self.field_row("Token URL", "tokenUrl", token_url.clone(), false, _window, cx));
                }
                col = col.child(self.field_row("Client ID", "clientId", client_id.clone(), false, _window, cx));
                col = col.child(self.field_row("Client Secret", "clientSecret", client_secret.clone(), true, _window, cx));
                col = col.child(self.field_row("Scope", "scope", scope.clone(), false, _window, cx));
                if grant_type == "password" {
                    col = col.child(self.field_row("Username", "username", username.clone(), false, _window, cx));
                    col = col.child(self.field_row("Password", "password", password.clone(), true, _window, cx));
                }
                if grant_type == "authorization_code" || grant_type == "implicit" {
                    col = col.child(self.field_row("Callback URL", "callbackUrl", callback_url.clone(), false, _window, cx));
                }
                if grant_type == "authorization_code" {
                    col = col.child(self.bool_row("Use PKCE", "usePkce", *use_pkce, cx));
                }

                // Token status + Get Token / Clear Token actions.
                let status = self.workspace.read(cx).oauth_status(&auth);
                let status_text = if status.has_token {
                    if status.is_expired {
                        "Token expired"
                    } else {
                        "Token valid"
                    }
                } else {
                    "No token"
                };
                let status_color = if status.has_token && !status.is_expired {
                    theme::success()
                } else {
                    theme::text_muted()
                };
                col = col.child(
                    div().flex().items_center().gap_2().mt_2().mb_2()
                        .child(
                            div().text_xs().text_color(status_color)
                                .child(SharedString::from(status_text)),
                        )
                        .child(
                            div().id("oauth-get-token").px_3().py_1().rounded_lg()
                                .text_xs().cursor_pointer().bg(theme::accent())
                                .text_color(gpui_kit::white())
                                .child(SharedString::from("Get Token"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.get_token(window, cx);
                                })),
                        )
                        .child(
                            div().id("oauth-clear-token").px_3().py_1().rounded_lg()
                                .text_xs().cursor_pointer().border_1()
                                .border_color(theme::border())
                                .text_color(theme::text_muted())
                                .child(SharedString::from("Clear Token"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_token(cx);
                                })),
                        ),
                );
            }
            AuthConfig::Digest { username, password } => {
                col = col.child(self.field_row("Username", "username", username.clone(), false, _window, cx));
                col = col.child(self.field_row("Password", "password", password.clone(), true, _window, cx));
            }
            AuthConfig::AwsV4 { access_key, secret_key, region, service, session_token } => {
                col = col.child(self.field_row("Access Key", "accessKey", access_key.clone(), false, _window, cx));
                col = col.child(self.field_row("Secret Key", "secretKey", secret_key.clone(), true, _window, cx));
                col = col.child(self.field_row("Region", "region", region.clone(), false, _window, cx));
                col = col.child(self.field_row("Service", "service", service.clone(), false, _window, cx));
                col = col.child(self.field_row("Session Token", "sessionToken", session_token.clone(), true, _window, cx));
            }
            AuthConfig::Ntlm { username, password, domain, workstation } => {
                col = col.child(self.field_row("Username", "username", username.clone(), false, _window, cx));
                col = col.child(self.field_row("Password", "password", password.clone(), true, _window, cx));
                col = col.child(self.field_row("Domain", "domain", domain.clone(), false, _window, cx));
                col = col.child(self.field_row("Workstation", "workstation", workstation.clone(), false, _window, cx));
            }
            AuthConfig::JwtBearer { secret, algorithm, payload, header_prefix } => {
                col = col.child(self.enum_row(
                    "Algorithm", "algorithm", algorithm.clone(),
                    &["HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "ES256", "ES384"], cx,
                ));
                col = col.child(self.field_row("Secret / Key", "secret", secret.clone(), true, _window, cx));
                col = col.child(self.field_row("Payload (JSON)", "payload", payload.clone(), false, _window, cx));
                col = col.child(self.field_row("Header Prefix", "headerPrefix", header_prefix.clone(), false, _window, cx));
            }
            AuthConfig::Saml {
                idp_url, entity_id, assertion_consumer_url, certificate, name_id_format, saml_token,
            } => {
                col = col.child(self.field_row("IdP URL", "idpUrl", idp_url.clone(), false, _window, cx));
                col = col.child(self.field_row("Entity ID", "entityId", entity_id.clone(), false, _window, cx));
                col = col.child(self.field_row("Assertion Consumer URL", "assertionConsumerUrl", assertion_consumer_url.clone(), false, _window, cx));
                col = col.child(self.field_row("Certificate", "certificate", certificate.clone(), false, _window, cx));
                col = col.child(self.field_row("NameID Format", "nameIdFormat", name_id_format.clone(), false, _window, cx));
                col = col.child(self.field_row("SAML Token", "samlToken", saml_token.clone(), true, _window, cx));
            }
        }

        col
    }
}

impl AuthEditor {
    /// Compact enum selector rendered as a row of small buttons.
    fn enum_row(
        &mut self,
        label: &'static str,
        field: &'static str,
        value: String,
        options: &[&str],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div().flex().flex_col().gap_1().mb_2();
        row = row.child(
            div().text_xs().text_color(theme::text_muted())
                .child(SharedString::from(label)),
        );
        let mut buttons = div().flex().flex_wrap().gap_1();
        for opt in options {
            let is_active = opt == &value.as_str();
            let field_val = field;
            let opt_val = opt.to_string();
            buttons = buttons.child(
                div()
                    .id(SharedString::from(format!("auth-enum-{}-{}", field, opt)))
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .text_xs()
                    .cursor_pointer()
                    .bg(if is_active { theme::bg_surface() } else { transparent_black() })
                    .text_color(if is_active { theme::accent() } else { theme::text_muted() })
                    .child(SharedString::from(*opt))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_field(field_val, opt_val.clone(), cx);
                    })),
            );
        }
        row.child(buttons).into_any_element()
    }
}

/// Mutate a string field on an AuthConfig variant, by field key.
fn set_auth_field(auth: &mut AuthConfig, field: &str, value: String) {
    match auth {
        AuthConfig::Bearer { token } => if field == "token" { *token = value },
        AuthConfig::Basic { username, password } => match field {
            "username" => *username = value,
            "password" => *password = value,
            _ => {}
        },
        AuthConfig::ApiKey { key, value: v, add_to } => match field {
            "key" => *key = value,
            "value" => *v = value,
            "addTo" => *add_to = value,
            _ => {}
        },
        AuthConfig::Oauth2 {
            grant_type, auth_url, token_url, client_id, client_secret, scope,
            callback_url, username, password, ..
        } => match field {
            "grantType" => *grant_type = value,
            "authUrl" => *auth_url = value,
            "tokenUrl" => *token_url = value,
            "clientId" => *client_id = value,
            "clientSecret" => *client_secret = value,
            "scope" => *scope = value,
            "callbackUrl" => *callback_url = value,
            "username" => *username = value,
            "password" => *password = value,
            _ => {}
        },
        AuthConfig::Digest { username, password } => match field {
            "username" => *username = value,
            "password" => *password = value,
            _ => {}
        },
        AuthConfig::AwsV4 { access_key, secret_key, region, service, session_token } => match field {
            "accessKey" => *access_key = value,
            "secretKey" => *secret_key = value,
            "region" => *region = value,
            "service" => *service = value,
            "sessionToken" => *session_token = value,
            _ => {}
        },
        AuthConfig::Ntlm { username, password, domain, workstation } => match field {
            "username" => *username = value,
            "password" => *password = value,
            "domain" => *domain = value,
            "workstation" => *workstation = value,
            _ => {}
        },
        AuthConfig::JwtBearer { secret, algorithm, payload, header_prefix } => match field {
            "secret" => *secret = value,
            "algorithm" => *algorithm = value,
            "payload" => *payload = value,
            "headerPrefix" => *header_prefix = value,
            _ => {}
        },
        AuthConfig::Saml {
            idp_url, entity_id, assertion_consumer_url, certificate, name_id_format, saml_token,
        } => match field {
            "idpUrl" => *idp_url = value,
            "entityId" => *entity_id = value,
            "assertionConsumerUrl" => *assertion_consumer_url = value,
            "certificate" => *certificate = value,
            "nameIdFormat" => *name_id_format = value,
            "samlToken" => *saml_token = value,
            _ => {}
        },
        AuthConfig::None => {}
    }
}

fn set_auth_bool(auth: &mut AuthConfig, field: &str, value: bool) {
    if let AuthConfig::Oauth2 { use_pkce, .. } = auth {
        if field == "usePkce" {
            *use_pkce = value;
        }
    }
}
