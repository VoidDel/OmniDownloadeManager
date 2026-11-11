//! HLS (`.m3u8`): a video cut into many short files listed by a playlist. The best variant of a
//! master playlist is chosen, its segments are fetched several at once (decrypted when the
//! playlist uses AES-128) and written one after the other into one file: MPEG-TS segments make
//! a `.ts` file, fragmented MP4 (`#EXT-X-MAP`) an `.mp4`. Live playlists (no `#EXT-X-ENDLIST`)
//! are not recorded.
//!
//! Resuming: the segments written so far are kept in the task's segment table (one entry each,
//! its byte range in the file); a restart truncates the file to them and goes on from the next.
//! The playlist's address and segment count (`hls:<n>` in the ETag slot) must be unchanged.

use std::{collections::HashMap, sync::Arc, time::Duration};

use aes::{
    Aes128,
    cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray},
};
use futures_util::{StreamExt, stream::FuturesOrdered};
use omnidl_core::{
    Error, ErrorCode, Job, Outcome, ProbeInfo, Result, ResumeState, Segment,
    disk::{backoff, claim_name, place},
    filename,
};
use reqwest::{Client, StatusCode, header::RANGE};
use tokio::{io::AsyncWriteExt, sync::Mutex};
use url::Url;

use crate::client::{self, request_error, status_error};

/// How many segments are fetched at once.
const MAX_PARALLEL: usize = 8;
/// How often progress is saved.
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// Whether `url` names a playlist (`….m3u8`, any query).
pub(crate) fn is_playlist_url(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".m3u8")
}

/// Whether a `Content-Type` is a playlist's.
pub(crate) fn is_playlist_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|c| {
        let c = c
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        matches!(
            c.as_str(),
            "application/vnd.apple.mpegurl"
                | "application/x-mpegurl"
                | "audio/mpegurl"
                | "audio/x-mpegurl"
        )
    })
}

/// A run that stopped in a playlist: its state says so.
pub(crate) fn is_hls_state(r: &ResumeState) -> bool {
    r.etag.as_deref().is_some_and(|e| e.starts_with("hls:"))
}

// ------------------------------------------------------------------------------- the playlist

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    uri: Url,
    iv: Option<[u8; 16]>,
}

/// One file to fetch: a segment, or the init section of fragmented MP4.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Part {
    url: Url,
    /// `(length, offset)` of a byte range of `url`.
    range: Option<(u64, u64)>,
    key: Option<Key>,
    /// Media sequence number (the IV when the key has none).
    seq: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Media {
    /// The init section first when there is one, then the segments.
    parts: Vec<Part>,
    fmp4: bool,
    ended: bool,
}

/// A variant of a master playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Variant {
    bandwidth: u64,
    url: Url,
    /// The group of its sound when that is a playlist of its own (`AUDIO="…"`).
    audio: Option<String>,
}

/// A sound playlist of a master playlist (`#EXT-X-MEDIA:TYPE=AUDIO` with a URI).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rendition {
    group: String,
    url: Url,
    default: bool,
}

enum Playlist {
    Master(Vec<Variant>, Vec<Rendition>),
    Media(Media),
}

/// `KEY=value,KEY="quoted, value"` → pairs.
fn attributes(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let (value, next) = if let Some(q) = after.strip_prefix('"') {
            match q.split_once('"') {
                Some((v, n)) => (v, n.trim_start_matches(',')),
                None => (q, ""),
            }
        } else {
            match after.split_once(',') {
                Some((v, n)) => (v, n),
                None => (after, ""),
            }
        };
        out.insert(key.trim().to_ascii_uppercase(), value.trim().to_owned());
        rest = next.trim_start();
    }
    out
}

