//! File names (docs/development-plan.md §4.2.2, §4.7): cleaning a name from a server or a URL
//! so it is valid on every platform, numbering duplicates, and the category of a file.

use crate::Category;

/// Suffix of a file being downloaded.
pub const PARTIAL_SUFFIX: &str = ".odm";

/// Longest name kept, in bytes (most file systems allow 255; room for ` (99)` and `.odm`).
const MAX_BYTES: usize = 200;

/// Extensions of two parts, kept together when numbering (`a (1).tar.gz`).
const DOUBLE_EXTENSIONS: &[&str] = &[".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4"];

/// Name of the partial file of `name`.
pub fn partial(name: &str) -> String {
    format!("{name}{PARTIAL_SUFFIX}")
}

/// A name valid on Windows, macOS and Linux: characters Windows forbids and control characters
/// become `_`, leading and trailing dots and spaces go, Windows device names (`CON`, `COM1`…)
/// get a `_`, and long names are cut (keeping the extension). Never empty.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut s = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    if s.is_empty() || s.chars().all(|c| c == '_') {
        return "download".into();
    }
    let (stem, ext) = split_extension(&s);
    if is_reserved(stem) {
        s = format!("{stem}_{ext}");
    }
    truncate(&s)
}

fn is_reserved(stem: &str) -> bool {
    let upper = stem.trim_end().to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0')
}

fn truncate(s: &str) -> String {
    if s.len() <= MAX_BYTES {
        return s.to_owned();
    }
    let (stem, ext) = split_extension(s);
    let ext = if ext.len() <= 24 { ext } else { "" };
    let mut cut = MAX_BYTES - ext.len();
    while !stem.is_char_boundary(cut.min(stem.len())) {
        cut -= 1;
    }
    format!("{}{ext}", stem[..cut.min(stem.len())].trim_end())
}

/// `("archive", ".tar.gz")`, `("a", ".iso")`, `("README", "")`, `(".hidden", "")`.
pub fn split_extension(name: &str) -> (&str, &str) {
    let lower = name.to_ascii_lowercase();
    if let Some(ext) = DOUBLE_EXTENSIONS.iter().find(|e| lower.ends_with(*e))
        && name.len() > ext.len()
    {
        return name.split_at(name.len() - ext.len());
    }
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name.split_at(i),
        _ => (name, ""),
    }
}

/// `name (n).ext`.
pub fn numbered(name: &str, n: u32) -> String {
    let (stem, ext) = split_extension(name);
    format!("{stem} ({n}){ext}")
}

/// The last path segment of a URL, percent-decoded and cleaned; `None` when there is none
/// (`https://example.com/`).
pub fn from_url(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.split(['?', '#']).next()?;
    let (_, path) = path.split_once('/')?;
    let last = path.rsplit('/').find(|s| !s.is_empty())?;
    let decoded = percent_decode(last);
    let name = sanitize(&decoded);
    (name != "download" || decoded.trim() == "download").then_some(name)
}

/// A torrent's name before its metadata is known: the `dn` of a magnet link, or the name of a
/// `.torrent` file without its extension.
pub fn for_torrent(source: &str) -> Option<String> {
    let s = source.trim();
    let raw = if s
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("magnet:"))
    {
        let query = s.split_once('?').map_or("", |(_, q)| q);
        let dn = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("dn="))?
            .replace('+', " ");
        percent_decode(&dn)
    } else {
        let last = s.rsplit(['/', '\\']).find(|p| !p.is_empty())?;
        let last = percent_decode(last.split(['?', '#']).next()?);
        let cut = last.len().checked_sub(".torrent".len())?;
        if !last.is_char_boundary(cut) || !last[cut..].eq_ignore_ascii_case(".torrent") {
            return None;
        }
        last[..cut].to_owned()
    };
    (!raw.trim().is_empty()).then(|| sanitize(&raw))
}

/// `scheme://user:password@host/…` → the link without them, the user and the password (both
/// percent-decoded); `None` without credentials.
pub fn split_credentials(url: &str) -> Option<(String, String, Option<String>)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let (userinfo, host) = authority.rsplit_once('@')?;
    let (user, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u, Some(percent_decode(p))),
        None => (userinfo, None),
    };
    Some((
        format!("{scheme}://{host}{tail}"),
        percent_decode(user),
        password,
    ))
}

/// Decodes `%XX` escapes as UTF-8 (invalid sequences are replaced).
pub fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode_bytes(s)).into_owned()
}

