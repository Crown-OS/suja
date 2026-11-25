//! A one-request HTTP server for backend tests.

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub struct ReceivedRequest {
    pub head: String,
    pub body: Value,
}

/// Serves one request with `status` and the event-stream `body`. Gives the base URL
/// `http://127.0.0.1:<port>` and, later, the request it got.
pub async fn serve_once(
    status: &'static str,
    body: String,
) -> std::io::Result<(String, oneshot::Receiver<ReceivedRequest>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (seen, request) = oneshot::channel();
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        let Some(received) = read_request(&mut socket).await else {
            return;
        };
        let _ = seen.send(received);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
    });
    Ok((format!("http://{address}"), request))
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<ReceivedRequest> {
    let mut received = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = socket.read(&mut buffer).await.ok()?;
        if count == 0 {
            return None;
        }
        received.extend_from_slice(&buffer[..count]);
        let text = String::from_utf8_lossy(&received);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        if body.len() >= content_length(head) {
            return Some(ReceivedRequest {
                head: head.to_owned(),
                body: serde_json::from_str(body).unwrap_or(Value::Null),
            });
        }
    }
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse().ok())
        })
        .unwrap_or(0)
}
