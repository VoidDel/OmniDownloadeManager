//! Segments of a multi-connection download and work stealing (docs/development-plan.md §4.2.3).
//!
//! A download starts as `n` equal segments, one per connection. A connection that finishes its
//! segment takes an unfinished one nobody is on (after a resume there are more segments than
//! connections), or else splits the segment with the most bytes left and takes its second half.
//! The connection on the split segment notices because its next block no longer fits
//! ([`SegmentTable::reserve`] returns less than asked) and stops there.
//!
//! Bytes go through two steps so that saved progress never runs ahead of the file: a connection
//! [`reserve`](SegmentTable::reserve)s the bytes it received, queues them to the writer, then
//! [`commit`](SegmentTable::commit)s them. `downloaded` (what is saved) counts committed bytes;
//! splits start after reserved ones.

use serde::{Deserialize, Serialize};

/// Files smaller than twice this use one connection; first segments are at least this long.
pub const MIN_SEGMENT: u64 = 1 << 20;
/// A segment is split for an idle connection only while at least this much of it is left.
pub const MIN_STEAL: u64 = 2 << 20;

/// Bytes `start..=end` of the file (`end` is `None` while the length is unknown), of which the
/// first `downloaded` are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start: u64,
    pub end: Option<u64>,
    pub downloaded: u64,
}

impl Segment {
    /// Offset of the next byte to download.
    pub fn pos(&self) -> u64 {
        self.start + self.downloaded
    }

    /// Bytes left, `None` while the length is unknown.
    pub fn remaining(&self) -> Option<u64> {
        self.end.map(|e| (e + 1).saturating_sub(self.pos()))
    }

    pub fn is_done(&self) -> bool {
        self.remaining() == Some(0)
    }
}

/// Splits a download of `total` bytes (or unknown length) for `connections` connections.
/// An empty file has no segments.
pub fn plan(total: Option<u64>, connections: u32) -> Vec<Segment> {
    let Some(total) = total else {
        return vec![Segment {
            start: 0,
            end: None,
            downloaded: 0,
        }];
    };
    if total == 0 {
        return Vec::new();
    }
    let n = (total / MIN_SEGMENT).clamp(1, u64::from(connections.max(1)));
    let size = total / n;
    (0..n)
        .map(|i| {
            let start = i * size;
            let end = if i == n - 1 {
                total - 1
            } else {
                start + size - 1
            };
            Segment {
                start,
                end: Some(end),
                downloaded: 0,
            }
        })
        .collect()
}

/// A segment as the details panel draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    pub start: u64,
    pub end: Option<u64>,
    pub downloaded: u64,
    /// A connection is on it.
    pub active: bool,
}

#[derive(Debug, Clone)]
struct Slot {
    seg: Segment,
    /// Reserved by the connection but not yet committed.
    inflight: u64,
    active: bool,
}

impl Slot {
    /// Bytes nobody has reserved yet.
    fn unreserved(&self) -> Option<u64> {
        self.seg
            .end
            .map(|e| (e + 1).saturating_sub(self.seg.pos() + self.inflight))
    }
}

/// The segments of a running download. Indices stay valid: splits append.
#[derive(Debug, Clone, Default)]
pub struct SegmentTable {
    slots: Vec<Slot>,
}

