//! The writer actor (docs/development-plan.md §4.2.4): one thread per download owns the file;
//! connections send it `(offset, bytes)` through a bounded channel. One owner means no
//! contention on the handle, back-pressure when the disk is slower than the network, and a
//! single place that knows what is on disk.

use std::{
    fs::File,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

/// Blocks queued between the connections and the disk.
const QUEUE: usize = 64;
/// Adjacent blocks already queued are merged into one write up to this size.
const MERGE_LIMIT: usize = 4 << 20;

enum Cmd {
    Write {
        offset: u64,
        data: Bytes,
    },
    /// Replies once every block queued before it is written (and synced, with `sync`).
    Flush {
        sync: bool,
        done: oneshot::Sender<io::Result<()>>,
    },
    /// Like `Flush` with `sync`, then closes the file and stops.
    Close {
        done: oneshot::Sender<io::Result<()>>,
    },
}

#[derive(Default)]
struct Shared {
    written: AtomicU64,
    /// The write error that stopped the thread.
    failure: Mutex<Option<(io::ErrorKind, String)>>,
}

/// Handle to a writer thread. Clones share it; the thread ends (closing the file) on
/// [`close`](Self::close), on a write error, or when every handle is dropped.
#[derive(Clone)]
pub struct FileWriter {
    tx: mpsc::Sender<Cmd>,
    shared: Arc<Shared>,
}

impl FileWriter {
    /// Starts the writer thread for `file`.
    pub fn spawn(file: File) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel(QUEUE);
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        std::thread::Builder::new()
            .name("omnidl-writer".into())
            .spawn(move || run(file, rx, &s))?;
        Ok(Self { tx, shared })
    }

    /// Queues `data` to be written at `offset`. Waits while the queue is full.
    pub async fn write(&self, offset: u64, data: Bytes) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.tx
            .send(Cmd::Write { offset, data })
            .await
            .map_err(|_| self.failure())
    }

    /// Waits until every block queued so far is written, and with `sync` on the disk
    /// (`sync_data`). Progress saved after this never claims bytes that are not on disk.
    pub async fn flush(&self, sync: bool) -> io::Result<()> {
        let (done, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Flush { sync, done })
            .await
            .map_err(|_| self.failure())?;
        rx.await.map_err(|_| self.failure())?
    }

    /// Writes and syncs what is queued, then closes the file (so it can be renamed). Later
    /// writes fail.
    pub async fn close(&self) -> io::Result<()> {
        let (done, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Close { done })
            .await
            .map_err(|_| self.failure())?;
        rx.await.map_err(|_| self.failure())?
    }

    /// Bytes written to the file so far.
    pub fn written(&self) -> u64 {
        self.shared.written.load(Ordering::Relaxed)
    }

    fn failure(&self) -> io::Error {
        match &*self
            .shared
            .failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            Some((kind, msg)) => io::Error::new(*kind, msg.clone()),
            None => io::Error::new(io::ErrorKind::BrokenPipe, "the file writer has stopped"),
        }
    }
}

fn run(file: File, mut rx: mpsc::Receiver<Cmd>, shared: &Shared) {
    let mut next: Option<Cmd> = None;
    loop {
        let cmd = match next.take() {
            Some(c) => c,
            None => match rx.blocking_recv() {
                Some(c) => c,
                None => return,
            },
        };
        match cmd {
            Cmd::Write { offset, data } => {
                // Merge the blocks that continue this one and are already waiting.
                let mut merged: Option<Vec<u8>> = None;
                let mut end = offset + data.len() as u64;
                while merged.as_ref().map_or(data.len(), Vec::len) < MERGE_LIMIT {
                    match rx.try_recv() {
                        Ok(Cmd::Write { offset: o, data: d }) if o == end => {
                            merged
                                .get_or_insert_with(|| data.to_vec())
                                .extend_from_slice(&d);
                            end += d.len() as u64;
                        }
                        Ok(other) => {
                            next = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                let buf = merged.as_deref().unwrap_or(&data);
                if let Err(e) = write_at(&file, buf, offset) {
                    tracing::error!("writing at {offset}: {e}");
                    *shared.failure.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((e.kind(), e.to_string()));
                    return;
                }
                shared
                    .written
                    .fetch_add(buf.len() as u64, Ordering::Relaxed);
            }
            // Everything queued before this is written. The sync runs on a clone of the handle
            // in its own thread, so writing goes on meanwhile; it covers at least those writes.
            Cmd::Flush { sync: false, done } => {
                let _ = done.send(Ok(()));
            }
            Cmd::Flush { sync: true, done } => match file.try_clone() {
                Ok(f) => {
                    let spawned = std::thread::Builder::new()
                        .name("omnidl-sync".into())
                        .spawn(move || {
                            let _ = done.send(f.sync_data());
                        });
                    if let Err(e) = spawned {
                        tracing::warn!("sync thread: {e}");
                    }
                }
                Err(e) => {
                    let _ = done.send(Err(e));
                }
            },
            Cmd::Close { done } => {
                let r = file.sync_data();
                drop(file);
                let _ = done.send(r);
                return;
            }
        }
    }
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file;

    #[tokio::test(flavor = "multi_thread")]
    async fn writes_blocks_from_many_senders_in_any_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.odm");
        let len = 1 << 20;
        let w = FileWriter::spawn(file::create(&path, Some(len)).unwrap()).unwrap();
        let expected: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let expected = Bytes::from(expected);

        // Four senders, each writing its quarter in 4 KiB blocks, last block first.
        let mut joins = Vec::new();
        for part in 0..4u64 {
            let w = w.clone();
            let data = expected.clone();
            joins.push(tokio::spawn(async move {
                let q = len / 4;
                let mut offsets: Vec<u64> = (part * q..(part + 1) * q).step_by(4096).collect();
                offsets.reverse();
                for o in offsets {
                    let o = o as usize;
                    w.write(o as u64, data.slice(o..o + 4096)).await.unwrap();
                }
            }));
        }
        for j in joins {
            j.await.unwrap();
        }
        w.flush(true).await.unwrap();
        assert_eq!(w.written(), len);
        w.close().await.unwrap();
        assert!(w.write(0, Bytes::from_static(b"x")).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), expected);
    }

    #[tokio::test]
    async fn merges_adjacent_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.odm");
        let w = FileWriter::spawn(file::create(&path, None).unwrap()).unwrap();
        for i in 0..100u8 {
            w.write(u64::from(i) * 3, Bytes::from(vec![i; 3]))
                .await
                .unwrap();
        }
        w.close().await.unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len(), 300);
        assert!(data.chunks(3).enumerate().all(|(i, c)| c == [i as u8; 3]));
    }
}