fn hex16(s: &str) -> Option<[u8; 16]> {
    let h = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if h.len() > 32 || h.is_empty() {
        return None;
    }
    let h = format!("{h:0>32}");
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// `n[@o]`; without `@o` the range follows the previous one of the same file.
fn byte_range(s: &str, next: u64) -> Option<(u64, u64)> {
    let (len, off) = match s.trim().split_once('@') {
        Some((l, o)) => (l.parse().ok()?, o.parse().ok()?),
        None => (s.trim().parse().ok()?, next),
    };
    Some((len, off))
}

fn parse(text: &str, base: &Url) -> Result<Playlist> {
    let bad = |why: &str| Error::new(ErrorCode::InvalidUrl, format!("{base}: {why}"));
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next().map(|l| l.trim_start_matches('\u{feff}')) != Some("#EXTM3U") {
        return Err(bad("not an HLS playlist"));
    }
    let join = |u: &str| base.join(u).map_err(|e| bad(&e.to_string()));
    let mut variants = Vec::new();
    let mut renditions = Vec::new();
    let mut media = Media::default();
    let mut seq = 0u64;
    let mut key: Option<Key> = None;
    let mut pending_range: Option<String> = None;
    let mut pending_variant: Option<(u64, Option<String>)> = None;
    let mut next_offset: HashMap<Url, u64> = HashMap::new();
    for line in lines {
        if let Some(a) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let attrs = attributes(a);
            pending_variant = Some((
                attrs
                    .get("BANDWIDTH")
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0),
                attrs.get("AUDIO").cloned(),
            ));
        } else if let Some(a) = line.strip_prefix("#EXT-X-MEDIA:") {
            let attrs = attributes(a);
            if attrs.get("TYPE").map(String::as_str) == Some("AUDIO")
                && let (Some(group), Some(uri)) = (attrs.get("GROUP-ID"), attrs.get("URI"))
            {
                renditions.push(Rendition {
                    group: group.clone(),
                    url: join(uri)?,
                    default: attrs.get("DEFAULT").map(String::as_str) == Some("YES"),
                });
            }
        } else if let Some(n) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = n.trim().parse().unwrap_or(0);
        } else if let Some(a) = line.strip_prefix("#EXT-X-KEY:") {
            let attrs = attributes(a);
            key = match attrs.get("METHOD").map(String::as_str) {
                Some("NONE") | None => None,
                Some("AES-128") => Some(Key {
                    uri: join(attrs.get("URI").ok_or_else(|| bad("a key without URI"))?)?,
                    iv: attrs.get("IV").and_then(|v| hex16(v)),
                }),
                Some(m) => {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        format!("{base}: {m} encryption is not supported"),
                    ));
                }
            };
        } else if let Some(a) = line.strip_prefix("#EXT-X-MAP:") {
            let attrs = attributes(a);
            let url = join(attrs.get("URI").ok_or_else(|| bad("a map without URI"))?)?;
            let range = attrs.get("BYTERANGE").and_then(|r| byte_range(r, 0));
            media.fmp4 = true;
            if !media.parts.iter().any(|p| p.url == url && p.range == range) {
                media.parts.push(Part {
                    url,
                    range,
                    key: key.clone(),
                    seq,
                });
            }
        } else if let Some(r) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_range = Some(r.to_owned());
        } else if line.starts_with("#EXT-X-ENDLIST") {
            media.ended = true;
        } else if line.starts_with('#') {
            // #EXTINF and the rest: nothing to keep.
        } else if let Some((bandwidth, audio)) = pending_variant.take() {
            variants.push(Variant {
                bandwidth,
                url: join(line)?,
                audio,
            });
        } else {
            let url = join(line)?;
            let range = pending_range.take().and_then(|r| {
                let next = next_offset.get(&url).copied().unwrap_or(0);
                byte_range(&r, next)
            });
            if let Some((len, off)) = range {
                next_offset.insert(url.clone(), off + len);
            }
            media.parts.push(Part {
                url,
                range,
                key: key.clone(),
                seq,
            });
            seq += 1;
        }
    }
    if !variants.is_empty() {
        return Ok(Playlist::Master(variants, renditions));
    }
    if media.parts.is_empty() {
        return Err(bad("an empty playlist"));
    }
    Ok(Playlist::Media(media))
}

