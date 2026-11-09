//! Where a task points (host, path, account, TLS) and opening a logged-in session there.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use omnidl_core::{Error, ErrorCode, JobOptions, Result, filename};
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, aws_lc_rs},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use rustls_platform_verifier::BuilderVerifierExt;
use suppaftp::{
    FtpError, Mode,
    tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream},
    types::FileType,
};
use url::Url;

/// One FTP control connection (TLS when the link says `ftps://`).
pub(crate) type Ftp = AsyncRustlsFtpStream;

/// How long an active-mode data connection is waited for.
const ACTIVE_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Security {
    Plain,
    /// `ftps://`: `AUTH TLS` on the control connection, then protected data connections.
    Explicit,
    /// `ftps://host:990`: TLS from the first byte.
    Implicit,
}

/// What a task's link names, with the account to log in with.
#[derive(Clone)]
pub(crate) struct Target {
    /// The link without credentials.
    pub url: String,
    pub host: String,
    pub port: u16,
    /// Decoded path on the server.
    pub path: String,
    pub security: Security,
    pub user: String,
    pub password: String,
}

impl Target {
    pub fn parse(source: &str, o: &JobOptions) -> Result<Self> {
        let invalid = |why: &str| Error::new(ErrorCode::InvalidUrl, format!("{source}: {why}"));
        let mut url = Url::parse(source.trim()).map_err(|e| invalid(&e.to_string()))?;
        let security = match url.scheme() {
            "ftp" => Security::Plain,
            "ftps" if url.port() == Some(990) => Security::Implicit,
            "ftps" => Security::Explicit,
            _ => return Err(invalid("not an ftp:// or ftps:// link")),
        };
        let host = url
            .host_str()
            .ok_or_else(|| invalid("no server"))?
            .to_owned();
        let port = url.port().unwrap_or(match security {
            Security::Implicit => 990,
            _ => 21,
        });
        let path = filename::percent_decode(url.path());
        if path.is_empty() || path == "/" || path.ends_with('/') {
            return Err(invalid("the link points at a folder, not a file"));
        }
        let user = match url.username() {
            "" => o.username.clone(),
            u => Some(filename::percent_decode(u)),
        };
        let password = url
            .password()
            .map(filename::percent_decode)
            .or_else(|| o.password.clone());
        let _ = url.set_username("");
        let _ = url.set_password(None);
        let (user, password) = match user {
            Some(u) => (u, password.unwrap_or_default()),
            None => ("anonymous".to_owned(), "anonymous@".to_owned()),
        };
        Ok(Self {
            url: url.to_string(),
            host,
            port,
            path,
            security,
            user,
            password,
        })
    }

    /// The name the file gets: the last segment of the path.
    pub fn file_name(&self) -> String {
        let last = self.path.rsplit('/').next().unwrap_or_default();
        filename::sanitize(last)
    }

    /// The host for the TLS certificate check (without IPv6 brackets).
    fn domain(&self) -> &str {
        self.host.trim_start_matches('[').trim_end_matches(']')
    }
}

/// Connects, secures, logs in and switches to binary transfers.
pub(crate) async fn open(t: &Target, o: &JobOptions) -> Result<Ftp> {
    let addr = resolve(t, o.connect_timeout).await?;
    let mut ftp = match t.security {
        Security::Implicit => {
            let connector = tls(o.insecure_tls)?;
            timed(
                o.connect_timeout,
                Ftp::connect_secure_implicit(addr, connector, t.domain()),
            )
            .await?
        }
        _ => Ftp::connect_timeout(addr, o.connect_timeout)
            .await
            .map_err(ftp_error)?,
    };
    if t.security == Security::Explicit {
        ftp = timed(
            o.read_timeout,
            ftp.into_secure(tls(o.insecure_tls)?, t.domain()),
        )
        .await?;
    }
    if o.ftp_active {
        ftp = ftp.active_mode(ACTIVE_WAIT);
    } else {
        ftp.set_mode(Mode::Passive);
        // A server behind NAT often announces its private address: use the one we reached.
        ftp.set_passive_nat_workaround(true);
    }
    timed(
        o.read_timeout,
        ftp.login(t.user.as_str(), t.password.as_str()),
    )
    .await?;
    timed(o.read_timeout, ftp.transfer_type(FileType::Binary)).await?;
    Ok(ftp)
}

