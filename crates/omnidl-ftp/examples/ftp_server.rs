//! Serves a folder over FTP on loopback, for trying FTP downloads in the app:
//!
//! ```text
//! cargo run -p omnidl-ftp --example ftp_server -- <folder> [port] [user:password]
//! ```
//!
//! Without an account anyone logs in (anonymous).

use std::sync::Arc;

use async_trait::async_trait;
use libunftp::{ServerBuilder, auth::AnonymousAuthenticator};
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};
use unftp_sbe_fs::Filesystem;

#[derive(Debug)]
struct Account(String, String);

#[async_trait]
impl Authenticator for Account {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        if username == self.0 && creds.password.as_deref() == Some(self.1.as_str()) {
            Ok(Principal {
                username: username.into(),
            })
        } else {
            Err(AuthenticationError::BadPassword)
        }
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let root = std::path::PathBuf::from(args.next().expect("a folder to serve"));
    let port: u16 = args.next().map_or(2121, |p| p.parse().expect("a port"));
    let auth: Arc<dyn Authenticator + Send + Sync> = match args.next() {
        Some(a) => {
            let (u, p) = a.split_once(':').expect("user:password");
            Arc::new(Account(u.into(), p.into()))
        }
        None => Arc::new(AnonymousAuthenticator),
    };
    let server = ServerBuilder::with_authenticator(
        Box::new(move || Filesystem::new(root.clone()).expect("the folder exists")),
        auth,
    )
    .greeting("Omni Download test server")
    .build()
    .expect("building the server");
    println!("serving on ftp://127.0.0.1:{port}/");
    server
        .listen(format!("127.0.0.1:{port}"))
        .await
        .expect("listening");
}
