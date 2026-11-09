//! A stand-in for the app in the host's tests: listens on `$OMNIDL_IPC_ENDPOINT` and answers
//! every request with its own arguments, then exits after a while.

use omnidl_ipc::{Listener, Request, Response};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let listener = Listener::bind(&omnidl_ipc::endpoint()).expect("bind");
    tokio::spawn(listener.run(move |frame: Vec<u8>| {
        let args = args.clone();
        async move {
            let id = serde_json::from_slice::<Request>(&frame)
                .map(|r| r.id)
                .unwrap_or_default();
            Response::ok(id, serde_json::json!({ "fake_app": args })).to_bytes()
        }
    }));
    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
}
