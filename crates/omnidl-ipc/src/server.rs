//! The app's side of the local pipe (Tokio): accepts connections and answers each frame with
//! the handler's reply, one connection per host.

use std::{future::Future, io};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::MAX_REQUEST;

/// Reads one frame; `None` at the end of the stream.
pub async fn read_frame_async<R: AsyncRead + Unpin>(
    r: &mut R,
    max: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {n} bytes (limit {max})"),
        ));
    }
    let mut buf = vec![0; n];
    match r.read_exact(&mut buf).await {
        Ok(_) => Ok(Some(buf)),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn write_frame_async<W: AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> io::Result<()> {
    let n = u32::try_from(data.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    w.write_all(&n.to_le_bytes()).await?;
    w.write_all(data).await?;
    w.flush().await
}

async fn serve_connection<S, F, Fut>(mut s: S, handle: F)
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(Vec<u8>) -> Fut,
    Fut: Future<Output = Vec<u8>>,
{
    loop {
        let frame = match read_frame_async(&mut s, MAX_REQUEST).await {
            Ok(Some(f)) => f,
            Ok(None) => return,
            // A broken or oversized frame ends the connection; the host reports the failure.
            Err(_) => return,
        };
        let reply = handle(frame).await;
        if write_frame_async(&mut s, &reply).await.is_err() {
            return;
        }
    }
}

/// The app's listener.
pub struct Listener {
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
}

impl Listener {
    /// Takes the endpoint. Fails with `AddrInUse` when another process serves it. Call it inside
    /// a Tokio runtime: the pipe or socket registers with its reactor.
    #[cfg(windows)]
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let next = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(endpoint)
            .map_err(|e| {
                // ERROR_ACCESS_DENIED: the first instance exists already.
                if e.raw_os_error() == Some(5) {
                    io::Error::new(io::ErrorKind::AddrInUse, e)
                } else {
                    e
                }
            })?;
        Ok(Self {
            name: endpoint.to_owned(),
            next,
        })
    }

    #[cfg(unix)]
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::path::Path::new(endpoint);
        if path.exists() {
            // A live app answers; a stale socket from a crash is removed.
            if std::os::unix::net::UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{endpoint} is in use"),
                ));
            }
            std::fs::remove_file(path)?;
        }
        if let Some(dir) = path.parent()
            && !dir.exists()
        {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let inner = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self { inner })
    }

    /// Serves connections until an accept fails; each frame is answered with `handle(frame)`.
    pub async fn run<F, Fut>(self, handle: F) -> io::Result<()>
    where
        F: Fn(Vec<u8>) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Vec<u8>> + Send + 'static,
    {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let Self { name, mut next } = self;
            loop {
                next.connect().await?;
                let conn = std::mem::replace(
                    &mut next,
                    ServerOptions::new()
                        .reject_remote_clients(true)
                        .create(&name)?,
                );
                tokio::spawn(serve_connection(conn, handle.clone()));
            }
        }
        #[cfg(unix)]
        {
            loop {
                let (conn, _) = self.inner.accept().await?;
                tokio::spawn(serve_connection(conn, handle.clone()));
            }
        }
    }
}