// ------------------------------------------------------------------------------- fetching

async fn get_text(client: &Client, url: &Url) -> Result<String> {
    let resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| request_error(&e))?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    resp.text().await.map_err(|e| request_error(&e))
}

/// The media playlist behind `url` (a master's best variant, highest bandwidth) and, when that
/// variant's sound is a playlist of its own, the sound's.
async fn media_playlists(client: &Client, url: &Url) -> Result<(Url, Media, Option<Media>)> {
    let mut url = url.clone();
    let mut audio: Option<Url> = None;
    for _ in 0..3 {
        match parse(&get_text(client, &url).await?, &url)? {
            Playlist::Media(m) => {
                let sound = match audio.filter(|a| *a != url) {
                    Some(a) => match parse(&get_text(client, &a).await?, &a)? {
                        Playlist::Media(s) => Some(s),
                        Playlist::Master(..) => {
                            return Err(Error::new(
                                ErrorCode::InvalidUrl,
                                format!("{a}: a sound playlist that is a master playlist"),
                            ));
                        }
                    },
                    None => None,
                };
                return Ok((url, m, sound));
            }
            Playlist::Master(mut variants, renditions) => {
                variants.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
                let best = variants.swap_remove(0);
                // The group's default sound, else its first.
                audio = best.audio.as_ref().and_then(|g| {
                    let mut group = renditions.iter().filter(|r| &r.group == g);
                    let first = group.clone().next();
                    group.find(|r| r.default).or(first).map(|r| r.url.clone())
                });
                url = best.url;
            }
        }
    }
    Err(Error::new(
        ErrorCode::InvalidUrl,
        format!("{url}: playlists that never reach the media"),
    ))
}

/// What one run fetches: the parts in order, and where the sound to join begins.
struct Plan {
    /// Where the parts come from (the media playlist, or the picture's file): with `tag`, what
    /// a resumed run checks.
    final_url: Url,
    tag: String,
    parts: Vec<Part>,
    /// The first part of the sound when it is joined to the picture at the end.
    sound_from: Option<usize>,
    fmp4: bool,
    /// The bytes in all, when known beforehand (a pair of files).
    total: Option<u64>,
}

/// A playlist (and its own sound playlist, joined when both are fragmented MP4).
async fn playlist_plan(client: &Client, url: &Url) -> Result<Plan> {
    let (media_url, media, sound) = media_playlists(client, url).await?;
    if !media.ended {
        return Err(live());
    }
    let count = media.parts.len();
    let mut parts = media.parts;
    let mut tag = format!("hls:{count}");
    let mut sound_from = None;
    match sound {
        Some(s) if media.fmp4 && s.fmp4 && s.ended => {
            tag = format!("hls:{count}+{}", s.parts.len());
            sound_from = Some(parts.len());
            parts.extend(s.parts);
        }
        Some(_) => tracing::warn!(
            "{media_url}: the sound is a playlist of its own that cannot be joined (MPEG-TS); \
             the picture only"
        ),
        None => {}
    }
    Ok(Plan {
        final_url: media_url,
        tag,
        parts,
        sound_from,
        fmp4: media.fmp4,
        total: None,
    })
}

/// How much of a file one request of a pair fetches.
const CHUNK: u64 = 4 << 20;

/// A file of a pair in parts of `CHUNK` bytes (one part when the server ignores ranges).
fn chunks(url: &Url, total: u64, ranges: bool, seq: &mut u64) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut part = |range| {
        parts.push(Part {
            url: url.clone(),
            range,
            key: None,
            seq: *seq,
        });
        *seq += 1;
    };
    if !ranges {
        part(None);
    } else {
        let mut off = 0;
        while off < total {
            let len = CHUNK.min(total - off);
            part(Some((len, off)));
            off += len;
        }
    }
    parts
}

