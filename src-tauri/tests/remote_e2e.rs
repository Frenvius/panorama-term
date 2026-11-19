use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use panorama_lib::remote::{connect, pair, start_proxy, Identity};
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use x25519_dalek::{PublicKey, StaticSecret};
#[path = "../../sidecar-rs/src/mobile/wire.rs"]
mod wire;

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
async fn mobile_v1(uri: &str) {
    let u = url::Url::parse(uri).unwrap();
    let params: std::collections::HashMap<_, _> = u
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let secret = StaticSecret::from([91; 32]);
    let public = PublicKey::from(&secret).to_bytes();
    let cn = [31; 24];
    let host: [u8; 32] = STANDARD.decode(&params["k"]).unwrap().try_into().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}", params["h"]))
        .await
        .unwrap();
    ws.send(Message::Text(
        json!({"t":"hello","pk":STANDARD.encode(public),"n":STANDARD.encode(cn)})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let Message::Text(s) = ws.next().await.unwrap().unwrap() else {
        panic!()
    };
    let hello: Value = serde_json::from_str(&s).unwrap();
    let sn: [u8; 24] = STANDARD
        .decode(hello["n"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let mut chan = wire::Channel::new(
        &wire::session_key(&secret, &host, &public, &host, &cn, &sn).unwrap(),
        wire::DIR_CLIENT,
    );
    let mut intro = vec![0];
    intro.extend_from_slice(
        json!({"t":"pair","code":params["c"],"name":"Phone"})
            .to_string()
            .as_bytes(),
    );
    ws.send(Message::Binary(chan.seal(&intro).into()))
        .await
        .unwrap();
    let Message::Binary(b) = ws.next().await.unwrap().unwrap() else {
        panic!()
    };
    let welcome = chan.open(&b).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&welcome[1..]).unwrap()["t"],
        "welcome"
    );
    let mut req = vec![0];
    req.extend_from_slice(
        json!({"id":1,"m":"host.call","p":{"command":"store_read","args":{"name":"x"}}})
            .to_string()
            .as_bytes(),
    );
    ws.send(Message::Binary(chan.seal(&req).into()))
        .await
        .unwrap();
    let Message::Binary(b) = ws.next().await.unwrap().unwrap() else {
        panic!()
    };
    let res = chan.open(&b).unwrap();
    let res: Value = serde_json::from_slice(&res[1..]).unwrap();
    assert_eq!(res["err"], "Desktop capability required");
}

#[tokio::test]
async fn desktop_pair_bridge_revoke_and_mobile_v1() {
    let dir = std::env::temp_dir().join(format!("panorama-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let host_dir = dir.join("host");
    let client_dir = dir.join("client");
    std::fs::create_dir_all(&host_dir).unwrap();
    std::env::set_var("PANORAMA_CONFIG_DIR", &client_dir);
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
        .env("PANORAMA_CONFIG_DIR", &host_dir)
        .env("PANORAMA_SIDECAR_PORT", port.to_string())
        .env("PANORAMA_MOBILE_PORT", mobile_port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _sidecar = Sidecar { child, dir };
    let secret_path = host_dir.join("host-service-secret");
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
    let (provider, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (out, mut incoming) = tokio::sync::mpsc::channel::<Value>(32);
    let provider_events = out.clone();
    let provider_task = tokio::spawn(async move {
        let (mut sink, mut stream) = provider.split();
        loop {
            tokio::select! {
                Some(Ok(Message::Text(text))) = stream.next() => {
                    let req: Value = serde_json::from_str(&text).unwrap();
                    let out = out.clone();
                    tokio::spawn(async move {
                        if req["command"] == "git_fetch" { tokio::time::sleep(Duration::from_secs(2)).await; }
                        let _ = out.send(json!({"id":req["id"],"result":{"from":"provider"}})).await;
                    });
                },
                Some(reply) = incoming.recv() => if sink.send(Message::Text(reply.to_string().into())).await.is_err() { break; },
                else => break,
            }
        }
    });
    let pairing = http(port, "/mobile/pair?desktop=1").await;
    let uri = loopback(pairing["uri"].as_str().unwrap(), mobile_port);
    let mut wrong = url::Url::parse(&uri).unwrap();
    let args: Vec<(String, String)> = wrong
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    wrong
        .query_pairs_mut()
        .clear()
        .extend_pairs(args.iter().map(|(k, v)| {
            (
                k.as_str(),
                if k == "k" {
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
                } else {
                    v.as_str()
                },
            )
        }));
    assert!(pair(wrong.as_str(), "Wrong host").await.is_err());
    let identity = pair(&uri, "Desktop").await.unwrap();
    assert!(pair(&uri, "Reused").await.is_err());
    let loaded = Identity::load().unwrap();
    assert_eq!(loaded.device_id, identity.device_id);
    let mut session = connect(&loaded).await.unwrap();
    assert_eq!(
        session
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    let proxy = start_proxy(&identity).await.unwrap();
    let caller = proxy.caller();
    let slow = tokio::spawn(async move { caller.call("git_fetch", json!({})).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let now = std::time::Instant::now();
    assert_eq!(
        proxy
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    assert!(now.elapsed() < Duration::from_millis(200));
    assert!(slow.await.unwrap().is_ok());
    let mut events = proxy.subscribe();
    provider_events
        .send(json!({"event":"resync"}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if events.recv().await.unwrap()["event"] == "resync" {
                break;
            }
        }
    })
    .await
    .unwrap();
    http(port, "/mobile/enable?on=0").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(session
        .call("store_read", json!({"name":"workspace"}))
        .await
        .is_err());
    http(port, "/mobile/enable?on=1").await;
    assert_eq!(
        session
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if events.recv().await.unwrap()["event"] == "connected" {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        proxy
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    http(port, &format!("/mobile/revoke?id={}", identity.device_id)).await;
    assert!(session
        .call("store_read", json!({"name":"workspace"}))
        .await
        .is_err());
    assert!(connect(&identity).await.is_err());
    let mobile = http(port, "/mobile/pair").await;
    mobile_v1(&loopback(mobile["uri"].as_str().unwrap(), mobile_port)).await;
    provider_task.abort();
}
