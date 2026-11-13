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
        .http1_only();
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
}
