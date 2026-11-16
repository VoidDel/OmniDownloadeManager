//! A download that is the answer to a form (`TaskOptions::post`): the request is a POST with
//! what the form sent, as the browser extension saw it.
//!
//! The server answers each such request with the whole file, and sending it again may not be
//! free (a one-time code, a counter): a run asks once and writes that answer to the file, in
//! one connection, without ranges, and a transfer that fails is not repeated on its own.
//!
//! Probing sends the form too (the app checks the server gives the file before the browser
//! gives its own download up). That answer is kept (`keep`) and the download that follows reads
//! it (`take`) instead of sending the form once more: a code that works only once still brings
//! the file.

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use omnidl_core::{Error, JobOptions, Result};
use parking_lot::Mutex;
use reqwest::{
    Client, Method, RequestBuilder, Response,
    header::{CONTENT_TYPE, HeaderValue},
};
use url::Url;

use crate::{
    auth::Auth,
    client::{request_error, response_error},
    probe::{self, Probe},
};

/// What the form sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Form {
    content_type: HeaderValue,
    body: Bytes,
}

impl Form {
    /// The form of a job, if it has one.
    pub fn of(o: &JobOptions) -> Result<Option<Self>> {
        let Some(post) = &o.post else {
            return Ok(None);
        };
        let content_type = HeaderValue::from_str(post.content_type.trim())
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::invalid("the form has no usable Content-Type"))?;
        let body = STANDARD
            .decode(post.body.trim())
            .map_err(|e| Error::invalid(format!("the form's body is not base64: {e}")))?;
        Ok(Some(Self {
            content_type,
            body: body.into(),
        }))
    }

    fn apply(&self, r: RequestBuilder) -> RequestBuilder {
        r.header(CONTENT_TYPE, self.content_type.clone())
            .body(self.body.clone())
    }

    /// Sends the form. The answer's headers say what the file is; its body is the file.
    pub async fn submit(&self, client: &Client, auth: &Auth, url: &Url) -> Result<Answer> {
        let r = auth
            .send(client, Method::POST, url, |r| self.apply(r))
            .await
            .map_err(|e| request_error(&e))?;
        if !r.status().is_success() {
            return Err(response_error(&r));
        }
        Ok(Answer {
            probe: probe::of_whole(&r),
            response: r,
        })
    }
}

/// The answer to a form: what it says about the file, and the file on its way.
pub(crate) struct Answer {
    pub probe: Probe,
    pub response: Response,
}

/// How long a probe's answer waits for its download (the quick-download window may be open
/// meanwhile). An older one is dropped and the download sends the form itself.
const KEEP_FOR: Duration = Duration::from_secs(10 * 60);
/// Answers kept at most (each holds a connection).
const KEEP_AT_MOST: usize = 8;

struct Kept {
    url: String,
    form: Form,
    at: Instant,
    answer: Answer,
}

fn kept() -> &'static Mutex<Vec<Kept>> {
    static KEPT: OnceLock<Mutex<Vec<Kept>>> = OnceLock::new();
    KEPT.get_or_init(Default::default)
}

/// Keeps the answer a probe got to `form` at `url`, for the download (`take`).
pub(crate) fn keep(url: &Url, form: &Form, answer: Answer) {
    let mut k = kept().lock();
    k.retain(|e| e.at.elapsed() < KEEP_FOR && !(e.url == url.as_str() && e.form == *form));
    if k.len() >= KEEP_AT_MOST {
        k.remove(0);
    }
    k.push(Kept {
        url: url.as_str().to_owned(),
        form: form.clone(),
        at: Instant::now(),
        answer,
    });
}

/// The answer a probe got to `form` at `url`, if it is still there (once).
pub(crate) fn take(url: &Url, form: &Form) -> Option<Answer> {
    let mut k = kept().lock();
    k.retain(|e| e.at.elapsed() < KEEP_FOR);
    let i = k
        .iter()
        .position(|e| e.url == url.as_str() && e.form == *form)?;
    Some(k.remove(i).answer)
}
