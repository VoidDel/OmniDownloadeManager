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
    /// `/capped` requests refused with 503, and the lock that makes checking and counting a
    /// body one step.
    pub refused: AtomicUsize,
    pub admit: Mutex<()>,
    /// `/lagging`: the slow body was started, and dropped before its end.
    pub lag_started: AtomicUsize,
    pub lag_cut: AtomicUsize,
    /// User agents `/picky` was asked with.
    pub agents: Mutex<Vec<String>>,
    /// Bodies dropped before their end (connections that stopped early).
    pub cut: AtomicUsize,
    /// `/throttled`: when the server's shared bandwidth is free again.
    pub bandwidth: Mutex<Option<tokio::time::Instant>>,
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
        .route("/capped/{max}/{size}", get(capped))
        .route("/lagging/{size}", get(lagging))
        .route("/throttled/{per}/{total}/{size}", get(throttled))
        .route("/burst/{fast}/{slow}/{burst}/{size}", get(burst))
        .route("/picky/{size}", get(picky))
        .route("/changing/{size}", get(changing))
        .route("/nohead/{size}", get(nohead))
        .route("/named/{size}", get(named))
        .route("/chunked/{size}", get(chunked))
        .route("/plainget/{size}", get(plainget))
        .route("/hls/{kind}/{n}/{name}", get(hls))
        .route("/fmp4/{handler}/{frags}/{size}", get(fmp4_file))
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
    /// The slow body of `/lagging`.
    lagging: bool,
    /// The server's bandwidth for all connections together (bytes per second).
    total: Option<u64>,
    /// After this many bytes of a body, the pause after each chunk becomes this.
    burst: Option<(u64, Duration)>,
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
            lagging: false,
            total: None,
            burst: None,
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

