//! The blocking client against the async server over a real pipe / socket.

#![cfg(feature = "tokio")]

use omnidl_ipc::{Listener, MAX_REPLY, connect, read_frame, write_frame};

fn unique_endpoint(tag: &str) -> String {
    let n = std::process::id();
    if cfg!(windows) {
        format!(r"\\.\pipe\omnidl-test-{tag}-{n}")
    } else {
        std::env::temp_dir()
            .join(format!("omnidl-test-{tag}-{n}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_frames_on_many_connections() {
    let ep = unique_endpoint("echo");
    let listener = Listener::bind(&ep).unwrap();
    tokio::spawn(listener.run(|frame: Vec<u8>| async move {
        let mut out = b"re:".to_vec();
        out.extend(frame);
        out
    }));
    // A second listener on the same endpoint is refused.
    assert_eq!(
        Listener::bind(&ep).err().map(|e| e.kind()),
        Some(std::io::ErrorKind::AddrInUse)
    );
    let ep2 = ep.clone();
    let replies = tokio::task::spawn_blocking(move || {
        (0..3)
            .map(|i| {
                let mut c = connect(&ep2).unwrap();
                let mut got = Vec::new();
                for j in 0..2 {
                    write_frame(&mut c, format!("{i}.{j}").as_bytes()).unwrap();
                    got.push(
                        String::from_utf8(read_frame(&mut c, MAX_REPLY).unwrap().unwrap()).unwrap(),
                    );
                }
                got
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap();
    assert_eq!(replies[2], ["re:2.0", "re:2.1"]);
}

#[test]
fn connecting_to_nothing_fails_fast() {
    let t = std::time::Instant::now();
    assert!(connect(&unique_endpoint("absent")).is_err());
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
}
