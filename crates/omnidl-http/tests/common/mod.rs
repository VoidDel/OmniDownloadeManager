//! A local HTTP server for the engine tests. Content is generated from the offset, so any range
//! of any size can be served and checked without storing files.

#![allow(dead_code)]

use std::{
    net::SocketAddr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use bytes::Bytes;
use parking_lot::Mutex;

/// The byte at `offset` of every served file.
pub fn byte_at(offset: u64) -> u8 {
    let x = offset.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((x >> 56) as u8) ^ (offset as u8)
}

pub fn content(size: u64) -> Vec<u8> {
    (0..size).map(byte_at).collect()
}

/// Asserts the file at `path` is the served content of `size` bytes (streamed: files can be
/// large).
pub fn assert_content(path: &Path, size: u64) {
    use std::io::Read;
    let mut f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(f.metadata().unwrap().len(), size, "{}", path.display());
    let mut buf = vec![0u8; 8 << 20];
    let mut offset = 0u64;
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        if let Some(i) = buf[..n]
            .iter()
            .enumerate()
            .position(|(i, &b)| b != byte_at(offset + i as u64))
        {
            panic!("{}: wrong byte at {}", path.display(), offset + i as u64);
        }
        offset += n as u64;
    }
    assert_eq!(offset, size);
}

#[derive(Default)]
pub struct Stats {
    pub requests: AtomicUsize,
    /// Bodies being sent now, and the most at once.
    pub open: AtomicUsize,
    pub max_open: AtomicUsize,
    /// Start offsets of the ranged GETs served.
    pub ranges: Mutex<Vec<u64>>,
    /// The ETag of `/changing` is `"v<n>"`.
    pub version: AtomicU64,
    /// `/flaky` bodies left that break off early.
    pub flaky_left: AtomicI64,
}

pub struct Server {
    pub addr: SocketAddr,
    pub stats: Arc<Stats>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

pub async fn start() -> Server {
    start_at("127.0.0.1:0").await
}

/// The server on a fixed address (the `test_server` example). Besides the test routes,
/// `/get/{size}/{name}` serves a file named by its URL at full speed and `/dl/{size}/{name}` at
/// about 1 MB/s per connection.
pub async fn start_at(addr: &str) -> Server {
    let stats = Arc::new(Stats::default());
    stats.flaky_left.store(6, Ordering::SeqCst);
    let app = Router::new()
        .route("/get/{size}/{name}", get(get_named))
        .route("/dl/{size}/{name}", get(slow_named))
        .route("/file/{size}", get(file))
        .route("/norange/{size}", get(norange))
        .route("/flaky/{size}", get(flaky))
        .route("/slow/{size}", get(slow))
        .route("/changing/{size}", get(changing))
        .route("/nohead/{size}", get(nohead))
        .route("/named/{size}", get(named))
        .route("/chunked/{size}", get(chunked))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route(
            "/redirect",
            get(|| async { Redirect::temporary("/named/1000") }),
        )
        .with_state(stats.clone());
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { addr, stats }
}

#[derive(Clone)]
struct Opts {
    ranges: bool,
    etag: String,
    disposition: Option<&'static str>,
    /// Bytes per chunk and the pause after each.
    chunk: usize,
    delay: Option<Duration>,
    /// The body breaks off after this many bytes.
    break_after: Option<u64>,
    /// No Content-Length (chunked transfer).
    unknown_length: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            ranges: true,
            etag: "\"v0\"".into(),
            disposition: None,
            chunk: 64 << 10,
            delay: None,
            break_after: None,
            unknown_length: false,
        }
    }
}

type S = State<Arc<Stats>>;
type Size = UrlPath<u64>;

async fn get_named(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath((size, _)): UrlPath<(u64, String)>,
) -> Response {
    serve(&method, &headers, size, Opts::default(), &s)
}

