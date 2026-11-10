//! What a `.torrent` says, read without a session (the details of a task that is not running).

use librqbit::torrent_from_bytes;
use omnidl_core::{Error, ErrorCode, ProbeFile, Result, filename};

/// The parts of a torrent's metadata the app shows.
#[derive(Debug, Clone)]
pub(crate) struct Meta {
    pub info_hash: String,
    pub name: Option<String>,
    pub files: Vec<ProbeFile>,
    pub piece_size: u64,
    pub pieces: u32,
    pub trackers: Vec<String>,
    /// Only the torrent's own tracker may be used (BEP 27).
    pub private: bool,
}

impl Meta {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let invalid = |e: &dyn std::fmt::Display| {
            Error::new(
                ErrorCode::InvalidRequest,
                format!("not a valid torrent: {e}"),
            )
        };
        let t = torrent_from_bytes(bytes).map_err(|e| invalid(&e))?;
        let private = t.info.data.private;
        let info = t.info.data.clone().validate().map_err(|e| invalid(&e))?;
        let files = info
            .iter_file_details()
            .map(|f| ProbeFile {
                path: f.filename.to_vec().join("/"),
                size: f.len,
            })
            .collect();
        let mut trackers: Vec<String> = Vec::new();
        for t in t.iter_announce() {
            if let Ok(s) = std::str::from_utf8(t.as_ref())
                && !trackers.iter().any(|x| x == s)
            {
                trackers.push(s.to_owned());
            }
        }
        Ok(Self {
            info_hash: t.info_hash.as_string(),
            name: info.name().map(|n| n.into_owned()),
            files,
            piece_size: info.lengths().default_piece_length().into(),
            pieces: info.lengths().total_pieces(),
            trackers,
            private,
        })
    }

    pub fn size(&self, only: Option<&[usize]>) -> u64 {
        self.files
            .iter()
            .enumerate()
            .filter(|(i, _)| only.is_none_or(|o| o.contains(i)))
            .map(|(_, f)| f.size)
            .sum()
    }

    /// The task's name and where the torrent is written under the save folder: a single file
    /// goes straight in (its name is the task's), several files go in a folder named after the
    /// torrent.
    pub fn layout(&self, fallback: &str) -> Layout {
        if let [file] = self.files.as_slice() {
            let first = file.path.split('/').next().unwrap_or_default();
            return Layout {
                name: first.to_owned(),
                subfolder: None,
            };
        }
        let name = filename::sanitize(self.name.as_deref().unwrap_or(fallback));
        Layout {
            name: name.clone(),
            subfolder: Some(name),
        }
    }
}

pub(crate) struct Layout {
    /// What `Task::file_name` holds: the file, or the folder.
    pub name: String,
    pub subfolder: Option<String>,
}
