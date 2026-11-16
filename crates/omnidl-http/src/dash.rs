//! DASH (`.mpd`): a video described by an XML manifest, its picture and sound in segments of
//! fragmented MP4. The highest-bandwidth picture and sound of the first period are chosen; their
//! segments become one HLS-style plan (`hls::Plan`): fetched several at once, written one after
//! the other, the sound joined to the picture at the end (`mp4::join`), resumable.
//!
//! Segments are found from `SegmentTemplate` (`$Number$`, `$Time$`, `SegmentTimeline`,
//! `%0Nd` widths), `SegmentList`, or a representation that is one file (`SegmentBase`, or only a
//! `BaseURL`), fetched in ranges. `BaseURL`s are resolved level by level (manifest, period,
//! adaptation set, representation). Not downloaded: live manifests (`type="dynamic"`), encrypted
//! ones (`ContentProtection`), WebM streams (the join is MP4's), periods after the first.

use omnidl_core::{Error, ErrorCode, Result};
use quick_xml::{Reader, XmlVersion, events::Event};
use reqwest::Client;
use url::Url;

use crate::hls::{Part, Plan};

/// More segments than this in one stream are no manifest worth trusting.
const MAX_SEGMENTS: u64 = 200_000;

/// Whether a text is a DASH manifest (its root element `MPD`, with or without a prefix).
pub(crate) fn is_mpd(text: &str) -> bool {
    let head = &text[..text.len().min(4096)];
    head.contains("<MPD") || head.contains(":MPD")
}

fn unsupported(url: &Url, why: &str) -> Error {
    Error::new(ErrorCode::Unsupported, format!("{url}: {why}"))
}

fn bad(url: &Url, why: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::InvalidUrl, format!("{url}: {why}"))
}

// ------------------------------------------------------------------------------- the XML tree

/// An element: its local name (no prefix), attributes, children and text.
#[derive(Debug, Default, Clone)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn num(&self, key: &str) -> Option<u64> {
        self.attr(key).and_then(|v| v.trim().parse().ok())
    }
}

/// The element tree of a manifest.
fn tree(text: &str) -> std::result::Result<Node, String> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        match event {
            Event::Start(e) | Event::Empty(e) if stack.len() > 64 => {
                return Err(format!(
                    "elements nested too deep at <{}>",
                    e.local_name().into_inner().to_owned()
                ));
            }
            Event::Start(e) => stack.push(element(&e)?),
            Event::Empty(e) => {
                let node = element(&e)?;
                stack.last_mut().ok_or("no parent")?.children.push(node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("an end without a start")?;
                stack
                    .last_mut()
                    .ok_or("an end without a start")?
                    .children
                    .push(node);
            }
            Event::Text(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&t.into_inner());
                }
            }
            Event::GeneralRef(r) => {
                let c = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match &*r {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    }
                    .to_owned(),
                };
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let root = stack.pop().ok_or("empty")?;
    root.children
        .into_iter()
        .find(|c| c.name == "MPD")
        .ok_or_else(|| "no MPD element".to_owned())
}

fn element(e: &quick_xml::events::BytesStart<'_>) -> std::result::Result<Node, String> {
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let value = a
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?;
        attrs.push((
            a.key.local_name().into_inner().to_owned(),
            value.into_owned(),
        ));
    }
    Ok(Node {
        name: e.local_name().into_inner().to_owned(),
        attrs,
        ..Node::default()
    })
}

// ------------------------------------------------------------------------------- values

/// Seconds of an ISO 8601 duration (`PT1H2M3.5S`, `P1DT2H`); years and months are not used by
/// manifests and not read.
fn duration(s: &str) -> Option<f64> {
    let s = s.trim().strip_prefix('P')?;
    let (days, time) = s.split_once('T').unwrap_or((s, ""));
    let mut secs = 0.0;
    let mut part = |text: &str, units: &[(char, f64)]| -> Option<()> {
        let mut num = String::new();
        for c in text.chars() {
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
            } else {
                let (_, mult) = units.iter().find(|(u, _)| *u == c)?;
                secs += num.parse::<f64>().ok()? * mult;
                num.clear();
            }
        }
        num.is_empty().then_some(())
    };
    part(days, &[('D', 86_400.0), ('W', 604_800.0)])?;
    part(time, &[('H', 3600.0), ('M', 60.0), ('S', 1.0)])?;
    Some(secs)
}

