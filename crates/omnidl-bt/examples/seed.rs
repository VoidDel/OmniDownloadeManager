//! Seeds a file or folder on loopback, for trying torrents in the app without the internet:
//!
//! ```text
//! cargo run -p omnidl-bt --release --example seed -- <file or folder> [port] [KiB/s]
//! ```
//!
//! Writes `<name>.torrent` next to the data and prints a magnet link whose `x.pe` points here.
//! No DHT or trackers: the app finds this peer through that hint (or local discovery).

use std::{net::Ipv4Addr, path::PathBuf};

use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    create_torrent, limits::LimitsConfig, spawn_utils::BlockingSpawner,
};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("a file or folder to seed"));
    let path = std::fs::canonicalize(&path).expect("the path exists");
    let port: u16 = args.next().map_or(17881, |p| p.parse().expect("a port"));
    let rate = args
        .next()
        .map(|k| k.parse::<u32>().expect("KiB/s") * 1024)
        .and_then(std::num::NonZeroU32::new);

    let made = create_torrent(
        &path,
        CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: None,
        },
        &BlockingSpawner::new(4),
    )
    .await
    .expect("making the torrent");
    let bytes = made.as_bytes().expect("encoding the torrent");
    let parent = path.parent().expect("a parent folder").to_path_buf();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let torrent_file = parent.join(format!("{name}.torrent"));
    std::fs::write(&torrent_file, &bytes).expect("writing the .torrent");

    let session = Session::new_with_opts(
        parent.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            listen: Some(ListenerOptions {
                listen_addr: (Ipv4Addr::LOCALHOST, port).into(),
                ..Default::default()
            }),
            ratelimits: LimitsConfig {
                upload_bps: rate,
                download_bps: None,
            },
            ..Default::default()
        },
    )
    .await
    .expect("starting the session");
    let output = if path.is_dir() { path.clone() } else { parent };
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(bytes),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(output.to_string_lossy().into_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("adding the torrent")
        .into_handle()
        .expect("a handle");
    handle
        .wait_until_initialized()
        .await
        .expect("checking the data");
    let hash = made.info_hash().as_string();
    println!("torrent: {}", torrent_file.display());
    println!("magnet:?xt=urn:btih:{hash}&dn={name}&x.pe=127.0.0.1:{port}");
    println!("seeding on 127.0.0.1:{port}; Ctrl+C to stop");
    tokio::signal::ctrl_c().await.ok();
    session.stop().await;
}
