//! What the single-file engines (HTTP, FTP) share around the partial file: room on the disk, a
//! free name, putting the finished file in place, and the delay between retries.

use std::{path::Path, time::Duration};

use omnidl_storage::file;

use crate::{ConflictPolicy, Error, ErrorCode, Result, filename};

const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Fails early when a file of `total` bytes cannot fit in `dir`.
pub fn ensure_space(dir: &Path, total: Option<u64>) -> Result<()> {
    let Some(total) = total else {
        return Ok(());
    };
    match file::available_space(dir) {
        Ok(free) if free < total => Err(Error::new(
            ErrorCode::DiskFull,
            format!("{total} bytes needed, {free} free in {}", dir.display()),
        )),
        _ => Ok(()),
    }
}

/// Picks the first free name `base`, `base (1)`… and creates its partial file. A name is taken
/// when another download has its partial file, or (when renaming) the file exists.
pub fn claim_name(
    dir: &Path,
    base: &str,
    total: Option<u64>,
    conflict: ConflictPolicy,
) -> Result<(String, std::fs::File)> {
    for n in 0..10_000 {
        let name = if n == 0 {
            base.to_owned()
        } else {
            filename::numbered(base, n)
        };
        if conflict == ConflictPolicy::Rename && dir.join(&name).exists() {
            continue;
        }
        let partial = dir.join(filename::partial(&name));
        match file::create_new(&partial, total) {
            Ok(f) => return Ok((name, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(&e, partial.display())),
        }
    }
    Err(Error::internal(format!("no free name for {base}")))
}

/// Renames the finished partial file of `name` into place and returns the final name (numbered
/// when renaming and someone created the file meanwhile).
pub fn place(dir: &Path, name: &str, conflict: ConflictPolicy) -> Result<String> {
    let partial = dir.join(filename::partial(name));
    let mut target_name = name.to_owned();
    if conflict == ConflictPolicy::Rename
        && dir.join(name).exists()
        && let Some(free) = (1..10_000)
            .map(|n| filename::numbered(name, n))
            .find(|n| !dir.join(n).exists() && !dir.join(filename::partial(n)).exists())
    {
        target_name = free;
    }
    let target = dir.join(&target_name);
    std::fs::rename(&partial, &target).map_err(|e| Error::io(&e, target.display()))?;
    Ok(target_name)
}

/// Exponential backoff from `base`: base, 2·base, 4·base… at most a minute.
pub fn backoff(base: Duration, attempt: u32) -> Duration {
    base.saturating_mul(1 << attempt.saturating_sub(1).min(16))
        .min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_and_places_names() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("a.bin"), b"x").unwrap();
        let (name, _f) = claim_name(d, "a.bin", Some(3), ConflictPolicy::Rename).unwrap();
        assert_eq!(name, "a (1).bin");
        let (second, _g) = claim_name(d, "a.bin", None, ConflictPolicy::Rename).unwrap();
        assert_eq!(second, "a (2).bin");
        // Someone took "a (1).bin" meanwhile.
        std::fs::write(d.join("a (1).bin"), b"y").unwrap();
        drop(_f);
        assert_eq!(
            place(d, &name, ConflictPolicy::Rename).unwrap(),
            "a (1) (1).bin"
        );
        assert_eq!(backoff(Duration::from_secs(1), 3), Duration::from_secs(4));
        assert_eq!(backoff(Duration::from_secs(1), 30), MAX_BACKOFF);
    }
}
