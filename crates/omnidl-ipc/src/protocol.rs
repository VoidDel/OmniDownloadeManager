//! Messages (docs/development-plan.md §5.5). Field names are snake_case on the wire, as the plan's
//! examples; a request is `{ "id", "type", "payload"? }`, a response `{ "id", "ok", "result" |
//! "error" }`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The Native Messaging host name the extensions connect to.
pub const HOST_NAME: &str = "app.omnidownload.host";

/// A message from an extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Chosen by the extension; echoed in the response.
    pub id: String,
    /// `chrome`, `edge`, `firefox`…: which browser's extension sent it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(flatten)]
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum Body {
    /// Is the app there? Never starts it.
    Ping,
    /// Version, speed and recent tasks for the popup. Never starts it.
    GetStatus,
    /// Shows the main window, starting the app if needed.
    OpenApp,
    /// A download the browser was about to make (or a link from the context menu).
    AddDownload(AddDownload),
    /// Several links ("download all links on this page"): opens the new-download dialog.
    AddBatch(AddBatch),
    /// A magnet link (BitTorrent arrives in Phase 3).
    AddMagnet(AddMagnet),
}

impl Body {
    /// Whether the host should start the app for this message.
    pub fn launches_app(&self) -> bool {
        !matches!(self, Self::Ping | Self::GetStatus)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddDownload {
    pub url: String,
    /// The name the browser chose (Content-Disposition already decoded), without a folder.
    pub filename: Option<String>,
    /// Bytes, when the browser knows.
    pub size: Option<u64>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    /// `Cookie` header value for `url`.
    pub cookies: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub mime: Option<String>,
    /// The page the download started from.
    pub tab_url: Option<String>,
    /// Start without asking (no quick-download window).
    pub silent: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddBatch {
    pub items: Vec<AddDownload>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddMagnet {
    pub uri: String,
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
}

impl Response {
    pub fn ok(id: impl Into<String>, result: impl Serialize) -> Self {
        Self {
            id: id.into(),
            ok: true,
            result: Some(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
            error: None,
        }
    }

    pub fn err(id: impl Into<String>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ok: false,
            result: None,
            error: Some(BridgeError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The app is not running (and was not started: a status query, or starting it failed).
    AppNotRunning,
    /// Not a message this version understands.
    BadRequest,
    /// Understood but not supported yet (magnet links before Phase 3).
    Unsupported,
    /// The pairing token of the HTTP channel is missing or wrong.
    Unauthorized,
    Internal,
}

/// `ping` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    pub app_version: String,
}

/// `get_status` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub app_version: String,
    /// Downloads running.
    pub running: u32,
    /// Bytes per second, all downloads.
    pub download_speed: u64,
    /// The most recently added tasks, newest first.
    pub recent: Vec<RecentTask>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentTask {
    pub id: String,
    pub name: String,
    /// The app's task status (`downloading`, `completed`…).
    pub status: String,
    /// 0–1, when the size is known.
    pub progress: Option<f64>,
}

/// `add_download` result: what happened to the download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddResult {
    pub action: AddAction,
    /// The user was asked (the quick-download window): the extension need not say more.
    #[serde(default)]
    pub asked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddAction {
    /// Downloading in the app.
    Started,
    /// Added paused ("download later").
    Queued,
    /// The user wants the browser to download it after all.
    Browser,
    /// The user closed the window: nobody downloads it.
    Cancelled,
    /// Shown in the new-download dialog (batches).
    Dialog,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn requests_look_like_the_plan() {
        let r: Request = serde_json::from_value(json!({
            "id": "a1b2", "browser": "edge", "type": "add_download",
            "payload": {
                "url": "https://example.com/file.zip", "filename": "file.zip", "size": 104857600,
                "referer": "https://example.com/page", "cookies": "session=abc; theme=dark",
                "headers": { "Authorization": "Bearer x" }, "mime": "application/zip",
                "tab_url": "https://example.com/page"
            }
        }))
        .unwrap();
        assert_eq!(r.id, "a1b2");
        assert_eq!(r.browser.as_deref(), Some("edge"));
        let Body::AddDownload(d) = &r.body else {
            panic!("{r:?}")
        };
        assert_eq!(d.size, Some(104857600));
        assert_eq!(d.headers["Authorization"], "Bearer x");
        assert!(!d.silent);
        assert!(r.body.launches_app());

        let ping: Request = serde_json::from_value(json!({ "id": "1", "type": "ping" })).unwrap();
        assert_eq!(ping.body, Body::Ping);
        assert!(!ping.body.launches_app());
        assert_eq!(
            serde_json::to_value(&ping).unwrap(),
            json!({ "id": "1", "type": "ping" })
        );
        assert!(serde_json::from_value::<Request>(json!({ "id": "1", "type": "nope" })).is_err());
    }

    #[test]
    fn responses_look_like_the_plan() {
        let ok = Response::ok(
            "a1b2",
            AddResult {
                action: AddAction::Started,
                asked: false,
                task_id: Some("uuid".into()),
            },
        );
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            json!({ "id": "a1b2", "ok": true, "result": { "action": "started", "asked": false, "task_id": "uuid" } })
        );
        let err = Response::err("a1b2", ErrorCode::AppNotRunning, "not running");
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({ "id": "a1b2", "ok": false, "error": { "code": "app_not_running", "message": "not running" } })
        );
    }
}
