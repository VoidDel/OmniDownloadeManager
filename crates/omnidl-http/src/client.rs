//! The HTTP client of a download and how its failures map to task errors.

use std::error::Error as _;

use omnidl_core::{Error, ErrorCode, JobOptions, ProxyConfig, Result};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, ACCEPT_ENCODING, COOKIE, HeaderMap, HeaderName, HeaderValue, REFERER},
};

/// A client for one download with user agent `ua` (`ua::candidates`): it, the referer, cookies,
/// extra headers and proxy apply to every request. Connections are pooled up to the download's
/// connection count.
pub(crate) fn build_as(o: &JobOptions, ua: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    // Bytes on disk must match Content-Length: no compressed transfer.
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    if let Some(r) = &o.referer {
        headers.insert(REFERER, header_value("Referer", r)?);
    }
    if let Some(c) = &o.cookies {
        headers.insert(COOKIE, header_value("Cookie", c)?);
    }
    for (name, value) in &o.headers {
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| Error::invalid(format!("invalid header name {name:?}")))?;
        headers.insert(name.clone(), header_value(name.as_str(), value)?);
    }
    let mut b = Client::builder()
        .user_agent(ua.to_owned())
        .default_headers(headers)
        .connect_timeout(o.connect_timeout)
        .read_timeout(o.read_timeout)
        .redirect(redirects(o.allow_insecure_redirect))
        .pool_max_idle_per_host(o.connections as usize)
        .tcp_nodelay(true)
        .http1_only()
        // `User-Agent:`, as browsers and curl write header names, not hyper's `user-agent:`:
        // a server may tell a program by the lower case (Cloudflare's browser check refuses a
        // browser's user agent that comes with it, even with the clearance cookie).
        .http1_title_case_headers();
    b = match &o.proxy {
        // reqwest reads the system settings by default.
        ProxyConfig::System => b,
        ProxyConfig::None => b.no_proxy(),
        ProxyConfig::Custom { url } => b.proxy(
            reqwest::Proxy::all(url.trim())
                .map_err(|e| Error::invalid(format!("invalid proxy {url:?}: {e}")))?,
        ),
    };
    b.build()
        .map_err(|e| Error::internal(format!("HTTP client: {}", chain(&e))))
}

/// A redirect that was not followed: from `https://` to `http://`.
#[derive(Debug)]
struct InsecureRedirect(String);

impl std::fmt::Display for InsecureRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "redirected from https to {}", self.0)
    }
}

impl std::error::Error for InsecureRedirect {}

/// Whether a redirect from a page of scheme `from` to one of scheme `to` is followed: not from
/// an encrypted connection to a plain one (what was asked for over TLS would arrive, and
/// cookies go out, in the clear), unless the settings allow it.
fn redirect_allowed(from: &str, to: &str, allow_insecure: bool) -> bool {
    allow_insecure || !(from == "https" && to == "http")
}

/// Up to ten redirects, each checked by `redirect_allowed`.
fn redirects(allow_insecure: bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("too many redirects");
        }
        let from = attempt.previous().last().map_or("", |u| u.scheme());
        if redirect_allowed(from, attempt.url().scheme(), allow_insecure) {
            attempt.follow()
        } else {
            // The address without its query, which may carry a token.
            let mut to = attempt.url().clone();
            to.set_query(None);
            attempt.error(InsecureRedirect(to.to_string()))
        }
    })
}

fn header_value(name: &str, value: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(value.trim())
        .map_err(|_| Error::invalid(format!("invalid value for header {name}")))
}

/// A failed request or body read.
pub(crate) fn request_error(e: &reqwest::Error) -> Error {
    let message = chain(e);
    let code = if insecure_redirect(e) {
        ErrorCode::InsecureRedirect
    } else if e.is_timeout() {
        ErrorCode::Timeout
    } else if e.is_builder() {
        ErrorCode::InvalidUrl
    } else if e.is_redirect() {
        ErrorCode::HttpStatus
    } else if is_tls(&message) {
        ErrorCode::Tls
    } else {
        ErrorCode::Network
    };
    Error::new(code, message)
}

fn insecure_redirect(e: &reqwest::Error) -> bool {
    let mut source = e.source();
    while let Some(c) = source {
        if c.is::<InsecureRedirect>() {
            return true;
        }
        source = c.source();
    }
    false
}

/// An error status from the server.
pub(crate) fn status_error(status: StatusCode) -> Error {
    let code = match status.as_u16() {
        404 | 410 => ErrorCode::NotFound,
        401 | 407 => ErrorCode::AuthRequired,
        403 => ErrorCode::Forbidden,
        _ => ErrorCode::HttpStatus,
    };
    Error {
        code,
        message: format!("the server answered {status}"),
        status: Some(status.as_u16()),
    }
}

/// What a Cloudflare browser check adds to the message of the refusal it answers with
/// (`cf-mitigated: challenge`): a page the browser solves, which a program cannot.
const BROWSER_CHECK: &str = "Cloudflare browser check";

/// An error status from the server, telling a Cloudflare browser check apart.
pub(crate) fn response_error(r: &reqwest::Response) -> Error {
    let mut e = status_error(r.status());
    let challenge = r
        .headers()
        .get_all("cf-mitigated")
        .iter()
        .any(|v| v.as_bytes().eq_ignore_ascii_case(b"challenge"));
    if challenge {
        e.message = format!("{} ({BROWSER_CHECK})", e.message);
    }
    e
}

/// The error with its causes: reqwest's own message is only the outermost one.
fn chain(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut source = e.source();
    while let Some(c) = source {
        let m = c.to_string();
        if !s.contains(&m) {
            s.push_str(": ");
            s.push_str(&m);
        }
        source = c.source();
    }
    s
}

fn is_tls(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("certificate") || m.contains("tls") || m.contains("handshake")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_leave_https_for_http() {
        assert!(!redirect_allowed("https", "http", false));
        assert!(redirect_allowed("https", "http", true));
        for (from, to) in [("https", "https"), ("http", "https"), ("http", "http")] {
            assert!(redirect_allowed(from, to, false), "{from} to {to}");
        }
        // The error is told apart from any other redirect error.
        let e = Error::new(ErrorCode::InsecureRedirect, "x");
        assert!(!e.is_transient());
    }

    /// Header names go out as browsers and curl write them (`User-Agent:`), not in lower case.
    #[tokio::test]
    async fn writes_header_names_in_title_case() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut c, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = c.read(&mut buf).await.unwrap();
                assert!(n > 0, "the request ended early");
                request.extend_from_slice(&buf[..n]);
            }
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let o = JobOptions {
            referer: Some("http://example.com/page".into()),
            cookies: Some("a=1".into()),
            headers: [("x-custom-token".to_owned(), "t".to_owned())].into(),
            proxy: ProxyConfig::None,
            ..JobOptions::default()
        };
        let client = build_as(&o, "Mozilla/5.0 Test").unwrap();
        let r = client
            .get(format!("http://{addr}/file.bin"))
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let request = server.await.unwrap();
        for line in [
            "Host: ",
            "User-Agent: Mozilla/5.0 Test",
            "Accept: */*",
            "Accept-Encoding: identity",
            "Referer: http://example.com/page",
            "Cookie: a=1",
            "Range: bytes=0-0",
            "X-Custom-Token: t",
        ] {
            let wanted = format!("\r\n{line}");
            assert!(request.contains(&wanted), "no {line:?} in {request:?}");
        }
    }
}
