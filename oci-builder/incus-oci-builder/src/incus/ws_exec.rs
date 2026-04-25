//! Live exec output streaming via the Incus WebSocket exec API.
//!
//! When `wait-for-websocket: true` is set in the exec request, Incus returns
//! an operation with WebSocket URLs for stdin (fd 0), stdout (fd 1), stderr
//! (fd 2), and a control channel. We connect to stdout and stderr and stream
//! their output to the terminal in real time, then wait for the control
//! channel to signal completion and return the exit code.
//!
//! Socket path: the WebSocket URL returned by Incus is relative to the Unix
//! socket, e.g. `/1.0/operations/<id>/websocket?secret=<s>`. We connect
//! using `hyperlocal`'s Unix socket connector wrapped in a tungstenite
//! handshake.

use std::path::Path;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tracing::{debug, instrument};

/// Connect to a WebSocket endpoint on the Incus Unix socket and stream all
/// messages to `writer` until the connection closes.
///
/// Returns when the server closes the connection.
#[instrument(skip(socket_path, writer), fields(path))]
pub async fn stream_ws_to_writer<W: AsyncWriteExt + Unpin>(
    socket_path: &Path,
    ws_path: &str,
    writer: &mut W,
) -> Result<()> {
    use tokio::net::UnixStream;
    use tokio_tungstenite::{client_async, tungstenite::client::IntoClientRequest};

    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connecting to Incus socket {}", socket_path.display()))?;

    // Build a fake HTTP/WS URL — tungstenite needs a URL for the handshake
    // Host header but the actual transport is the Unix stream.
    let url = format!("ws://localhost{ws_path}");
    let request = url
        .into_client_request()
        .context("building WebSocket request")?;

    let (mut ws, _) = client_async(request, stream)
        .await
        .context("WebSocket handshake")?;

    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                writer.write_all(&data).await.context("writing ws data")?;
            }
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                writer
                    .write_all(text.as_bytes())
                    .await
                    .context("writing ws text")?;
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
            Err(e) => {
                debug!("ws stream closed: {e}");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Read a JSON control message from the Incus control WebSocket and extract
/// the exit code. The control channel sends `{"command":"exit","return":<n>}`.
pub async fn read_exit_code(socket_path: &Path, ws_path: &str) -> Result<i32> {
    use tokio::net::UnixStream;
    use tokio_tungstenite::{client_async, tungstenite::client::IntoClientRequest};

    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connecting to control socket {}", socket_path.display()))?;

    let url = format!("ws://localhost{ws_path}");
    let request = url
        .into_client_request()
        .context("building control WebSocket request")?;

    let (mut ws, _) = client_async(request, stream)
        .await
        .context("control WebSocket handshake")?;

    while let Some(msg) = ws.next().await {
        match msg {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v.get("command").and_then(|c| c.as_str()) == Some("exit") {
                        return Ok(v
                            .get("return")
                            .and_then(|r| r.as_i64())
                            .map(|r| r as i32)
                            .unwrap_or(0));
                    }
                }
            }
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
            Err(e) => {
                debug!("control ws closed: {e}");
                break;
            }
            _ => {}
        }
    }
    Ok(0)
}
