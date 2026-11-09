//! The key that seals stored cookies and headers (omnidl-core `Secrets`): 32 random bytes in
//! `<app data>/secret.key`. On Windows the file holds the key protected with DPAPI (only this
//! Windows account can unprotect it); elsewhere it is readable by its owner only (0600). A key
//! file rather than the system keychain: no keychain prompts for unsigned builds, no Secret
//! Service needed on Linux (ADR-003).

use std::{io, path::Path};

const FILE: &str = "secret.key";

/// The key in `dir`, created on first use. `None` when it can be neither read nor created (the
/// app then stores request details unsealed).
pub fn load_or_create(dir: &Path) -> Option<[u8; 32]> {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(raw) => match unprotect(&raw).and_then(|k| <[u8; 32]>::try_from(k).ok()) {
            Some(k) => return Some(k),
            None => tracing::warn!("{}: unreadable key; making a new one", path.display()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!("{}: {e}", path.display());
            return None;
        }
    }
    let key = omnidl_core::AesSecrets::generate_key();
    let raw = protect(&key)?;
    if let Err(e) = write_private(&path, &raw) {
        tracing::warn!("{}: {e}", path.display());
        return None;
    }
    Some(key)
}

#[cfg(unix)]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(data)
}

#[cfg(windows)]
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    std::fs::write(path, data)
}

#[cfg(not(windows))]
fn protect(key: &[u8]) -> Option<Vec<u8>> {
    Some(key.to_vec())
}

#[cfg(not(windows))]
fn unprotect(raw: &[u8]) -> Option<Vec<u8>> {
    Some(raw.to_vec())
}

#[cfg(windows)]
fn protect(key: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: key.len() as u32,
        pbData: key.as_ptr().cast_mut(),
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: valid input blob; DPAPI allocates `out`, freed below.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    take_blob(ok != 0, out)
}

#[cfg(windows)]
fn unprotect(raw: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: raw.len() as u32,
        pbData: raw.as_ptr().cast_mut(),
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    take_blob(ok != 0, out)
}

/// Copies a DPAPI output blob and frees it.
#[cfg(windows)]
fn take_blob(
    ok: bool,
    out: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
) -> Option<Vec<u8>> {
    if !ok || out.pbData.is_null() {
        return None;
    }
    // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { windows_sys::Win32::Foundation::LocalFree(out.pbData.cast()) };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let a = load_or_create(dir.path()).unwrap();
        let b = load_or_create(dir.path()).unwrap();
        assert_eq!(a, b);
        let raw = std::fs::read(dir.path().join(FILE)).unwrap();
        if cfg!(windows) {
            assert_ne!(&raw[..], &a[..], "protected on disk");
        }
        // A damaged file is replaced.
        std::fs::write(dir.path().join(FILE), b"junk").unwrap();
        assert_ne!(load_or_create(dir.path()).unwrap(), a);
    }
}