/// `a-b` (inclusive) as `(length, offset)`.
fn byte_range(s: &str) -> Option<(u64, u64)> {
    let (a, b) = s.trim().split_once('-')?;
    let (a, b): (u64, u64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (b >= a).then(|| (b - a + 1, a))
}

/// A `SegmentTemplate` address with its identifiers filled in: `$RepresentationID$`,
/// `$Bandwidth$`, `$Number$`, `$Time$` (each may carry a `%0Nd` width), `$$`.
fn fill(template: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('$') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        let (name, width) = match token.split_once('%') {
            Some((n, f)) => (
                n,
                f.trim_start_matches('0')
                    .trim_end_matches('d')
                    .parse::<usize>()
                    .ok(),
            ),
            None => (token, None),
        };
        let value = match name {
            "" => Some("$".to_owned()),
            "RepresentationID" => Some(id.to_owned()),
            "Bandwidth" => Some(bandwidth.to_string()),
            "Number" => Some(number.to_string()),
            "Time" => Some(time.to_string()),
            _ => None,
        };
        match (value, width) {
            (Some(v), Some(w)) if name != "RepresentationID" => out.push_str(&format!("{v:0>w$}")),
            (Some(v), _) => out.push_str(&v),
            (None, _) => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------------------- choosing

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Audio,
    Other,
}

/// What an adaptation set holds: by its content type, its or its first representation's MIME
/// type, else the codecs.
fn kind_of(set: &Node) -> Kind {
    let rep = set.child("Representation");
    let pick = |k: &str| set.attr(k).or_else(|| rep.and_then(|r| r.attr(k)));
    let by = |s: &str| {
        let s = s.to_ascii_lowercase();
        if s.starts_with("video") {
            Kind::Video
        } else if s.starts_with("audio") {
            Kind::Audio
        } else {
            Kind::Other
        }
    };
    if let Some(k) = pick("contentType").map(by).filter(|k| *k != Kind::Other) {
        return k;
    }
    if let Some(m) = pick("mimeType") {
        return by(m);
    }
    let codecs = pick("codecs").unwrap_or_default().to_ascii_lowercase();
    if ["avc", "hev", "hvc", "av01", "vp0", "vp9", "dvh"]
        .iter()
        .any(|c| codecs.starts_with(c))
    {
        Kind::Video
    } else if ["mp4a", "ac-3", "ec-3", "opus", "flac"]
        .iter()
        .any(|c| codecs.starts_with(c))
    {
        Kind::Audio
    } else {
        Kind::Other
    }
}

fn webm(set: &Node, rep: &Node) -> bool {
    rep.attr("mimeType")
        .or_else(|| set.attr("mimeType"))
        .is_some_and(|m| m.to_ascii_lowercase().contains("webm"))
}

/// `base` moved by a node's `BaseURL`.
fn based(base: &Url, node: &Node) -> std::result::Result<Url, url::ParseError> {
    match node
        .child("BaseURL")
        .map(|b| b.text.trim())
        .filter(|t| !t.is_empty())
    {
        Some(t) => base.join(t),
        None => Ok(base.clone()),
    }
}

/// A stream to fetch: its segments (the init section first), or one file in ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Track {
    Segments(Vec<(Url, Option<(u64, u64)>)>),
    Whole(Url),
}

/// What a manifest offers: the picture and the sound chosen.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Chosen {
    pub video: Option<Track>,
    pub audio: Option<Track>,
}

/// `SegmentTemplate` attributes and timeline, the inner levels over the outer ones.
#[derive(Default)]
struct Template<'a> {
    media: Option<&'a str>,
    init: Option<&'a str>,
    start: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    timeline: Option<&'a Node>,
}

impl<'a> Template<'a> {
    fn over(mut self, n: Option<&'a Node>) -> Self {
        if let Some(n) = n {
            self.media = n.attr("media").or(self.media);
            self.init = n.attr("initialization").or(self.init);
            self.start = n.num("startNumber").or(self.start);
            self.timescale = n.num("timescale").or(self.timescale);
            self.duration = n.num("duration").or(self.duration);
            self.timeline = n.child("SegmentTimeline").or(self.timeline);
        }
        self
    }
}

