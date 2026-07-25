use std::{io, path::Path};

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(unix)]
use crate::SOCKET_FILE;
use crate::{Endpoint, LocalError};

pub(crate) trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub(crate) type BoxedIo = Box<dyn AsyncIo>;

pub(crate) async fn read_message<T: DeserializeOwned>(
    io: &mut (impl AsyncRead + Unpin),
    max_bytes: usize,
) -> Result<T, LocalError> {
    let mut header = [0_u8; 4];
    io.read_exact(&mut header)
        .await
        .map_err(|error| LocalError::io("read frame header", error))?;
    let length = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_| LocalError::Protocol("frame length is not addressable".to_owned()))?;
    if length == 0 || length > max_bytes {
        return Err(LocalError::Protocol(format!(
            "frame length must be between 1 and {max_bytes} bytes"
        )));
    }
    let mut payload = vec![0_u8; length];
    io.read_exact(&mut payload)
        .await
        .map_err(|error| LocalError::io("read frame payload", error))?;
    serde_json::from_slice(&payload).map_err(LocalError::from)
}

pub(crate) async fn write_message<T: Serialize>(
    io: &mut (impl AsyncWrite + Unpin),
    value: &T,
    max_bytes: usize,
) -> Result<(), LocalError> {
    let payload = encode_message(value, max_bytes)?;
    let length = u32::try_from(payload.len())
        .map_err(|_| LocalError::Protocol("encoded frame is too large".to_owned()))?;
    io.write_all(&length.to_be_bytes())
        .await
        .map_err(|error| LocalError::io("write frame header", error))?;
    io.write_all(&payload)
        .await
        .map_err(|error| LocalError::io("write frame payload", error))?;
    io.shutdown()
        .await
        .map_err(|error| LocalError::io("finish frame", error))
}

pub(crate) fn encode_message<T: Serialize>(
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, LocalError> {
    let mut buffer = BoundedBuffer::new(max_bytes);
    let encoded = serde_json::to_writer(&mut buffer, value);
    if buffer.exceeded {
        return Err(LocalError::Protocol(format!(
            "encoded frame length must be between 1 and {max_bytes} bytes"
        )));
    }
    encoded?;
    if buffer.bytes.is_empty() {
        return Err(LocalError::Protocol(format!(
            "encoded frame length must be between 1 and {max_bytes} bytes"
        )));
    }
    Ok(buffer.bytes)
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
    exceeded: bool,
}

impl BoundedBuffer {
    const fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
            exceeded: false,
        }
    }
}

