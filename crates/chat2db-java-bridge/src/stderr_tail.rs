use std::{collections::VecDeque, io, sync::Arc};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::Mutex,
};

use crate::StderrSnapshot;

#[derive(Clone, Debug)]
pub(crate) struct StderrTail {
    inner: Arc<Mutex<TailState>>,
    capacity: usize,
}

#[derive(Debug, Default)]
struct TailState {
    bytes: VecDeque<u8>,
    total_bytes: u64,
    truncated: bool,
}

impl StderrTail {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TailState::default())),
            capacity,
        }
    }

    pub(crate) async fn drain<R>(&self, mut reader: R) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        let mut buffer = [0_u8; 4096];
        loop {
            let bytes_read = reader.read(&mut buffer).await?;
            if bytes_read == 0 {
                return Ok(());
            }
            self.append(&buffer[..bytes_read]).await;
        }
    }

    pub(crate) async fn snapshot(&self) -> StderrSnapshot {
        let state = self.inner.lock().await;
        StderrSnapshot {
            bytes: state.bytes.iter().copied().collect(),
            total_bytes: state.total_bytes,
            truncated: state.truncated,
        }
    }

    async fn append(&self, bytes: &[u8]) {
        let mut state = self.inner.lock().await;
        state.total_bytes = state
            .total_bytes
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));

        if bytes.len() >= self.capacity {
            state.bytes.clear();
            state
                .bytes
                .extend(bytes[bytes.len().saturating_sub(self.capacity)..].iter());
            state.truncated = true;
            return;
        }

        let overflow = state
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(self.capacity);
        if overflow > 0 {
            state.bytes.drain(..overflow);
            state.truncated = true;
        }
        state.bytes.extend(bytes);
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::duplex;

    use super::StderrTail;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn retains_only_the_bounded_stderr_tail() {
        let tail = StderrTail::new(8);
        let (mut writer, reader) = duplex(32);
        let drain_tail = tail.clone();
        let drain = tokio::spawn(async move { drain_tail.drain(reader).await });

        writer
            .write_all(b"0123456789")
            .await
            .expect("diagnostic bytes must write");
        drop(writer);
        drain
            .await
            .expect("drain task must join")
            .expect("drain must succeed");

        let snapshot = tail.snapshot().await;
        assert_eq!(snapshot.bytes, b"23456789");
        assert_eq!(snapshot.total_bytes, 10);
        assert!(snapshot.truncated);
    }
}
