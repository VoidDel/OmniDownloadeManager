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
/// when another download has its partial file, or (when renaming) the file exists. When asking,
/// an existing file of the name `base` is the user's to decide about
/// (`ErrorCode::FileExists`).
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
        let file = dir.join(&name);
        match conflict {
            ConflictPolicy::Overwrite => {}
            ConflictPolicy::Ask if n == 0 && file.exists() => {
                return Err(Error::new(
                    ErrorCode::FileExists,
                    format!("{} already exists", file.display()),
                ));
            }
            ConflictPolicy::Rename | ConflictPolicy::Ask if file.exists() => continue,
            ConflictPolicy::Rename | ConflictPolicy::Ask => {}
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
/// when someone created the file meanwhile, unless overwriting: a finished download is not held
/// back to ask).
pub fn place(dir: &Path, name: &str, conflict: ConflictPolicy) -> Result<String> {
    let partial = dir.join(filename::partial(name));
    let mut target_name = name.to_owned();
    if conflict != ConflictPolicy::Overwrite
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

/// Marks a finished file as downloaded from `url` (found on the page `referer`), as browsers
/// do, so the system treats it as it treats their downloads: on Windows the `Zone.Identifier`
/// stream (programs ask before running it, Office opens it protected), on macOS the
/// `com.apple.quarantine` attribute (Gatekeeper checks it when first opened). Files from this
/// computer or the local network are not marked, as the systems themselves do not. A file
/// system that cannot hold the mark (FAT, some network shares) goes without.
pub fn mark_downloaded(path: &Path, url: &str, referer: Option<&str>) {
    if is_local(url) {
        return;
    }
    if let Err(e) = write_mark(path, url, referer) {
        tracing::debug!("marking {} as downloaded: {e}", path.display());
    }
}

/// The link without `user:password@` (it goes into the mark, which anything can read).
fn public_url(url: &str) -> String {
    filename::split_credentials(url).map_or_else(|| url.to_owned(), |(clean, ..)| clean)
}

/// A host on this computer or the local network: a name without a dot, `localhost`, a loopback,
/// private or link-local address.
fn is_local(url: &str) -> bool {
    let clean = public_url(url);
    let Some((_, rest)) = clean.split_once("://") else {
        return false;
    };
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let host = match authority.strip_prefix('[') {
        // `[::1]:8080`
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local (fc00::/7) and link-local (fe80::/10).
                || ip.segments()[0] & 0xfe00 == 0xfc00
                || ip.segments()[0] & 0xffc0 == 0xfe80
        }
        Err(_) => !host.contains('.') || host.eq_ignore_ascii_case("localhost"),
    }
}

#[cfg(windows)]
fn write_mark(path: &Path, url: &str, referer: Option<&str>) -> std::io::Result<()> {
    // Zone 3 is "Internet". One value per line; a line break would start another.
    let line = |s: &str| s.replace(['\r', '\n'], "");
    let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referer.filter(|r| r.starts_with("http://") || r.starts_with("https://")) {
        text.push_str(&format!("ReferrerUrl={}\r\n", line(&public_url(r))));
    }
    text.push_str(&format!("HostUrl={}\r\n", line(&public_url(url))));
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    std::fs::write(stream, text)
}

#[cfg(target_os = "macos")]
fn write_mark(path: &Path, _url: &str, _referer: Option<&str>) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    // `flags;time;agent;event id`: 0x0081 is "downloaded, not yet approved by the user", the
    // time is in hexadecimal seconds, and the event id (a row of the system's quarantine
    // database, which also holds the link) is left empty.
    let secs = crate::now_ms() / 1000;
    let value = format!("0081;{secs:08x};Omni Download;");
    let file = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `file` and the name are NUL-terminated strings, and `value` is valid for the
    // length passed.
    let r = unsafe {
        libc::setxattr(
            file.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn write_mark(_path: &Path, _url: &str, _referer: Option<&str>) -> std::io::Result<()> {
    // Linux has no such mark.
    Ok(())
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

    #[test]
    fn asks_about_an_existing_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        // Nothing there: the name is free.
        let (name, f) = claim_name(d, "a.bin", None, ConflictPolicy::Ask).unwrap();
        assert_eq!(name, "a.bin");
        // Another download has the name: numbered without asking.
        let (second, g) = claim_name(d, "a.bin", None, ConflictPolicy::Ask).unwrap();
        assert_eq!(second, "a (1).bin");
        drop((f, g));
        // The file exists: the user's to decide.
        std::fs::write(d.join("b.bin"), b"x").unwrap();
        let e = claim_name(d, "b.bin", None, ConflictPolicy::Ask).unwrap_err();
        assert_eq!(e.code, ErrorCode::FileExists);
        assert!(!d.join(filename::partial("b.bin")).exists());
        // Overwriting takes the name; the finished file replaces the old one.
        let (name, f) = claim_name(d, "b.bin", None, ConflictPolicy::Overwrite).unwrap();
        assert_eq!(name, "b.bin");
        drop(f);
        std::fs::write(d.join(filename::partial("b.bin")), b"new").unwrap();
        assert_eq!(
            place(d, "b.bin", ConflictPolicy::Overwrite).unwrap(),
            "b.bin"
        );
        assert_eq!(std::fs::read(d.join("b.bin")).unwrap(), b"new");
        // A file that appeared while downloading does not hold a finished download back.
        std::fs::write(d.join("a.bin"), b"x").unwrap();
        assert_eq!(place(d, "a.bin", ConflictPolicy::Ask).unwrap(), "a (2).bin");
    }

    #[test]
    fn knows_local_hosts() {
        for local in [
            "http://localhost/a",
            "http://127.0.0.1:8080/a",
            "http://192.168.1.5/a",
            "http://10.0.0.1/a",
            "ftp://nas/pub/a.iso",
            "http://[::1]:8080/a",
            "http://[fe80::1]/a",
            "http://user:pw@intranet/a",
        ] {
            assert!(is_local(local), "{local}");
        }
        for remote in [
            "https://example.com/a",
            "http://8.8.8.8/a",
            "https://user:pw@example.com:8443/a?x=1",
            "http://[2001:db8::1]/a",
        ] {
            assert!(!is_local(remote), "{remote}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn marks_a_download_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        // From the local network: no mark.
        mark_downloaded(&file, "http://192.168.1.2/setup.exe", None);
        assert!(std::fs::read(&stream).is_err());
        mark_downloaded(
            &file,
            "https://bob:secret@dl.example.com/setup.exe?t=1",
            Some("https://example.com/get"),
        );
        assert_eq!(
            std::fs::read_to_string(&stream).unwrap(),
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/get\r\n\
             HostUrl=https://dl.example.com/setup.exe?t=1\r\n"
        );
        // The file itself is untouched.
        assert_eq!(std::fs::read(&file).unwrap(), b"MZ");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn marks_a_download_on_macos() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.dmg");
        std::fs::write(&file, b"x").unwrap();
        mark_downloaded(&file, "https://example.com/app.dmg", None);
        let path = std::ffi::CString::new(file.as_os_str().as_bytes()).unwrap();
        let mut buf = [0u8; 128];
        // SAFETY: NUL-terminated strings and a buffer of the length passed.
        let n = unsafe {
            libc::getxattr(
                path.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                0,
            )
        };
        assert!(n > 0, "{}", std::io::Error::last_os_error());
        let value = std::str::from_utf8(&buf[..n as usize]).unwrap();
        assert!(
            value.starts_with("0081;") && value.ends_with(";Omni Download;"),
            "{value}"
        );
    }
}
