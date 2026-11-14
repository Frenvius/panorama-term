use futures_util::{SinkExt, StreamExt};
use panorama_lib::{
    remote::{connect, pair},
    service_bridge::{self, Watches},
};
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

struct Sidecar {
    child: Child,
    dir: PathBuf,
}
impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn http(port: u16, path: &str) -> Value {
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
    let mut data = Vec::new();
    stream.read_to_end(&mut data).await.unwrap();
    let body = data.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    serde_json::from_slice(&data[body..]).unwrap()
}
fn loopback(uri: &str, port: u16) -> String {
    let mut url = url::Url::parse(uri).unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(
        pairs
            .iter()
            .map(|(k, v)| (k.as_str(), if k == "h" { "" } else { v.as_str() })),
    );
    url.query_pairs_mut()
        .append_pair("h", &format!("127.0.0.1:{port}"));
    url.into()
}

#[tokio::test]
async fn remote_data_and_events_through_sidecar() {
    let dir = std::env::temp_dir().join(format!("panorama-invoke-e2e-{}", std::process::id()));
    let host = dir.join("host");
    let client = dir.join("client");
    let repo = dir.join("repo");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::env::set_var("PANORAMA_CONFIG_DIR", &client);
    assert!(Command::new("git")
        .arg("init")
        .arg(&repo)
        .output()
        .unwrap()
        .status
        .success());
    let expected: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let file = repo.join("large.bin");
    std::fs::write(&file, &expected).unwrap();
    let port = free_port();
    let mobile_port = loop {
        let p = free_port();
        if p != port {
            break p;
        }
    };
    let rustc = Command::new("rustc").arg("-vV").output().unwrap();
    let info = String::from_utf8(rustc.stdout).unwrap();
    let triple = info.lines().find_map(|l| l.strip_prefix("host: ")).unwrap();
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("sidecar-{triple}{}", std::env::consts::EXE_SUFFIX));
    let child = Command::new(bin)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("PANORAMA_CONFIG_DIR", &host)
        .env("PANORAMA_SIDECAR_PORT", port.to_string())
        .env("PANORAMA_MOBILE_PORT", mobile_port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _sidecar = Sidecar { child, dir };
    let secret_path = host.join("host-service-secret");
    tokio::time::timeout(Duration::from_secs(20), async {
        while !secret_path.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let secret = std::fs::read_to_string(secret_path).unwrap();
    let mut request = format!("ws://127.0.0.1:{port}/host-service")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {secret}").parse().unwrap());
    let (mut provider, _) = tokio_tungstenite::connect_async_with_config(request, None, true)
        .await
        .unwrap();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Value>(1);
    let mut store_events = panorama_lib::store::subscribe();
    let mut provider_ui = panorama_lib::store::subscribe();
    let provider_task = tokio::spawn(async move {
        let mut watches = Watches::default();
        loop {
            tokio::select! {
                msg = provider.next() => {
                    let Some(Ok(Message::Text(text))) = msg else { break };
                    assert!(text.len() < 1024 * 1024);
                    let req: Value = serde_json::from_str(&text).unwrap();
                    let command = req["command"].as_str().unwrap();
                    let result = match watches.data(command, &req["args"]).await { Some(r) => r, None => service_bridge::dispatch(command, &req["args"]).await };
                    let response = match result { Ok(result) => json!({"id":req["id"],"result":result}), Err(error) => json!({"id":req["id"],"error":error}) }.to_string();
                    assert!(response.len() < 1024 * 1024);
                    provider.send(Message::Text(response.into())).await.unwrap();
                },
                event = store_events.recv() => if let Ok(payload) = event {
                    provider.send(Message::Text(json!({"event":"store:changed","payload":payload}).to_string().into())).await.unwrap();
                },
                event = event_rx.recv() => if let Some(event) = event { provider.send(Message::Text(event.to_string().into())).await.unwrap(); } else { break; }
            }
        }
    });
    let pairing = http(port, "/mobile/pair?desktop=1").await;
    let identity = pair(
        &loopback(pairing["uri"].as_str().unwrap(), mobile_port),
        "Desktop",
    )
    .await
    .unwrap();
    let mut session = connect(&identity).await.unwrap();
    let mut events = connect(&identity).await.unwrap();
    events.subscribe().await.unwrap();
    let status = session
        .call("git_status", json!({"path":repo, "limit":null}))
        .await
        .unwrap();
    assert!(status.to_string().contains("large.bin"), "{status}");
    let start = std::time::Instant::now();
    assert_eq!(
        session
            .call_bytes("read_file_bytes", json!({"path":file}))
            .await
            .unwrap(),
        expected
    );
    let elapsed = start.elapsed();
    eprintln!("read_file_bytes 3 MiB: {elapsed:?}");
    assert!(
        elapsed < Duration::from_millis(1500),
        "file transfer: {elapsed:?}"
    );
    assert_eq!(
        session
            .call_bytes(
                "git_blob",
                json!({"path":repo, "file":"large.bin", "old":false, "commit":null, "base":null})
            )
            .await
            .unwrap(),
        expected
    );
    event_tx
        .send(json!({"event":"git:repo","payload":{"root":"test"}}))
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), events.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event["event"], "git:repo");
    assert_eq!(event["payload"]["root"], "test");
    service_bridge::dispatch(
        "store_write",
        &json!({"name":"workspaces/local.json","value":{"tiles":[1]},"origin":"provider"}),
    )
    .await
    .unwrap();
    let provider_local = provider_ui.recv().await.unwrap();
    assert_eq!(provider_local["origin"], "provider");
    let local = tokio::time::timeout(Duration::from_secs(5), events.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(local["payload"]["name"], "workspaces/local.json");
    assert_eq!(local["payload"]["origin"], "provider");
    session
        .call(
            "store_write",
            json!({"name":"workspaces/remote.json","value":{"tiles":[2]},"origin":"client"}),
        )
        .await
        .unwrap();
    let provider_remote = tokio::time::timeout(Duration::from_secs(5), provider_ui.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provider_remote["origin"], "client");
    assert_eq!(provider_remote["name"], "workspaces/remote.json");
    let remote = tokio::time::timeout(Duration::from_secs(5), events.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(remote["payload"]["name"], "workspaces/remote.json");
    assert_eq!(remote["payload"]["origin"], "client");
    assert!(
        remote["payload"]["revision"].as_i64().unwrap()
            > local["payload"]["revision"].as_i64().unwrap()
    );
    assert_eq!(
        service_bridge::dispatch("store_read", &json!({"name":"workspaces/remote.json"}))
            .await
            .unwrap(),
        json!({"tiles":[2]})
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(300), events.next_event())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(300), provider_ui.recv())
            .await
            .is_err()
    );
    provider_task.abort();
}
