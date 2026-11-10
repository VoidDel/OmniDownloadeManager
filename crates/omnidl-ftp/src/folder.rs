//! A folder on an FTP server with its subfolders: listed for the new-download dialog (its files to
//! choose from, as a torrent's) and for adding it, one task per file. `MLSD` when the server has
//! it, `LIST` otherwise; symbolic links are left out (they can loop), and so is everything past a
//! depth and a number of files.

use std::{str::FromStr, time::Duration};

use omnidl_core::{Error, ErrorCode, JobOptions, Result};
use suppaftp::list::{File, ListParser};
use url::Url;

use crate::session::{self, Ftp, Target, timed};

/// At most this many files are listed.
pub const MAX_FILES: usize = 5000;
/// Subfolders deeper than this are not entered.
const MAX_DEPTH: usize = 16;

/// A file of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFile {
    /// Its own link, with the folder link's account if it had one.
    pub url: String,
    /// Relative to the folder, `/`-separated.
    pub path: String,
    pub size: u64,
}

/// Whether `source` is an FTP link to a folder (it ends with `/`).
pub fn is_folder(source: &str) -> bool {
    Url::parse(source.trim()).is_ok_and(|u| {
        matches!(u.scheme(), "ftp" | "ftps") && u.host_str().is_some() && u.path().ends_with('/')
    })
}

/// The folder's own name (the last segment of the link), for the folder the files go into.
pub fn folder_name(source: &str) -> String {
    let name = Url::parse(source.trim())
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.rfind(|p| !p.is_empty()).map(str::to_owned))
                .or_else(|| u.host_str().map(str::to_owned))
        })
        .unwrap_or_default();
    omnidl_core::filename::sanitize(&omnidl_core::filename::percent_decode(&name))
}

/// Every file of the folder `source` and its subfolders, in path order.
pub async fn list(source: &str, o: &JobOptions) -> Result<Vec<FolderFile>> {
    let target = Target::parse_folder(source, o)?;
    let base = Url::parse(source.trim())
        .map_err(|e| Error::new(ErrorCode::InvalidUrl, format!("{source}: {e}")))?;
    let mut ftp = session::open(&target, o).await?;
    let mut out = Vec::new();
    let walked = walk(&mut ftp, &target, &base, o, &mut out).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), ftp.quit()).await;
    walked?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

async fn walk(
    ftp: &mut Ftp,
    t: &Target,
    base: &Url,
    o: &JobOptions,
    out: &mut Vec<FolderFile>,
) -> Result<()> {
    // (folder on the server, its path relative to the link, depth)
    let mut todo = vec![(t.path.clone(), Vec::<String>::new(), 0usize)];
    while let Some((dir, rel, depth)) = todo.pop() {
        for e in entries(ftp, &dir, o).await? {
            let name = e.name().to_owned();
            if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                continue;
            }
            if e.is_directory() {
                if depth + 1 < MAX_DEPTH {
                    let mut sub = rel.clone();
                    sub.push(name.clone());
                    todo.push((format!("{dir}{name}/"), sub, depth + 1));
                }
            } else if e.is_file() {
                let mut url = base.clone();
                if let Ok(mut segs) = url.path_segments_mut() {
                    segs.pop_if_empty();
                    segs.extend(rel.iter().map(String::as_str));
                    segs.push(&name);
                }
                let mut path = rel.join("/");
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(&name);
                out.push(FolderFile {
                    url: url.to_string(),
                    path,
                    size: e.size() as u64,
                });
                if out.len() >= MAX_FILES {
                    tracing::info!("{}: listing stopped at {MAX_FILES} files", t.url);
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// The entries of one folder: `MLSD`, else `LIST` (Unix or DOS style lines).
async fn entries(ftp: &mut Ftp, dir: &str, o: &JobOptions) -> Result<Vec<File>> {
    match timed(o.read_timeout, ftp.mlsd(Some(dir))).await {
        Ok(lines) => Ok(lines
            .iter()
            .filter_map(|l| ListParser::parse_mlsd(l).ok())
            .collect()),
        Err(e) if e.code == ErrorCode::FtpReply => {
            let lines = timed(o.read_timeout, ftp.list(Some(dir))).await?;
            Ok(lines
                .iter()
                .filter_map(|l| File::from_str(l).ok())
                .collect())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_folder_links_and_their_names() {
        assert!(is_folder("ftp://h/pub/linux/"));
        assert!(is_folder("ftps://u:p@h:990/"));
        assert!(!is_folder("ftp://h/pub/linux.iso"));
        assert!(!is_folder("https://h/pub/"));
        assert_eq!(folder_name("ftp://h/pub/My%20Files/"), "My Files");
        assert_eq!(folder_name("ftp://ftp.example.org/"), "ftp.example.org");
    }
}
