//! File checksums (docs/development-plan.md §4.2, "完整性校验"): the value a user expects for a
//! download, checked once the file is finished, and the values of a finished file on demand.

use std::{io::Read, path::Path};

use md5::Md5;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl HashAlgo {
    pub const ALL: [Self; 4] = [Self::Md5, Self::Sha1, Self::Sha256, Self::Sha512];

    /// Hex digits of a value.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Md5 => 32,
            Self::Sha1 => 40,
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        let s: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match s.as_str() {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }

    fn from_len(n: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.hex_len() == n)
    }
}

/// The value a download should have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    pub algo: HashAlgo,
    /// Lowercase hex.
    pub hex: String,
}

impl Checksum {
    /// Reads a value as people paste it: bare hex (the algorithm from its length), or with the
    /// algorithm before it (`sha256:…`, `SHA-256 …`, `md5=…`), or as a line of a `*sum` file
    /// (`<hex>  <name>`). `None` when it is not one.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        // "algo<sep>hex"
        if let Some(i) = text.find([':', '=', ' ', '\t']) {
            let (name, rest) = (&text[..i], text[i + 1..].trim());
            if let Some(algo) = HashAlgo::from_name(name) {
                let hex = rest.split_whitespace().next().unwrap_or_default();
                return Self::of(Some(algo), hex);
            }
        }
        // "hex<spaces>name" or just "hex"
        let hex = text.split_whitespace().next().unwrap_or_default();
        Self::of(None, hex.trim_start_matches('*'))
    }

    fn of(algo: Option<HashAlgo>, hex: &str) -> Option<Self> {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let algo = algo.or_else(|| HashAlgo::from_len(hex.len()))?;
        (algo.hex_len() == hex.len()).then(|| Self {
            algo,
            hex: hex.to_ascii_lowercase(),
        })
    }

    /// Whether `file` has this value.
    pub fn matches(&self, file: &Path) -> std::io::Result<bool> {
        let got = digests(file, &[self.algo])?;
        Ok(got.first().is_some_and(|(_, hex)| *hex == self.hex))
    }
}

/// The values of `file` for `algos`, in one pass over it.
pub fn digests(file: &Path, algos: &[HashAlgo]) -> std::io::Result<Vec<(HashAlgo, String)>> {
    let mut f = std::fs::File::open(file)?;
    let mut md5 = algos.contains(&HashAlgo::Md5).then(Md5::new);
    let mut sha1 = algos.contains(&HashAlgo::Sha1).then(Sha1::new);
    let mut sha256 = algos.contains(&HashAlgo::Sha256).then(Sha256::new);
    let mut sha512 = algos.contains(&HashAlgo::Sha512).then(Sha512::new);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let chunk = &buf[..n];
        if let Some(h) = md5.as_mut() {
            h.update(chunk);
        }
        if let Some(h) = sha1.as_mut() {
            h.update(chunk);
        }
        if let Some(h) = sha256.as_mut() {
            h.update(chunk);
        }
        if let Some(h) = sha512.as_mut() {
            h.update(chunk);
        }
    }
    let mut out = Vec::new();
    for &a in algos {
        let bytes: Vec<u8> = match a {
            HashAlgo::Md5 => md5.take().map(|h| h.finalize().to_vec()),
            HashAlgo::Sha1 => sha1.take().map(|h| h.finalize().to_vec()),
            HashAlgo::Sha256 => sha256.take().map(|h| h.finalize().to_vec()),
            HashAlgo::Sha512 => sha512.take().map(|h| h.finalize().to_vec()),
        }
        .unwrap_or_default();
        if !bytes.is_empty() {
            out.push((a, hex(&bytes)));
        }
    }
    Ok(out)
}

/// The value of `data` (HTTP Digest authentication hashes short strings).
pub fn hex_of(algo: HashAlgo, data: &[u8]) -> String {
    match algo {
        HashAlgo::Md5 => hex(&Md5::digest(data)),
        HashAlgo::Sha1 => hex(&Sha1::digest(data)),
        HashAlgo::Sha256 => hex(&Sha256::digest(data)),
        HashAlgo::Sha512 => hex(&Sha512::digest(data)),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";
    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn reads_values_as_people_paste_them() {
        let sha = Checksum::parse(ABC_SHA256).unwrap();
        assert_eq!(sha.algo, HashAlgo::Sha256);
        assert_eq!(
            Checksum::parse(&ABC_MD5.to_uppercase()).unwrap().hex,
            ABC_MD5
        );
        assert_eq!(
            Checksum::parse(&format!("sha256:{ABC_SHA256}")),
            Some(sha.clone())
        );
        assert_eq!(
            Checksum::parse(&format!("SHA-256 {ABC_SHA256}")),
            Some(sha.clone())
        );
        assert_eq!(
            Checksum::parse(&format!("{ABC_SHA256}  file.iso")),
            Some(sha.clone())
        );
        assert_eq!(
            Checksum::parse(&format!("{ABC_SHA256} *file.iso")),
            Some(sha)
        );
        assert_eq!(
            Checksum::parse(&format!("md5={ABC_MD5}")).unwrap().algo,
            HashAlgo::Md5
        );
        // The algorithm named must fit the length.
        assert_eq!(Checksum::parse(&format!("sha1:{ABC_MD5}")), None);
        assert_eq!(Checksum::parse("not a hash"), None);
        assert_eq!(Checksum::parse("abc123"), None);
        assert_eq!(Checksum::parse(""), None);
    }

    #[test]
    fn hashes_a_file_in_one_pass() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("abc");
        std::fs::write(&f, b"abc").unwrap();
        let got = digests(&f, &[HashAlgo::Md5, HashAlgo::Sha1, HashAlgo::Sha256]).unwrap();
        assert_eq!(
            got,
            vec![
                (HashAlgo::Md5, ABC_MD5.to_owned()),
                (HashAlgo::Sha1, ABC_SHA1.to_owned()),
                (HashAlgo::Sha256, ABC_SHA256.to_owned()),
            ]
        );
        assert!(Checksum::parse(ABC_SHA1).unwrap().matches(&f).unwrap());
        assert!(
            !Checksum::parse(ABC_MD5.replace('9', "8").as_str())
                .unwrap()
                .matches(&f)
                .unwrap()
        );
    }
}
