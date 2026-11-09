//! Per-task options (docs/development-plan.md appendix B). Every field is optional: unset ones
//! follow the global settings ([`crate::EngineConfig`]) at the time the task runs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskOptions {
    /// The name the user chose; otherwise the engine names the file from the server's answer.
    pub file_name: Option<String>,
    /// Connections for this task (1–64).
    pub connections: Option<u32>,
    /// Bytes per second; `None` is unlimited (the global limit still applies).
    pub speed_limit: Option<u64>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    /// `Cookie` header value (from the browser extension, Phase 2).
    pub cookies: Option<String>,
    /// Extra request headers.
    pub headers: BTreeMap<String, String>,
    pub proxy: Option<ProxyConfig>,
    /// BitTorrent: indices of the files to download, in the torrent's order (`None`: all).
    pub only_files: Option<Vec<usize>>,
}

/// Where requests go.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ProxyConfig {
    /// The operating system's proxy settings.
    #[default]
    System,
    /// Direct connections.
    None,
    /// `http://`, `https://`, `socks5://` or `socks5h://` (remote DNS), with optional
    /// `user:password@`.
    Custom { url: String },
}
