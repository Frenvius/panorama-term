#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use panorama_lib::remote::{pair, start_proxy};
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

struct Host {
    child: Child,
    dir: PathBuf,
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", self.child.id())])
            .status();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn get(port: u16, path: &str) -> Value {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes[bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..])
        .unwrap()
}
async fn status(endpoint: &str, path: &str, origin: &str) -> String {
    let (address, token) = endpoint.split_once('/').unwrap();
    let mut s = tokio::net::TcpStream::connect(address).await.unwrap();
    s.write_all(format!("GET /{token}{path} HTTP/1.1\r\nHost: {address}\r\nOrigin: {origin}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut data = Vec::new();
    s.read_to_end(&mut data).await.unwrap();
    String::from_utf8_lossy(&data).into_owned()
}
async fn wait_text<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>, needle: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let m = ws.next().await.unwrap().unwrap();
            if m.into_data()
                .windows(needle.len())
                .any(|w| w == needle.as_bytes())
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("terminal output timed out: {needle}"));
}
async fn wait_output<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let data = ws.next().await.unwrap().unwrap().into_data();
            if data
                .windows(18)
                .filter(|w| *w == b"panorama-remote-ok")
                .count()
                >= 2
                && data.windows(5).any(|w| w == b"31 91")
            {
                break;
            }
        }
    })
    .await
    .expect("terminal output or dimensions missing");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminal_proxy_routes_flow_and_resize() {
    let dir = std::env::temp_dir().join(format!("panorama-proxy-e2e-{}", std::process::id()));
    let host_dir = dir.join("host");
    let client_dir = dir.join("client");
    std::fs::create_dir_all(&host_dir).unwrap();
    std::env::set_var("PANORAMA_CONFIG_DIR", &client_dir);
    let sidecar_port = port();
    let host_port = port();
    let mobile_port = port();
    let output = Command::new("rustc").arg("-vV").output().unwrap();
    let info = String::from_utf8(output.stdout).unwrap();
    let triple = info.lines().find_map(|l| l.strip_prefix("host: ")).unwrap();
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("sidecar-{triple}"));
    let child = Command::new("setsid")
        .arg(&bin)
        .arg("host")
        .env("HOME", &dir)
        .env("PANORAMA_CONFIG_DIR", &host_dir)
        .env("PANORAMA_HOST_PORT", host_port.to_string())
        .env("PANORAMA_SIDECAR_PORT", sidecar_port.to_string())
        .env("PANORAMA_MOBILE_PORT", mobile_port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _host = Host {
        child,
        dir: dir.clone(),
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        while tokio::net::TcpStream::connect(("127.0.0.1", sidecar_port))
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let pairing = get(sidecar_port, "/mobile/pair?desktop=1").await;
    let mut uri = url::Url::parse(pairing["uri"].as_str().unwrap()).unwrap();
    let args: Vec<(String, String)> = uri
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .filter(|(k, _)| k != "h")
        .collect();
    uri.query_pairs_mut()
        .clear()
        .extend_pairs(args)
        .append_pair("h", &format!("127.0.0.1:{mobile_port}"));
    let identity = pair(uri.as_str(), "Desktop").await.unwrap();
    let proxy = start_proxy(&identity).await.unwrap();
    assert!(
        status(&proxy.endpoint, "/mobile/state", "tauri://localhost")
            .await
            .starts_with("HTTP/1.1 403")
    );
    let (address, _) = proxy.endpoint.split_once('/').unwrap();
    let wrong = format!("{address}/wrong");
    assert!(status(&wrong, "/run/status", "tauri://localhost")
        .await
        .starts_with("HTTP/1.1 403"));
    assert!(
        status(&proxy.endpoint, "/run/status", "https://evil.example")
            .await
            .starts_with("HTTP/1.1 403")
    );
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        status(
            &proxy.endpoint,
            "/run/status?kind=run&tile=x",
            "tauri://localhost",
        ),
    )
    .await
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("Access-Control-Allow-Origin: tauri://localhost"));
    assert!(!response.contains("Access-Control-Allow-Origin: *"));
    let (mut local, _) = tokio_tungstenite::connect_async(format!(
        "ws://127.0.0.1:{sidecar_port}/pty?tileId=proxy-test&cols=91&rows=31"
    ))
    .await
    .unwrap();
    wait_text(&mut local, "\"ready\"").await;
    let (mut remote, _) = tokio_tungstenite::connect_async_with_config(
        format!(
            "ws://{}/pty?tileId=proxy-test&cols=22&rows=12",
            proxy.endpoint
        ),
        None,
        true,
    )
    .await
    .unwrap();
    wait_text(&mut remote, "\"cols\":91").await;
    remote
        .send(Message::Text(
            json!({"t":"resize","cols":20,"rows":10}).to_string().into(),
        ))
        .await
        .unwrap();
    remote
        .send(Message::Text(
            json!({"t":"in","d":"echo panorama-remote-ok; stty size\r"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    wait_output(&mut remote).await;
    wait_text(&mut local, "panorama-remote-ok").await;
    let command = format!(
        r#"python3 -c 'import sys,tty,termios; old=termios.tcgetattr(0); tty.setraw(0); print("LATENCY_"+"READY",flush=True); c=sys.stdin.read(1); print("LATENCY_"+"ECHO:"+c,flush=True); termios.tcsetattr(0,termios.TCSANOW,old)'{}"#,
        "\r"
    );
    remote
        .send(Message::Text(
            json!({"t":"in","d":command}).to_string().into(),
        ))
        .await
        .unwrap();
    wait_text(&mut remote, "LATENCY_READY").await;
    let start = std::time::Instant::now();
    remote
        .send(Message::Text(json!({"t":"in","d":"Z"}).to_string().into()))
        .await
        .unwrap();
    wait_text(&mut remote, "LATENCY_ECHO:Z").await;
    let latency = start.elapsed();
    eprintln!("terminal key echo: {latency:?}");
    assert!(
        latency < Duration::from_millis(30),
        "terminal key echo: {latency:?}"
    );
    let payload: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i * 31 % 256) as u8).collect();
    let path = dir.join("payload.bin");
    remote
        .send(Message::Text(
            json!({"t":"in","d":"stty -echo\r"}).to_string().into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let command = format!(
        r#"python3 -c 'import sys,tty,termios; old=termios.tcgetattr(0); tty.setraw(0); print("PAYLOAD_READY",flush=True); open("{}","wb").write(sys.stdin.buffer.read(3145728)); termios.tcsetattr(0,termios.TCSANOW,old); print("PAYLOAD_DONE",flush=True)'{}"#,
        path.display(),
        "\r"
    );
    remote
        .send(Message::Text(
            json!({"t":"in","d":command}).to_string().into(),
        ))
        .await
        .unwrap();
    wait_text(&mut remote, "PAYLOAD_READY").await;
    let start = std::time::Instant::now();
    remote
        .send(Message::Binary(payload.clone().into()))
        .await
        .unwrap();
    wait_text(&mut remote, "PAYLOAD_DONE").await;
    let elapsed = start.elapsed();
    eprintln!("terminal 3 MiB: {elapsed:?}");
    assert!(
        elapsed < Duration::from_millis(1500),
        "terminal transfer: {elapsed:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), payload);
    let mut events = proxy.subscribe();
    get(sidecar_port, "/mobile/enable?on=0").await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if events.recv().await.unwrap()["event"] == "reconnecting" {
                break;
            }
        }
    })
    .await
    .unwrap();
    get(sidecar_port, "/mobile/enable?on=1").await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if events.recv().await.unwrap()["event"] == "connected" {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (mut restored, _) = tokio_tungstenite::connect_async_with_config(
        format!(
            "ws://{}/pty?tileId=proxy-test&cols=22&rows=12",
            proxy.endpoint
        ),
        None,
        true,
    )
    .await
    .unwrap();
    wait_text(&mut restored, "\"cols\":91").await;
    restored
        .send(Message::Text(
            json!({"t":"in","d":"stty echo; echo RECONNECTED_TERMINAL\r"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    wait_text(&mut restored, "RECONNECTED_TERMINAL").await;
    restored
        .send(Message::Text(
            json!({"t":"visible","visible":false}).to_string().into(),
        ))
        .await
        .unwrap();
    restored
        .send(Message::Text(
            json!({"t":"focus","focused":false}).to_string().into(),
        ))
        .await
        .unwrap();
    local
        .send(Message::Text(
            json!({"t":"in","d":"printf VISIBLE_LOCAL_ONLY\\n\r"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    wait_text(&mut local, "VISIBLE_LOCAL_ONLY").await;
    restored
        .send(Message::Text(
            json!({"t":"visible","visible":true}).to_string().into(),
        ))
        .await
        .unwrap();
    restored
        .send(Message::Text(
            json!({"t":"focus","focused":true}).to_string().into(),
        ))
        .await
        .unwrap();
    local.send(Message::Text(json!({"t":"in","d":"printf '\\033]2;shared-title-r4\\007\\033]52;c;c2hhcmVkLWNsaXAtcjQ=\\007'\r"}).to_string().into())).await.unwrap();
    wait_text(&mut local, "\"t\":\"clipboard\"").await;
    wait_text(&mut restored, "\"t\":\"clipboard\"").await;
    wait_text(&mut local, "\"t\":\"title\"").await;
    wait_text(&mut restored, "\"t\":\"title\"").await;
    local
        .send(Message::Text(
            json!({"t":"in","d":"cd /tmp; printf '\\033]7;file://localhost/tmp\\007'\r"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    wait_text(&mut local, "\"cwd\":\"/tmp\"").await;
    wait_text(&mut restored, "\"cwd\":\"/tmp\"").await;
}
