//! Joins two fragmented MP4 streams, typically the picture and the sound a site serves apart
//! (DASH's `.m4s` files, an HLS stream with its own audio playlist), into one MP4 file holding
//! both tracks. Nothing is decoded: the init sections' tracks are put into one `moov`, the
//! second stream's tracks get new IDs, and the fragments (`moof` + `mdat`) of both are copied
//! in order of their decode time, their sequence numbers and track IDs rewritten (absolute
//! data offsets moved along). Indexes that would point into the old files (`sidx`, `mfra`) are
//! left out.
//!
//! Only fragmented MP4 is joined (an MP4 with its samples in the `moov` is not: sites do not
//! serve split streams that way).

use std::{
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    ops::Range,
    path::Path,
};

use omnidl_core::{Error, ErrorCode, Result};

type FourCc = [u8; 4];

/// A box in a byte buffer: its type, where it starts, where its payload starts, where it ends.
#[derive(Debug, Clone, Copy)]
struct BoxAt {
    kind: FourCc,
    start: usize,
    body: usize,
    end: usize,
}

fn bad(why: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unsupported, why)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| bad("a broken MP4 box"))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    b.get(at..at + 8)
        .map(|s| u64::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| bad("a broken MP4 box"))
}

fn put_u32(b: &mut [u8], at: usize, v: u32) -> Result<()> {
    b.get_mut(at..at + 4)
        .ok_or_else(|| bad("a broken MP4 box"))?
        .copy_from_slice(&v.to_be_bytes());
    Ok(())
}

fn put_u64(b: &mut [u8], at: usize, v: u64) -> Result<()> {
    b.get_mut(at..at + 8)
        .ok_or_else(|| bad("a broken MP4 box"))?
        .copy_from_slice(&v.to_be_bytes());
    Ok(())
}

/// The boxes in `b[range]`, one level.
fn children(b: &[u8], range: Range<usize>) -> Result<Vec<BoxAt>> {
    let mut out = Vec::new();
    let mut at = range.start;
    while at + 8 <= range.end {
        let size = u32_at(b, at)? as usize;
        let kind: FourCc = b[at + 4..at + 8].try_into().unwrap();
        let (body, end) = match size {
            0 => (at + 8, range.end),
            1 => (at + 16, at + u64_at(b, at + 8)? as usize),
            n => (at + 8, at + n),
        };
        if end < body || end > range.end {
            return Err(bad("a broken MP4 box"));
        }
        out.push(BoxAt {
            kind,
            start: at,
            body,
            end,
        });
        at = end;
    }
    Ok(out)
}

fn find(b: &[u8], range: Range<usize>, kind: &FourCc) -> Result<Option<BoxAt>> {
    Ok(children(b, range)?.into_iter().find(|x| &x.kind == kind))
}

