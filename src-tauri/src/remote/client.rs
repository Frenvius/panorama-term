use super::wire::{self, Channel};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    secret: [u8; 32],
    host_key: [u8; 32],
    addresses: Vec<String>,
    pub device_id: String,
    #[serde(default = "default_host_name")]
    pub host_name: String,
}
fn default_host_name() -> String {
    "Remote host".into()
}
impl Identity {
    pub(super) fn update_addresses(&mut self, welcome: &Value, connected: &str) {
        let mut current = Vec::new();
        if let Some(addresses) = welcome["addresses"].as_array() {
            for address in addresses.iter().filter_map(Value::as_str) {
                if address.parse::<std::net::SocketAddr>().is_ok()
                    && !current.iter().any(|s| s == address)
                {
                    current.push(address.to_owned());
                }
            }
        }
        if !current.iter().any(|s| s == connected) {
            current.push(connected.to_owned());
        }
        self.addresses = current;
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = crate::store::base_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.save_to(&dir.join("remote-identity.json"))
    }
    pub fn load() -> Result<Self, String> {
        let path = crate::store::base_dir()?.join("remote-identity.json");
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    pub fn forget() -> Result<(), String> {
        let path = crate::store::base_dir()?.join("remote-identity.json");
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn save_to(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        file.write_all(&serde_json::to_vec(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
}

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub struct Session {
    identity: Identity,
    connection: Option<(Socket, Channel)>,
    next_id: u64,
}
fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn fixed<const N: usize>(s: &str) -> Result<[u8; N], String> {
    STANDARD
        .decode(s)
        .map_err(|_| "Invalid key or nonce")?
        .try_into()
        .map_err(|_| "Invalid key or nonce".into())
}
fn parse_uri(uri: &str) -> Result<(Vec<String>, [u8; 32], String), String> {
    let url = url::Url::parse(uri).map_err(|e| e.to_string())?;
    if url.scheme() != "panorama-mobile" || url.host_str() != Some("pair") {
        return Err("Invalid pairing URI".into());
    }
    let mut key = None;
    let mut code = None;
    let mut addresses = None;
    let mut version = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "v" => version = Some(v.into_owned()),
            "k" => key = Some(fixed(&v)?),
            "c" => code = Some(v.into_owned()),
            "h" => {
                addresses = Some(
                    v.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                )
            }
            _ => {}
        }
    }
    if version.as_deref() != Some("1") {
        return Err("Unsupported protocol version".into());
    }
    let addresses = addresses
        .filter(|a| !a.is_empty())
        .ok_or("No LAN address")?;
    for address in &addresses {
        address
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "Invalid LAN address")?;
    }
    Ok((
        addresses,
        key.ok_or("Missing host key")?,
        code.filter(|c| !c.is_empty())
            .ok_or("Missing pairing code")?,
    ))
}
async fn send(ws: &mut Socket, channel: &mut Channel, value: Value) -> Result<(), String> {
    let mut data = vec![0];
    data.extend_from_slice(value.to_string().as_bytes());
    ws.send(Message::Binary(channel.seal(&data).into()))
        .await
        .map_err(|e| e.to_string())
}
async fn receive(ws: &mut Socket, channel: &mut Channel) -> Result<Value, String> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Binary(data))) => {
                let data = channel.open(&data).ok_or("Invalid encrypted message")?;
                if data.first() != Some(&0) {
                    continue;
                }
                return serde_json::from_slice(&data[1..]).map_err(|e| e.to_string());
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Err(e)) => return Err(e.to_string()),
            Some(Ok(Message::Close(frame))) => return Err(closed(frame)),
            _ => return Err("Connection closed".into()),
        }
    }
}
#[derive(Clone, Debug)]
pub(super) enum Transport {
    Lan(String),
    Relay,
}