/// About 1 MB/s per connection; more than `max` bodies at once are refused with 503.
async fn capped(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath((max, size)): UrlPath<(usize, u64)>,
) -> Response {
    let _admit = s.admit.lock();
    if method == Method::GET
        && headers.contains_key(header::RANGE)
        && s.open.load(Ordering::SeqCst) >= max
    {
        s.refused.fetch_add(1, Ordering::SeqCst);
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let o = Opts {
        chunk: 16 << 10,
        delay: Some(Duration::from_millis(15)),
        ..Opts::default()
    };
    // Counts the body before the next request is checked.
    serve(&method, &headers, size, o, &s)
}

/// About 1 MB/s per connection, except the first body not starting at 0: about 40 kB/s.
async fn lagging(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let later = matches!(parse_range(&headers, size), Some(Ok((a, _))) if a > 0);
    let lag = method == Method::GET && later && s.lag_started.fetch_add(1, Ordering::SeqCst) == 0;
    let o = Opts {
        chunk: 16 << 10,
        delay: Some(Duration::from_millis(if lag { 400 } else { 15 })),
        lagging: lag,
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

/// A server that limits each connection to `per` KiB/s and, unless `total` is 0, all of them
/// together to `total` KiB/s.
async fn throttled(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath((per, total, size)): UrlPath<(u64, u64, u64)>,
) -> Response {
    let chunk = 16 << 10;
    let o = Opts {
        chunk,
        delay: Some(Duration::from_secs_f64(
            chunk as f64 / (per as f64 * 1024.0),
        )),
        total: (total > 0).then_some(total * 1024),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

/// A server that limits each connection with a delay: `fast` KiB/s for its first `burst` KiB,
/// then `slow` KiB/s.
async fn burst(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath((fast, slow, burst, size)): UrlPath<(u64, u64, u64, u64)>,
) -> Response {
    let chunk = 16 << 10;
    let pause = |kib: u64| Duration::from_secs_f64(chunk as f64 / (kib as f64 * 1024.0));
    let o = Opts {
        chunk,
        delay: Some(pause(fast)),
        burst: Some((burst * 1024, pause(slow))),
        ..Opts::default()
    };
    serve(&method, &headers, size, o, &s)
}

/// Refuses browsers' user agents (403), as some mirrors do with programs that pretend to be one.
async fn picky(method: Method, headers: HeaderMap, State(s): S, UrlPath(size): Size) -> Response {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    s.agents.lock().push(ua.clone());
    if ua.contains("Mozilla") {
        return StatusCode::FORBIDDEN.into_response();
    }
    serve(&method, &headers, size, Opts::default(), &s)
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

/// Like a webmail attachment: `HEAD` and ranged `GET`s answer 200 with an empty body; only a
/// plain `GET` gives the file.
async fn plainget(
    method: Method,
    headers: HeaderMap,
    State(s): S,
    UrlPath(size): Size,
) -> Response {
    if method == Method::HEAD || headers.contains_key(header::RANGE) {
        return Response::builder()
            .header(header::CONTENT_LENGTH, 0)
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::empty())
            .unwrap();
    }
    let o = Opts {
        ranges: false,
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

/// Decrements the open-body count when the body is done or dropped; counts the lagging body
/// dropped before its end.
struct OpenGuard {
    stats: Arc<Stats>,
    lagging: bool,
    finished: bool,
}

impl OpenGuard {
    fn finish(&mut self) {
        self.finished = true;
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.stats.open.fetch_sub(1, Ordering::SeqCst);
        if !self.finished {
            self.stats.cut.fetch_add(1, Ordering::SeqCst);
        }
        if self.lagging && !self.finished {
            self.stats.lag_cut.fetch_add(1, Ordering::SeqCst);
        }
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
    let guard = OpenGuard {
        stats: s.clone(),
        lagging: o.lagging,
        finished: false,
    };
    let stop = start + len;
    let limit = o.break_after.map_or(stop, |n| (start + n).min(stop));
    let body = futures_util::stream::unfold((start, guard), move |(pos, mut guard)| {
        let o = o.clone();
        async move {
            if pos >= stop {
                guard.finish();
                return None;
            }
            if pos >= limit {
                let e = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "broken");
                return Some((Err(e), (stop, guard)));
            }
            let delay = match o.burst {
                Some((after, slow)) if pos - start >= after => Some(slow),
                _ => o.delay,
            };
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
            let next = (pos + o.chunk as u64).min(limit);
            if next >= stop {
                // The last chunk: hyper stops polling once the length is sent.
                guard.finish();
            }
            if let Some(rate) = o.total {
                // The chunk's turn on the shared line.
                let at = {
                    let mut free = guard.stats.bandwidth.lock();
                    let now = tokio::time::Instant::now();
                    let at = free.map_or(now, |f| f.max(now));
                    *free = Some(at + Duration::from_secs_f64((next - pos) as f64 / rate as f64));
                    at
                };
                tokio::time::sleep_until(at).await;
            }
            let data: Vec<u8> = (pos..next).map(byte_at).collect();
            Some((Ok(Bytes::from(data)), (next, guard)))
        }
    });
    b.body(Body::from_stream(body)).unwrap()
}

// ------------------------------------------------------------------------------- HLS

/// Bytes of each HLS segment (not a multiple of 16: the encrypted ones need padding).
pub const HLS_SEG: u64 = 100_003;
/// The AES-128 key of the encrypted streams.
pub const HLS_KEY: [u8; 16] = *b"0123456789abcdef";

/// What `n` segments make once joined (and decrypted).
pub fn hls_content(n: u64) -> Vec<u8> {
    content(n * HLS_SEG)
}

fn encrypt(data: &[u8], iv: [u8; 16]) -> Vec<u8> {
    use aes::{
        Aes128,
        cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray},
    };
    let cipher = Aes128::new(GenericArray::from_slice(&HLS_KEY));
    let pad = 16 - data.len() % 16;
    let mut plain = data.to_vec();
    plain.extend(std::iter::repeat_n(pad as u8, pad));
    let mut prev = iv;
    let mut out = Vec::with_capacity(plain.len());
    for chunk in plain.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        for (b, p) in block.iter_mut().zip(prev) {
            *b ^= p;
        }
        cipher.encrypt_block(&mut block);
        prev.copy_from_slice(&block);
        out.extend_from_slice(&block);
    }
    out
}

/// `/hls/{kind}/{n}/…`: `kind` is `plain`, `enc` (AES-128, IV = sequence number), `slow`
/// (each segment 200 ms late) or `live` (no end). `master.m3u8` offers this stream and a smaller
/// one; `index.m3u8` lists `n` segments `seg<i>.ts`.
async fn hls(State(s): S, UrlPath((kind, n, name)): UrlPath<(String, u64, String)>) -> Response {
    s.requests.fetch_add(1, Ordering::SeqCst);
    let playlist = |text: String| {
        (
            [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
            text,
        )
            .into_response()
    };
    match name.as_str() {
        "master.m3u8" => playlist(format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=200000\n/hls/{kind}/1/index.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=1280x720\n/hls/{kind}/{n}/index.m3u8\n"
        )),
        "index.m3u8" => {
            let mut t = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n");
            if kind == "enc" {
                t.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n");
            }
            for i in 0..n {
                t.push_str(&format!("#EXTINF:2.0,\nseg{i}.ts\n"));
            }
            if kind != "live" {
                t.push_str("#EXT-X-ENDLIST\n");
            }
            playlist(t)
        }
        "key.bin" => HLS_KEY.to_vec().into_response(),
        seg => {
            let Some(i) = seg
                .strip_prefix("seg")
                .and_then(|r| r.strip_suffix(".ts"))
                .and_then(|i| i.parse::<u64>().ok())
                .filter(|&i| i < n)
            else {
                return StatusCode::NOT_FOUND.into_response();
            };
            if kind == "slow" {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let data: Vec<u8> = (i * HLS_SEG..(i + 1) * HLS_SEG).map(byte_at).collect();
            let body = if kind == "enc" {
                encrypt(&data, (i as u128).to_be_bytes())
            } else {
                data
            };
            ([(header::CONTENT_TYPE, "video/mp2t")], body).into_response()
        }
    }
}

// ------------------------------------------------------------------ fragmented MP4 (DASH)

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

fn mp4_full(kind: &[u8; 4], version: u8, flags: u32, rest: &[u8]) -> Vec<u8> {
    let mut p = vec![version];
    p.extend_from_slice(&flags.to_be_bytes()[1..]);
    p.extend_from_slice(rest);
    mp4_box(kind, &p)
}

/// The data of fragment `i` of the `handler` stream: recognisable in the joined file.
pub fn fmp4_data(handler: &[u8; 4], i: u32, size: usize) -> Vec<u8> {
    (0..size)
        .map(|j| handler[0] ^ (i as u8) ^ (j as u8))
        .collect()
}

/// A fragmented MP4 of one track (`vide` or `soun`, track 1, 1000 per second): its init
/// section, then `frags` fragments a second apart with `size` bytes of data each.
pub fn fmp4(handler: &[u8; 4], frags: u32, size: usize) -> Vec<u8> {
    let mut mvhd = vec![0u8; 96];
    mvhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[92..96].copy_from_slice(&2u32.to_be_bytes());
    let mut tkhd = vec![0u8; 80];
    tkhd[8..12].copy_from_slice(&1u32.to_be_bytes());
    let mut mdhd = vec![0u8; 20];
    mdhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
    let mut hdlr = vec![0u8; 21];
    hdlr[4..8].copy_from_slice(handler);
    let mdia = mp4_box(
        b"mdia",
        &[
            mp4_full(b"mdhd", 0, 0, &mdhd),
            mp4_full(b"hdlr", 0, 0, &hdlr),
        ]
        .concat(),
    );
    let trak = mp4_box(b"trak", &[mp4_full(b"tkhd", 0, 3, &tkhd), mdia].concat());
    let mut trex = 1u32.to_be_bytes().to_vec();
    trex.extend_from_slice(&[0u8; 16]);
    let mvex = mp4_box(b"mvex", &mp4_full(b"trex", 0, 0, &trex));
    let moov = mp4_box(
        b"moov",
        &[mp4_full(b"mvhd", 0, 0, &mvhd), trak, mvex].concat(),
    );
    let mut out = [mp4_box(b"ftyp", b"iso5\0\0\0\0iso5dash"), moov].concat();
    for i in 0..frags {
        let data = fmp4_data(handler, i, size);
        let tfhd = mp4_full(b"tfhd", 0, 0x020000, &1u32.to_be_bytes());
        let tfdt = mp4_full(b"tfdt", 1, 0, &(u64::from(i) * 1000).to_be_bytes());
        let trun = |off: i32| {
            let mut r = 1u32.to_be_bytes().to_vec();
            r.extend_from_slice(&off.to_be_bytes());
            r.extend_from_slice(&(size as u32).to_be_bytes());
            mp4_full(b"trun", 0, 0x000201, &r)
        };
        let moof = |off| {
            mp4_box(
                b"moof",
                &[
                    mp4_full(b"mfhd", 0, 0, &(i + 1).to_be_bytes()),
                    mp4_box(b"traf", &[tfhd.clone(), tfdt.clone(), trun(off)].concat()),
                ]
                .concat(),
            )
        };
        let off = moof(0).len() as i32 + 8;
        out.extend(moof(off));
        out.extend(mp4_box(b"mdat", &data));
    }
    out
}

async fn fmp4_file(
    headers: HeaderMap,
    UrlPath((handler, frags, size)): UrlPath<(String, u32, usize)>,
) -> Response {
    let h: [u8; 4] = handler.as_bytes().try_into().unwrap_or(*b"vide");
    let body = fmp4(&h, frags, size);
    let total = body.len() as u64;
    match parse_range(&headers, total) {
        Some(Ok((a, b))) => Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, "video/mp4")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes {a}-{b}/{total}"))
            .header(header::CONTENT_LENGTH, b - a + 1)
            .body(Body::from(body[a as usize..=b as usize].to_vec()))
            .unwrap(),
        Some(Err(())) => StatusCode::RANGE_NOT_SATISFIABLE.into_response(),
        None => Response::builder()
            .header(header::CONTENT_TYPE, "video/mp4")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, total)
            .body(Body::from(body))
            .unwrap(),
    }
}
