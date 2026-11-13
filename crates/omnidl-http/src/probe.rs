//! Probing (docs/development-plan.md §4.2.1) and naming (§4.2.2).
//!
//! `HEAD` first. When it is refused or does not tell length and range support, a
//! `GET` with `Range: bytes=0-0`: a 206 answer gives the length in `Content-Range` and proves
//! ranges work; a 200 means they do not.
//!
//! A length of 0 counts as unknown: some servers (webmail attachments, for one) answer `HEAD`
//! or a ranged `GET` with an empty body and the file only to a plain `GET`. The download then
//! reads the body to its end, which an empty file ends at once.

use omnidl_core::{Error, ErrorCode, Result, filename};
use reqwest::{
    Client, Method, Response, StatusCode,
    header::{
        ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
        HeaderMap, LAST_MODIFIED, RANGE,
    },
};
use url::Url;

use crate::{
    auth::Auth,
    client::{request_error, status_error},
};

/// What the server says about a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Probe {
    pub final_url: Url,
    pub total: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    /// From `Content-Disposition`.
    pub disposition_name: Option<String>,
}

impl Probe {
    /// The file name: `Content-Disposition`, else the last segment of the final URL (then of
    /// the original one), with an extension from `Content-Type` when it has none.
    pub fn file_name(&self, original: &Url) -> String {
        let name = self
            .disposition_name
            .as_deref()
            .map(filename::sanitize)
            .or_else(|| filename::from_url(self.final_url.as_str()))
            .or_else(|| filename::from_url(original.as_str()));
        let Some(name) = name else {
            let ext = self.content_type.as_deref().and_then(extension_for);
            return format!("download_{}{}", omnidl_core::now_ms(), ext.unwrap_or(""));
        };
        match self.content_type.as_deref().and_then(extension_for) {
            Some(ext) if filename::split_extension(&name).1.is_empty() => format!("{name}{ext}"),
            _ => name,
        }
    }
}

pub(crate) async fn probe(client: &Client, auth: &Auth, url: &Url) -> Result<Probe> {
    let head = match auth.send(client, Method::HEAD, url, |r| r).await {
        Ok(r) => Some(r),
        // A network failure fails the GET too; a server that mishandles HEAD may not.
        Err(e) if e.is_timeout() || e.is_connect() => return Err(request_error(&e)),
        Err(_) => None,
    };
    let from_head = head.filter(|r| r.status().is_success()).map(|r| {
        let h = r.headers();
        let total = length(h, CONTENT_LENGTH);
        let ranges = header_str(h, ACCEPT_RANGES).is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
        (describe(&r, total, ranges && total.is_some()), ranges)
    });
    if let Some((p, true)) = &from_head
        && p.total.is_some()
    {
        return Ok(p.clone());
    }
    // Ask for one byte to learn the length and whether ranges work.
    let r = match auth
        .send(client, Method::GET, url, |r| r.header(RANGE, "bytes=0-0"))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return match from_head {
                Some((p, _)) => Ok(p),
                None => Err(request_error(&e)),
            };
        }
    };
    let status = r.status();
    match status {
        StatusCode::PARTIAL_CONTENT => {
            let total = header_str(r.headers(), CONTENT_RANGE).and_then(content_range_total);
            Ok(describe(&r, total, total.is_some()))
        }
        // An empty file, or a server that will not range: read the body to its end.
        StatusCode::RANGE_NOT_SATISFIABLE => {
            let total = header_str(r.headers(), CONTENT_RANGE)
                .and_then(content_range_total)
                .filter(|&n| n > 0);
            Ok(describe(&r, total, false))
        }
        s if s.is_success() => {
            let total = length(r.headers(), CONTENT_LENGTH);
            Ok(describe(&r, total, false))
        }
        s => match from_head {
            Some((p, _)) => Ok(p),
            None => Err(status_error(s)),
        },
    }
    // The ranged GET's body (one byte, or the whole file on a 200) is dropped unread, which
    // closes that connection.
}

