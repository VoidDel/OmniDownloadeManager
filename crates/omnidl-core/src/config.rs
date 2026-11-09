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
    /// Connections per download unless the task says otherwise.
    pub connections: u32,
    /// Global download limit in bytes per second.
    pub speed_limit: Option<u64>,
    pub user_agent: String,
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
            speed_limit: None,
            user_agent: default_user_agent().into(),
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