/// The picture at `video` and the sound at `audio`: both their sizes.
async fn pair_sizes(client: &Client, video: &Url, audio: &Url) -> Result<[(Url, u64, bool); 2]> {
    let (v, a) = futures_util::future::try_join(
        crate::probe::probe(client, video),
        crate::probe::probe(client, audio),
    )
    .await?;
    let sized = |p: crate::probe::Probe, what: &Url| {
        p.total
            .map(|t| (p.final_url, t, p.resumable))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unsupported,
                    format!("{what}: the server does not say the size; cannot join the streams"),
                )
            })
    };
    Ok([sized(v, video)?, sized(a, audio)?])
}

/// A video whose picture and sound are separate files (a site's DASH streams), joined.
async fn pair_plan(client: &Client, video: &Url, audio: &str) -> Result<Plan> {
    let audio = crate::parse_url(audio)?;
    let [(vu, vt, vr), (au, at, ar)] = pair_sizes(client, video, &audio).await?;
    let mut seq = 0;
    let mut parts = chunks(&vu, vt, vr, &mut seq);
    let sound_from = Some(parts.len());
    parts.extend(chunks(&au, at, ar, &mut seq));
    Ok(Plan {
        final_url: vu,
        tag: format!("hls:pair:{vt}:{at}"),
        parts,
        sound_from,
        fmp4: true,
        total: Some(vt + at),
    })
}

/// What the new-download dialog shows for a playlist: the file name and that it is a stream.
pub(crate) async fn probe(
    client: &Client,
    url: &Url,
    file_name: Option<&str>,
) -> Result<(Url, String)> {
    let plan = playlist_plan(client, url).await?;
    Ok((plan.final_url, output_name(url, file_name, plan.fmp4)))
}

/// What the new-download dialog shows for a picture with its sound apart: one MP4 of both.
pub(crate) async fn probe_pair(client: &Client, video: &Url, audio: &str) -> Result<ProbeInfo> {
    let audio = crate::parse_url(audio)?;
    let [(vu, vt, _), (_, at, _)] = pair_sizes(client, video, &audio).await?;
    Ok(ProbeInfo {
        file_name: output_name(video, None, true),
        final_url: vu.to_string(),
        total: Some(vt + at),
        resumable: true,
        content_type: Some("video/mp4".into()),
        etag: None,
        last_modified: None,
        files: Vec::new(),
    })
}

fn live() -> Error {
    Error::new(
        ErrorCode::Unsupported,
        "a live stream (the playlist has no end): not recorded",
    )
}

/// The name of the file: the one given (with the stream's extension), else the playlist's, else
/// (generic names such as `index.m3u8`) the folder it is in.
fn output_name(url: &Url, given: Option<&str>, fmp4: bool) -> String {
    let ext = if fmp4 { "mp4" } else { "ts" };
    if let Some(n) = given.map(str::trim).filter(|n| !n.is_empty()) {
        let stem = n.rsplit_once('.').map_or(n, |(s, e)| {
            if matches!(
                e.to_ascii_lowercase().as_str(),
                "m3u8" | "ts" | "mp4" | "m4s" | "m4v" | "m4a"
            ) {
                s
            } else {
                n
            }
        });
        return filename::sanitize(&format!("{stem}.{ext}"));
    }
    let segs: Vec<String> = url
        .path_segments()
        .map(|s| {
            s.filter(|p| !p.is_empty())
                .map(filename::percent_decode)
                .collect()
        })
        .unwrap_or_default();
    let stem = segs
        .last()
        .and_then(|s| s.rsplit_once('.').map(|(a, _)| a.to_owned()))
        .unwrap_or_default();
    const GENERIC: [&str; 9] = [
        "index",
        "playlist",
        "master",
        "prog_index",
        "chunklist",
        "video",
        "main",
        "mono",
        "media",
    ];
    let stem = if stem.is_empty() || GENERIC.contains(&stem.to_ascii_lowercase().as_str()) {
        segs.iter()
            .rev()
            .nth(1)
            .cloned()
            .or_else(|| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "video".into())
    } else {
        stem
    };
    filename::sanitize(&format!("{stem}.{ext}"))
}

