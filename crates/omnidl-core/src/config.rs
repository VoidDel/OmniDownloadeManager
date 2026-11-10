//! Global engine settings (docs/development-plan.md §4.5). The app keeps them in its settings
//! file and hands them to [`crate::TaskManager::set_config`].

use serde::{Deserialize, Serialize};

use crate::ProxyConfig;

/// What happens when the file name is taken in the target folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// `name (1).ext`.
    #[default]
    Rename,
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineConfig {
    /// Downloads running at once.
    pub max_concurrent: u32,
    /// Connections per download unless the task says otherwise (when not tuned).
    pub connections: u32,
    /// Tune the number of connections by the speed they bring (`conns::Tuner`), up to
    /// `auto_max_connections`, unless the task sets a number.
    pub auto_connections: bool,
    pub auto_max_connections: u32,
    /// Global download limit in bytes per second.
    pub speed_limit: Option<u64>,
    /// The user agent when not chosen automatically.
    pub user_agent: String,
    /// Start with a browser's user agent and switch to other common ones when a server refuses
    /// it (403 / 406). Off: always `user_agent`.
    pub user_agent_auto: bool,
    pub proxy: ProxyConfig,
    pub connect_timeout_secs: u32,
    /// A connection that receives nothing for this long is dropped and retried.
    pub read_timeout_secs: u32,
    /// Attempts per connection after a network error or a 5xx, with exponential backoff.
    pub max_retries: u32,
    pub conflict: ConflictPolicy,
    /// Downloads that were running when the app quit start again on launch.
    pub resume_on_startup: bool,
    /// Sync the file to disk before each progress checkpoint (crash-safe resume). Off trades
    /// that guarantee for less disk work.
    pub sync_writes: bool,
    /// BitTorrent: keep sharing a torrent after it finished downloading.
    pub seeding: bool,
    /// Stop seeding once this many times the torrent's size was uploaded (0: no limit).
    pub seed_ratio: f64,
    /// Stop seeding this many minutes after the download finished (0: no limit).
    pub seed_minutes: u32,
    /// BitTorrent upload limit in bytes per second.
    pub upload_limit: Option<u64>,
    /// Port for incoming BitTorrent connections (0: any free port). Takes effect on restart,
    /// like the two switches below.
    pub bt_port: u16,
    /// Find peers through the distributed hash table (needed for most magnet links).
    pub bt_dht: bool,
    /// Ask the router to forward `bt_port` (UPnP).
    pub bt_upnp: bool,
    /// IP filtering: peers in the block lists (`bt_ip_filter_lists`, web addresses or files,
    /// and the addresses in `bt_banned`) are not connected to. Read when BitTorrent starts.
    pub bt_ip_filter: bool,
    /// Was a single `btIpFilterList`.
    #[serde(alias = "btIpFilterList", deserialize_with = "one_or_many")]
    pub bt_ip_filter_lists: Vec<String>,
    pub bt_banned: Vec<String>,
    /// Trackers added to every torrent that is not private: `bt_trackers`, and the lists
    /// fetched from `bt_trackers_urls` (web addresses or files, kept by the app, refreshed
    /// daily). Used by tasks that start after a change.
    pub bt_extra_trackers: bool,
    pub bt_trackers: Vec<String>,
    pub bt_trackers_urls: Vec<String>,
    /// FTP sessions per download unless the task says otherwise (servers often limit them).
    pub ftp_connections: u32,
    /// FTP active mode by default.
    pub ftp_active: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 3,
            connections: 8,
            auto_connections: true,
            auto_max_connections: 16,
            speed_limit: None,
            user_agent: default_user_agent().into(),
            user_agent_auto: true,
            proxy: ProxyConfig::System,
            connect_timeout_secs: 15,
            read_timeout_secs: 30,
            max_retries: 10,
            conflict: ConflictPolicy::Rename,
            resume_on_startup: true,
            sync_writes: true,
            seeding: true,
            seed_ratio: 1.5,
            seed_minutes: 24 * 60,
            upload_limit: None,
            bt_port: 6881,
            bt_dht: true,
            bt_upnp: true,
            bt_ip_filter: false,
            bt_ip_filter_lists: Vec::new(),
            bt_banned: Vec::new(),
            bt_extra_trackers: false,
            bt_trackers: Vec::new(),
            bt_trackers_urls: Vec::new(),
            ftp_connections: 2,
            ftp_active: false,
        }
    }
}

