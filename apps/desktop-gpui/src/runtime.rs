//! Tokio runtime bridge.
//!
//! GPUI runs async tasks on its own executor, which has no Tokio reactor.
//! Networking crates (`reqwest`, `axum`, `tokio-tungstenite`) require a Tokio
//! reactor, so all such work is executed on a dedicated Tokio runtime and the
//! result is delivered back through a `tokio::sync::oneshot` channel that the
//! GPUI task can await.

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Initialize the background Tokio runtime. Call once at startup.
pub fn init() {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("apiark-tokio")
            .build()
            .expect("Failed to create Tokio runtime")
    });
}

fn handle() -> tokio::runtime::Handle {
    RUNTIME
        .get()
        .expect("Tokio runtime not initialized — call runtime::init() first")
        .handle()
        .clone()
}

/// Run a future on the Tokio runtime and await its result from a GPUI task.
///
/// `F` must be `Send + 'static` (it runs on a Tokio worker thread) and `T`
/// must be `Send + 'static` (it crosses back via oneshot).
pub async fn run_on_tokio<F, T>(future: F) -> Result<T, String>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle().spawn(async move {
        let result = future.await;
        let _ = tx.send(result);
    });
    rx.await.map_err(|_| "Tokio task was cancelled".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn tokio_reactor_is_available_for_reqwest() {
        // This is the exact failure mode that crashed Send before the bridge:
        // reqwest::Client builder requires an active Tokio reactor.
        let _client = reqwest::Client::new();
    }

    #[test]
    fn run_on_tokio_delivers_result() {
        init();
        let value = handle().block_on(run_on_tokio(async { 42 })).unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn reqwest_send_works_through_bridge() {
        // Reproduces the Send button path: reqwest on the GPUI executor used to
        // panic with "there is no reactor running". Through the bridge it works.
        init();
        let result = handle().block_on(run_on_tokio(async {
            let client = reqwest::Client::new();
            match client.get("http://127.0.0.1:1").send().await {
                Ok(r) => Ok(r.status().as_u16()),
                Err(e) => Err(e.to_string()),
            }
        }));
        // A connection error is expected (nothing listening on :1); the key is
        // that we get a Result back instead of a panic.
        assert!(result.is_ok(), "bridge should not panic: {result:?}");
    }
}