/// The segments of a representation (`base` resolved to its level).
fn track(url: &Url, base: &Url, levels: [&Node; 3], period_secs: Option<f64>) -> Result<Track> {
    let [period, set, rep] = levels;
    let id = rep.attr("id").unwrap_or_default();
    let bandwidth = rep.num("bandwidth").unwrap_or(0);
    let join = |s: &str| base.join(s).map_err(|e| bad(url, format!("{s}: {e}")));

    let t = Template::default()
        .over(period.child("SegmentTemplate"))
        .over(set.child("SegmentTemplate"))
        .over(rep.child("SegmentTemplate"));
    if let Some(media) = t.media {
        let mut out = Vec::new();
        if let Some(init) = t.init {
            out.push((join(&fill(init, id, bandwidth, 0, 0))?, None));
        }
        let start = t.start.unwrap_or(1);
        let timescale = t.timescale.unwrap_or(1).max(1);
        let mut add = |number: u64, time: u64| -> Result<()> {
            if out.len() as u64 > MAX_SEGMENTS {
                return Err(bad(url, "too many segments"));
            }
            out.push((join(&fill(media, id, bandwidth, number, time))?, None));
            Ok(())
        };
        if let Some(timeline) = t.timeline {
            let end = period_secs.map(|s| (s * timescale as f64).round() as u64);
            let (mut time, mut number) = (0u64, start);
            let s: Vec<&Node> = timeline.all("S").collect();
            for (i, seg) in s.iter().enumerate() {
                let d = seg
                    .num("d")
                    .filter(|d| *d > 0)
                    .ok_or_else(|| bad(url, "a timeline entry without a duration"))?;
                if let Some(t0) = seg.num("t") {
                    time = t0;
                }
                let r: i64 = seg
                    .attr("r")
                    .and_then(|r| r.trim().parse().ok())
                    .unwrap_or(0);
                let count = if r >= 0 {
                    r as u64 + 1
                } else {
                    // Repeated up to the next entry's start, or the period's end.
                    let until = s
                        .get(i + 1)
                        .and_then(|n| n.num("t"))
                        .or(end)
                        .ok_or_else(|| bad(url, "a timeline that repeats to an unknown end"))?;
                    until.saturating_sub(time).div_ceil(d)
                };
                for _ in 0..count {
                    add(number, time)?;
                    number += 1;
                    time += d;
                }
            }
        } else {
            let d = t
                .duration
                .filter(|d| *d > 0)
                .ok_or_else(|| bad(url, "a segment template without a duration or timeline"))?;
            let secs = period_secs.ok_or_else(|| bad(url, "no duration for the period"))?;
            let count = ((secs * timescale as f64) / d as f64).ceil() as u64;
            for i in 0..count {
                add(start + i, i * d)?;
            }
        }
        return Ok(Track::Segments(out));
    }

    if let Some(list) = rep
        .child("SegmentList")
        .or_else(|| set.child("SegmentList"))
    {
        let mut out = Vec::new();
        if let Some(init) = list.child("Initialization") {
            let at = init
                .attr("sourceURL")
                .map(join)
                .transpose()?
                .unwrap_or_else(|| base.clone());
            out.push((at, init.attr("range").and_then(byte_range)));
        }
        for s in list.all("SegmentURL") {
            let at = s
                .attr("media")
                .map(join)
                .transpose()?
                .unwrap_or_else(|| base.clone());
            out.push((at, s.attr("mediaRange").and_then(byte_range)));
        }
        if out.len() as u64 > MAX_SEGMENTS {
            return Err(bad(url, "too many segments"));
        }
        return Ok(Track::Segments(out));
    }

    // `SegmentBase`, or only an address: the representation is one file.
    Ok(Track::Whole(base.clone()))
}