impl EngineConfig {
    /// Clamps values the UI or a hand-edited file may have pushed out of range.
    pub fn normalized(mut self) -> Self {
        self.max_concurrent = self.max_concurrent.clamp(1, 32);
        self.connections = self.connections.clamp(1, 64);
        self.auto_max_connections = self.auto_max_connections.clamp(2, 64);
        self.ftp_connections = self.ftp_connections.clamp(1, 8);
        self.connect_timeout_secs = self.connect_timeout_secs.clamp(1, 600);
        self.read_timeout_secs = self.read_timeout_secs.clamp(1, 3600);
        self.max_retries = self.max_retries.min(100);
        if self.speed_limit == Some(0) {
            self.speed_limit = None;
        }
        if self.upload_limit == Some(0) {
            self.upload_limit = None;
        }
        if !self.seed_ratio.is_finite() || self.seed_ratio < 0.0 {
            self.seed_ratio = 0.0;
        }
        self.seed_ratio = self.seed_ratio.min(1000.0);
        if self.user_agent.trim().is_empty() {
            self.user_agent = default_user_agent().into();
        }
        self
    }
}

/// A list, or the single value an older settings file has (empty: none).
fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) if s.trim().is_empty() => Vec::new(),
        OneOrMany::One(s) => vec![s.trim().to_owned()],
        OneOrMany::Many(v) => v,
    })
}

/// Common user agents to choose from (name, value), in the order automatic choice tries the
/// ones after the browser's: this app's own name, another browser, then download tools.
pub fn user_agent_presets() -> Vec<(&'static str, String)> {
    let (firefox, edge) = if cfg!(target_os = "windows") {
        (
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:144.0) Gecko/20100101 Firefox/144.0",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0",
        )
    } else if cfg!(target_os = "macos") {
        (
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:144.0) Gecko/20100101 Firefox/144.0",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0",
        )
    } else {
        (
            "Mozilla/5.0 (X11; Linux x86_64; rv:144.0) Gecko/20100101 Firefox/144.0",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0",
        )
    };
    vec![
        ("Chrome", default_user_agent().into()),
        (
            "Omni Download",
            format!("OmniDownload/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("Firefox", firefox.into()),
        ("Edge", edge.into()),
        (
            "Safari",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like \
             Gecko) Version/26.0 Safari/605.1.15"
                .into(),
        ),
        ("Wget", "Wget/1.24.5".into()),
        ("curl", "curl/8.7.1".into()),
    ]
}

/// A desktop Chrome user agent for this platform. Some servers refuse unknown clients; a
/// browser's string is what download managers send by default (each task can override it).
pub fn default_user_agent() -> &'static str {
    if cfg!(target_os = "windows") {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/142.0.0.0 Safari/537.36"
    } else if cfg!(target_os = "macos") {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/142.0.0.0 Safari/537.36"
    } else {
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/142.0.0.0 Safari/537.36"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_single_block_list_of_older_settings() {
        let old: EngineConfig =
            serde_json::from_str(r#"{"btIpFilterList": " https://a.example/list.gz "}"#).unwrap();
        assert_eq!(old.bt_ip_filter_lists, ["https://a.example/list.gz"]);
        let none: EngineConfig = serde_json::from_str(r#"{"btIpFilterList": ""}"#).unwrap();
        assert!(none.bt_ip_filter_lists.is_empty());
        let new: EngineConfig = serde_json::from_str(r#"{"btIpFilterLists": ["a", "b"]}"#).unwrap();
        assert_eq!(new.bt_ip_filter_lists, ["a", "b"]);
    }
}