fn room(key: &[u8; 32]) -> String {
    let hash = Sha512::new()
        .chain_update(b"panorama-relay/v1/room")
        .chain_update(key)
        .finalize();
    hash[..16].iter().map(|b| format!("{b:02x}")).collect()
}
fn close_message(code: u16) -> Option<&'static str> {
    match code {
        4404 => Some("Host is offline (relay)"),
        4408 => Some("Host did not answer (relay)"),
        4429 => Some("Relay is full, try again later"),
        _ => None,
    }
}
fn closed(frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> String {
    frame
        .and_then(|f| close_message(f.code.into()))
        .unwrap_or("Connection closed")
        .into()
}
pub(super) async fn handshake(
    identity: &Identity,
    intro: Value,
) -> Result<(Socket, Channel, Value, Transport), String> {
    handshake_inner(identity, intro, true).await
}
async fn handshake_inner(
    identity: &Identity,
    intro: Value,
    relay: bool,
) -> Result<(Socket, Channel, Value, Transport), String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let secret = StaticSecret::from(identity.secret);
    let public = PublicKey::from(&secret).to_bytes();
    let mut targets: Vec<(String, Transport, u64)> = identity
        .addresses
        .iter()
        .enumerate()
        .map(|(i, a)| {
            (
                format!("ws://{a}"),
                Transport::Lan(a.clone()),
                250 * i as u64,
            )
        })
        .collect();
    if relay {
        let base = std::env::var("PANORAMA_RELAY_URL")
            .unwrap_or_else(|_| "wss://relay.usepanorama.dev".into());
        targets.push((
            format!(
                "{}/v1/connect/{}",
                base.trim_end_matches('/'),
                room(&identity.host_key)
            ),
            Transport::Relay,
            1500,
        ));
    }
    let mut attempts = futures_util::stream::FuturesUnordered::new();
    for (url, transport, delay) in targets {
        let intro = intro.clone();
        let secret = secret.clone();
        attempts.push(async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let client_nonce: [u8; 24] = random()?;
            let attempt = async {
                let (mut ws, _) = tokio_tungstenite::connect_async_with_config(url, None, true)
                    .await
                    .map_err(|e| e.to_string())?;
                ws.send(Message::Text(
                json!({"t":"hello","pk":STANDARD.encode(public),"n":STANDARD.encode(client_nonce)})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|e| e.to_string())?;
                let hello = match ws.next().await {
                    Some(Ok(Message::Text(s))) => {
                        serde_json::from_str::<Value>(&s).map_err(|e| e.to_string())?
                    }
                    Some(Ok(Message::Close(frame))) => return Err(closed(frame)),
                    _ => return Err("Invalid host hello".into()),
                };
                if hello["t"] != "hello"
                    || hello["v"] != 1
                    || fixed::<32>(hello["pk"].as_str().unwrap_or(""))? != identity.host_key
                {
                    return Err("Host key or protocol mismatch".into());
                }
                let server_nonce = fixed::<24>(hello["n"].as_str().unwrap_or(""))?;
                let key = wire::session_key(
                    &secret,
                    &identity.host_key,
                    &public,
                    &identity.host_key,
                    &client_nonce,
                    &server_nonce,
                )
                .ok_or("Invalid host key")?;
                let mut channel = Channel::new(&key, super::wire::DIR_CLIENT);
                send(&mut ws, &mut channel, intro.clone()).await?;
                let welcome = receive(&mut ws, &mut channel).await?;
                if welcome["t"] != "welcome" || welcome["v"] != 1 {
                    return Err("Authentication denied".into());
                }
                Ok((ws, channel, welcome, transport))
            };
            tokio::time::timeout(Duration::from_secs(15), attempt)
                .await
                .unwrap_or_else(|_| Err("Connection timed out".into()))
        });
    }
    let mut last = "Host unavailable".to_owned();
    let mut relay_error = None;
    while let Some(result) = attempts.next().await {
        match result {
            Ok(value) => return Ok(value),
            Err(e) => {
                if e.contains("(relay)") || e.starts_with("Relay is full") {
                    relay_error = Some(e.clone());
                }
                last = e;
            }
        }
    }
    Err(relay_error.unwrap_or(last))
}
#[cfg(test)]
async fn race_addresses<'a, T, F, Fut>(addresses: &'a [String], attempt: F) -> Result<T, String>
where
    F: Fn(&'a String) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut attempts = futures_util::stream::FuturesUnordered::new();
    for (index, address) in addresses.iter().enumerate() {
        let future = attempt(address);
        attempts.push(async move {
            tokio::time::sleep(Duration::from_millis(250 * index as u64)).await;
            future.await
        });
    }
    let mut last = "Host unavailable".to_owned();
    while let Some(result) = attempts.next().await {
        match result {
            Ok(result) => return Ok(result),
            Err(e) => last = e,
        }
    }
    Err(last)
}
pub async fn pair(uri: &str, name: &str) -> Result<Identity, String> {
    let (addresses, host_key, code) = parse_uri(uri)?;
    let host_name = url::Url::parse(uri)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "n")
                .map(|(_, v)| v.into_owned())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(default_host_name);
    let mut identity = Identity {
        secret: random()?,
        host_key,
        addresses,
        device_id: String::new(),
        host_name,
    };
    let (_, _, welcome, address) = handshake_inner(
        &identity,
        json!({"t":"pair","code":code,"name":name,"capability":"desktop"}),
        false,
    )
    .await?;
    identity.device_id = welcome["deviceId"]
        .as_str()
        .ok_or("Missing device ID")?
        .to_owned();
    if let Transport::Lan(address) = &address {
        identity.update_addresses(&welcome, address);
    }
    identity.save()?;
    Ok(identity)
}
pub async fn connect(identity: &Identity) -> Result<Session, String> {
    let mut session = Session {
        identity: identity.clone(),
        connection: None,
        next_id: 0,
    };
    session.reconnect().await?;
    Ok(session)
}
impl Session {
    async fn reconnect(&mut self) -> Result<(), String> {
        let mut last = String::new();
        for delay in [0, 100, 300] {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            match handshake(&self.identity, json!({"t":"auth"})).await {
                Ok((ws, channel, welcome, address)) => {
                    if let Transport::Lan(address) = &address {
                        self.identity.update_addresses(&welcome, address);
                    }
                    let _ = self.identity.save();
                    self.connection = Some((ws, channel));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
    pub async fn subscribe(&mut self) -> Result<(), String> {
        let (ws, channel) = self.connection.as_mut().ok_or("Connection closed")?;
        send(ws, channel, json!({"m":"host.subscribe", "id":0})).await?;
        let reply = receive(ws, channel).await?;
        if reply["t"] == "res" && reply["ok"] == true {
            Ok(())
        } else {
            Err("Subscription failed".into())
        }
    }
    pub async fn next_event(&mut self) -> Result<Value, String> {
        let (ws, channel) = self.connection.as_mut().ok_or("Connection closed")?;
        loop {
            let event = receive(ws, channel).await?;
            if event["t"] == "host.event" {
                return Ok(event);
            }
        }
    }
    pub async fn call_bytes(&mut self, command: &str, args: Value) -> Result<Vec<u8>, String> {
        let mut result = self.call(command, args).await?;
        let mut bytes = Vec::new();
        loop {
            let chunk = STANDARD
                .decode(result["bytes"].as_str().ok_or("Invalid byte chunk")?)
                .map_err(|e| e.to_string())?;
            bytes.extend(chunk);
            match result["next"].as_str() {
                Some(token) => {
                    result = self
                        .call("read_bytes_chunk", json!({"token":token}))
                        .await?
                }
                None => return Ok(bytes),
            }
        }
    }
    pub async fn call(&mut self, command: &str, args: Value) -> Result<Value, String> {
        if self.connection.is_none() {
            self.reconnect().await?;
        }
        self.next_id += 1;
        let id = self.next_id;
        let (ws, channel) = self.connection.as_mut().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(65), async {
            send(
                ws,
                channel,
                json!({"m":"host.call","p":{"command":command,"args":args},"id":id}),
            )
            .await?;
            loop {
                let reply = receive(ws, channel).await?;
                if reply["t"] == "res" && reply["id"] == id {
                    return if reply["ok"] == true {
                        Ok(reply["r"].clone())
                    } else {
                        Err(reply["err"]
                            .as_str()
                            .unwrap_or("Host call failed")
                            .to_owned())
                    };
                }
            }
        })
        .await
        .unwrap_or_else(|_| Err("Host call timed out".into()));
        if result.is_err() {
            self.connection = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_room_and_close_codes() {
        assert_eq!(room(&[2; 32]), "f68d2281730f799eb87f6b719e1b9ca2");
        assert_eq!(close_message(4404), Some("Host is offline (relay)"));
        assert_eq!(close_message(4408), Some("Host did not answer (relay)"));
        assert_eq!(close_message(4429), Some("Relay is full, try again later"));
        assert_eq!(close_message(1000), None);
    }
    #[test]
    fn uri_validation() {
        let key = url::form_urlencoded::byte_serialize(STANDARD.encode([3; 32]).as_bytes())
            .collect::<String>();
        let uri = format!("panorama-mobile://pair?v=1&k={key}&c=abc&h=127.0.0.1%3A1234");
        let (addresses, host, code) = parse_uri(&uri).unwrap();
        assert_eq!(addresses, ["127.0.0.1:1234"]);
        assert_eq!(host, [3; 32]);
        assert_eq!(code, "abc");
        assert!(parse_uri(&uri.replace("v=1", "v=2")).is_err());
        assert!(parse_uri(&uri.replace("127.0.0.1%3A1234", "bad")).is_err());
    }
    #[test]
    fn identity_is_private_and_roundtrips() {
        let path = std::env::temp_dir().join(format!("panorama-identity-{}", std::process::id()));
        let identity = Identity {
            secret: [1; 32],
            host_key: [2; 32],
            addresses: vec!["127.0.0.1:1".into()],
            device_id: "id".into(),
            host_name: "Host".into(),
        };
        identity.save_to(&path).unwrap();
        let restored: Identity = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.secret, identity.secret);
        assert_eq!(restored.host_key, identity.host_key);
        assert_eq!(restored.host_name, "Host");
        let mut old = serde_json::to_value(&identity).unwrap();
        old.as_object_mut().unwrap().remove("host_name");
        assert_eq!(
            serde_json::from_value::<Identity>(old).unwrap().host_name,
            "Remote host"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn addresses_race_and_cancel_slower_attempts() {
        struct Dropped(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let addresses = vec!["slow".to_string(), "fast".to_string()];
        let start = tokio::time::Instant::now();
        let result = race_addresses(&addresses, |address| {
            let cancelled = cancelled.clone();
            async move {
                if address == "slow" {
                    let _guard = Dropped(cancelled);
                    std::future::pending::<()>().await;
                }
                Ok(address.clone())
            }
        })
        .await
        .unwrap();
        assert_eq!(result, "fast");
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[test]
    fn welcome_updates_valid_addresses_and_keeps_fallbacks() {
        let mut identity = Identity {
            secret: [1; 32],
            host_key: [2; 32],
            addresses: vec!["127.0.0.1:1".into()],
            device_id: "id".into(),
            host_name: "Host".into(),
        };
        identity.update_addresses(
            &json!({"addresses":["192.168.1.2:3", "invalid", "192.168.1.2:3"]}),
            "127.0.0.1:1",
        );
        assert_eq!(identity.addresses, ["192.168.1.2:3", "127.0.0.1:1"]);
        identity.update_addresses(&json!({"addresses":[]}), "127.0.0.1:1");
        assert_eq!(identity.addresses, ["127.0.0.1:1"]);
    }
}