/// The picture and the sound to download from a manifest at `url`.
pub(crate) fn choose(text: &str, url: &Url) -> Result<Chosen> {
    let mpd = tree(text).map_err(|e| bad(url, format!("not a DASH manifest: {e}")))?;
    if mpd.attr("type").is_some_and(|t| t.trim() == "dynamic") {
        return Err(crate::hls::live());
    }
    let base = based(url, &mpd).map_err(|e| bad(url, e))?;
    let period = mpd.child("Period").ok_or_else(|| bad(url, "no period"))?;
    if mpd.all("Period").count() > 1 {
        tracing::info!("{url}: several periods; the first only");
    }
    let base = based(&base, period).map_err(|e| bad(url, e))?;
    let period_secs = period
        .attr("duration")
        .and_then(duration)
        .or_else(|| mpd.attr("mediaPresentationDuration").and_then(duration));

    let mut best: [Option<(u64, &Node, &Node)>; 2] = [None, None];
    let mut protected = false;
    let mut skipped_webm = false;
    for set in period.all("AdaptationSet") {
        let slot = match kind_of(set) {
            Kind::Video => 0,
            Kind::Audio => 1,
            Kind::Other => continue,
        };
        for rep in set.all("Representation") {
            if set.child("ContentProtection").is_some() || rep.child("ContentProtection").is_some()
            {
                protected = true;
                continue;
            }
            if webm(set, rep) {
                skipped_webm = true;
                continue;
            }
            let bw = rep.num("bandwidth").unwrap_or(0);
            if best[slot].is_none_or(|(b, ..)| bw > b) {
                best[slot] = Some((bw, set, rep));
            }
        }
    }
    if best.iter().all(Option::is_none) {
        return Err(if protected {
            unsupported(url, "the stream is encrypted (DRM)")
        } else if skipped_webm {
            unsupported(url, "WebM streams are not supported")
        } else {
            bad(url, "no video or audio in the manifest")
        });
    }
    let mut out = Chosen::default();
    for (slot, found) in best.into_iter().enumerate() {
        let Some((_, set, rep)) = found else { continue };
        let base = based(&base, set)
            .and_then(|b| based(&b, rep))
            .map_err(|e| bad(url, e))?;
        let t = track(url, &base, [period, set, rep], period_secs)?;
        if slot == 0 {
            out.video = Some(t);
        } else {
            out.audio = Some(t);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------- the plan

/// A track's parts, numbered from `seq`; a whole file in ranges of its size.
async fn parts(client: &Client, track: Track, seq: &mut u64) -> Result<(Vec<Part>, Option<u64>)> {
    match track {
        Track::Segments(list) => Ok((
            list.into_iter()
                .map(|(url, range)| {
                    *seq += 1;
                    Part {
                        url,
                        range,
                        key: None,
                        seq: *seq - 1,
                    }
                })
                .collect(),
            None,
        )),
        Track::Whole(url) => {
            let auth = crate::auth::Auth::none();
            let p = crate::probe::probe(client, &auth, &url).await?;
            let total = p.total.ok_or_else(|| {
                unsupported(&url, "the server does not say the size of the stream")
            })?;
            Ok((
                crate::hls::chunks(&p.final_url, total, p.resumable, seq),
                Some(total),
            ))
        }
    }
}

/// The plan for a manifest at `url` (its text read): the picture's parts, then the sound's to
/// join to it.
pub(crate) async fn plan(client: &Client, url: &Url, text: &str) -> Result<Plan> {
    let chosen = choose(text, url)?;
    let mut seq = 0;
    let mut all = Vec::new();
    let mut total = Some(0u64);
    let mut counts = [0usize; 2];
    let mut sound_from = None;
    for (i, track) in [chosen.video, chosen.audio].into_iter().enumerate() {
        let Some(track) = track else { continue };
        if i == 1 && !all.is_empty() {
            sound_from = Some(all.len());
        }
        let (p, size) = parts(client, track, &mut seq).await?;
        total = total.zip(size).map(|(a, b)| a + b);
        counts[i] = p.len();
        all.extend(p);
    }
    let [video_count, audio_count] = counts;
    if all.is_empty() {
        return Err(bad(url, "no segments"));
    }
    Ok(Plan {
        final_url: url.clone(),
        tag: format!("hls:dash:{video_count}+{audio_count}"),
        parts: all,
        sound_from,
        fmp4: true,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.e.com/v/manifest.mpd?token=1").unwrap()
    }

    fn urls(t: &Track) -> Vec<String> {
        match t {
            Track::Segments(s) => s
                .iter()
                .map(|(u, r)| match r {
                    Some((len, off)) => format!("{u} {off}+{len}"),
                    None => u.to_string(),
                })
                .collect(),
            Track::Whole(u) => vec![format!("whole {u}")],
        }
    }

    #[test]
    fn reads_durations_ranges_and_templates() {
        assert_eq!(duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(duration("P1DT2H"), Some(93_600.0));
        assert_eq!(duration("PT0S"), Some(0.0));
        assert_eq!(duration("1H"), None);
        assert_eq!(byte_range("100-199"), Some((100, 100)));
        assert_eq!(byte_range("9-1"), None);
        assert_eq!(
            fill("$RepresentationID$/s-$Number%05d$.m4s", "v1", 9, 42, 0),
            "v1/s-00042.m4s"
        );
        assert_eq!(
            fill("t-$Time$-$Bandwidth$-$$.m4s", "x", 800, 1, 9000),
            "t-9000-800-$.m4s"
        );
        assert_eq!(fill("$Unknown$/$Number", "x", 1, 2, 3), "$Unknown$/$Number");
        assert!(is_mpd(
            "<?xml version=\"1.0\"?>\n<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\">"
        ));
        assert!(is_mpd("<dash:MPD>"));
        assert!(!is_mpd("#EXTM3U"));
    }

    #[test]
    fn takes_the_best_picture_and_sound_of_a_template() {
        let mpd = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT9.5S">
  <BaseURL>media/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/$Number%03d$.m4s" startNumber="1" timescale="1000" duration="4000"/>
      <Representation id="v360" bandwidth="800000" width="640" height="360"/>
      <Representation id="v1080" bandwidth="5000000" width="1920" height="1080"/>
    </AdaptationSet>
    <AdaptationSet contentType="audio" lang="en">
      <Representation id="a" bandwidth="128000" mimeType="audio/mp4">
        <BaseURL>https://audio.e.com/x/</BaseURL>
        <SegmentTemplate initialization="init.mp4" media="$Number$.m4s?a=1&amp;b=2" duration="2" timescale="1"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet mimeType="text/vtt"><Representation id="s" bandwidth="1"/></AdaptationSet>
  </Period>
</MPD>"#;
        let c = choose(mpd, &base()).unwrap();
        assert_eq!(
            urls(c.video.as_ref().unwrap()),
            [
                "https://cdn.e.com/v/media/v1080/init.mp4",
                "https://cdn.e.com/v/media/v1080/001.m4s",
                "https://cdn.e.com/v/media/v1080/002.m4s",
                "https://cdn.e.com/v/media/v1080/003.m4s",
            ]
        );
        let a = urls(c.audio.as_ref().unwrap());
        assert_eq!(a.len(), 6, "9.5 s in 2 s segments: 5, and the init section");
        assert_eq!(a[0], "https://audio.e.com/x/init.mp4");
        assert_eq!(a[5], "https://audio.e.com/x/5.m4s?a=1&b=2");
    }

    #[test]
    fn follows_a_timeline() {
        let mpd = r#"<MPD><Period duration="PT10S"><AdaptationSet contentType="video">
            <SegmentTemplate timescale="1000" initialization="i-$RepresentationID$.mp4" media="t-$Time$.m4s">
              <SegmentTimeline><S t="500" d="2000" r="1"/><S d="1000"/><S d="3000" r="-1"/></SegmentTimeline>
            </SegmentTemplate>
            <Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        let c = choose(mpd, &base()).unwrap();
        assert_eq!(
            urls(c.video.as_ref().unwrap())
                .iter()
                .map(|u| u.rsplit('/').next().unwrap().to_owned())
                .collect::<Vec<_>>(),
            // 500, 2500, then 4500, then 5500 and 8500 up to the period's end (10 000).
            [
                "i-v.mp4",
                "t-500.m4s",
                "t-2500.m4s",
                "t-4500.m4s",
                "t-5500.m4s",
                "t-8500.m4s"
            ]
        );
        assert!(c.audio.is_none());
    }

    #[test]
    fn reads_segment_lists_and_single_files() {
        let mpd = r#"<MPD mediaPresentationDuration="PT4S"><Period>
          <AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1">
            <BaseURL>video.mp4</BaseURL>
            <SegmentList><Initialization range="0-99"/><SegmentURL mediaRange="100-199"/><SegmentURL media="other.m4s"/></SegmentList>
          </Representation></AdaptationSet>
          <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
            <BaseURL>sound.mp4</BaseURL><SegmentBase indexRange="800-999"><Initialization range="0-799"/></SegmentBase>
          </Representation></AdaptationSet></Period></MPD>"#;
        let c = choose(mpd, &base()).unwrap();
        assert_eq!(
            urls(c.video.as_ref().unwrap()),
            [
                "https://cdn.e.com/v/video.mp4 0+100",
                "https://cdn.e.com/v/video.mp4 100+100",
                "https://cdn.e.com/v/other.m4s",
            ]
        );
        assert_eq!(
            urls(c.audio.as_ref().unwrap()),
            ["whole https://cdn.e.com/v/sound.mp4"]
        );
    }

    #[test]
    fn refuses_live_encrypted_and_webm_streams() {
        let live = r#"<MPD type="dynamic"><Period/></MPD>"#;
        assert!(choose(live, &base()).unwrap_err().message.contains("live"));
        let drm = r#"<MPD mediaPresentationDuration="PT4S"><Period><AdaptationSet mimeType="video/mp4">
            <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/>
            <Representation id="v" bandwidth="1"><BaseURL>v.mp4</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
        let e = choose(drm, &base()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unsupported);
        assert!(e.message.contains("DRM"), "{}", e.message);
        let webm = r#"<MPD><Period><AdaptationSet mimeType="video/webm"><Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(choose(webm, &base()).unwrap_err().message.contains("WebM"));
        assert!(choose("<html>", &base()).is_err());
        let no_duration = r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate media="$Number$.m4s" duration="2"/><Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(
            choose(no_duration, &base())
                .unwrap_err()
                .message
                .contains("duration")
        );
    }
}
