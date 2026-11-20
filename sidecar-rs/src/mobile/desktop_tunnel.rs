use super::{desktop_routes, tunnel};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub(crate) fn secret() -> &'static str {
    static SECRET: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SECRET.get_or_init(|| {
        super::random_bytes::<32>()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio_tungstenite::tungstenite::Message;

pub async fn open(req: &Value, mux: &mut tunnel::Mux, out: &tunnel::Out) {
    let Some(id) = req["id"].as_u64() else { return };
    let method = req["method"].as_str().unwrap_or("").to_owned();
    let target = req["target"].as_str().unwrap_or("").to_owned();
    let ws = req["ws"].as_bool().unwrap_or(false);
    if !desktop_routes::allowed(&method, &target, ws) || target.len() > 8192 {
        let _ = send_control(out, json!({"t":"stream","op":"error","id":id}));
        return;
    }
    let (pipe, worker) = tokio::io::duplex(64 * 1024);
    if !mux.insert(id, pipe) {
        let _ = send_control(out, json!({"t":"stream","op":"error","id":id}));
        return;
    }
    tokio::spawn(async move {
        serve(worker, &method, &target, ws).await;
    });
}
async fn serve(mut pipe: DuplexStream, method: &str, target: &str, ws: bool) {
    let mut head = Vec::new();
    let mut byte = [0];
    while head.len() < 16384 && !head.ends_with(b"\r\n\r\n") {
        if pipe.read_exact(&mut byte).await.is_err() {
            return;
        }
        head.push(byte[0]);
    }
    let Ok(head_text) = std::str::from_utf8(&head) else {
        return;
    };
    let Some(first) = head_text.lines().next() else {
        return;
    };
    let mut parts = first.split_whitespace();
    if parts.next() != Some(method)
        || parts.next() != Some(target)
        || parts.next() != Some("HTTP/1.1")
        || !head.ends_with(b"\r\n\r\n")
    {
        return;
    }
    if !ws {
        let Ok(mut local) =
            tokio::net::TcpStream::connect(("127.0.0.1", super::super::port())).await
        else {
            return;
        };
        let mut patched = head[..head.len() - 4].to_vec();
        patched.extend_from_slice(format!("x-panorama-remote: {}\r\n\r\n", secret()).as_bytes());
        if local.write_all(&patched).await.is_ok() {
            let _ = tokio::io::copy_bidirectional(&mut pipe, &mut local).await;
        }
        return;
    }
    // Complete the browser's handshake locally, then relay complete WebSocket messages.
    let key = head_text.lines().find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
            .map(|(_, v)| v.trim())
    });
    let Some(key) = key else { return };
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
    let response = format!(
        concat!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n",
            "Connection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n"
        ),
        accept
    );
    if pipe.write_all(response.as_bytes()).await.is_err() {
        return;
    }
    let Ok(mut request) =
        format!("ws://127.0.0.1:{}{}", super::super::port(), target).into_client_request()
    else {
        return;
    };
    request
        .headers_mut()
        .insert("x-panorama-remote", secret().parse().unwrap());
    let Ok((host, _)) = tokio_tungstenite::connect_async_with_config(request, None, true).await
    else {
        return;
    };
    let browser = tokio_tungstenite::WebSocketStream::from_raw_socket(
        pipe,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    let (mut bt, mut br) = browser.split();
    let (mut ht, mut hr) = host.split();
    loop {
        tokio::select! {
            msg = br.next() => match msg {
                Some(Ok(Message::Text(text)))
                    if target.split('?').next() == Some("/pty")
                        && !super::remote_pty_message_allowed(&text) => {},
                Some(Ok(msg)) => if ht.send(msg).await.is_err() { break; },
                _ => break,
            },
            msg = hr.next() => match msg { Some(Ok(msg)) => if bt.send(msg).await.is_err() { break; }, _ => break },
        }
    }
}

fn send_control(out: &tunnel::Out, value: Value) -> Result<(), ()> {
    let mut v = vec![0];
    v.extend_from_slice(value.to_string().as_bytes());
    out.try_send(v).map_err(|_| ())
}
