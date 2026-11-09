//! The partial file of a download (`<name>.odm`, docs/development-plan.md §4.2.4, §4.7).

use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

/// Creates (or truncates) the partial file of a new download and, when the length is known,
/// sizes it up front: the connections then write anywhere in it, and running out of space shows
/// up now rather than halfway through.
pub fn create(path: &Path, len: Option<u64>) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    if let Some(len) = len.filter(|&l| l > 0) {
        reserve(&file, len)?;
    }
    Ok(file)
}

/// Like [`create`], but fails with `AlreadyExists` when the file exists: how a new download
/// claims a name no other download is using.
pub fn create_new(path: &Path, len: Option<u64>) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Some(len) = len.filter(|&l| l > 0)
        && let Err(e) = reserve(&file, len)
    {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(file)
}

/// Opens the partial file of a download being resumed. Fails if it is gone.
pub fn open_existing(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

/// Bytes free for the current user on the volume holding `path` (which must exist).
pub fn available_space(path: &Path) -> io::Result<u64> {
    fs4::available_space(path)
}

/// Windows: a sparse file of the full length. NTFS zero-fills everything between the end of the
/// written data and a write further on before doing that write, so without this the first block
/// written by the last connection of a large file would wait for gigabytes of zeros. Sparse
/// files allocate as they are written (FAT and exFAT have no sparse files; there it is a plain
/// `set_len`).
#[cfg(windows)]
fn reserve(file: &File, len: u64) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE};

    let mut returned = 0u32;
    // SAFETY: a valid handle owned by `file`; FSCTL_SET_SPARSE takes no buffers.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        tracing::debug!("sparse file: {}", io::Error::last_os_error());
    }
    file.set_len(len)
}

/// Linux and macOS: real preallocation (`fallocate` / `F_PREALLOCATE`), which also catches a
/// full disk now. File systems without it get a plain `set_len`.
#[cfg(not(windows))]
fn reserve(file: &File, len: u64) -> io::Result<()> {
    if let Err(e) = fs4::FileExt::allocate(file, len) {
        if e.kind() == io::ErrorKind::StorageFull {
            return Err(e);
        }
        tracing::debug!("preallocation: {e}");
    }
    if file.metadata()?.len() < len {
        file.set_len(len)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_sized_file_and_reopens_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/a.bin.odm");
        let f = create(&path, Some(10 << 20)).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 10 << 20);
        drop(f);
        let f = open_existing(&path).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 10 << 20);
        drop(f);
        // A new download truncates what was there.
        let f = create(&path, None).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 0);
        assert!(open_existing(&dir.path().join("missing")).is_err());
        assert_eq!(
            create_new(&path, Some(5)).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            create_new(&dir.path().join("b"), Some(5))
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            5
        );
        assert!(available_space(dir.path()).unwrap() > 0);
    }
}