/// Decodes `%XX` escapes to bytes.
pub fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// The category of a file, from its extension.
pub fn category(name: &str) -> Category {
    let (_, ext) = split_extension(name);
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let ext = ext.rsplit('.').next().unwrap_or("");
    match ext {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" | "ts"
        | "m2ts" | "3gp" | "rmvb" => Category::Video,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "ape" | "aiff" => {
            Category::Audio
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "txt" | "rtf" | "epub" | "mobi" | "md" | "csv" => Category::Document,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "lz4" | "cab" => {
            Category::Archive
        }
        "exe" | "msi" | "msix" | "appx" | "pkg" | "deb" | "rpm" | "apk" | "appimage"
        | "flatpak" => Category::Program,
        "iso" | "img" | "dmg" | "vhd" | "vhdx" | "vmdk" | "qcow2" | "wim" => Category::DiskImage,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic"
        | "avif" | "ico" | "psd" => Category::Image,
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_for_every_platform() {
        assert_eq!(sanitize("report.pdf"), "report.pdf");
        assert_eq!(
            sanitize("a/b\\c:d*e?f\"g<h>i|j.txt"),
            "a_b_c_d_e_f_g_h_i_j.txt"
        );
        assert_eq!(sanitize("  ..name.. "), "name");
        assert_eq!(sanitize("tab\there"), "tab_here");
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize("..."), "download");
        assert_eq!(sanitize("///"), "download");
        assert_eq!(sanitize("CON"), "CON_");
        assert_eq!(sanitize("con.txt"), "con_.txt");
        assert_eq!(sanitize("COM1.log"), "COM1_.log");
        assert_eq!(sanitize("COM0.log"), "COM0.log");
        assert_eq!(sanitize("CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(sanitize("视频 第1集.mp4"), "视频 第1集.mp4");
        let long = format!("{}.iso", "长".repeat(100));
        let s = sanitize(&long);
        assert!(s.len() <= MAX_BYTES && s.ends_with(".iso"), "{s}");
    }

    #[test]
    fn numbers_duplicates_before_the_extension() {
        assert_eq!(numbered("a.iso", 1), "a (1).iso");
        assert_eq!(numbered("src.tar.gz", 2), "src (2).tar.gz");
        assert_eq!(numbered("README", 3), "README (3)");
        assert_eq!(numbered(".bashrc", 1), ".bashrc (1)");
    }

    #[test]
    fn splits_credentials_off_links() {
        assert_eq!(
            split_credentials("ftp://bob:p%40ss@h.example:2121/pub/a.iso"),
            Some((
                "ftp://h.example:2121/pub/a.iso".into(),
                "bob".into(),
                Some("p@ss".into())
            ))
        );
        assert_eq!(
            split_credentials("ftp://anon@h/a"),
            Some(("ftp://h/a".into(), "anon".into(), None))
        );
        assert_eq!(split_credentials("ftp://h/a@b"), None);
        assert_eq!(split_credentials("ftp://h/a"), None);
    }

    #[test]
    fn names_torrents_before_their_metadata() {
        let m = "magnet:?xt=urn:btih:abc&dn=Big+Buck%20Bunny&tr=udp%3A%2F%2Ft";
        assert_eq!(for_torrent(m).as_deref(), Some("Big Buck Bunny"));
        assert_eq!(for_torrent("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(
            for_torrent(r"C:\x\Ubuntu 24.04.TORRENT").as_deref(),
            Some("Ubuntu 24.04")
        );
        assert_eq!(
            for_torrent("https://e.com/a/debian.torrent?x=1").as_deref(),
            Some("debian")
        );
        assert_eq!(for_torrent("https://e.com/a.zip"), None);
    }

    #[test]
    fn names_from_urls() {
        assert_eq!(
            from_url("https://example.com/files/ubuntu-24.04.iso?x=1#y").as_deref(),
            Some("ubuntu-24.04.iso")
        );
        assert_eq!(
            from_url("https://example.com/%E6%96%87%E6%A1%A3.pdf").as_deref(),
            Some("文档.pdf")
        );
        assert_eq!(
            from_url("https://example.com/a/b/").as_deref(),
            Some("b"),
            "trailing slash"
        );
        assert_eq!(from_url("https://example.com/"), None);
        assert_eq!(from_url("https://example.com"), None);
        assert_eq!(
            from_url("https://e.com/a%2Fb.txt").as_deref(),
            Some("a_b.txt")
        );
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn categorizes_by_extension() {
        assert_eq!(category("a.MKV"), Category::Video);
        assert_eq!(category("a.tar.gz"), Category::Archive);
        assert_eq!(category("setup.exe"), Category::Program);
        assert_eq!(category("ubuntu.iso"), Category::DiskImage);
        assert_eq!(category("paper.pdf"), Category::Document);
        assert_eq!(category("README"), Category::Other);
    }
}
