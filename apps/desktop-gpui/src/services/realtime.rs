//! Real-time protocol clients — WebSocket and SSE.
//!
//! Migrated from the Tauri `ws_connect` / `sse_connect` commands. In Tauri the
//! backend pushed events to the JS layer via `app.emit`; here the background
//! Tokio tasks push events into an `mpsc` channel that the UI drains on a timer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};

use crate::models::KeyValuePair;

const MSG_QUEUE_SIZE: usize = 512;

/// A single real-time event delivered to the UI.
#[derive(Debug, Clone)]
pub struct RealtimeEvent {
    pub connection_id: String,
    pub kind: RealtimeEventKind,
}

#[derive(Debug, Clone)]
pub enum RealtimeEventKind {
    /// Connection lifecycle: "connecting" | "connected" | "disconnected".
    Status { state: &'static str, error: Option<String> },
    /// A WebSocket text frame.
    WsText { text: String, direction: &'static str },
    /// A binary frame, represented by its size (matches the Tauri UI).
    WsBinary { size: usize, direction: &'static str },
    /// An SSE event with optional `event:` / `id:` fields.
    Sse { data: String, event: Option<String>, id: Option<String> },
}

/// Handles owned by the UI for one live connection.
struct ConnectionHandle {
    outgoing: mpsc::Sender<String>,
    cancel: watch::Sender<bool>,
}

/// Parse one line of an SSE stream into a field. `None` means ignore (comment
/// or `retry:`), `Some(("data"|"event"|"id", value))` otherwise.
fn parse_sse_field(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_end_matches('\r');
    for prefix in ["data:", "event:", "id:"] {
        if let Some(value) = line.strip_prefix(prefix) {
            return Some((&prefix[..prefix.len() - 1], value.trim_start()));
        }
    }
    None
}

/// Manages all live WebSocket / SSE connections and fans their events out to
/// one shared channel. One instance is stored on `AppState`.
pub struct RealtimeManager {
    events_tx: mpsc::UnboundedSender<RealtimeEvent>,
    events_rx: mpsc::UnboundedReceiver<RealtimeEvent>,
    connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
}

impl Default for RealtimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RealtimeManager {
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            events_tx,
            events_rx,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Drain all pending events. Cheap to call on a UI timer.
    pub fn try_recv(&mut self) -> Vec<RealtimeEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            out.push(event);
        }
        out
    }

    pub fn is_connected(&self, id: &str) -> bool {
        self.connections
            .lock()
            .map(|c| c.contains_key(id))
            .unwrap_or(false)
    }

    pub fn disconnect(&self, id: &str) -> Result<(), String> {
        let handle = self
            .connections
            .lock()
            .map_err(|e| e.to_string())?
            .remove(id);
        if let Some(handle) = handle {
            let _ = handle.cancel.send(true);
        }
        Ok(())
    }

    pub fn disconnect_all(&self) {
        if let Ok(mut connections) = self.connections.lock() {
            for (_, handle) in connections.drain() {
                let _ = handle.cancel.send(true);
            }
        }
    }

    pub fn send(&self, id: &str, text: String) -> Result<(), String> {
        let handle = self
            .connections
            .lock()
            .map_err(|e| e.to_string())?
            .get(id)
            .map(|h| h.outgoing.clone());
        match handle {
            Some(tx) => tx.try_send(text).map_err(|e| e.to_string()),
            None => Err("Connection not found".to_string()),
        }
    }

    fn emit(&self, connection_id: &str, kind: RealtimeEventKind) {
        let _ = self.events_tx.send(RealtimeEvent {
            connection_id: connection_id.to_string(),
            kind,
        });
    }

    /// Connect a WebSocket with optional custom headers.
    pub fn ws_connect(
        &self,
        connection_id: String,
        url: String,
        headers: Vec<KeyValuePair>,
    ) -> Result<(), String> {
        {
            let connections = self.connections.lock().map_err(|e| e.to_string())?;
            if connections.contains_key(&connection_id) {
                return Err("Connection already exists".to_string());
            }
        }

        let (outgoing, mut outgoing_rx) = mpsc::channel::<String>(MSG_QUEUE_SIZE);
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        self.connections.lock().map_err(|e| e.to_string())?.insert(
            connection_id.clone(),
            ConnectionHandle {
                outgoing,
                cancel: cancel_tx,
            },
        );
        self.emit(
            &connection_id,
            RealtimeEventKind::Status {
                state: "connecting",
                error: None,
            },
        );

        let connections = Arc::clone(&self.connections);
        let events_tx = self.events_tx.clone();
        let status_tx = events_tx.clone();
        let emit_status = move |id: &str, state: &'static str, error: Option<String>| {
            let _ = status_tx.send(RealtimeEvent {
                connection_id: id.to_string(),
                kind: RealtimeEventKind::Status { state, error },
            });
        };

        crate::runtime::handle_spawn(async move {
            let mut request = match url.as_str().into_client_request() {
                Ok(r) => r,
                Err(e) => {
                    connections.lock().map(|mut c| c.remove(&connection_id)).ok();
                    emit_status(
                        &connection_id,
                        "disconnected",
                        Some(format!("Invalid URL: {e}")),
                    );
                    return;
                }
            };
            for header in &headers {
                if !header.enabled || header.key.is_empty() {
                    continue;
                }
                if let (Ok(name), Ok(value)) = (
                    tungstenite::http::header::HeaderName::try_from(header.key.as_str()),
                    tungstenite::http::header::HeaderValue::try_from(header.value.as_str()),
                ) {
                    request.headers_mut().insert(name, value);
                }
            }

            let stream = match tokio_tungstenite::connect_async(request).await {
                Ok((stream, _)) => stream,
                Err(e) => {
                    connections.lock().map(|mut c| c.remove(&connection_id)).ok();
                    emit_status(
                        &connection_id,
                        "disconnected",
                        Some(format!("Connection failed: {e}")),
                    );
                    return;
                }
            };
            emit_status(&connection_id, "connected", None);

            let (mut sink, mut source) = stream.split();
            loop {
                tokio::select! {
                    _ = cancel_rx.changed() => {
                        let _ = sink.close().await;
                        emit_status(&connection_id, "disconnected", None);
                        break;
                    }
                    Some(text) = outgoing_rx.recv() => {
                        if sink.send(WsMessage::Text(text.clone())).await.is_err() {
                            break;
                        }
                        let _ = events_tx.send(RealtimeEvent {
                            connection_id: connection_id.clone(),
                            kind: RealtimeEventKind::WsText {
                                text,
                                direction: "sent",
                            },
                        });
                    }
                    Some(message) = source.next() => {
                        match message {
                            Ok(WsMessage::Text(text)) => {
                                let _ = events_tx.send(RealtimeEvent {
                                    connection_id: connection_id.clone(),
                                    kind: RealtimeEventKind::WsText {
                                        text,
                                        direction: "received",
                                    },
                                });
                            }
                            Ok(WsMessage::Binary(bytes)) => {
                                let _ = events_tx.send(RealtimeEvent {
                                    connection_id: connection_id.clone(),
                                    kind: RealtimeEventKind::WsBinary {
                                        size: bytes.len(),
                                        direction: "received",
                                    },
                                });
                            }
                            Ok(WsMessage::Close(_)) => break,
                            Ok(_) => {}
                            Err(e) => {
                                emit_status(
                                    &connection_id,
                                    "disconnected",
                                    Some(format!("WebSocket error: {e}")),
                                );
                                break;
                            }
                        }
                    }
                }
            }
            connections.lock().map(|mut c| c.remove(&connection_id)).ok();
            emit_status(&connection_id, "disconnected", None);
        });
        Ok(())
    }