/// AES-128-CBC with PKCS#7 padding.
fn decrypt(key: &[u8; 16], iv: [u8; 16], data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(Error::new(
            ErrorCode::Network,
            "an encrypted segment of a broken length",
        ));
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut prev = iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        out.extend(block.iter().zip(prev.iter()).map(|(b, p)| b ^ p));
        prev.copy_from_slice(chunk);
    }
    let pad = *out.last().unwrap_or(&0) as usize;
    if (1..=16).contains(&pad)
        && out.len() >= pad
        && out[out.len() - pad..].iter().all(|&b| b as usize == pad)
    {
        out.truncate(out.len() - pad);
    }
    Ok(out)
}

struct Fetcher {
    job: Arc<Job>,
    client: Client,
    retry_delay: Duration,
    keys: Mutex<HashMap<Url, [u8; 16]>>,
}

impl Fetcher {
    async fn key(&self, uri: &Url) -> Result<[u8; 16]> {
        if let Some(k) = self.keys.lock().await.get(uri) {
            return Ok(*k);
        }
        let bytes = self.bytes(uri, None).await?;
        let k: [u8; 16] = bytes.as_slice().try_into().map_err(|_| {
            Error::new(
                ErrorCode::Network,
                format!("{uri}: a key that is not 16 bytes"),
            )
        })?;
        self.keys.lock().await.insert(uri.clone(), k);
        Ok(k)
    }

    /// One request, retried while the failure is transient.
    async fn bytes(&self, url: &Url, range: Option<(u64, u64)>) -> Result<Vec<u8>> {
        let mut attempt = 0;
        loop {
            let mut req = self.client.get(url.clone());
            if let Some((len, off)) = range {
                req = req.header(
                    RANGE,
                    format!("bytes={off}-{}", off + len.saturating_sub(1)),
                );
            }
            let r = async {
                let resp = req.send().await.map_err(|e| request_error(&e))?;
                match resp.status() {
                    StatusCode::OK | StatusCode::PARTIAL_CONTENT => {}
                    s => return Err(status_error(s)),
                }
                Ok(resp.bytes().await.map_err(|e| request_error(&e))?.to_vec())
            };
            let e = tokio::select! {
                r = r => match r {
                    Ok(b) => return Ok(b),
                    Err(e) => e,
                },
                _ = self.job.cancel.cancelled() => return Err(Error::internal("cancelled")),
            };
            attempt += 1;
            if !e.is_transient() || attempt > self.job.options.max_retries {
                return Err(e);
            }
            tracing::debug!("{url} (retry {attempt}): {e}");
            tokio::select! {
                _ = tokio::time::sleep(backoff(self.retry_delay, attempt)) => {}
                _ = self.job.cancel.cancelled() => return Err(Error::internal("cancelled")),
            }
        }
    }

    /// A part, decrypted.
    async fn part(&self, p: &Part) -> Result<Vec<u8>> {
        let data = self.bytes(&p.url, p.range).await?;
        match &p.key {
            None => Ok(data),
            Some(k) => {
                let key = self.key(&k.uri).await?;
                let iv = k.iv.unwrap_or_else(|| (p.seq as u128).to_be_bytes());
                decrypt(&key, iv, &data)
            }
        }
    }
}

// ------------------------------------------------------------------------------- one run

pub(crate) async fn run(retry_delay: Duration, job: Arc<Job>) -> Outcome {
    let resume = job.resume.clone();
    match go(retry_delay, &job).await {
        Ok(o) => o,
        Err((_, state)) if job.cancel.is_cancelled() => {
            Outcome::Stopped(state.or(resume).unwrap_or_default())
        }
        Err((e, state)) => Outcome::Failed(e, state.or(resume)),
    }
}

type Failed = (Error, Option<ResumeState>);

