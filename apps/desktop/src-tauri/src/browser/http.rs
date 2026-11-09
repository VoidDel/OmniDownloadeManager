//! The local HTTP fallback channel (ADR-002), off by default: for browsers whose Native
//! Messaging cannot reach the host (Snap, Flatpak). `POST /v1/request` on 127.0.0.1 with the same
//! JSON as the pipe, `Authorization: Bearer <token>`, and an extension `Origin` (or none): web
//! pages cannot use it. The user pairs once by copying `omnidl:<port>:<token>` from the settings
//! into the extension's options.

use std::{net::Ipv4Addr, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use omnidl_ipc::{ErrorCode, Response};
use tauri::AppHandle;
use tokio::sync::oneshot;

use super::handler;

/// A running server; dropping `stop` (or sending on it) shuts it down.
pub struct Running {
    pub port: u16,
    pub stop: oneshot::Sender<()>,
}

#[derive(Clone)]
struct Ctx {
    app: AppHandle,
    token: Arc<str>,
}

/// A new pairing token (244 random bits).
pub fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn pairing_code(port: u16, token: &str) -> String {
    format!("omnidl:{port}:{token}")
}

/// Listens on `port` (or any free port when it is 0 or taken).
pub async fn start(app: AppHandle, port: u16, token: String) -> std::io::Result<Running> {
    let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(l) => l,
        Err(e) if port == 0 => return Err(e),
        Err(e) => {
            tracing::warn!("HTTP bridge port {port}: {e}; using another");
            tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?
        }
    };
    let port = listener.local_addr()?.port();
    let router = Router::new()
        .route("/v1/request", post(request))
        .with_state(Ctx {
            app,
            token: token.into(),
        });
    let (stop, stopped) = oneshot::channel::<()>();
    tauri::async_runtime::spawn(async move {
        let r = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
        if let Err(e) = r {
            tracing::warn!("HTTP bridge: {e}");
        }
    });
    tracing::info!("HTTP bridge on 127.0.0.1:{port}");
    Ok(Running { port, stop })
}

/// Requests from extension pages, or without an Origin (a background fetch may send none).
fn allowed_origin(headers: &HeaderMap) -> bool {
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(o) => [
            "chrome-extension://",
            "moz-extension://",
            "safari-web-extension://",
        ]
        .iter()
        .any(|p| o.starts_with(p)),
    }
}

/// Compares in constant time (the token is a secret).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn request(State(ctx): State<Ctx>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let json = [(header::CONTENT_TYPE, "application/json")];
    if !allowed_origin(&headers) {
        let r = Response::err("", ErrorCode::Unauthorized, "only extensions may call this");
        return (StatusCode::FORBIDDEN, json, r.to_bytes());
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !same(token.as_bytes(), ctx.token.as_bytes()) {
        let r = Response::err("", ErrorCode::Unauthorized, "wrong pairing code");
        return (StatusCode::UNAUTHORIZED, json, r.to_bytes());
    }
    let reply = handler::handle_frame(ctx.app.clone(), body.to_vec()).await;
    (StatusCode::OK, json, reply)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn checks_origin_and_token() {
        let mut h = HeaderMap::new();
        assert!(allowed_origin(&h));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("chrome-extension://abc"),
        );
        assert!(allowed_origin(&h));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!allowed_origin(&h));
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
        let t = new_token();
        assert_eq!(t.len(), 64);
        assert_eq!(pairing_code(1234, "x"), "omnidl:1234:x");
    }
}
