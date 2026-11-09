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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        }
    }
}

impl EngineConfig {
    /// Clamps values the UI or a hand-edited file may have pushed out of range.
    pub fn normalized(mut self) -> Self {
        self.max_concurrent = self.max_concurrent.clamp(1, 32);
        self.connections = self.connections.clamp(1, 64);
        self.connect_timeout_secs = self.connect_timeout_secs.clamp(1, 600);
        self.read_timeout_secs = self.read_timeout_secs.clamp(1, 3600);
        self.max_retries = self.max_retries.min(100);
        if self.speed_limit == Some(0) {
            self.speed_limit = None;
        }
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