    /// Connect an SSE stream and parse `data:` / `event:` / `id:` lines.
    pub fn sse_connect(
        &self,
        connection_id: String,
        url: String,
        headers: Vec<KeyValuePair>,
    ) -> Result<(), String> {
        {
            let connections = self.connections.lock().map_err(|e| e.to_string())?;
            if connections.contains_key(&connection_id) {
                return Err("Connection already exists".to_string());
            }
        }

        let (outgoing, _outgoing_rx) = mpsc::channel::<String>(1);
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        self.connections.lock().map_err(|e| e.to_string())?.insert(
            connection_id.clone(),
            ConnectionHandle {
                outgoing,
                cancel: cancel_tx,
            },
        );
        self.emit(
            &connection_id,
            RealtimeEventKind::Status {
                state: "connecting",
                error: None,
            },
        );

        let connections = Arc::clone(&self.connections);
        let events_tx = self.events_tx.clone();
        let status_tx = events_tx.clone();
        let emit_status = move |id: &str, state: &'static str, error: Option<String>| {
            let _ = status_tx.send(RealtimeEvent {
                connection_id: id.to_string(),
                kind: RealtimeEventKind::Status { state, error },
            });
        };

        crate::runtime::handle_spawn(async move {
            let mut builder = reqwest::Client::new().get(&url);
            for header in &headers {
                if header.enabled && !header.key.is_empty() {
                    builder = builder.header(&header.key, &header.value);
                }
            }
            let response = match builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    connections.lock().map(|mut c| c.remove(&connection_id)).ok();
                    emit_status(
                        &connection_id,
                        "disconnected",
                        Some(format!("Connection failed: {e}")),
                    );
                    return;
                }
            };
            emit_status(&connection_id, "connected", None);