fn describe(r: &Response, total: Option<u64>, resumable: bool) -> Probe {
    let h = r.headers();
    Probe {
        final_url: r.url().clone(),
        total,
        resumable,
        etag: header_str(h, ETAG).map(str::to_owned),
        last_modified: header_str(h, LAST_MODIFIED).map(str::to_owned),
        content_type: header_str(h, CONTENT_TYPE).map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        }),
        disposition_name: h
            .get(CONTENT_DISPOSITION)
            .and_then(|v| disposition_name(v.as_bytes())),
    }
}

fn header_str(h: &HeaderMap, name: reqwest::header::HeaderName) -> Option<&str> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// A length header; 0 counts as unknown (see the module docs).
fn length(h: &HeaderMap, name: reqwest::header::HeaderName) -> Option<u64> {
    header_str(h, name)
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
}

/// `bytes 0-0/1234` → 1234; `bytes */1234` → 1234; `bytes 0-0/*` → unknown.
pub(crate) fn content_range_total(v: &str) -> Option<u64> {
    v.rsplit_once('/')?.1.trim().parse().ok()
}

/// `bytes 100-199/1234` → (100, 199).
pub(crate) fn content_range_start(v: &str) -> Option<u64> {
    let range = v.trim().strip_prefix("bytes")?.trim_start();
    range.split(['-', '/']).next()?.trim().parse().ok()
}

/// The file name of a `Content-Disposition` header (RFC 6266): `filename*` (RFC 5987,
/// `UTF-8''…` percent-encoded) wins over `filename`. Raw UTF-8 and percent-encoded `filename`
/// values, which browsers accept, are accepted too.
pub(crate) fn disposition_name(raw: &[u8]) -> Option<String> {
    let value = match std::str::from_utf8(raw) {
        Ok(s) => s.to_owned(),
        // Latin-1, as HTTP header bytes are defined.
        Err(_) => raw.iter().map(|&b| char::from(b)).collect(),
    };
    let mut plain = None;
    let mut extended = None;
    for param in split_params(&value).into_iter().skip(1) {
        let Some((key, val)) = param.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        if key == "filename*" {
            extended = decode_ext_value(val);
        } else if key == "filename" {
            let v = unquote(val);
            let v = if v.contains('%') {
                filename::percent_decode(&v)
            } else {
                v
            };
            plain = Some(v);
        }
    }
    extended
        .or(plain)
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or("").to_owned())
        .filter(|n| !n.trim().is_empty())
}

/// Splits on `;` outside quotes.
fn split_params(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut escaped = false;
    for c in s.chars() {
        let cur = out.last_mut().expect("never empty");
        match c {
            _ if escaped => {
                cur.push(c);
                escaped = false;
            }
            '\\' if quoted => {
                cur.push(c);
                escaped = true;
            }
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            ';' if !quoted => out.push(String::new()),
            _ => cur.push(c),
        }
    }
    out
}

fn unquote(v: &str) -> String {
    match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            out
        }
        None => v.to_owned(),
    }
}

/// `UTF-8'en'%E2%82%AC.txt` → `€.txt`.
fn decode_ext_value(v: &str) -> Option<String> {
    let v = unquote(v);
    let mut parts = v.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _lang = parts.next()?;
    let encoded = parts.next()?;
    if charset == "utf-8" || charset.is_empty() {
        Some(filename::percent_decode(encoded))
    } else {
        // ISO-8859-1: each byte is a code point.
        Some(
            filename::percent_decode_bytes(encoded)
                .into_iter()
                .map(char::from)
                .collect(),
        )
    }
}

/// An extension for common download types when the name has none.
fn extension_for(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "application/pdf" => ".pdf",
        "application/zip" | "application/x-zip-compressed" => ".zip",
        "application/x-7z-compressed" => ".7z",
        "application/vnd.rar" | "application/x-rar-compressed" => ".rar",
        "application/gzip" | "application/x-gzip" => ".gz",
        "application/x-tar" => ".tar",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => ".exe",
        "application/x-msi" => ".msi",
        "application/x-iso9660-image" => ".iso",
        "application/x-apple-diskimage" => ".dmg",
        "application/vnd.android.package-archive" => ".apk",
        "application/vnd.debian.binary-package" => ".deb",
        "application/json" => ".json",
        "text/html" => ".html",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/x-matroska" => ".mkv",
        "audio/mpeg" => ".mp3",
        "audio/flac" => ".flac",
        "audio/ogg" => ".ogg",
        _ => return None,
    })
}

