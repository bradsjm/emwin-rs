//! Small TCP connection helpers used by the client runtime.

use crate::qbt_receiver::error::{QbtReceiverError, QbtReceiverResult};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

/// Opens a TCP connection with an explicit timeout boundary.
///
/// # Errors
///
/// Returns the underlying connection error or a timed-out `io::Error`.
pub async fn connect_with_timeout(
    host: &str,
    port: u16,
    timeout: Duration,
) -> io::Result<TcpStream> {
    let addr = format!("{host}:{port}");
    match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
        Ok(res) => res,
        Err(_elapsed) => Err(io::Error::new(io::ErrorKind::TimedOut, "connect timeout")),
    }
}

/// Formats a stable `host:port` label for logs and metrics.
pub fn endpoint_label(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Writes all bytes with an explicit timeout boundary.
pub async fn write_all_with_timeout<W>(
    writer: &mut W,
    bytes: &[u8],
    timeout: Duration,
) -> QbtReceiverResult<()>
where
    W: AsyncWrite + Unpin,
{
    tokio::time::timeout(timeout, writer.write_all(bytes))
        .await
        .map_err(|_| QbtReceiverError::WriteTimeout)?
        .map_err(QbtReceiverError::Io)
}

#[cfg(test)]
mod tests {
    use super::write_all_with_timeout;
    use crate::qbt_receiver::error::QbtReceiverError;
    use std::time::Duration;

    #[tokio::test]
    async fn write_all_with_timeout_returns_timeout_error() {
        let (mut writer, _reader) = tokio::io::duplex(1);
        let payload = vec![b'x'; 1024];

        let error = write_all_with_timeout(&mut writer, &payload, Duration::from_millis(10))
            .await
            .expect_err("write should time out");

        assert!(matches!(error, QbtReceiverError::WriteTimeout));
    }
}
