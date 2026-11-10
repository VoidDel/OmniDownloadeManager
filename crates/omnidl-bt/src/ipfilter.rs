//! IP filtering (BT settings): peers whose address is in a block list are not connected to, and
//! their connections are refused. librqbit reads one list in the P2P format (`name:start-end`);
//! the lists people use come in other formats too, so they are turned into that one here:
//! P2P, eMule's `ipfilter.dat` (`start - end , level , name`; levels below 128 block), CIDR,
//! `start-end` ranges and single addresses, IPv4 and IPv6, plain or gzip-compressed.

use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    str::FromStr,
};

/// The P2P line of a range, or `None` when `line` is not one (a comment, a DAT line that
/// allows, something else).
fn range(line: &str) -> Option<(IpAddr, IpAddr)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return None;
    }
    // ipfilter.dat: "start - end , level , name".
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() >= 2
        && let Ok(level) = parts[1].parse::<u32>()
    {
        if level >= 128 {
            return None;
        }
        return span(parts[0]);
    }
    // P2P: "name:start-end" (the name may hold colons; IPv6 addresses do).
    if let Some(r) = span(line) {
        return Some(r);
    }
    let mut at = line.len();
    while let Some(i) = line[..at].rfind(':') {
        if let Some(r) = span(&line[i + 1..]) {
            return Some(r);
        }
        at = i;
    }
    None
}

/// `start-end`, `a.b.c.d/n`, or one address.
fn span(s: &str) -> Option<(IpAddr, IpAddr)> {
    let s = s.trim();
    if let Some((a, b)) = s.split_once('-') {
        let (a, b) = (parse_ip(a)?, parse_ip(b)?);
        return (a.is_ipv4() == b.is_ipv4() && a <= b).then_some((a, b));
    }
    if let Some((addr, bits)) = s.split_once('/') {
        let bits: u32 = bits.trim().parse().ok()?;
        return match parse_ip(addr)? {
            IpAddr::V4(a) if bits <= 32 => {
                let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
                let start = a.to_bits() & mask;
                Some((
                    Ipv4Addr::from_bits(start).into(),
                    Ipv4Addr::from_bits(start | !mask).into(),
                ))
            }
            IpAddr::V6(a) if bits <= 128 => {
                let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
                let start = a.to_bits() & mask;
                Some((
                    Ipv6Addr::from_bits(start).into(),
                    Ipv6Addr::from_bits(start | !mask).into(),
                ))
            }
            _ => None,
        };
    }
    let a = parse_ip(s)?;
    Some((a, a))
}

/// An address, with the leading zeros of `ipfilter.dat` (`001.002.003.004`) allowed.
fn parse_ip(s: &str) -> Option<IpAddr> {
    let s = s.trim();
    if let Ok(a) = IpAddr::from_str(s) {
        return Some(a);
    }
    let parts: Vec<u8> = s
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let octets: [u8; 4] = parts.try_into().ok()?;
    Some(Ipv4Addr::from(octets).into())
}

/// The text of a list file: gzip-compressed or not.
pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = String::new();
        let mut gz = flate2::read::GzDecoder::new(bytes);
        let mut raw = Vec::new();
        if gz.read_to_end(&mut raw).is_ok() {
            out = String::from_utf8_lossy(&raw).into_owned();
        }
        return out;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// `text` (any of the formats) as P2P lines, and how many ranges it holds.
pub fn to_p2p(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut n = 0;
    for (a, b) in text.lines().filter_map(range) {
        out.push_str(&format!("omni:{a}-{b}\n"));
        n += 1;
    }
    (out, n)
}

/// Writes the list librqbit reads (`out`): the ranges of `lists` and of the addresses the user
/// blocked by hand. Returns how many there are.
pub fn write(lists: &[Vec<u8>], manual: &[String], out: &Path) -> std::io::Result<usize> {
    let mut text = manual.join("\n");
    for bytes in lists {
        text.push('\n');
        text.push_str(&decode(bytes));
    }
    let (p2p, n) = to_p2p(&text);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, p2p)?;
    Ok(n)
}

/// How many ranges the list at `path` holds (0 when there is none).
pub fn count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_usual_formats() {
        let text = "\
# a comment
Bad people:1.2.3.0-1.2.3.255
001.002.004.000 - 001.002.004.255 , 000 , eMule level 0
005.005.005.000 - 005.005.005.255 , 200 , allowed (level 200)
10.0.0.0/8
192.168.1.7
2001:db8::/32
some:name:with:colons:7.7.7.0-7.7.7.9
20.0.0.1 - 20.0.0.2
not an address
";
        let (p2p, n) = to_p2p(text);
        let lines: Vec<&str> = p2p.lines().collect();
        assert_eq!(n, 7, "{p2p}");
        assert_eq!(lines[0], "omni:1.2.3.0-1.2.3.255");
        assert_eq!(lines[1], "omni:1.2.4.0-1.2.4.255");
        assert_eq!(lines[2], "omni:10.0.0.0-10.255.255.255");
        assert_eq!(lines[3], "omni:192.168.1.7-192.168.1.7");
        assert_eq!(
            lines[4],
            "omni:2001:db8::-2001:db8:ffff:ffff:ffff:ffff:ffff:ffff"
        );
        assert_eq!(lines[5], "omni:7.7.7.0-7.7.7.9");
        assert_eq!(lines[6], "omni:20.0.0.1-20.0.0.2");
    }

    #[test]
    fn reads_gzip_lists_and_merges_the_hand_made_ones() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"list:9.9.9.0-9.9.9.9\n").unwrap();
        let bytes = gz.finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("ipfilter.p2p");
        let plain = b"7.7.7.7".to_vec();
        let n = write(&[bytes, plain], &["8.8.8.8".into()], &out).unwrap();
        assert_eq!(n, 3);
        assert_eq!(count(&out), 3);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "omni:8.8.8.8-8.8.8.8\nomni:9.9.9.0-9.9.9.9\nomni:7.7.7.7-7.7.7.7\n"
        );
    }
}