/// A box of `kind` around `payload`.
fn make_box(kind: &FourCc, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

// ------------------------------------------------------------------------- one stream

struct Track {
    id: u32,
    timescale: u32,
    handler: FourCc,
    /// The whole `trak` box.
    trak: Vec<u8>,
}

/// The init section of one stream (`ftyp`, `moov`).
struct Init {
    ftyp: Option<Vec<u8>>,
    /// The whole `mvhd` box.
    mvhd: Vec<u8>,
    tracks: Vec<Track>,
    /// `trex` boxes by track ID.
    trex: Vec<(u32, Vec<u8>)>,
    /// `mehd`'s fragment duration (in the movie's timescale), if there is one.
    fragment_duration: Option<u64>,
    /// The other boxes of the `moov` (`udta`, `meta`…), kept from the first stream.
    others: Vec<Vec<u8>>,
}

/// A fragment: its `moof` and what follows up to the next one (its `mdat`).
struct Fragment {
    /// Which stream (0 or 1).
    stream: usize,
    /// Where the `moof` starts in its stream (absolute data offsets count from the stream's
    /// start).
    stream_pos: u64,
    /// The `moof` box.
    moof: Vec<u8>,
    /// The bytes after the `moof` to copy as they are, in the source file.
    rest: Range<u64>,
    /// Decode time of its first sample, in seconds (`tfdt`), when known.
    time: Option<f64>,
}

fn read_exact_at(f: &mut File, pos: u64, len: usize) -> std::io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(pos))?;
    let mut buf = vec![0; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// A box header in the file at `pos`: `(type, header length, total length)`; `None` at the end.
fn header_at(f: &mut File, pos: u64, end: u64) -> std::io::Result<Option<(FourCc, u64, u64)>> {
    if pos + 8 > end {
        return Ok(None);
    }
    let h = read_exact_at(f, pos, 8)?;
    let size = u32::from_be_bytes(h[..4].try_into().unwrap()) as u64;
    let kind: FourCc = h[4..8].try_into().unwrap();
    Ok(Some(match size {
        0 => (kind, 8, end - pos),
        1 => {
            let l = read_exact_at(f, pos + 8, 8)?;
            (kind, 16, u64::from_be_bytes(l[..8].try_into().unwrap()))
        }
        n => (kind, 8, n),
    }))
}

fn parse_init(moov: &[u8], ftyp: Option<Vec<u8>>) -> Result<Init> {
    let all = 0..moov.len();
    let top = children(moov, all.clone())?;
    let moov_box = top
        .first()
        .filter(|b| &b.kind == b"moov")
        .ok_or_else(|| bad("no moov"))?;
    let mut init = Init {
        ftyp,
        mvhd: Vec::new(),
        tracks: Vec::new(),
        trex: Vec::new(),
        fragment_duration: None,
        others: Vec::new(),
    };
    let mut fragmented = false;
    for c in children(moov, moov_box.body..moov_box.end)? {
        let raw = moov[c.start..c.end].to_vec();
        match &c.kind {
            b"mvhd" => init.mvhd = raw,
            b"trak" => init.tracks.push(parse_trak(raw)?),
            b"mvex" => {
                fragmented = true;
                for m in children(moov, c.body..c.end)? {
                    match &m.kind {
                        b"trex" => init
                            .trex
                            .push((u32_at(moov, m.body + 4)?, moov[m.start..m.end].to_vec())),
                        b"mehd" => {
                            init.fragment_duration = Some(if moov[m.body] == 1 {
                                u64_at(moov, m.body + 4)?
                            } else {
                                u32_at(moov, m.body + 4)? as u64
                            })
                        }
                        _ => {}
                    }
                }
            }
            _ => init.others.push(raw),
        }
    }
    if init.mvhd.is_empty() || init.tracks.is_empty() {
        return Err(bad("an MP4 without tracks"));
    }
    if !fragmented {
        return Err(bad("not a fragmented MP4: cannot join it"));
    }
    Ok(init)
}

fn parse_trak(trak: Vec<u8>) -> Result<Track> {
    let t = children(&trak, 0..trak.len())?[0];
    let tkhd = find(&trak, t.body..t.end, b"tkhd")?.ok_or_else(|| bad("a track without tkhd"))?;
    let id = u32_at(
        &trak,
        tkhd.body + if trak[tkhd.body] == 1 { 20 } else { 12 },
    )?;
    let mdia = find(&trak, t.body..t.end, b"mdia")?.ok_or_else(|| bad("a track without mdia"))?;
    let mdhd =
        find(&trak, mdia.body..mdia.end, b"mdhd")?.ok_or_else(|| bad("a track without mdhd"))?;
    let timescale = u32_at(
        &trak,
        mdhd.body + if trak[mdhd.body] == 1 { 20 } else { 12 },
    )?;
    let handler = match find(&trak, mdia.body..mdia.end, b"hdlr")? {
        Some(h) => trak
            .get(h.body + 8..h.body + 12)
            .and_then(|s| s.try_into().ok())
            .unwrap_or(*b"    "),
        None => *b"    ",
    };
    Ok(Track {
        id,
        timescale,
        handler,
        trak,
    })
}

/// The track ID and decode time of a `moof`'s first `traf`.
fn first_time(moof: &[u8]) -> Result<Option<(u32, u64)>> {
    let m = children(moof, 0..moof.len())?[0];
    let Some(traf) = find(moof, m.body..m.end, b"traf")? else {
        return Ok(None);
    };
    let Some(tfhd) = find(moof, traf.body..traf.end, b"tfhd")? else {
        return Ok(None);
    };
    let id = u32_at(moof, tfhd.body + 4)?;
    Ok(find(moof, traf.body..traf.end, b"tfdt")?
        .map(|d| {
            if moof[d.body] == 1 {
                u64_at(moof, d.body + 4)
            } else {
                u32_at(moof, d.body + 4).map(u64::from)
            }
        })
        .transpose()?
        .map(|t| (id, t)))
}

/// One stream in `range` of the file: its init section and fragments.
fn read_stream(f: &mut File, range: Range<u64>, stream: usize) -> Result<(Init, Vec<Fragment>)> {
    let io = |e: std::io::Error| Error::io(&e, "reading the downloaded streams");
    let mut ftyp = None;
    let mut init: Option<Init> = None;
    let mut frags: Vec<Fragment> = Vec::new();
    let mut pos = range.start;
    while let Some((kind, _, len)) = header_at(f, pos, range.end).map_err(io)? {
        if len < 8 || pos + len > range.end {
            return Err(bad("a broken MP4 box"));
        }
        match &kind {
            b"ftyp" if ftyp.is_none() => {
                ftyp = Some(read_exact_at(f, pos, len as usize).map_err(io)?)
            }
            // HLS repeats nothing, but a second init section would only be the same one.
            b"moov" if init.is_none() => {
                let moov = read_exact_at(f, pos, len as usize).map_err(io)?;
                init = Some(parse_init(&moov, ftyp.clone())?);
            }
            b"moof" => {
                let moof = read_exact_at(f, pos, len as usize).map_err(io)?;
                frags.push(Fragment {
                    stream,
                    stream_pos: pos - range.start,
                    rest: pos + len..pos + len,
                    time: None,
                    moof,
                });
            }
            // Data of the fragment before (and whatever lies between it and its moof).
            b"mdat" => {
                if let Some(last) = frags.last_mut() {
                    last.rest.end = pos + len;
                }
            }
            // Indexes into this file, segment types, padding: not copied.
            _ => {}
        }
        pos += len;
    }
    let init = init.ok_or_else(|| bad("no MP4 init section"))?;
    if frags.is_empty() {
        return Err(bad("an MP4 without fragments"));
    }
    for fr in &mut frags {
        fr.time = first_time(&fr.moof)?.and_then(|(id, t)| {
            let ts = init.tracks.iter().find(|x| x.id == id)?.timescale;
            (ts > 0).then(|| t as f64 / ts as f64)
        });
    }
    Ok((init, frags))
}

// ------------------------------------------------------------------------- joining

/// `trak` with its track ID set.
fn with_track_id(trak: &[u8], id: u32) -> Result<Vec<u8>> {
    let mut out = trak.to_vec();
    let t = children(&out, 0..out.len())?[0];
    let tkhd = find(&out, t.body..t.end, b"tkhd")?.ok_or_else(|| bad("a track without tkhd"))?;
    let at = tkhd.body + if out[tkhd.body] == 1 { 20 } else { 12 };
    put_u32(&mut out, at, id)?;
    Ok(out)
}

/// The joined `moov`: the first stream's header, both streams' tracks, both `trex`.
fn joined_moov(a: &Init, b: &Init, b_ids: &[(u32, u32)]) -> Result<Vec<u8>> {
    let movie_scale = |mvhd: &[u8]| -> Result<u32> {
        let at = 8 + if mvhd[8] == 1 { 20 } else { 12 };
        u32_at(mvhd, at)
    };
    let (sa, sb) = (movie_scale(&a.mvhd)?, movie_scale(&b.mvhd)?);
    let scaled = |d: u64| {
        if sb == 0 {
            d
        } else {
            (d as u128 * sa as u128 / sb as u128) as u64
        }
    };
    let next_id = a
        .tracks
        .iter()
        .map(|t| t.id)
        .chain(b_ids.iter().map(|&(_, n)| n))
        .max()
        .unwrap_or(0)
        + 1;

    // mvhd: the longer duration of the two, and the next free track ID (its last field).
    let mut mvhd = a.mvhd.clone();
    let v1 = mvhd[8] == 1;
    let dur_at = 8 + if v1 { 24 } else { 16 };
    let dur = |m: &[u8]| -> Result<u64> {
        if m[8] == 1 {
            u64_at(m, 8 + 24)
        } else {
            u32_at(m, 8 + 16).map(u64::from)
        }
    };
    let longest = dur(&a.mvhd)?.max(scaled(dur(&b.mvhd)?));
    if v1 {
        put_u64(&mut mvhd, dur_at, longest)?;
    } else {
        put_u32(&mut mvhd, dur_at, longest.min(u32::MAX as u64) as u32)?;
    }
    let last = mvhd.len() - 4;
    put_u32(&mut mvhd, last, next_id)?;

    let mut body = mvhd;
    for t in &a.tracks {
        body.extend_from_slice(&t.trak);
    }
    for t in &b.tracks {
        let new = b_ids
            .iter()
            .find(|(o, _)| *o == t.id)
            .map_or(t.id, |&(_, n)| n);
        body.extend_from_slice(&with_track_id(&t.trak, new)?);
    }
    let mut mvex = Vec::new();
    let longest_fragments = match (a.fragment_duration, b.fragment_duration) {
        (None, None) => None,
        (x, y) => Some(x.unwrap_or(0).max(scaled(y.unwrap_or(0)))),
    };
    if let Some(d) = longest_fragments {
        let mut p = vec![1, 0, 0, 0];
        p.extend_from_slice(&d.to_be_bytes());
        mvex.extend_from_slice(&make_box(b"mehd", &p));
    }
    for (_, trex) in &a.trex {
        mvex.extend_from_slice(trex);
    }
    for (old, trex) in &b.trex {
        let mut t = trex.clone();
        let new = b_ids
            .iter()
            .find(|(o, _)| o == old)
            .map_or(*old, |&(_, n)| n);
        put_u32(&mut t, 12, new)?;
        mvex.extend_from_slice(&t);
    }
    body.extend_from_slice(&make_box(b"mvex", &mvex));
    for o in &a.others {
        body.extend_from_slice(o);
    }
    Ok(make_box(b"moov", &body))
}

/// The `moof` rewritten for its place in the joined file: sequence number, track IDs, and
/// absolute data offsets moved by `shift`.
fn rewrite_moof(moof: &[u8], seq: u32, ids: &[(u32, u32)], shift: i128) -> Result<Vec<u8>> {
    let mut out = moof.to_vec();
    let m = children(&out, 0..out.len())?[0];
    for c in children(&out, m.body..m.end)? {
        match &c.kind {
            b"mfhd" => put_u32(&mut out, c.body + 4, seq)?,
            b"traf" => {
                if let Some(tfhd) = find(&out, c.body..c.end, b"tfhd")? {
                    let flags = u32_at(&out, tfhd.body)? & 0x00ff_ffff;
                    let id = u32_at(&out, tfhd.body + 4)?;
                    if let Some(&(_, new)) = ids.iter().find(|(o, _)| *o == id) {
                        put_u32(&mut out, tfhd.body + 4, new)?;
                    }
                    if flags & 1 != 0 {
                        let old = u64_at(&out, tfhd.body + 8)? as i128;
                        let moved = u64::try_from(old + shift)
                            .map_err(|_| bad("a broken fragment offset"))?;
                        put_u64(&mut out, tfhd.body + 8, moved)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Joins the stream in `first` and the one in `second` (byte ranges of `src`) into `out`.
/// Picture and sound may come in either order; two streams of the same kind are refused.
pub(crate) fn join(src: &Path, first: Range<u64>, second: Range<u64>, out: &Path) -> Result<()> {
    let mut f = File::open(src).map_err(|e| Error::io(&e, src.display()))?;
    let (a, mut frags) = read_stream(&mut f, first, 0)?;
    let (b, b_frags) = read_stream(&mut f, second, 1)?;
    let kinds = |i: &Init| i.tracks.iter().map(|t| t.handler).collect::<Vec<_>>();
    if kinds(&a).iter().any(|k| kinds(&b).contains(k)) {
        return Err(bad("both streams carry the same kind of track"));
    }
    // The second stream's tracks after the first's.
    let mut next = a.tracks.iter().map(|t| t.id).max().unwrap_or(0);
    let b_ids: Vec<(u32, u32)> = b
        .tracks
        .iter()
        .map(|t| {
            next += 1;
            (t.id, next)
        })
        .collect();
    frags.extend(b_frags);
    // In decode order, so players read picture and sound side by side; without times, one
    // stream after the other.
    if frags.iter().all(|x| x.time.is_some()) {
        frags.sort_by(|x, y| x.time.unwrap().total_cmp(&y.time.unwrap()));
    }

    let io = |e: std::io::Error| Error::io(&e, out.display());
    let mut w = BufWriter::with_capacity(1 << 20, File::create(out).map_err(io)?);
    let ftyp = a.ftyp.as_ref().or(b.ftyp.as_ref());
    let mut pos = 0u64;
    if let Some(ftyp) = ftyp {
        w.write_all(ftyp).map_err(io)?;
        pos += ftyp.len() as u64;
    }
    let moov = joined_moov(&a, &b, &b_ids)?;
    w.write_all(&moov).map_err(io)?;
    pos += moov.len() as u64;
    let mut buf = vec![0u8; 1 << 20];
    for (i, fr) in frags.iter().enumerate() {
        let ids: &[(u32, u32)] = if fr.stream == 1 { &b_ids } else { &[] };
        let shift = pos as i128 - fr.stream_pos as i128;
        let moof = rewrite_moof(&fr.moof, i as u32 + 1, ids, shift)?;
        w.write_all(&moof).map_err(io)?;
        f.seek(SeekFrom::Start(fr.rest.start))
            .map_err(|e| Error::io(&e, src.display()))?;
        let mut left = fr.rest.end - fr.rest.start;
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            f.read_exact(&mut buf[..n])
                .map_err(|e| Error::io(&e, src.display()))?;
            w.write_all(&buf[..n]).map_err(io)?;
            left -= n as u64;
        }
        pos += moof.len() as u64 + (fr.rest.end - fr.rest.start);
    }
    w.flush().map_err(io)?;
    w.into_inner()
        .map_err(|e| io(e.into_error()))?
        .sync_all()
        .map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(kind: &FourCc, version: u8, flags: u32, rest: &[u8]) -> Vec<u8> {
        let mut p = vec![version];
        p.extend_from_slice(&flags.to_be_bytes()[1..]);
        p.extend_from_slice(rest);
        make_box(kind, &p)
    }

    /// An init section with one track: `id`, timescale, handler.
    fn init(id: u32, timescale: u32, handler: &FourCc) -> Vec<u8> {
        let mut mvhd = vec![0u8; 96];
        mvhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[12..16].copy_from_slice(&0u32.to_be_bytes());
        mvhd[92..96].copy_from_slice(&(id + 1).to_be_bytes());
        let mut tkhd = vec![0u8; 80];
        tkhd[8..12].copy_from_slice(&id.to_be_bytes());
        let mut mdhd = vec![0u8; 20];
        mdhd[8..12].copy_from_slice(&timescale.to_be_bytes());
        let mut hdlr = vec![0u8; 21];
        hdlr[4..8].copy_from_slice(handler);
        let mdia = make_box(
            b"mdia",
            &[full(b"mdhd", 0, 0, &mdhd), full(b"hdlr", 0, 0, &hdlr)].concat(),
        );
        let trak = make_box(b"trak", &[full(b"tkhd", 0, 3, &tkhd), mdia].concat());
        let mut trex = id.to_be_bytes().to_vec();
        trex.extend_from_slice(&[0u8; 16]);
        let mvex = make_box(b"mvex", &full(b"trex", 0, 0, &trex));
        let moov = make_box(b"moov", &[full(b"mvhd", 0, 0, &mvhd), trak, mvex].concat());
        [make_box(b"ftyp", b"isom\0\0\0\0isomiso6"), moov].concat()
    }

    /// A fragment of track `id` starting at `time`, its data `data`; with `absolute`, the data
    /// offset counts from the file's start (`moof_pos`, where the fragment will lie).
    fn fragment(id: u32, seq: u32, time: u64, data: &[u8], absolute: Option<u64>) -> Vec<u8> {
        let mfhd = full(b"mfhd", 0, 0, &seq.to_be_bytes());
        let tfdt = full(b"tfdt", 1, 0, &time.to_be_bytes());
        // One sample of `data.len()` bytes; data offset patched below.
        let mut trun_rest = 1u32.to_be_bytes().to_vec();
        trun_rest.extend_from_slice(&0i32.to_be_bytes());
        trun_rest.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let trun = full(b"trun", 0, 0x000201, &trun_rest);
        let build = |base: Option<u64>, off: i32| {
            let tfhd = match base {
                Some(b) => {
                    let mut r = id.to_be_bytes().to_vec();
                    r.extend_from_slice(&b.to_be_bytes());
                    full(b"tfhd", 0, 0x000001, &r)
                }
                None => full(b"tfhd", 0, 0x020000, &id.to_be_bytes()),
            };
            let mut trun = trun.clone();
            let at = trun.len() - 8;
            trun[at..at + 4].copy_from_slice(&off.to_be_bytes());
            make_box(
                b"moof",
                &[
                    mfhd.clone(),
                    make_box(b"traf", &[tfhd, tfdt.clone(), trun].concat()),
                ]
                .concat(),
            )
        };
        // The moof's size does not depend on the offsets: data starts right after it + 8.
        let probe = build(absolute, 0);
        let off = probe.len() as i32 + 8;
        let moof = match absolute {
            Some(moof_pos) => build(Some(moof_pos), off),
            None => build(None, off),
        };
        [moof, make_box(b"mdat", data)].concat()
    }

    /// A fragment as read back: sequence number, track ID, its data.
    type ReadFragment = (u32, u32, Vec<u8>);

    /// Reads back the joined file: the track IDs in its moov, and its fragments.
    fn read_back(b: &[u8]) -> (Vec<u32>, Vec<ReadFragment>) {
        let mut ids = Vec::new();
        let mut frags = Vec::new();
        for top in children(b, 0..b.len()).unwrap() {
            match &top.kind {
                b"moov" => {
                    for c in children(b, top.body..top.end).unwrap() {
                        if &c.kind == b"trak" {
                            ids.push(parse_trak(b[c.start..c.end].to_vec()).unwrap().id);
                        }
                    }
                }
                b"moof" => {
                    let seq = u32_at(
                        b,
                        find(b, top.body..top.end, b"mfhd").unwrap().unwrap().body + 4,
                    )
                    .unwrap();
                    let traf = find(b, top.body..top.end, b"traf").unwrap().unwrap();
                    let tfhd = find(b, traf.body..traf.end, b"tfhd").unwrap().unwrap();
                    let flags = u32_at(b, tfhd.body).unwrap() & 0xffffff;
                    let id = u32_at(b, tfhd.body + 4).unwrap();
                    let base = if flags & 1 != 0 {
                        u64_at(b, tfhd.body + 8).unwrap() as usize
                    } else {
                        top.start
                    };
                    let trun = find(b, traf.body..traf.end, b"trun").unwrap().unwrap();
                    let off = u32_at(b, trun.body + 8).unwrap() as usize;
                    let len = u32_at(b, trun.body + 12).unwrap() as usize;
                    frags.push((seq, id, b[base + off..base + off + len].to_vec()));
                }
                _ => {}
            }
        }
        (ids, frags)
    }

    #[test]
    fn joins_picture_and_sound_in_time_order() {
        // Picture: track 1, 90 kHz, fragments at 0 s and 2 s (absolute offsets). Sound: also
        // track 1, 48 kHz, at 0 s and 1 s, with an index box to leave out.
        let mut video = init(1, 90_000, b"vide");
        let at = video.len() as u64;
        video.extend(fragment(1, 1, 0, b"V0", Some(at)));
        let at = video.len() as u64;
        video.extend(fragment(1, 2, 180_000, b"V2", Some(at)));
        let mut audio = init(1, 48_000, b"soun");
        audio.extend(make_box(b"sidx", &[0u8; 24]));
        audio.extend(fragment(1, 1, 0, b"A0", None));
        audio.extend(fragment(1, 2, 48_000, b"A1", None));

        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("both");
        std::fs::write(&src, [video.clone(), audio.clone()].concat()).unwrap();
        let out = dir.path().join("out.mp4");
        let v = video.len() as u64;
        join(&src, 0..v, v..v + audio.len() as u64, &out).unwrap();

        let joined = std::fs::read(&out).unwrap();
        let (ids, frags) = read_back(&joined);
        assert_eq!(ids, [1, 2]);
        let got: Vec<(u32, u32, &[u8])> = frags
            .iter()
            .map(|(s, i, d)| (*s, *i, d.as_slice()))
            .collect();
        assert_eq!(
            got,
            [
                (1, 1, &b"V0"[..]),
                (2, 2, &b"A0"[..]),
                (3, 2, &b"A1"[..]),
                (4, 1, &b"V2"[..])
            ]
        );
        assert!(!joined.windows(4).any(|w| w == b"sidx"));
        // mvhd's next track ID follows the new one.
        let moov = children(&joined, 0..joined.len())
            .unwrap()
            .into_iter()
            .find(|b| &b.kind == b"moov")
            .unwrap();
        let mvhd = find(&joined, moov.body..moov.end, b"mvhd")
            .unwrap()
            .unwrap();
        assert_eq!(u32_at(&joined, mvhd.end - 4).unwrap(), 3);
    }

    #[test]
    fn sound_may_come_first_but_not_twice_the_same() {
        let mut a = init(1, 48_000, b"soun");
        a.extend(fragment(1, 1, 0, b"A", None));
        let mut v = init(1, 90_000, b"vide");
        v.extend(fragment(1, 1, 0, b"V", None));
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("s");
        std::fs::write(&src, [a.clone(), v.clone()].concat()).unwrap();
        let out = dir.path().join("o.mp4");
        let n = a.len() as u64;
        join(&src, 0..n, n..n + v.len() as u64, &out).unwrap();
        assert_eq!(read_back(&std::fs::read(&out).unwrap()).0, [1, 2]);

        std::fs::write(&src, [a.clone(), a.clone()].concat()).unwrap();
        let e = join(&src, 0..n, n..2 * n, &out).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unsupported);
    }

    /// Real streams: `OMNIDL_JOIN_A`, `OMNIDL_JOIN_B` (fragmented MP4 files), joined into
    /// `OMNIDL_JOIN_OUT` to play back. `cargo test -p omnidl-http joins_real -- --ignored`.
    #[test]
    #[ignore]
    fn joins_real_streams() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
        let (a, b) = (
            std::fs::read(var("OMNIDL_JOIN_A")).unwrap(),
            std::fs::read(var("OMNIDL_JOIN_B")).unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("both");
        std::fs::write(&src, [a.as_slice(), b.as_slice()].concat()).unwrap();
        let n = a.len() as u64;
        join(
            &src,
            0..n,
            n..n + b.len() as u64,
            Path::new(&var("OMNIDL_JOIN_OUT")),
        )
        .unwrap();
    }

    #[test]
    fn refuses_what_is_not_fragmented() {
        let plain = [
            make_box(b"ftyp", b"isom\0\0\0\0"),
            make_box(b"moov", &full(b"mvhd", 0, 0, &[0u8; 96])),
            make_box(b"mdat", b"x"),
        ]
        .concat();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("p");
        std::fs::write(&src, [plain.clone(), plain.clone()].concat()).unwrap();
        let n = plain.len() as u64;
        assert!(join(&src, 0..n, n..2 * n, &dir.path().join("o")).is_err());
    }
}
