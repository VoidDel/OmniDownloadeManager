//! The host binary as a browser runs it: frames on stdin, replies on stdout.

use std::{
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use omnidl_ipc::{Listener, MAX_REPLY, Request, Response, read_frame, write_frame};
use serde_json::{Value, json};

fn unique_endpoint(tag: &str) -> String {
    let n = std::process::id();
    if cfg!(windows) {
        format!(r"\\.\pipe\omnidl-host-{tag}-{n}")
    } else {
        std::env::temp_dir()
            .join(format!("omnidl-host-{tag}-{n}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

fn host(endpoint: &str, app: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_omnidl-nm-host"))
        // Chrome passes the extension's origin; the host ignores it.
        .arg("chrome-extension://abc/")
        .env("OMNIDL_IPC_ENDPOINT", endpoint)
        .env("OMNIDL_APP_PATH", app)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Sends `msgs` and returns the replies, then closes stdin and checks the host exits cleanly.
fn exchange(mut child: Child, msgs: &[Value]) -> Vec<Value> {
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut replies = Vec::new();
    for m in msgs {
        write_frame(&mut stdin, &serde_json::to_vec(m).unwrap()).unwrap();
        let r = read_frame(&mut stdout, MAX_REPLY)
            .unwrap()
            .expect("a reply");
        replies.push(serde_json::from_slice(&r).unwrap());
    }
    stdin.flush().unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success());
    replies
}

fn missing_app() -> PathBuf {
    std::env::temp_dir().join("omnidl-no-such-app.exe")
}

#[tokio::test(flavor = "multi_thread")]
async fn relays_messages_to_the_app() {
    let ep = unique_endpoint("relay");
    let listener = Listener::bind(&ep).unwrap();
    tokio::spawn(listener.run(|frame: Vec<u8>| async move {
        let r: Request = serde_json::from_slice(&frame).unwrap();
        Response::ok(r.id.clone(), json!({ "echo": r })).to_bytes()
    }));
    let ep2 = ep.clone();
    let replies = tokio::task::spawn_blocking(move || {
        exchange(
            host(&ep2, &missing_app()),
            &[
                json!({ "id": "1", "type": "ping" }),
                json!({ "id": "2", "type": "add_download", "payload": { "url": "https://e.com/a.zip", "cookies": "s=1" } }),
            ],
        )
    })
    .await
    .unwrap();
    assert_eq!(replies[0]["id"], "1");
    assert_eq!(replies[0]["result"]["echo"]["type"], "ping");
    assert_eq!(replies[1]["result"]["echo"]["payload"]["cookies"], "s=1");
}

#[test]
fn reports_an_absent_app_without_starting_it_for_status() {
    let ep = unique_endpoint("absent");
    let replies = exchange(
        host(&ep, &missing_app()),
        &[
            json!({ "id": "s", "type": "get_status" }),
            json!({ "id": "d", "type": "add_download", "payload": { "url": "https://e.com/a" } }),
        ],
    );
    assert_eq!(replies[0]["ok"], false);
    assert_eq!(replies[0]["error"]["code"], "app_not_running");
    // A download tries to start the app; there is none here.
    assert_eq!(replies[1]["error"]["code"], "app_not_running");
    assert!(
        replies[1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("could not start")
    );
}

#[test]
fn starts_the_app_in_the_background_for_a_download() {
    let fake = PathBuf::from(env!("CARGO_BIN_EXE_omnidl-nm-host"))
        .with_file_name("examples")
        .join(if cfg!(windows) {
            "fake_app.exe"
        } else {
            "fake_app"
        });
    assert!(
        fake.is_file(),
        "build the example first: {}",
        fake.display()
    );
    let ep = unique_endpoint("launch");
    let mut child = host(&ep, &fake);
    // The fake app reads the endpoint from the environment it inherits from the host.
    let replies = {
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        write_frame(&mut stdin, br#"{"id":"o","type":"open_app"}"#).unwrap();
        let r = read_frame(&mut stdout, MAX_REPLY).unwrap().unwrap();
        drop(stdin);
        r
    };
    child.wait().unwrap();
    let v: Value = serde_json::from_slice(&replies).unwrap();
    assert_eq!(v["result"]["fake_app"], json!(["--background"]), "{v}");
}