async fn resolve(t: &Target, limit: Duration) -> Result<SocketAddr> {
    let host = format!("{}:{}", t.host, t.port);
    let found = tokio::time::timeout(limit, tokio::net::lookup_host(host.as_str()))
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, format!("resolving {}", t.host)))?
        .map_err(|e| Error::new(ErrorCode::Network, format!("resolving {}: {e}", t.host)))?;
    let all: Vec<SocketAddr> = found.collect();
    all.iter()
        .find(|a| a.is_ipv4())
        .or(all.first())
        .copied()
        .ok_or_else(|| Error::new(ErrorCode::Network, format!("{} has no address", t.host)))
}

/// Runs an FTP step with a time limit.
pub(crate) async fn timed<T>(
    limit: Duration,
    f: impl Future<Output = suppaftp::FtpResult<T>>,
) -> Result<T> {
    match tokio::time::timeout(limit, f).await {
        Ok(r) => r.map_err(ftp_error),
        Err(_) => Err(Error::new(
            ErrorCode::Timeout,
            "the FTP server did not answer in time",
        )),
    }
}

/// An FTP failure in the app's terms. Replies 4xx are temporary (retried); 530 means the
/// account was refused, 550 that the file is not there (or not readable).
pub(crate) fn ftp_error(e: FtpError) -> Error {
    match e {
        FtpError::ConnectionError(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Error::new(ErrorCode::Timeout, io.to_string())
        }
        FtpError::ConnectionError(io) => Error::new(ErrorCode::Network, io.to_string()),
        FtpError::SecureError(s) => Error::new(ErrorCode::Tls, s),
        FtpError::UnexpectedResponse(r) => {
            let code = r.status.code();
            let text = String::from_utf8_lossy(&r.body).trim().to_owned();
            let message = if text.starts_with(&code.to_string()) {
                text
            } else {
                format!("{code} {text}")
            };
            let kind = match code {
                400..=499 => ErrorCode::Network,
                530 | 532 => ErrorCode::AuthRequired,
                550 | 551 | 553 => ErrorCode::NotFound,
                _ => ErrorCode::FtpReply,
            };
            Error {
                code: kind,
                message,
                status: u16::try_from(code).ok(),
            }
        }
        FtpError::BadResponse => Error::new(ErrorCode::Network, "unreadable FTP reply"),
        FtpError::InvalidAddress(e) => Error::new(ErrorCode::Network, e.to_string()),
        FtpError::DataConnectionAlreadyOpen => Error::internal("data connection already open"),
    }
}

/// TLS for FTPS: the system's certificate store, or (asked for) no check at all.
fn tls(insecure: bool) -> Result<AsyncRustlsConnector> {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::new(ErrorCode::Tls, e.to_string()))?;
    let config = if insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
            .with_no_client_auth()
    } else {
        builder
            .with_platform_verifier()
            .map_err(|e| Error::new(ErrorCode::Tls, e.to_string()))?
            .with_no_client_auth()
    };
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)).into())
}

/// Accepts any certificate (signatures are still checked).
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_links() {
        let o = JobOptions::default();
        let t = Target::parse("ftp://h.example/pub/a%20b.iso", &o).unwrap();
        assert_eq!((t.host.as_str(), t.port), ("h.example", 21));
        assert_eq!(t.path, "/pub/a b.iso");
        assert_eq!(t.file_name(), "a b.iso");
        assert_eq!(
            (t.user.as_str(), t.security),
            ("anonymous", Security::Plain)
        );

        let t = Target::parse("ftps://bob:p%40ss@h:2121/x.bin", &o).unwrap();
        assert_eq!((t.user.as_str(), t.password.as_str()), ("bob", "p@ss"));
        assert_eq!(t.url, "ftps://h:2121/x.bin");
        assert_eq!(t.security, Security::Explicit);
        let t = Target::parse("ftps://h:990/x.bin", &o).unwrap();
        assert_eq!(t.security, Security::Implicit);

        let with_account = JobOptions {
            username: Some("amy".into()),
            password: Some("pw".into()),
            ..Default::default()
        };
        let t = Target::parse("ftp://h/x", &with_account).unwrap();
        assert_eq!((t.user.as_str(), t.password.as_str()), ("amy", "pw"));
        assert!(Target::parse("ftp://h/dir/", &o).is_err());
        assert!(Target::parse("http://h/x", &o).is_err());
    }
}