impl SegmentTable {
    /// A table of segments nobody works on yet.
    pub fn new(segments: Vec<Segment>) -> Self {
        Self {
            slots: segments
                .into_iter()
                .map(|seg| Slot {
                    seg,
                    inflight: 0,
                    active: false,
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn segment(&self, idx: usize) -> Segment {
        self.slots[idx].seg
    }

    /// Work for an idle connection: an unfinished segment nobody is on, else the second half of
    /// the segment with the most left. Marks it taken. `None`: nothing worth splitting.
    pub fn claim(&mut self) -> Option<usize> {
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| !s.active && !s.seg.is_done())
        {
            self.slots[i].active = true;
            return Some(i);
        }
        let (i, left) = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.active)
            .filter_map(|(i, s)| s.unreserved().map(|r| (i, r)))
            .max_by_key(|&(_, r)| r)?;
        if left < MIN_STEAL {
            return None;
        }
        let s = &mut self.slots[i];
        let mid = s.seg.pos() + s.inflight + left / 2;
        let end = s.seg.end?;
        s.seg.end = Some(mid - 1);
        self.slots.push(Slot {
            seg: Segment {
                start: mid,
                end: Some(end),
                downloaded: 0,
            },
            inflight: 0,
            active: true,
        });
        Some(self.slots.len() - 1)
    }

    /// The connection on `idx` received `len` bytes at its position: reserves and returns how
    /// many of them belong to the segment. Less than `len` (possibly 0) means the segment ends
    /// there, perhaps because it was split: write that many and stop.
    pub fn reserve(&mut self, idx: usize, len: u64) -> u64 {
        let s = &mut self.slots[idx];
        let n = s.unreserved().map_or(len, |left| len.min(left));
        s.inflight += n;
        n
    }

    /// The reserved bytes are queued to the writer: they count as downloaded.
    pub fn commit(&mut self, idx: usize, len: u64) {
        let s = &mut self.slots[idx];
        let len = len.min(s.inflight);
        s.inflight -= len;
        s.seg.downloaded += len;
    }

    /// The connection on `idx` stopped (finished, failed or cancelled); uncommitted bytes are
    /// dropped and someone else may take what is left.
    pub fn release(&mut self, idx: usize) {
        if let Some(s) = self.slots.get_mut(idx) {
            s.inflight = 0;
            s.active = false;
        }
    }

    /// Forgets what segment `idx` downloaded (a server without ranges can only start over).
    pub fn reset(&mut self, idx: usize) {
        if let Some(s) = self.slots.get_mut(idx) {
            s.seg.downloaded = 0;
            s.inflight = 0;
        }
    }

    /// An unknown-length download ended: its length is what arrived. Releases the segment. An
    /// empty body leaves no segments (an empty file is complete).
    pub fn finish_unknown(&mut self, idx: usize) {
        let Some(s) = self.slots.get_mut(idx) else {
            return;
        };
        if s.seg.end.is_none() {
            if s.seg.pos() == 0 {
                self.slots.clear();
                return;
            }
            s.seg.end = Some(s.seg.pos() - 1);
        }
        s.inflight = 0;
        s.active = false;
    }

    /// Committed bytes.
    pub fn downloaded(&self) -> u64 {
        self.slots.iter().map(|s| s.seg.downloaded).sum()
    }

    /// Every segment is done (unknown-length segments never are; see `finish_unknown`).
    pub fn is_complete(&self) -> bool {
        self.slots.iter().all(|s| s.seg.is_done())
    }

    /// Connections working.
    pub fn active(&self) -> usize {
        self.slots.iter().filter(|s| s.active).count()
    }

    /// The segments in file order, as saved.
    pub fn segments(&self) -> Vec<Segment> {
        let mut v: Vec<Segment> = self.slots.iter().map(|s| s.seg).collect();
        v.sort_by_key(|s| s.start);
        v
    }

    /// The segments in file order, for the UI.
    pub fn views(&self) -> Vec<SegmentView> {
        let mut v: Vec<SegmentView> = self
            .slots
            .iter()
            .map(|s| SegmentView {
                start: s.seg.start,
                end: s.seg.end,
                downloaded: s.seg.downloaded,
                active: s.active,
            })
            .collect();
        v.sort_by_key(|s| s.start);
        v
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// The segments tile `0..total` exactly and none has more downloaded than its length.
    fn assert_tiles(segs: &[Segment], total: u64) {
        let mut next = 0;
        for s in segs {
            assert_eq!(s.start, next, "gap or overlap in {segs:?}");
            let end = s.end.expect("known length");
            assert!(end >= s.start, "empty segment in {segs:?}");
            assert!(s.downloaded <= end - s.start + 1);
            next = end + 1;
        }
        assert_eq!(next, total);
    }

    #[test]
    fn plans_by_size() {
        assert_eq!(
            plan(Some(100), 8).len(),
            1,
            "small files use one connection"
        );
        assert_eq!(plan(Some(3 * MIN_SEGMENT), 8).len(), 3);
        assert_eq!(plan(Some(5 << 30), 8).len(), 8);
        assert!(plan(Some(0), 8).is_empty());
        assert_eq!(plan(None, 8)[0].end, None);
    }

    #[test]
    fn steals_the_second_half_of_the_largest_segment() {
        let mut t = SegmentTable::new(plan(Some(16 * MIN_SEGMENT), 2));
        let a = t.claim().unwrap();
        let b = t.claim().unwrap();
        // `a` finishes; `b` has read 1 MiB.
        let len_a = t.segment(a).remaining().unwrap();
        assert_eq!(t.reserve(a, len_a), len_a);
        t.commit(a, len_a);
        assert_eq!(t.reserve(b, MIN_SEGMENT), MIN_SEGMENT);
        t.commit(b, MIN_SEGMENT);
        t.release(a);
        let c = t.claim().unwrap();
        assert_eq!(c, 2);
        // b had 7 MiB left: it keeps 3.5, c takes 3.5.
        assert_eq!(t.segment(b).remaining(), Some(7 * MIN_SEGMENT / 2));
        assert_eq!(t.segment(c).remaining(), Some(7 * MIN_SEGMENT / 2));
        // A block b received across the new boundary is cut there.
        let left = t.segment(b).remaining().unwrap();
        assert_eq!(t.reserve(b, left + 100), left);
        assert_tiles(&t.segments(), 16 * MIN_SEGMENT);
    }

    #[test]
    fn idle_connections_take_abandoned_segments_first() {
        let mut segs = plan(Some(8 * MIN_SEGMENT), 4);
        segs[1].downloaded = segs[1].end.unwrap() - segs[1].start + 1;
        let mut t = SegmentTable::new(segs);
        let picked: Vec<_> = std::iter::from_fn(|| t.claim()).take(3).collect();
        assert_eq!(picked, [0, 2, 3], "the finished segment is skipped");
    }

    #[test]
    fn unknown_length_is_one_segment_until_it_ends() {
        let mut t = SegmentTable::new(plan(None, 8));
        let a = t.claim().unwrap();
        assert_eq!(t.claim(), None, "nothing to split");
        assert_eq!(t.reserve(a, 500), 500);
        t.commit(a, 500);
        assert!(!t.is_complete());
        t.finish_unknown(a);
        assert!(t.is_complete());
        assert_eq!(t.segments()[0].end, Some(499));
    }

    proptest! {
        #[test]
        fn plan_tiles_the_file(total in 1u64..(1 << 40), connections in 1u32..64) {
            let segs = plan(Some(total), connections);
            prop_assert!(!segs.is_empty() && segs.len() <= connections as usize);
            assert_tiles(&segs, total);
        }

        /// Connections reading blocks, finishing, failing and stealing in any order keep the
        /// segments tiling the file, and draining them downloads every byte exactly once.
        #[test]
        fn stealing_keeps_the_file_covered(
            total in (2 * MIN_SEGMENT)..(64 * MIN_SEGMENT),
            connections in 1u32..16,
            ops in proptest::collection::vec((0u8..4, 0usize..16, 1u64..(3 * MIN_SEGMENT)), 1..300),
        ) {
            let mut t = SegmentTable::new(plan(Some(total), connections));
            let mut workers: Vec<usize> = (0..connections).filter_map(|_| t.claim()).collect();
            let mut committed = 0u64;
            for (op, w, len) in ops {
                if workers.is_empty() {
                    break;
                }
                let w = w % workers.len();
                let idx = workers[w];
                match op {
                    // Read a block; a short reservation ends the segment.
                    0 | 1 => {
                        let n = t.reserve(idx, len);
                        t.commit(idx, n);
                        committed += n;
                        if n < len {
                            t.release(idx);
                            workers.swap_remove(w);
                            workers.extend(t.claim());
                        }
                    }
                    // A connection fails: its segment goes back to the pool.
                    2 => {
                        t.release(idx);
                        workers.swap_remove(w);
                    }
                    // A new connection joins.
                    _ => workers.extend(t.claim()),
                }
                assert_tiles(&t.segments(), total);
                prop_assert_eq!(t.downloaded(), committed);
            }
            // Drain: one connection at a time finishes everything.
            for idx in workers.drain(..) {
                t.release(idx);
            }
            while let Some(idx) = t.claim() {
                let n = t.reserve(idx, u64::MAX);
                t.commit(idx, n);
                committed += n;
                t.release(idx);
            }
            prop_assert!(t.is_complete());
            prop_assert_eq!(committed, total);
            assert_tiles(&t.segments(), total);
        }
    }
}