// The error carries the state to save: large, but once per run.
#[allow(clippy::result_large_err)]
async fn go(retry_delay: Duration, job: &Arc<Job>) -> Result<Outcome, Failed> {
    let early = |e: Error| (e, None);
    let url = crate::parse_url(&job.source).map_err(early)?;
    // The user agent that worked for this host, if another did (`ua`).
    let ua = crate::ua::preferred(&job.options, &url);
    let client = client::build_as(&job.options, &ua).map_err(early)?;
    let plan = match job.options.audio_source.as_deref() {
        Some(audio) => pair_plan(&client, &url, audio).await,
        None => playlist_plan(&client, &url).await,
    }
    .map_err(early)?;
    let Plan {
        final_url: media_url,
        tag,
        parts: all_parts,
        sound_from,
        fmp4,
        total,
    } = plan;
    let count = all_parts.len();
    let dir = job.save_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| early(Error::io(&e, dir.display())))?;

    // Where it stopped, if the playlist is the same.
    let resumed = job.resume.as_ref().filter(|r| {
        r.etag.as_deref() == Some(tag.as_str())
            && r.final_url.as_deref() == Some(media_url.as_str())
            && !r.segments.is_empty()
    });
    let (file_name, done) = match resumed {
        Some(r) => {
            let partial = dir.join(filename::partial(&r.file_name));
            let written: u64 = r.segments.iter().map(|s| s.downloaded).sum();
            let ok = std::fs::OpenOptions::new()
                .write(true)
                .open(&partial)
                .and_then(|f| f.set_len(written))
                .is_ok();
            if ok {
                (r.file_name.clone(), r.segments.clone())
            } else {
                tracing::warn!("{}: cannot continue; starting over", partial.display());
                let name = r.file_name.clone();
                omnidl_storage::file::create(&partial, None)
                    .map_err(|e| early(Error::io(&e, partial.display())))?;
                (name, Vec::new())
            }
        }
        None => match job.resume.as_ref().map(|r| r.file_name.clone()) {
            // Started before (another playlist now): over again, same name.
            Some(name) => {
                let partial = dir.join(filename::partial(&name));
                omnidl_storage::file::create(&partial, None)
                    .map_err(|e| early(Error::io(&e, partial.display())))?;
                (name, Vec::new())
            }
            None => {
                let base = output_name(&url, job.file_name.as_deref(), fmp4);
                let (name, _) =
                    claim_name(&dir, &base, None, job.options.conflict).map_err(early)?;
                (name, Vec::new())
            }
        },
    };
    let partial = dir.join(filename::partial(&file_name));
    let mut state = ResumeState {
        file_name: file_name.clone(),
        final_url: Some(media_url.to_string()),
        total,
        etag: Some(tag),
        last_modified: None,
        resumable: true,
        segments: done,
        completed_at: None,
    };
    let mut written: u64 = state.segments.iter().map(|s| s.downloaded).sum();
    job.progress.set_downloaded(written);
    if total.is_some() {
        job.progress.set_total(total);
    }
    job.checkpoint.started(&state).await.map_err(early)?;

    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&partial)
        .await
        .map_err(|e| (Error::io(&e, partial.display()), Some(state.clone())))?;
    let fetcher = Arc::new(Fetcher {
        job: job.clone(),
        client,
        retry_delay,
        keys: Mutex::new(HashMap::new()),
    });
    job.conns.begin(
        job.options.conn_mode(),
        MAX_PARALLEL as u32,
        omnidl_core::conns::AUTO_START,
    );
    let first = state.segments.len();
    // In order, as many at a time as the connections asked for (by the user or the tuner).
    let mut todo = all_parts.into_iter().skip(first);
    let mut parts = FuturesOrdered::new();
    let mut last_save = tokio::time::Instant::now();
    loop {
        while parts.len() < job.conns.target() as usize {
            let Some(p) = todo.next() else { break };
            let f = fetcher.clone();
            parts.push_back(async move { f.part(&p).await });
        }
        job.progress.set_connections(parts.len() as u32);
        let Some(r) = parts.next().await else { break };
        let data = r.map_err(|e| (e, Some(state.clone())))?;
        let len = data.len() as u64;
        tokio::select! {
            _ = job.throttle(len) => {}
            _ = job.cancel.cancelled() => return Err((Error::internal("cancelled"), Some(state))),
        }
        file.write_all(&data)
            .await
            .map_err(|e| (Error::io(&e, "writing the file"), Some(state.clone())))?;
        state.segments.push(Segment {
            start: written,
            end: (len > 0).then(|| written + len - 1),
            downloaded: len,
        });
        written += len;
        job.progress.add(len);
        job.conns
            .observe(job.progress.downloaded(), job.speed_limit());
        // The size so far, spread over what is left: an estimate until the end (a pair of
        // files knows it from the start).
        if total.is_none() {
            let n = state.segments.len() as u64;
            job.progress
                .set_total(((n as usize) < count).then(|| written / n.max(1) * count as u64));
        }
        if last_save.elapsed() >= SAVE_EVERY {
            last_save = tokio::time::Instant::now();
            file.flush()
                .await
                .map_err(|e| (Error::io(&e, "writing the file"), Some(state.clone())))?;
            if job.options.sync_writes {
                let _ = file.sync_data().await;
            }
            job.checkpoint
                .save(&state)
                .await
                .map_err(|e| (e, Some(state.clone())))?;
        }
        if job.cancel.is_cancelled() {
            return Err((Error::internal("cancelled"), Some(state)));
        }
    }
    job.progress.set_connections(0);
    file.flush()
        .await
        .map_err(|e| (Error::io(&e, "writing the file"), Some(state.clone())))?;
    drop(file);
    // Picture and sound apart: joined into one MP4 in place of the two.
    let written = match sound_from {
        Some(i) => join_sound(&partial, &state, i)
            .await
            .map_err(|e| (e, Some(state.clone())))?,
        None => written,
    };
    job.progress.set_total(Some(written));
    let name =
        place(&dir, &file_name, job.options.conflict).map_err(|e| (e, Some(state.clone())))?;
    Ok(Outcome::Completed {
        file_name: name,
        total: written,
    })
}

