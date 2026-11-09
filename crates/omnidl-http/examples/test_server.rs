//! The engine tests' HTTP server on a fixed port, for trying the app by hand:
//! `cargo run -p omnidl-http --example test_server -- 127.0.0.1:18080`, then add links such as
//! `http://127.0.0.1:18080/dl/524288000/ubuntu-24.04.3-desktop-amd64.iso` (about 1 MB/s per
//! connection) or `/get/{size}/{name}` (full speed). Content is generated from the offset.

#[path = "../tests/common/mod.rs"]
mod common;

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:18080".into());
    let server = common::start_at(&addr).await;
    println!("serving on http://{}", server.addr);
    tokio::signal::ctrl_c().await.ok();
}