/// Whether the remote file is no longer the one partly downloaded: only validators the server
/// sent both times are compared.
pub(crate) fn changed(
    p: &Probe,
    total: Option<u64>,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Option<String> {
    if let (Some(a), Some(b)) = (total, p.total)
        && a != b
    {
        return Some(format!("size changed from {a} to {b} bytes"));
    }
    if let (Some(a), Some(b)) = (etag, p.etag.as_deref())
        && a != b
    {
        return Some(format!("ETag changed from {a} to {b}"));
    }
    if let (Some(a), Some(b)) = (last_modified, p.last_modified.as_deref())
        && a != b
    {
        return Some(format!("Last-Modified changed from {a} to {b}"));
    }
    None
}

pub(crate) fn remote_changed(detail: String) -> Error {
    Error::new(ErrorCode::RemoteChanged, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_content_disposition() {
        let d = |s: &str| disposition_name(s.as_bytes());
        assert_eq!(
            d("attachment; filename=\"a b.zip\"").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(
            d("attachment; filename=plain.txt").as_deref(),
            Some("plain.txt")
        );
        assert_eq!(
            d("attachment; filename=\"fallback.bin\"; filename*=UTF-8''%E6%B5%8B%E8%AF%95.bin")
                .as_deref(),
            Some("测试.bin")
        );
        assert_eq!(
            d("attachment; filename*=utf-8'en'%E2%82%AC%20rates.txt").as_deref(),
            Some("€ rates.txt")
        );
        assert_eq!(
            d("attachment; filename*=iso-8859-1''%A3%20rates.txt").as_deref(),
            Some("£ rates.txt")
        );
        assert_eq!(
            d("attachment; filename=\"a\\\"b;c.txt\"").as_deref(),
            Some("a\"b;c.txt")
        );
        assert_eq!(
            d("inline; filename=\"/etc/passwd\"").as_deref(),
            Some("passwd")
        );
        assert_eq!(
            d("attachment; filename=\"%E6%96%87.pdf\"").as_deref(),
            Some("文.pdf")
        );
        assert_eq!(
            disposition_name("attachment; filename=\"文档.pdf\"".as_bytes()).as_deref(),
            Some("文档.pdf")
        );
        assert_eq!(d("attachment"), None);
        assert_eq!(d("attachment; filename=\"\""), None);
    }

    #[test]
    fn reads_content_range() {
        assert_eq!(content_range_total("bytes 0-0/1234"), Some(1234));
        assert_eq!(content_range_total("bytes */0"), Some(0));
        assert_eq!(content_range_total("bytes 0-0/*"), None);
        assert_eq!(content_range_start("bytes 100-199/1234"), Some(100));
        assert_eq!(content_range_start("bytes */1234"), None);
    }

    #[test]
    fn names_the_file() {
        let url = Url::parse("https://example.com/get?id=5").unwrap();
        let mut p = Probe {
            final_url: Url::parse("https://cdn.example.com/files/report").unwrap(),
            total: None,
            resumable: false,
            etag: None,
            last_modified: None,
            content_type: Some("application/pdf".into()),
            disposition_name: None,
        };
        assert_eq!(p.file_name(&url), "report.pdf", "extension from the type");
        p.disposition_name = Some("Q3 report.pdf".into());
        assert_eq!(p.file_name(&url), "Q3 report.pdf");
        p.disposition_name = None;
        p.final_url = Url::parse("https://cdn.example.com/").unwrap();
        assert_eq!(p.file_name(&url), "get.pdf", "the original URL");
        let bare = Url::parse("https://example.com/").unwrap();
        assert!(p.file_name(&bare).starts_with("download_"));
    }

    #[test]
    fn notices_remote_changes() {
        let p = Probe {
            final_url: Url::parse("https://e.com/a").unwrap(),
            total: Some(10),
            resumable: true,
            etag: Some("\"2\"".into()),
            last_modified: None,
            content_type: None,
            disposition_name: None,
        };
        assert!(changed(&p, Some(10), Some("\"2\""), Some("x")).is_none());
        assert!(changed(&p, Some(11), None, None).is_some());
        assert!(changed(&p, None, Some("\"1\""), None).is_some());
        assert!(changed(&p, None, None, None).is_none());
    }
}