/// Joins the picture (the parts before `sound_from`) and the sound in `partial` into one MP4,
/// which takes the partial file's place; its size.
async fn join_sound(
    partial: &std::path::Path,
    state: &ResumeState,
    sound_from: usize,
) -> Result<u64> {
    let split = state
        .segments
        .get(sound_from)
        .map(|s| s.start)
        .ok_or_else(|| Error::internal("the sound was not downloaded"))?;
    let end: u64 = state.segments.iter().map(|s| s.downloaded).sum();
    let partial = partial.to_path_buf();
    let mut joined = partial.clone().into_os_string();
    joined.push(".join");
    let joined = std::path::PathBuf::from(joined);
    tokio::task::spawn_blocking(move || {
        let r = crate::mp4::join(&partial, 0..split, split..end, &joined).and_then(|()| {
            std::fs::rename(&joined, &partial).map_err(|e| Error::io(&e, partial.display()))
        });
        if r.is_err() {
            let _ = std::fs::remove_file(&joined);
        }
        r?;
        std::fs::metadata(&partial)
            .map(|m| m.len())
            .map_err(|e| Error::io(&e, partial.display()))
    })
    .await
    .map_err(Error::internal)?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/show/ep1/index.m3u8").unwrap()
    }

    #[test]
    fn reads_a_master_playlist() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\"\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2800000\nhigh/index.m3u8\n";
        let Playlist::Master(v, r) = parse(text, &base()).unwrap() else {
            panic!("not a master")
        };
        assert_eq!(v.len(), 2);
        assert!(r.is_empty());
        assert_eq!(
            v[1],
            Variant {
                bandwidth: 2_800_000,
                url: Url::parse("https://cdn.example.com/show/ep1/high/index.m3u8").unwrap(),
                audio: None,
            }
        );
    }

    #[test]
    fn reads_sound_playlists_of_a_master() {
        let text = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"en\",DEFAULT=NO,URI=\"a/en.m3u8\"\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"zh\",DEFAULT=YES,URI=\"a/zh.m3u8\"\n\
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"zh\",URI=\"s/zh.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2800000,AUDIO=\"aac\"\nv/high.m3u8\n";
        let Playlist::Master(v, r) = parse(text, &base()).unwrap() else {
            panic!("not a master")
        };
        assert_eq!(v[0].audio.as_deref(), Some("aac"));
        assert_eq!(r.len(), 2);
        assert!(r[1].default && r[1].url.path().ends_with("/a/zh.m3u8"));
    }

    #[test]
    fn cuts_a_pair_of_files_into_ranges() {
        let url = base();
        let mut seq = 0;
        let parts = chunks(&url, CHUNK * 2 + 5, true, &mut seq);
        assert_eq!(
            parts.iter().map(|p| p.range).collect::<Vec<_>>(),
            [Some((CHUNK, 0)), Some((CHUNK, CHUNK)), Some((5, CHUNK * 2))]
        );
        assert_eq!(chunks(&url, 10, false, &mut seq)[0].range, None);
        assert_eq!(seq, 4);
    }

    #[test]
    fn reads_a_media_playlist_with_keys_and_ranges() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:7\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x0102\n#EXTINF:6.0,\nseg0.ts\n\
            #EXT-X-KEY:METHOD=NONE\n#EXTINF:6.0,\n#EXT-X-BYTERANGE:100@0\nall.ts\n\
            #EXTINF:6.0,\n#EXT-X-BYTERANGE:50\nall.ts\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(text, &base()).unwrap() else {
            panic!("not media")
        };
        assert!(m.ended && !m.fmp4);
        assert_eq!(m.parts.len(), 3);
        let k = m.parts[0].key.as_ref().unwrap();
        assert_eq!(k.uri.as_str(), "https://cdn.example.com/show/ep1/key.bin");
        assert_eq!(k.iv.unwrap()[14..], [1, 2]);
        assert_eq!(m.parts[0].seq, 7);
        assert!(m.parts[1].key.is_none());
        assert_eq!(m.parts[1].range, Some((100, 0)));
        assert_eq!(m.parts[2].range, Some((50, 100)));
        assert_eq!(m.parts[2].seq, 9);
    }

    #[test]
    fn knows_fmp4_live_and_bad_playlists() {
        let fmp4 = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\na.m4s\n#EXTINF:4,\nb.m4s\n";
        let Playlist::Media(m) = parse(fmp4, &base()).unwrap() else {
            panic!()
        };
        assert!(m.fmp4 && !m.ended);
        assert_eq!(
            m.parts[0].url.as_str(),
            "https://cdn.example.com/show/ep1/init.mp4"
        );
        assert_eq!(m.parts.len(), 3);
        assert!(parse("<html>", &base()).is_err());
        let sample = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n#EXTINF:4,\na.ts\n";
        assert_eq!(
            parse(sample, &base()).err().map(|e| e.code),
            Some(ErrorCode::Unsupported)
        );
    }

    #[test]
    fn names_the_output() {
        assert_eq!(output_name(&base(), None, false), "ep1.ts");
        assert_eq!(
            output_name(
                &Url::parse("https://a.com/v/movie.m3u8").unwrap(),
                None,
                true
            ),
            "movie.mp4"
        );
        assert_eq!(
            output_name(&base(), Some("Trailer.m3u8"), false),
            "Trailer.ts"
        );
        assert_eq!(
            output_name(&base(), Some("Episode 1"), false),
            "Episode 1.ts"
        );
    }

    #[test]
    fn decrypts_aes_128_cbc() {
        // One block of "hello" padded with 11 × 0x0b, encrypted with key 00..0f and IV 0.
        use aes::cipher::BlockEncrypt;
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let iv = [0u8; 16];
        let mut plain = b"hello".to_vec();
        plain.extend([11u8; 11]);
        let cipher = Aes128::new(GenericArray::from_slice(&key));
        let mut block = GenericArray::clone_from_slice(&plain);
        cipher.encrypt_block(&mut block);
        assert_eq!(decrypt(&key, iv, &block).unwrap(), b"hello");
    }
}
