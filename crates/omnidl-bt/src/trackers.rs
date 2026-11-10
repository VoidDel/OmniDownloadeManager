//! Trackers added to every torrent (BT settings): the user's own list and one fetched from a
//! web address (public tracker lists are plain text, one address per line, sometimes separated
//! by blank lines or commas). The engine adds them to each torrent that is not private (BEP 27)
//! and to magnet links while their metadata is fetched.

/// The tracker addresses in `text`, in order and without repeats: `http`, `https` and `udp`
/// URLs with a host. Anything else (comments, other schemes, junk) is skipped.
pub fn parse(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with("//"))
        .flat_map(|l| l.split(|c: char| c == ',' || c.is_whitespace()))
    {
        if let Some(u) = valid(token)
            && !out.contains(&u)
        {
            out.push(u);
        }
    }
    out
}

/// `s` as a tracker address librqbit can announce to, or `None`.
fn valid(s: &str) -> Option<String> {
    let u = url::Url::parse(s.trim()).ok()?;
    let ok = matches!(u.scheme(), "http" | "https" | "udp")
        && u.host_str().is_some_and(|h| !h.is_empty());
    ok.then(|| u.to_string())
}

/// Several lists as one, in order and without repeats.
pub fn merge<'a>(lists: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in lists.into_iter().flatten() {
        if let Some(u) = valid(t)
            && !out.contains(&u)
        {
            out.push(u);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_public_lists() {
        let text = "\
# trackers_best.txt
udp://tracker.example.org:1337/announce

http://tracker.example.net:80/announce
https://tracker.example.com:443/announce,udp://open.example.io:6969/announce
udp://tracker.example.org:1337/announce
wss://tracker.example.dev/announce
magnet:?xt=urn:btih:abc
not a url
udp:///announce
";
        assert_eq!(
            parse(text),
            [
                "udp://tracker.example.org:1337/announce",
                "http://tracker.example.net/announce",
                "https://tracker.example.com/announce",
                "udp://open.example.io:6969/announce",
            ]
        );
    }

    #[test]
    fn merges_without_repeats() {
        let a = vec!["udp://a.example:1/announce".to_owned(), "junk".to_owned()];
        let b = vec![
            "udp://b.example:2/announce".to_owned(),
            " udp://a.example:1/announce ".to_owned(),
        ];
        assert_eq!(
            merge([a.as_slice(), b.as_slice()]),
            ["udp://a.example:1/announce", "udp://b.example:2/announce"]
        );
    }
}