async fn slow_named(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath((size, _)): UrlPath<(u64, String)>,
) -> Response {
    let o = Opts {
        chunk: 16 << 10,
        delay: Some(Duration::from_millis(15)),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn file(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    serve(&method, &headers, size, Opts::default(), &s)
}

async fn norange(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let o = Opts {
        ranges: false,
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn flaky(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let breaks = method == Method::GET && s.flaky_left.fetch_sub(1, Ordering::SeqCst) > 0;
    let o = Opts {
        break_after: breaks.then_some(100 << 10),
        chunk: 16 << 10,
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn slow(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let o = Opts {
        chunk: 16 << 10,
        delay: Some(Duration::from_millis(15)),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn changing(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath(size): Size,
) -> Response {
    let o = Opts {
        etag: format!("\"v{}\"", s.version.load(Ordering::SeqCst)),
        chunk: 16 << 10,
        delay: Some(Duration::from_millis(15)),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn nohead(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    if method == Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    serve(&method, &headers, size, Opts::default(), &s)
}

async fn named(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let o = Opts {
        disposition: Some(
            "attachment; filename=\"fallback.bin\"; filename*=UTF-8''%E6%B5%8B%E8%AF%95%20%E6%96%87%E4%BB%B6.bin",
        ),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

async fn chunked(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let o = Opts {
        ranges: false,
        unknown_length: true,
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

/// `bytes=a-b` / `bytes=a-` → inclusive range; `Err` when unsatisfiable.
fn parse_range(headers: &HeaderMap, size: u64) -> Option<Result<(u64, u64), ()>> {
    let v = headers.get(header::RANGE)?.to_str().ok()?;
    let spec = v.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.parse().ok()?;
    if start >= size {
        return Some(Err(()));
    }
    let end = if b.is_empty() {
        size - 1
    } else {
        b.parse::<u64>().ok()?.min(size - 1)
    };
    Some(Ok((start, end)))
}

/// Decrements the open-body count when the body is done or dropped.
struct OpenGuard(Arc<Stats>);

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve(method: &Method, headers: &HeaderMap, size: u64, o: Opts, s: &Arc<Stats>) -> Response {
    let range = if o.ranges {
        parse_range(headers, size)
    } else {
        None
    };
    let (status, start, end) = match range {
        Some(Ok((a, b))) => (StatusCode::PARTIAL_CONTENT, a, b),
        Some(Err(())) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap();
        }
        None => (StatusCode::OK, 0, size.saturating_sub(1)),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    let mut b = Response::builder()
        .status(status)
        .header(header::ETAG, &o.etag)
        .header(header::LAST_MODIFIED, "Wed, 21 Oct 2015 07:28:00 GMT")
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if !o.unknown_length {
        b = b.header(header::CONTENT_LENGTH, len);
    }
    if o.ranges {
        b = b.header(header::ACCEPT_RANGES, "bytes");
    }
    if status == StatusCode::PARTIAL_CONTENT {
        b = b.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    if let Some(d) = o.disposition {
        b = b.header(header::CONTENT_DISPOSITION, d);
    }
    if *method == Method::HEAD {
        return b.body(Body::empty()).unwrap();
    }
    s.requests.fetch_add(1, Ordering::SeqCst);
    if status == StatusCode::PARTIAL_CONTENT {
        s.ranges.lock().push(start);
    }
    let open = s.open.fetch_add(1, Ordering::SeqCst) + 1;
    s.max_open.fetch_max(open, Ordering::SeqCst);
    let guard = OpenGuard(s.clone());
    let stop = start + len;
    let limit = o.break_after.map_or(stop, |n| (start + n).min(stop));
    let body = futures_util::stream::unfold((start, guard), move |(pos, guard)| {
        let o = o.clone();
        async move {
            if pos >= stop {
                return None;
            }
            if pos >= limit {
                let e = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "broken");
                return Some((Err(e), (stop, guard)));
            }
            if let Some(d) = o.delay {
                tokio::time::sleep(d).await;
            }
            let next = (pos + o.chunk as u64).min(limit);
            let data: Vec<u8> = (pos..next).map(byte_at).collect();
            Some((Ok(Bytes::from(data)), (next, guard)))
        }
    });
    b.body(Body::from_stream(body)).unwrap()
}