            let mut stream = response.bytes_stream();
            let mut data_lines: Vec<String> = Vec::new();
            let mut event_name: Option<String> = None;
            let mut event_id: Option<String> = None;

            loop {
                tokio::select! {
                    _ = cancel_rx.changed() => break,
                    chunk = stream.next() => {
                        let Some(chunk) = chunk else { break; };
                        let Ok(chunk) = chunk else { break; };
                        let text = String::from_utf8_lossy(&chunk);
                        for line in text.lines() {
                            if line.trim_end_matches('\r').is_empty() {
                                if !data_lines.is_empty() {
                                    let _ = events_tx.send(RealtimeEvent {
                                        connection_id: connection_id.clone(),
                                        kind: RealtimeEventKind::Sse {
                                            data: data_lines.join("\n"),
                                            event: event_name.take(),
                                            id: event_id.take(),
                                        },
                                    });
                                    data_lines.clear();
                                }
                                continue;
                            }
                            match parse_sse_field(line) {
                                Some(("data", value)) => data_lines.push(value.to_string()),
                                Some(("event", value)) => event_name = Some(value.to_string()),
                                Some(("id", value)) => event_id = Some(value.to_string()),
                                _ => {} // `retry:` and `:` comments are ignored.
                            }
                        }
                    }
                }
            }
            connections.lock().map(|mut c| c.remove(&connection_id)).ok();
            emit_status(&connection_id, "disconnected", None);
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_fields() {
        assert_eq!(parse_sse_field("data: hello"), Some(("data", "hello")));
        assert_eq!(parse_sse_field("data:{\r"), Some(("data", "{")));
        assert_eq!(parse_sse_field("event: message"), Some(("event", "message")));
        assert_eq!(parse_sse_field("id: 42"), Some(("id", "42")));
        assert_eq!(parse_sse_field("retry: 5000"), None);
        assert_eq!(parse_sse_field(": comment"), None);
    }

    #[test]
    fn manager_tracks_connections_and_events() {
        let mut manager = RealtimeManager::new();
        assert!(manager.try_recv().is_empty());
        assert!(!manager.is_connected("missing"));
        assert!(manager.send("missing", "hi".into()).is_err());
        assert!(manager.disconnect("missing").is_ok());
    }
}