impl io::Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.max_bytes.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "encoded frame exceeds its byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) struct Listener {
    inner: tokio::net::UnixListener,
    path: std::path::PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl Listener {
    pub(crate) fn bind(data_dir: &Path) -> Result<(Self, Endpoint), LocalError> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

        let path = data_dir.join(SOCKET_FILE);
        let expected_uid = rustix::process::geteuid().as_raw();
        let directory = std::fs::symlink_metadata(data_dir)
            .map_err(|error| LocalError::io("inspect attachment directory", error))?;
        if !directory.file_type().is_dir()
            || directory.uid() != expected_uid
            || directory.permissions().mode() & 0o077 != 0
        {
            return Err(LocalError::Unavailable(
                "attachment directory is not owned exclusively by the current user".to_owned(),
            ));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == expected_uid
                    && metadata.permissions().mode().trailing_zeros() >= 6 =>
            {
                std::fs::remove_file(&path)
                    .map_err(|error| LocalError::io("remove stale Unix socket", error))?;
            }
            Ok(_) => {
                return Err(LocalError::Unavailable(format!(
                    "refusing to replace non-socket endpoint at {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(LocalError::io("inspect Unix socket", error)),
        }
        let inner = tokio::net::UnixListener::bind(&path)
            .map_err(|error| LocalError::io("bind Unix socket", error))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| LocalError::io("secure Unix socket", error))?;
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| LocalError::io("inspect bound Unix socket", error))?;
        Ok((
            Self {
                inner,
                path: path.clone(),
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            Endpoint::UnixSocket { path },
        ))
    }

    pub(crate) async fn accept(&mut self) -> Result<BoxedIo, LocalError> {
        let expected_uid = rustix::process::geteuid().as_raw();
        loop {
            let (stream, _) = self
                .inner
                .accept()
                .await
                .map_err(|error| LocalError::io("accept Unix socket", error))?;
            let peer = stream
                .peer_cred()
                .map_err(|error| LocalError::io("inspect Unix peer credentials", error))?;
            if peer.uid() == expected_uid {
                return Ok(Box::new(stream));
            }
            tracing::warn!(
                peer_uid = peer.uid(),
                "rejected local attachment peer owned by another user"
            );
        }
    }

    pub(crate) fn cleanup(&self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode =>
            {
                if let Err(error) = std::fs::remove_file(&self.path) {
                    tracing::warn!(%error, path = %self.path.display(), "failed to remove local Unix socket");
                }
            }
            Ok(_) => {
                tracing::warn!(path = %self.path.display(), "refusing to remove replaced local Unix socket");
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = %self.path.display(), "failed to inspect local Unix socket during cleanup");
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(windows)]
pub(crate) struct Listener {
    name: String,
    pending: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

#[cfg(windows)]
impl Listener {
    pub(crate) fn bind(data_dir: &Path) -> Result<(Self, Endpoint), LocalError> {
        use std::ffi::OsStr;

        use chat2db_local_ipc_windows::{PipeInstanceKind, create_owner_only_named_pipe};
        use sha2::{Digest, Sha256};

        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = Sha256::digest(data_dir.as_os_str().to_string_lossy().as_bytes());
        let mut suffix = String::with_capacity(24);
        for byte in &digest[..12] {
            suffix.push(char::from(HEX[usize::from(byte >> 4)]));
            suffix.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        let name = format!(r"\\.\pipe\chat2db-rust-{suffix}");
        let pending = create_owner_only_named_pipe(
            OsStr::new(&name),
            PipeInstanceKind::First,
            WINDOWS_PIPE_MAX_INSTANCES,
        )
        .map_err(|error| LocalError::io("create Windows named pipe", error))?;
        Ok((
            Self {
                name: name.clone(),
                pending: Some(pending),
            },
            Endpoint::WindowsNamedPipe { name },
        ))
    }

    pub(crate) async fn accept(&mut self) -> Result<BoxedIo, LocalError> {
        use std::ffi::OsStr;

        use chat2db_local_ipc_windows::{PipeInstanceKind, create_owner_only_named_pipe};

        let server = self
            .pending
            .take()
            .ok_or_else(|| LocalError::Protocol("named pipe listener is not ready".to_owned()))?;
        server
            .connect()
            .await
            .map_err(|error| LocalError::io("accept Windows named pipe", error))?;
        self.pending = Some(
            create_owner_only_named_pipe(
                OsStr::new(&self.name),
                PipeInstanceKind::Additional,
                WINDOWS_PIPE_MAX_INSTANCES,
            )
            .map_err(|error| LocalError::io("create Windows named pipe instance", error))?,
        );
        Ok(Box::new(server))
    }

    #[allow(
        clippy::unused_self,
        reason = "Windows named pipes disappear with their owned handles"
    )]
    pub(crate) fn cleanup(&self) {}
}

#[cfg(windows)]
const WINDOWS_PIPE_MAX_INSTANCES: u8 = 254;

#[cfg(unix)]
pub(crate) async fn connect(
    endpoint: &Endpoint,
    _expected_process_id: u32,
) -> Result<BoxedIo, LocalError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    let Endpoint::UnixSocket { path } = endpoint else {
        return Err(LocalError::Protocol(
            "endpoint metadata does not describe a Unix socket".to_owned(),
        ));
    };
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| LocalError::io("inspect Unix socket", error))?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(LocalError::Unavailable(
            "local Unix socket is not owner-only".to_owned(),
        ));
    }
    tokio::net::UnixStream::connect(path)
        .await
        .map(|stream| Box::new(stream) as BoxedIo)
        .map_err(|error| LocalError::io("connect Unix socket", error))
}

#[cfg(windows)]
pub(crate) async fn connect(
    endpoint: &Endpoint,
    expected_process_id: u32,
) -> Result<BoxedIo, LocalError> {
    use std::time::Duration;

    use chat2db_local_ipc_windows::{is_named_pipe_busy, verify_named_pipe_server};
    use tokio::net::windows::named_pipe::ClientOptions;

    let Endpoint::WindowsNamedPipe { name } = endpoint else {
        return Err(LocalError::Protocol(
            "endpoint metadata does not describe a Windows named pipe".to_owned(),
        ));
    };
    loop {
        match ClientOptions::new().open(name) {
            Ok(stream) => {
                verify_named_pipe_server(&stream, expected_process_id)
                    .map_err(|error| LocalError::io("authenticate Windows named pipe", error))?;
                return Ok(Box::new(stream) as BoxedIo);
            }
            Err(error) if is_named_pipe_busy(&error) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(LocalError::io("connect Windows named pipe", error)),
        }
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("chat2db-local requires Unix-domain sockets or Windows named pipes");
