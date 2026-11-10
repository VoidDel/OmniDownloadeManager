//! User agents to try when a server refuses the configured one ("automatic" in the settings).
//! Some servers answer 403 to a browser's user agent coming from a program (mirrors of Linux
//! images do), others to anything but a browser. On 403 or 406 the probe is repeated with the
//! next of a few common user agents; the one that works is remembered for the host (for this run
//! of the app). A user agent chosen in the settings or by the task, and a task that carries
//! cookies (a browser's may only work with that browser's user agent), are kept.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use omnidl_core::{Error, JobOptions, Result};
use reqwest::Client;
use url::Url;

use crate::client;

/// What else is tried, in order, after the configured user agent: the presets of the settings
/// but other Chromium browsers (a server that refuses Chrome refuses those too).
fn others() -> Vec<String> {
    omnidl_core::user_agent_presets()
        .into_iter()
        .filter(|(name, _)| !matches!(*name, "Chrome" | "Edge" | "Safari"))
        .map(|(_, ua)| ua)
        .collect()
}

fn remembered() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn host(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_ascii_lowercase()
}

/// The user agents to try for `url`, in order: the one that worked for its host before, the
/// configured one, then the others. Only the configured one when it may not change.
pub(crate) fn candidates(o: &JobOptions, url: &Url) -> Vec<String> {
    if o.user_agent_fixed {
        return vec![o.user_agent.clone()];
    }
    let mut out: Vec<String> = Vec::new();
    let known = remembered()
        .lock()
        .ok()
        .and_then(|m| m.get(&host(url)).cloned());
    for ua in known
        .into_iter()
        .chain([o.user_agent.clone()])
        .chain(others())
    {
        if !ua.trim().is_empty() && !out.contains(&ua) {
            out.push(ua);
        }
    }
    out
}

/// The user agent to start with for `url` (HLS, which does not try others).
pub(crate) fn preferred(o: &JobOptions, url: &Url) -> String {
    candidates(o, url).swap_remove(0)
}

/// The server refused this user agent, perhaps: worth trying another.
pub(crate) fn refused(e: &Error) -> bool {
    matches!(e.status, Some(403 | 406))
}

/// `ua` worked for `url` where the configured one did not.
pub(crate) fn remember(url: &Url, ua: &str, configured: &str) {
    if let Ok(mut m) = remembered().lock() {
        if ua == configured {
            m.remove(&host(url));
        } else {
            tracing::info!("{}: using user agent {ua:?}", host(url));
            m.insert(host(url), ua.to_owned());
        }
    }
}

/// Runs `probe` with a client for each user agent in turn until the server does not refuse it;
/// returns that client (for the download) and the answer.
pub(crate) async fn probe_with<T, F, Fut>(
    o: &JobOptions,
    url: &Url,
    mut probe: F,
) -> Result<(Client, T)>
where
    F: FnMut(Client) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut refusal = None;
    for ua in candidates(o, url) {
        let client = client::build_as(o, &ua)?;
        match probe(client.clone()).await {
            Ok(answer) => {
                remember(url, &ua, &o.user_agent);
                return Ok((client, answer));
            }
            Err(e) if refused(&e) => {
                tracing::debug!("{url}: {e} as {ua:?}");
                refusal = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(refusal.unwrap_or_else(|| Error::internal("no user agent to try")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tries_others_unless_the_task_fixes_it() {
        let url = Url::parse("https://mirror.example/x.iso").unwrap();
        let o = JobOptions::default();
        let all = candidates(&o, &url);
        assert_eq!(all[0], o.user_agent);
        assert!(all[1].starts_with("OmniDownload/"));
        assert_eq!(all.len(), 5);
        let fixed = JobOptions {
            user_agent_fixed: true,
            ..JobOptions::default()
        };
        assert_eq!(
            candidates(&fixed, &url),
            std::slice::from_ref(&fixed.user_agent)
        );
        // What worked comes first next time.
        remember(&url, "Wget/1.24.5", &o.user_agent);
        assert_eq!(candidates(&o, &url)[0], "Wget/1.24.5");
        assert_eq!(preferred(&o, &url), "Wget/1.24.5");
        remember(&url, &o.user_agent, &o.user_agent);
        assert_eq!(candidates(&o, &url)[0], o.user_agent);
    }
}
