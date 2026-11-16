//! Answers the extensions' requests (docs/development-plan.md §5.5), whichever channel they came
//! by (the Native Messaging host's pipe or the HTTP fallback).

use omnidl_core::{NewTask, PostData, ProbeInfo, TaskKind, TaskOptions, filename};
use omnidl_ipc::{
    AddAction, AddBatch, AddDownload, AddResult, Body, ErrorCode, Pong, RecentTask, Request,
    Response, Status,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::quick::{self, QuickAction, QuickRequest};
use crate::state::AppState;

/// A frame in, a frame out.
pub async fn handle_frame(app: AppHandle, frame: Vec<u8>) -> Vec<u8> {
    match serde_json::from_slice::<Request>(&frame) {
        Ok(req) => handle(&app, req).await.to_bytes(),
        Err(e) => {
            let id = serde_json::from_slice::<serde_json::Value>(&frame)
                .ok()
                .and_then(|v| v["id"].as_str().map(str::to_owned))
                .unwrap_or_default();
            Response::err(id, ErrorCode::BadRequest, e.to_string()).to_bytes()
        }
    }
}

pub async fn handle(app: &AppHandle, req: Request) -> Response {
    let Some(state) = app.try_state::<AppState>() else {
        return Response::err(req.id, ErrorCode::Internal, "starting up");
    };
    if let Some(b) = req.browser.as_deref()
        && state.saw_browser(b)
    {
        let _ = app.emit("browser:seen", state.seen_browsers());
    }
    let id = req.id;
    match req.body {
        Body::Ping => Response::ok(
            id,
            Pong {
                app_version: env!("CARGO_PKG_VERSION").into(),
                capabilities: vec![omnidl_ipc::CAP_POST.into(), omnidl_ipc::CAP_DASH.into()],
            },
        ),
        Body::GetStatus => Response::ok(id, status(&state)),
        Body::OpenApp => {
            crate::focus_main(app);
            Response::ok(id, serde_json::json!({}))
        }
        Body::AddDownload(d) => match add_download(app, d, req.browser).await {
            Ok(r) => Response::ok(id, r),
            Err((code, msg)) => Response::err(id, code, msg),
        },
        Body::AddBatch(b) => {
            open_batch(app, b);
            Response::ok(
                id,
                AddResult {
                    action: AddAction::Dialog,
                    asked: true,
                    task_id: None,
                },
            )
        }
        // The dialog shows the torrent's files to choose from.
        Body::AddMagnet(m) => {
            if !m.uri.trim().to_ascii_lowercase().starts_with("magnet:") {
                return Response::err(id, ErrorCode::BadRequest, "not a magnet link");
            }
            crate::links::open(app, vec![m.uri.trim().to_owned()]);
            Response::ok(
                id,
                AddResult {
                    action: AddAction::Dialog,
                    asked: true,
                    task_id: None,
                },
            )
        }
    }
}

fn status(state: &AppState) -> Status {
    let tasks = state.manager.list();
    let running = tasks.iter().filter(|t| t.status.is_running()).count() as u32;
    let download_speed = tasks.iter().map(|t| t.speed_bps).sum();
    let mut recent: Vec<_> = tasks
        .iter()
        .filter(|t| t.status != omnidl_core::TaskStatus::Removed)
        .collect();
    recent.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    Status {
        app_version: env!("CARGO_PKG_VERSION").into(),
        running,
        download_speed,
        recent: recent
            .into_iter()
            .take(5)
            .map(|t| RecentTask {
                id: t.id.to_string(),
                name: t.file_name.clone(),
                status: t.status.as_str().into(),
                progress: if t.status == omnidl_core::TaskStatus::Completed {
                    Some(1.0)
                } else {
                    t.total_bytes
                        .filter(|&n| n > 0)
                        .map(|n| t.downloaded_bytes as f64 / n as f64)
                },
            })
            .collect(),
    }
}

/// The site a download belongs to: the page's host (without `www.`), else the file's.
pub fn site_of(d: &AddDownload) -> String {
    let host = |u: &str| {
        url::Url::parse(u).ok().and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
    };
    d.tab_url
        .as_deref()
        .and_then(host)
        .or_else(|| host(&d.url))
        .unwrap_or_default()
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The request details the browser would send, as task options.
fn options_of(d: &AddDownload) -> TaskOptions {
    TaskOptions {
        // The browser's name, already decoded from Content-Disposition; never a path.
        file_name: non_empty(&d.filename)
            .map(|n| filename::sanitize(n.rsplit(['/', '\\']).next().unwrap_or_default())),
        user_agent: non_empty(&d.user_agent),
        referer: non_empty(&d.referer).or_else(|| non_empty(&d.tab_url)),
        cookies: non_empty(&d.cookies),
        headers: d.headers.clone(),
        // An HTTP link's sound only: anything else is not a stream to join.
        audio_source: non_empty(&d.audio_url)
            .filter(|a| TaskKind::detect(a) == Some(TaskKind::Http)),
        // The variant of an HTTP link's HLS playlist.
        hls_variant: non_empty(&d.hls_variant)
            .filter(|v| TaskKind::detect(v) == Some(TaskKind::Http))
            .filter(|_| TaskKind::detect(&d.url) == Some(TaskKind::Http)),
        // What a form sent, for an HTTP link.
        post: d
            .post
            .as_ref()
            .filter(|p| !p.content_type.trim().is_empty())
            .filter(|_| TaskKind::detect(&d.url) == Some(TaskKind::Http))
            .map(|p| PostData {
                content_type: p.content_type.trim().to_owned(),
                body: p.body.clone(),
            }),
        ..Default::default()
    }
}

fn is_page(mime: &str) -> bool {
    let m = mime.split(';').next().unwrap_or_default().trim();
    m.eq_ignore_ascii_case("text/html") || m.eq_ignore_ascii_case("application/xhtml+xml")
}

/// The answer to a form sent a second time is a page, where the browser got a file: the form
/// works once (a security code used up, a session of the browser's own), and the page is not
/// what the user asked for.
fn page_instead_of_file(d: &AddDownload, probe: &ProbeInfo) -> bool {
    let wanted_page = match non_empty(&d.mime) {
        Some(m) => is_page(&m),
        // The browser did not say: by the name it gave the file.
        None => non_empty(&d.filename).is_some_and(|n| {
            matches!(
                filename::split_extension(&n)
                    .1
                    .to_ascii_lowercase()
                    .as_str(),
                ".html" | ".htm"
            )
        }),
    };
    !wanted_page && probe.content_type.as_deref().is_some_and(is_page)
}

/// Sends the form of a download once more, before the browser is told to give its own download
/// up: `None` when the server does not give the file again (the browser keeps its download).
async fn form_answer(state: &AppState, d: &AddDownload, task: &NewTask) -> Option<ProbeInfo> {
    match state.manager.probe(&task.source, &task.options).await {
        Ok(p) if page_instead_of_file(d, &p) => {
            tracing::info!("a form's second answer is a page: left to the browser");
            None
        }
        Ok(p) => Some(p),
        Err(e) => {
            tracing::info!("a form's second answer: {e}: left to the browser");
            None
        }
    }
}

async fn add_download(
    app: &AppHandle,
    d: AddDownload,
    browser: Option<String>,
) -> Result<AddResult, (ErrorCode, String)> {
    let url = d.url.trim().to_owned();
    match TaskKind::detect(&url) {
        Some(TaskKind::Http | TaskKind::Ftp) => {}
        // A torrent: its files are chosen in the new-download dialog.
        Some(TaskKind::Torrent) => {
            crate::links::open(app, vec![url]);
            return Ok(AddResult {
                action: AddAction::Dialog,
                asked: true,
                task_id: None,
            });
        }
        None => return Err((ErrorCode::BadRequest, format!("not a link: {url}"))),
    }
    let state = app.state::<AppState>();
    let settings = state.settings();
    let site = site_of(&d);
    let options = options_of(&d);
    // The folder of the file's category: by the browser's name for it, or the link's.
    let named = options
        .file_name
        .clone()
        .or_else(|| filename::from_url(&url))
        .unwrap_or_default();
    let mut task = NewTask {
        source: url.clone(),
        save_dir: state.dir_for(&named),
        options,
        start: true,
        probe: None,
    };
    // The answer to a form: only taken when the server gives the file a second time. What it
    // said then is shown as it is (the window does not ask again).
    if task.options.post.is_some() {
        match form_answer(&state, &d, &task).await {
            Some(p) => task.probe = Some(p),
            None => {
                return Ok(AddResult {
                    action: AddAction::Browser,
                    asked: false,
                    task_id: None,
                });
            }
        }
    }
    let ask = !d.silent
        && settings.browser.quick_dialog
        && !settings.browser.silent_sites.iter().any(|s| s == &site);
    if ask {
        let request = QuickRequest {
            url: url.clone(),
            file_name: task.options.file_name.clone(),
            size: d.size,
            mime: d.mime.clone(),
            site: site.clone(),
            browser,
            has_cookies: task.options.cookies.is_some(),
            has_referer: task.options.referer.is_some(),
            save_dir: task.save_dir.clone(),
            connections: settings.engine.connections,
            options: task.options.clone(),
            origin: quick::Origin::Browser,
            probe: task.probe.clone(),
        };
        let decision = quick::ask(app, request).await;
        match decision.action {
            QuickAction::Browser => {
                return Ok(AddResult {
                    action: AddAction::Browser,
                    asked: true,
                    task_id: None,
                });
            }
            QuickAction::Cancel => {
                return Ok(AddResult {
                    action: AddAction::Cancelled,
                    asked: true,
                    task_id: None,
                });
            }
            QuickAction::Start | QuickAction::Later => {
                let remember = decision.remember;
                decision.apply(&mut task, settings.engine.connections);
                if remember && !site.is_empty() {
                    state.update_settings(|s| {
                        if !s.browser.silent_sites.contains(&site) {
                            s.browser.silent_sites.push(site.clone());
                        }
                    });
                    let _ = app.emit("settings:changed", state.settings());
                }
            }
        }
    }
    let start = task.start;
    let added = state
        .manager
        .add(vec![task])
        .await
        .map_err(|e| (ErrorCode::Internal, e.message))?;
    Ok(AddResult {
        action: if start {
            AddAction::Started
        } else {
            AddAction::Queued
        },
        asked: ask,
        task_id: added.first().map(|t| t.id.to_string()),
    })
}

/// What the new-download dialog opens with: links one per line, and request details.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefill {
    pub text: String,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

/// A batch opens the new-download dialog in the main window with the links (the user picks).
fn open_batch(app: &AppHandle, b: AddBatch) {
    let first = b.items.first().map(options_of).unwrap_or_default();
    let prefill = Prefill {
        text: b
            .items
            .iter()
            .map(|i| i.url.trim())
            .filter(|u| !u.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        referer: first.referer,
        cookies: first.cookies,
        user_agent: first.user_agent,
    };
    crate::new_task(app, prefill);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_site_from_the_page() {
        let d = AddDownload {
            url: "https://cdn.example.net/f.zip".into(),
            tab_url: Some("https://www.example.com/page".into()),
            ..Default::default()
        };
        assert_eq!(site_of(&d), "example.com");
        let d = AddDownload {
            url: "https://cdn.example.net/f.zip".into(),
            ..Default::default()
        };
        assert_eq!(site_of(&d), "cdn.example.net");
    }

    #[test]
    fn keeps_what_the_browser_would_send() {
        let d = AddDownload {
            url: "https://e.com/f".into(),
            filename: Some("C:\\Users\\x\\Downloads\\report?.pdf".into()),
            cookies: Some(" s=1 ".into()),
            referer: Some(String::new()),
            tab_url: Some("https://e.com/page".into()),
            user_agent: Some("UA".into()),
            ..Default::default()
        };
        let o = options_of(&d);
        assert_eq!(o.file_name.as_deref(), Some("report_.pdf"));
        assert_eq!(o.cookies.as_deref(), Some("s=1"));
        assert_eq!(o.referer.as_deref(), Some("https://e.com/page"));
        assert_eq!(o.user_agent.as_deref(), Some("UA"));
        assert_eq!(o.post, None);
        assert_eq!(o.hls_variant, None);
    }

    #[test]
    fn keeps_the_chosen_hls_variant() {
        let d = AddDownload {
            url: "https://e.com/v/master.m3u8".into(),
            hls_variant: Some(" https://e.com/v/360p.m3u8 ".into()),
            ..Default::default()
        };
        assert_eq!(
            options_of(&d).hls_variant.as_deref(),
            Some("https://e.com/v/360p.m3u8")
        );
        let odd = AddDownload {
            hls_variant: Some("javascript:alert(1)".into()),
            ..d
        };
        assert_eq!(options_of(&odd).hls_variant, None);
    }

    #[test]
    fn keeps_what_a_form_sent() {
        let post = omnidl_ipc::PostBody {
            content_type: " application/x-www-form-urlencoded ".into(),
            body: "aW5udW09MQ==".into(),
        };
        let d = AddDownload {
            url: "https://e.com/download/1.html".into(),
            post: Some(post.clone()),
            ..Default::default()
        };
        let o = options_of(&d).post.unwrap();
        assert_eq!(o.content_type, "application/x-www-form-urlencoded");
        assert_eq!(o.body, "aW5udW09MQ==");
        // Not for a link that is not HTTP, nor without a type.
        let ftp = AddDownload {
            url: "ftp://e.com/f".into(),
            post: Some(post),
            ..Default::default()
        };
        assert_eq!(options_of(&ftp).post, None);
        let untyped = AddDownload {
            url: "https://e.com/f".into(),
            post: Some(omnidl_ipc::PostBody::default()),
            ..Default::default()
        };
        assert_eq!(options_of(&untyped).post, None);
    }

    #[test]
    fn tells_a_page_from_the_file_a_form_gave() {
        let probe = |content_type: &str| ProbeInfo {
            final_url: "https://e.com/download/1.html".into(),
            file_name: "1.html".into(),
            total: None,
            resumable: false,
            content_type: Some(content_type.into()),
            etag: None,
            last_modified: None,
            files: Vec::new(),
        };
        let d = |mime: Option<&str>, filename: Option<&str>| AddDownload {
            url: "https://e.com/download/1.html".into(),
            mime: mime.map(str::to_owned),
            filename: filename.map(str::to_owned),
            ..Default::default()
        };
        let pdf = d(Some("application/pdf"), Some("sheet.pdf"));
        assert!(page_instead_of_file(
            &pdf,
            &probe("text/html; charset=UTF-8")
        ));
        assert!(!page_instead_of_file(&pdf, &probe("application/pdf")));
        assert!(!page_instead_of_file(
            &pdf,
            &probe("application/octet-stream")
        ));
        // The browser did not say what it got: by the file's name.
        assert!(page_instead_of_file(
            &d(None, Some("sheet.PDF")),
            &probe("text/html")
        ));
        assert!(!page_instead_of_file(
            &d(None, Some("report.html")),
            &probe("text/html")
        ));
        // A page was what the form gave the browser too.
        assert!(!page_instead_of_file(
            &d(Some("text/html"), None),
            &probe("text/html")
        ));
    }
}
