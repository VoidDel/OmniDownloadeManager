//! Logging in to a server that asks for an account: Basic (RFC 7617) and Digest (RFC 7616, and
//! the older RFC 2069 form without `qop`).
//!
//! The account is the task's (`TaskOptions::username`, or `user:password@` in the link, which
//! is taken out of the link here so that only this module sends it). It goes to the link's
//! host only. Over `http://` nothing is sent before the server asks (401 with
//! `WWW-Authenticate`), so a server that offers Digest never sees the password; over `https://`
//! Basic is sent with the first request, as it was when the link carried the account. What the
//! server asked for is kept for the download: its other connections log in without asking.

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use omnidl_core::{HashAlgo, JobOptions, checksum::hex_of, filename};
use parking_lot::Mutex;
use reqwest::{
    Client, Method, RequestBuilder, Response, StatusCode,
    header::{AUTHORIZATION, HeaderValue, WWW_AUTHENTICATE},
};
use url::Url;

/// The account of one download and what its server asked for.
#[derive(Debug)]
pub(crate) struct Auth {
    account: Option<(String, String)>,
    /// The link's host: the account is for it alone.
    host: String,
    scheme: Mutex<Scheme>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Scheme {
    /// The server has not asked yet.
    Unknown,
    Basic,
    Digest(Digest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Digest {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    algo: HashAlgo,
    /// `MD5-sess`, `SHA-256-sess`: the key is derived from the nonces once.
    session: bool,
    /// `qop=auth`; without it the RFC 2069 form.
    qop: bool,
    /// Requests answered with this nonce.
    count: u32,
    cnonce: String,
}

/// What a request carried.
#[derive(Clone, PartialEq, Eq)]
enum Sent {
    Nothing,
    Basic,
    /// An answer to this nonce.
    Digest(String),
}

impl Auth {
    /// The account for `url`, taking `user:password@` out of it. The task's own account wins
    /// over the link's.
    pub fn new(o: &JobOptions, url: &mut Url) -> Self {
        let in_link = (!url.username().is_empty()).then(|| {
            (
                filename::percent_decode(url.username()),
                url.password()
                    .map(filename::percent_decode)
                    .unwrap_or_default(),
            )
        });
        let _ = url.set_username("");
        let _ = url.set_password(None);
        let account = match &o.username {
            Some(user) => Some((user.clone(), o.password.clone().unwrap_or_default())),
            None => in_link,
        };
        Self {
            account,
            host: url.host_str().unwrap_or_default().to_ascii_lowercase(),
            scheme: Mutex::new(Scheme::Unknown),
        }
    }

    /// No account: playlists, whose requests carry the one in their link (see `embed`).
    pub fn none() -> Self {
        Self {
            account: None,
            host: String::new(),
            scheme: Mutex::new(Scheme::Unknown),
        }
    }

    fn account_for(&self, url: &Url) -> Option<&(String, String)> {
        self.account.as_ref().filter(|_| {
            url.host_str()
                .is_some_and(|h| h.eq_ignore_ascii_case(&self.host))
        })
    }

    /// The `Authorization` header of a request, if it should carry one.
    fn header(&self, method: &Method, url: &Url) -> (Option<HeaderValue>, Sent) {
        let Some((user, password)) = self.account_for(url) else {
            return (None, Sent::Nothing);
        };
        let mut scheme = self.scheme.lock();
        let (value, sent) = match &mut *scheme {
            Scheme::Unknown if url.scheme() != "https" => return (None, Sent::Nothing),
            Scheme::Unknown | Scheme::Basic => (basic(user, password), Sent::Basic),
            Scheme::Digest(d) => {
                d.count += 1;
                (
                    d.answer(user, password, method.as_str(), url),
                    Sent::Digest(d.nonce.clone()),
                )
            }
        };
        match HeaderValue::from_bytes(value.as_bytes()) {
            Ok(mut v) => {
                v.set_sensitive(true);
                (Some(v), sent)
            }
            Err(_) => (None, Sent::Nothing),
        }
    }

    /// Reads the challenge of a 401 answer to a request that carried `sent`. `true`: the request
    /// is worth sending again, as the next one will log in differently.
    fn challenged(&self, resp: &Response, sent: &Sent) -> bool {
        if self.account_for(resp.url()).is_none() {
            return false;
        }
        let offers: Vec<Challenge> = resp
            .headers()
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(challenges)
            .collect();
        let mut scheme = self.scheme.lock();
        // The strongest offer that can be answered.
        if let Some((d, stale)) = offers
            .iter()
            .filter_map(Digest::from_challenge)
            .max_by_key(|(d, _)| d.algo == HashAlgo::Sha256)
        {
            // Our answer to this very nonce was refused: the name or the password is wrong.
            if *sent == Sent::Digest(d.nonce.clone()) && !stale {
                return false;
            }
            // Another connection may have taken the new nonce already (and counted with it).
            if !matches!(&*scheme, Scheme::Digest(old) if old.nonce == d.nonce) {
                *scheme = Scheme::Digest(d);
            }
            return true;
        }
        if offers.iter().any(|c| c.scheme == "basic") && *sent != Sent::Basic {
            *scheme = Scheme::Basic;
            return true;
        }
        false
    }

    /// Sends a request, logging in when the server asks. `with` adds what the request needs
    /// (its `Range`); it is called for each attempt. The answer may still be a 401: no account,
    /// a wrong one, or a way of logging in this module does not know.
    pub async fn send(
        &self,
        client: &Client,
        method: Method,
        url: &Url,
        with: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> reqwest::Result<Response> {
        let mut url = url.clone();
        // Not logged in → the challenge answered → once more when the nonce had expired.
        let mut left = 3;
        loop {
            left -= 1;
            let (header, sent) = self.header(&method, &url);
            let mut req = with(client.request(method.clone(), url.clone()));
            if let Some(h) = header {
                req = req.header(AUTHORIZATION, h);
            }
            let resp = req.send().await?;
            let redirected = resp.url() != &url;
            let again = match resp.status() {
                StatusCode::UNAUTHORIZED => self.challenged(&resp, &sent),
                // A Digest answer names the address it was made for: after a redirect it no
                // longer fits (Apache says 400). Asked again where the redirect led.
                StatusCode::BAD_REQUEST => matches!(sent, Sent::Digest(_)) && redirected,
                _ => false,
            };
            if !again || left == 0 {
                return Ok(resp);
            }
            if redirected {
                url = resp.url().clone();
            }
        }
    }
}

/// Puts the task's account into the link, for the requests this module does not send (HLS
/// playlists and their parts): the client then logs in with Basic, as it does for a link that
/// carries an account.
pub(crate) fn embed(o: &JobOptions, url: &mut Url) {
    if let Some(user) = &o.username {
        let _ = url.set_username(user);
        let _ = url.set_password(o.password.as_deref());
    }
}

/// The link without its account, to show and to store.
pub(crate) fn without_account(mut url: Url) -> Url {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url
}

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

impl Digest {
    /// A Digest challenge this module can answer, and whether it says the last nonce had only
    /// expired (`stale=true`).
    fn from_challenge(c: &Challenge) -> Option<(Self, bool)> {
        if c.scheme != "digest" {
            return None;
        }
        let name = c.params.get("algorithm").map_or("MD5", String::as_str);
        let (base, session) = match name.to_ascii_uppercase().strip_suffix("-SESS") {
            Some(base) => (base.to_owned(), true),
            None => (name.to_ascii_uppercase(), false),
        };
        let algo = match base.as_str() {
            "MD5" => HashAlgo::Md5,
            "SHA-256" => HashAlgo::Sha256,
            _ => return None,
        };
        let qop = match c.params.get("qop") {
            None => false,
            Some(list)
                if list
                    .split(',')
                    .any(|q| q.trim().eq_ignore_ascii_case("auth")) =>
            {
                true
            }
            // Only `auth-int` (the body is hashed too): not for downloads.
            Some(_) => return None,
        };
        let stale = c
            .params
            .get("stale")
            .is_some_and(|s| s.eq_ignore_ascii_case("true"));
        Some((
            Self {
                realm: c.params.get("realm").cloned().unwrap_or_default(),
                nonce: c.params.get("nonce")?.clone(),
                opaque: c.params.get("opaque").cloned(),
                algo,
                session,
                qop,
                count: 0,
                cnonce: uuid::Uuid::new_v4().simple().to_string(),
            },
            stale,
        ))
    }

    /// The `Authorization` value for a request (`count` already counts it).
    fn answer(&self, user: &str, password: &str, method: &str, url: &Url) -> String {
        let h = |s: String| hex_of(self.algo, s.as_bytes());
        let uri = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_owned(),
        };
        let mut ha1 = h(format!("{user}:{}:{password}", self.realm));
        if self.session {
            ha1 = h(format!("{ha1}:{}:{}", self.nonce, self.cnonce));
        }
        let ha2 = h(format!("{method}:{uri}"));
        let nc = format!("{:08x}", self.count);
        let response = if self.qop {
            h(format!(
                "{ha1}:{}:{nc}:{}:auth:{ha2}",
                self.nonce, self.cnonce
            ))
        } else {
            h(format!("{ha1}:{}:{ha2}", self.nonce))
        };
        let algorithm = match (self.algo, self.session) {
            (HashAlgo::Sha256, false) => "SHA-256",
            (HashAlgo::Sha256, true) => "SHA-256-sess",
            (_, false) => "MD5",
            (_, true) => "MD5-sess",
        };
        let mut out = format!(
            "Digest username={}, realm={}, nonce={}, uri={}, response=\"{response}\", algorithm={algorithm}",
            quoted(user),
            quoted(&self.realm),
            quoted(&self.nonce),
            quoted(&uri),
        );
        if self.qop {
            out.push_str(&format!(
                ", qop=auth, nc={nc}, cnonce={}",
                quoted(&self.cnonce)
            ));
        }
        if let Some(o) = &self.opaque {
            out.push_str(&format!(", opaque={}", quoted(o)));
        }
        out
    }
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One way of logging in a server offers: `Digest realm="x", nonce="y"`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Challenge {
    /// Lower case.
    scheme: String,
    /// Names in lower case.
    params: HashMap<String, String>,
}

/// The challenges of one `WWW-Authenticate` value. Several may share it, separated by commas
/// like their own parameters: a word that is not `name=value` starts a new one. A scheme may
/// instead be followed by one opaque word (`Negotiate YII=`), which is skipped.
fn challenges(value: &str) -> Vec<Challenge> {
    const BLANK: [char; 2] = [' ', '\t'];
    let mut out: Vec<Challenge> = Vec::new();
    let mut rest = value;
    // The last thing read was a scheme's name.
    let mut after_scheme = false;
    loop {
        let lead = rest.trim_start_matches(BLANK);
        let comma = lead.starts_with(',');
        rest = lead.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            return out;
        }
        let end = rest.find(['=', ',', ' ', '\t']).unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        // `name=value`; `abc==` and `abc=,` are the padding of an opaque word.
        let value = after
            .trim_start_matches(BLANK)
            .strip_prefix('=')
            .map(|v| v.trim_start_matches(BLANK))
            .filter(|v| !v.is_empty() && !v.starts_with(['=', ',']));
        if let (Some(value), Some(current)) = (value, out.last_mut()) {
            let (text, tail) = match value.strip_prefix('"') {
                Some(q) => unquote(q),
                None => {
                    let end = value.find(',').unwrap_or(value.len());
                    (value[..end].trim().to_owned(), &value[end..])
                }
            };
            current.params.insert(word.to_ascii_lowercase(), text);
            rest = tail;
            after_scheme = false;
        } else if (after_scheme && !comma) || word.is_empty() {
            // An opaque word, or something unreadable: on to the next comma.
            let skip = rest.find(',').unwrap_or(rest.len()).max(1);
            rest = &rest[skip.min(rest.len())..];
            after_scheme = false;
        } else {
            out.push(Challenge {
                scheme: word.to_ascii_lowercase(),
                params: HashMap::new(),
            });
            rest = after;
            after_scheme = true;
        }
    }
}

/// The text of a quoted string (after its opening quote) and what follows its closing one.
fn unquote(q: &str) -> (String, &str) {
    let mut text = String::new();
    let mut chars = q.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, escaped)) = chars.next() {
                    text.push(escaped);
                }
            }
            '"' => return (text, &q[i + 1..]),
            c => text.push(c),
        }
    }
    (text, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(url: &str, o: &JobOptions) -> (Auth, Url) {
        let mut url = Url::parse(url).unwrap();
        let a = Auth::new(o, &mut url);
        (a, url)
    }

    #[test]
    fn reads_challenges() {
        let c = challenges(
            r#"Digest realm="a, \"b\"", qop="auth,auth-int", nonce=abc, algorithm=SHA-256, stale=TRUE"#,
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].scheme, "digest");
        assert_eq!(c[0].params["realm"], "a, \"b\"");
        assert_eq!(c[0].params["qop"], "auth,auth-int");
        assert_eq!(c[0].params["nonce"], "abc");
        let (d, stale) = Digest::from_challenge(&c[0]).unwrap();
        assert!(stale && d.qop && !d.session);
        assert_eq!(d.algo, HashAlgo::Sha256);
        // Several in one value, in any order, with a scheme that has no parameters.
        let c = challenges(
            r#"Negotiate, Basic realm="files", charset="UTF-8", Digest realm="files", nonce="n1", opaque="o""#,
        );
        let schemes: Vec<&str> = c.iter().map(|c| c.scheme.as_str()).collect();
        assert_eq!(schemes, ["negotiate", "basic", "digest"]);
        assert_eq!(c[1].params["realm"], "files");
        assert_eq!(c[2].params["opaque"], "o");
        let c = challenges("Negotiate YII=, Basic realm=x");
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].params["realm"], "x");
        // What cannot be answered is not a Digest offer.
        for bad in [
            r#"Digest realm="r", nonce="n", algorithm=SHA-512-256"#,
            r#"Digest realm="r", nonce="n", qop="auth-int""#,
            r#"Digest realm="r""#,
            r#"Basic realm="r""#,
        ] {
            assert!(
                Digest::from_challenge(&challenges(bad)[0]).is_none(),
                "{bad}"
            );
        }
    }

    /// The example of RFC 7616 §3.9.1 (and of RFC 2617 for MD5).
    #[test]
    fn answers_digest_as_the_rfc_does() {
        let url = Url::parse("http://www.example.org/dir/index.html").unwrap();
        let mut d = Digest {
            realm: "http-auth@example.org".into(),
            nonce: "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v".into(),
            opaque: Some("FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS".into()),
            algo: HashAlgo::Sha256,
            session: false,
            qop: true,
            count: 1,
            cnonce: "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ".into(),
        };
        let a = d.answer("Mufasa", "Circle of Life", "GET", &url);
        assert!(
            a.contains(
                r#"response="753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1""#
            ),
            "{a}"
        );
        assert!(
            a.contains("algorithm=SHA-256, qop=auth, nc=00000001"),
            "{a}"
        );
        assert!(
            a.contains(r#"uri="/dir/index.html""#) && a.contains(r#"opaque="FQhe/"#),
            "{a}"
        );
        d.algo = HashAlgo::Md5;
        let a = d.answer("Mufasa", "Circle of Life", "GET", &url);
        assert!(
            a.contains(r#"response="8ca523f5e9506fed4657c9700eebdbec""#),
            "{a}"
        );
        // RFC 2069: no qop, no counters.
        d.qop = false;
        d.realm = "testrealm@host.com".into();
        d.nonce = "dcd98b7102dd2f0e8b11d0f600bfb0c093".into();
        let url = Url::parse("http://host.com/dir/index.html").unwrap();
        let a = d.answer("Mufasa", "CircleOfLife", "GET", &url);
        assert!(
            a.contains(r#"response="1949323746fe6a43ef61f9606e7febea""#),
            "{a}"
        );
        assert!(!a.contains("nc=") && !a.contains("cnonce="), "{a}");
    }

    #[test]
    fn takes_the_account_out_of_the_link_and_keeps_it_to_its_host() {
        let (a, url) = auth(
            "http://bob:s%40cret@files.example/a.bin?x=1",
            &JobOptions::default(),
        );
        assert_eq!(url.as_str(), "http://files.example/a.bin?x=1");
        assert_eq!(a.account, Some(("bob".into(), "s@cret".into())));
        // Plain HTTP: nothing before the server asks.
        assert!(a.header(&Method::GET, &url).0.is_none());
        // The task's own account wins.
        let o = JobOptions {
            username: Some("alice".into()),
            password: Some("pw".into()),
            ..JobOptions::default()
        };
        let (a, url) = auth("https://bob:x@files.example/a.bin", &o);
        assert_eq!(a.account, Some(("alice".into(), "pw".into())));
        // HTTPS: Basic at once.
        let (h, sent) = a.header(&Method::GET, &url);
        assert_eq!(h.unwrap().as_bytes(), b"Basic YWxpY2U6cHc=");
        assert!(sent == Sent::Basic);
        // Another host gets nothing.
        let other = Url::parse("https://cdn.example/a.bin").unwrap();
        assert!(a.header(&Method::GET, &other).0.is_none());
        // No account: nothing, ever.
        let (a, url) = auth("https://files.example/a.bin", &JobOptions::default());
        assert!(a.header(&Method::GET, &url).0.is_none());
    }
}
