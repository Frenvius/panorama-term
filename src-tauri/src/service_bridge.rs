use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::Listener;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub const HOST_EVENTS: &[&str] = &[
    "git:repo",
    "diff:changed",
    "note:changed",
    "store:changed",
    "file:changed",
    "run:manifests",
];

fn secret_path() -> Option<std::path::PathBuf> {
    let name = if cfg!(debug_assertions) {
        ".panorama-dev"
    } else {
        ".panorama"
    };
    Some(
        crate::config_dir::config_dir(
            std::env::var_os("PANORAMA_CONFIG_DIR").map(Into::into),
            dirs::home_dir(),
            name,
        )?
        .join("host-service-secret"),
    )
}
fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("Invalid {key}: {e}"))
}
fn value<T: serde::Serialize>(result: T) -> Result<Value, String> {
    serde_json::to_value(result).map_err(|e| e.to_string())
}

fn bytes(response: tauri::ipc::Response) -> Result<Vec<u8>, String> {
    use tauri::ipc::{InvokeResponseBody, IpcResponse};
    match response.body().map_err(|e| e.to_string())? {
        InvokeResponseBody::Raw(bytes) => Ok(bytes),
        _ => Err("Expected bytes".into()),
    }
}

pub async fn dispatch(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "store_read" => value(crate::store::store_read(arg(args, "name")?)?),
        "store_write" => value(crate::store::store_write(
            arg(args, "name")?,
            arg(args, "value")?,
            arg(args, "origin")?,
        )?),
        "store_write_many" => value(crate::store::store_write_many(
            arg(args, "entries")?,
            arg(args, "origin")?,
        )?),
        "store_delete" => value(crate::store::store_delete(
            arg(args, "name")?,
            arg(args, "origin")?,
        )?),
        "store_list" => value(crate::store::store_list(arg(args, "dir")?)?),
        "read_dir" => value(crate::read_dir(arg(args, "path")?).await?),
        "read_text_file" => value(crate::read_text_file(arg(args, "path")?)?),
        "read_file_tail" => value(crate::read_file_tail(
            arg(args, "path")?,
            arg(args, "maxBytes")?,
        )?),
        "home_dir" => value(
            dirs::home_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        "write_text_file" => value(crate::write_text_file(
            arg(args, "path")?,
            arg(args, "content")?,
        )?),
        "create_entry" => value(crate::create_entry(arg(args, "path")?, arg(args, "dir")?)?),
        "git_repos" => value(crate::git::git_repos(arg(args, "path")?).await?),
        "git_status" => {
            value(crate::git::git_status(arg(args, "path")?, arg(args, "limit")?).await?)
        }
        "git_commit" => value(
            crate::git::git_commit(
                arg(args, "path")?,
                arg(args, "files")?,
                arg(args, "message")?,
                arg(args, "amend")?,
            )
            .await?,
        ),
        "git_log_messages" => {
            value(crate::git::git_log_messages(arg(args, "path")?, arg(args, "limit")?).await?)
        }
        "git_log_graph" => {
            value(crate::git::git_log_graph(arg(args, "path")?, arg(args, "limit")?).await?)
        }
        "git_remote_url" => value(crate::git::git_remote_url(arg(args, "path")?).await?),
        "git_root" => value(crate::git::git_root(&arg::<String>(args, "path")?)?),
        "git_unpushed_commits" => {
            value(crate::git::git_unpushed_commits(arg(args, "path")?).await?)
        }
        "git_branches" => value(crate::git::git_branches(arg(args, "path")?).await?),
        "git_checkout" => value(
            crate::git::git_checkout(
                arg(args, "path")?,
                arg(args, "branch")?,
                arg(args, "stash")?,
            )
            .await?,
        ),
        "git_fetch" => value(crate::git::git_fetch(arg(args, "path")?).await?),
        "git_create_branch" => value(
            crate::git::git_create_branch(
                arg(args, "path")?,
                arg(args, "name")?,
                arg(args, "checkout")?,
                arg(args, "overwrite")?,
                arg(args, "startPoint")?,
            )
            .await?,
        ),
        "git_rename_branch" => value(
            crate::git::git_rename_branch(
                arg(args, "path")?,
                arg(args, "oldName")?,
                arg(args, "newName")?,
            )
            .await?,
        ),
        "git_delete_branch" => value(
            crate::git::git_delete_branch(
                arg(args, "path")?,
                arg(args, "fullName")?,
                arg(args, "isRemote")?,
            )
            .await?,
        ),
        "git_resolve_ref" => {
            value(crate::git::git_resolve_ref(arg(args, "path")?, arg(args, "reference")?).await?)
        }
        "git_merge_branch" => {
            value(crate::git::git_merge_branch(arg(args, "path")?, arg(args, "branch")?).await?)
        }
        "git_rebase_onto" => {
            value(crate::git::git_rebase_onto(arg(args, "path")?, arg(args, "branch")?).await?)
        }
        "git_update_branch" => {
            value(crate::git::git_update_branch(arg(args, "path")?, arg(args, "rebase")?).await?)
        }
        "git_push_current" => value(crate::git::git_push_current(arg(args, "path")?).await?),
        "git_set_upstream" => value(
            crate::git::git_set_upstream(
                arg(args, "path")?,
                arg(args, "branch")?,
                arg(args, "upstream")?,
            )
            .await?,
        ),
        "git_compare_with_current" => value(
            crate::git::git_compare_with_current(arg(args, "path")?, arg(args, "branch")?).await?,
        ),
        "git_toggle_branch_favorite" => value(
            crate::git::git_toggle_branch_favorite(arg(args, "path")?, arg(args, "fullName")?)
                .await?,
        ),
        "git_commit_files" => {
            value(crate::git::git_commit_files(arg(args, "path")?, arg(args, "commit")?).await?)
        }
        "git_diff_with_ref" => {
            value(crate::git::git_diff_with_ref(arg(args, "path")?, arg(args, "reference")?).await?)
        }
        "git_commit_diff_file" => value(
            crate::git::git_commit_diff_file(
                arg(args, "path")?,
                arg(args, "commit")?,
                arg(args, "file")?,
            )
            .await?,
        ),
        "git_diff_file" => value(
            crate::git::git_diff_file(arg(args, "path")?, arg(args, "file")?, arg(args, "base")?)
                .await?,
        ),
        "git_operation" => {
            value(crate::git::git_operation(arg(args, "path")?, arg(args, "action")?).await?)
        }
        "git_rollback_file" => {
            value(crate::git::git_rollback_file(arg(args, "path")?, arg(args, "file")?).await?)
        }
        "git_add_ignore" => value(
            crate::git::git_add_ignore(
                arg(args, "path")?,
                arg(args, "pattern")?,
                arg(args, "local")?,
            )
            .await?,
        ),
        "git_revert_hunk" => value(
            crate::git::git_revert_hunk(
                arg(args, "path")?,
                arg(args, "file")?,
                arg(args, "content")?,
                arg(args, "crlf")?,
            )
            .await?,
        ),
        "read_note" => value(crate::notes::read_note(
            arg(args, "wsId")?,
            arg(args, "noteId")?,
        )),
        "write_note" => value(crate::notes::write_note(
            arg(args, "wsId")?,
            arg(args, "noteId")?,
            arg(args, "content")?,
        )?),
        "delete_note" => value(crate::notes::delete_note(
            arg(args, "wsId")?,
            arg(args, "noteId")?,
        )?),
        "link_note" => value(crate::notes::link_note(
            arg(args, "wsId")?,
            arg(args, "noteId")?,
            arg(args, "termTileId")?,
            arg(args, "title")?,
        )?),
        "link_term" => value(crate::notes::link_term(
            arg(args, "aId")?,
            arg(args, "aName")?,
            arg(args, "bId")?,
            arg(args, "bName")?,
        )?),
        "unlink_term" => value(crate::notes::unlink_term(
            arg(args, "aId")?,
            arg(args, "bId")?,
        )?),
        "unlink_note" => value(crate::notes::unlink_note(
            arg(args, "noteId")?,
            arg(args, "termTileId")?,
        )?),
        "docker_available" => value(crate::docker::docker_available().await),
        "docker_ps" => value(crate::docker::docker_ps().await?),
        "docker_engine" => value(crate::docker::docker_engine(arg(args, "action")?).await?),
        "docker_action" => {
            value(crate::docker::docker_action(arg(args, "id")?, arg(args, "action")?).await?)
        }
        "agent_usage" => value(crate::usage::agent_usage(arg(args, "provider")?, arg(args, "accountId")?).await?),
        "agent_history" => value(
            crate::history::agent_history(
                arg(args, "agent")?,
                arg(args, "cwd")?,
                arg(args, "tileId")?,
            )
            .await?,
        ),
        "pi_session_summary" => value(crate::pi::pi_session_summary(arg(args, "sessionId")?)),
        "claude_session_summary" => value(crate::claude::claude_session_summary(
            arg(args, "sessionId")?,
            arg(args, "cwd")?,
        )),
        _ => Err("Command not allowed".into()),
    }
}

async fn git_blob_bytes(args: &Value) -> Result<Vec<u8>, String> {
    bytes(
        crate::git::git_blob(
            arg(args, "path")?,
            arg(args, "file")?,
            arg(args, "old")?,
            arg(args, "commit")?,
            arg(args, "base")?,
        )
        .await?,
    )
}

#[derive(Default)]
pub struct Watches {
    git_files: Vec<u32>,
    repos: Vec<String>,
    files: Vec<u32>,
    runs: Vec<u32>,
    chunks: std::collections::HashMap<String, (Vec<u8>, usize, std::time::Instant)>,
    uploads: std::collections::HashMap<String, (Vec<u8>, std::time::Instant)>,
    next: u64,
}
impl Watches {
    async fn dispatch(
        &mut self,
        app: &tauri::AppHandle,
        command: &str,
        args: &Value,
    ) -> Option<Result<Value, String>> {
        if let Some(result) = self.data(command, args).await {
            return Some(result);
        }
        Some(match command {
            "git_watch_file" => match arg(args, "path")
                .and_then(|path| arg(args, "file").map(|file| (path, file)))
            {
                Ok((path, file)) => match crate::git::git_watch_file(app.clone(), path, file).await
                {
                    Ok(id) => {
                        self.git_files.push(id);
                        value(id)
                    }
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            },
            "git_watch_repo" => match arg(args, "path") {
                Ok(path) => match crate::git::git_watch_repo(app.clone(), path).await {
                    Ok(repo) => match value(repo) {
                        Ok(v) => {
                            if let Some(root) = v["root"].as_str() {
                                self.repos.push(root.to_owned());
                            }
                            Ok(v)
                        }
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            },
            "watch_text_file" => {
                match arg(args, "path").and_then(|p| crate::watch_text_file(app.clone(), p)) {
                    Ok(id) => {
                        self.files.push(id);
                        value(id)
                    }
                    Err(e) => Err(e),
                }
            }
            "run_watch_manifests" => {
                match arg(args, "path").and_then(|p| crate::run_watch_manifests(app.clone(), p)) {
                    Ok(id) => {
                        self.runs.push(id);
                        value(id)
                    }
                    Err(e) => Err(e),
                }
            }
            "git_unwatch_file" => match arg(args, "id") {
                Ok(id) if take(&mut self.git_files, &id) => {
                    crate::git::git_unwatch_file(id).await;
                    Ok(Value::Null)
                }
                _ => Err("Unknown watch".into()),
            },
            "git_unwatch_repo" => match arg(args, "root") {
                Ok(root) if take(&mut self.repos, &root) => {
                    crate::git::git_unwatch_repo(root).await;
                    Ok(Value::Null)
                }
                _ => Err("Unknown watch".into()),
            },
            "unwatch_text_file" => match arg(args, "id") {
                Ok(id) if take(&mut self.files, &id) => {
                    crate::unwatch_text_file(id);
                    Ok(Value::Null)
                }
                _ => Err("Unknown watch".into()),
            },
            "run_unwatch_manifests" => match arg(args, "id") {
                Ok(id) if take(&mut self.runs, &id) => {
                    crate::run_unwatch_manifests(id);
                    Ok(Value::Null)
                }
                _ => Err("Unknown watch".into()),
            },
            _ => return None,
        })
    }
    pub async fn data(&mut self, command: &str, args: &Value) -> Option<Result<Value, String>> {
        self.expire();
        Some(match command {
            "read_file_bytes" | "git_blob" | "read_temp_image" => {
                let result = match command {
                    "read_file_bytes" => arg(args, "path")
                        .and_then(crate::read_file_bytes)
                        .and_then(bytes),
                    "read_temp_image" => arg(args, "path")
                        .and_then(crate::read_temp_image)
                        .and_then(bytes),
                    _ => git_blob_bytes(args).await,
                };
                match result {
                    Ok(data) => self.insert_bytes(data),
                    Err(e) => Err(e),
                }
            }
            "read_bytes_chunk" => match arg::<String>(args, "token") {
                Ok(token) if self.chunks.contains_key(&token) => Ok(self.chunk(&token)),
                _ => Err("Unknown byte stream".into()),
            },
            "upload_image_chunk" => self.upload_image(args).and_then(|done| match done {
                Some((name, data)) => value(crate::save_temp_image(&name, &data)?),
                None => Ok(Value::Null),
            }),
            _ => return None,
        })
    }
    fn insert_bytes(&mut self, data: Vec<u8>) -> Result<Value, String> {
        if self.chunks.len() >= 8
            || data.len()
                + self
                    .chunks
                    .values()
                    .map(|(bytes, _, _)| bytes.len())
                    .sum::<usize>()
                > 64 * 1024 * 1024
        {
            return Err("Byte stream limit exceeded".into());
        }
        self.next += 1;
        let token = self.next.to_string();
        self.chunks
            .insert(token.clone(), (data, 0, std::time::Instant::now()));
        Ok(self.chunk(&token))
    }
    fn upload_image(&mut self, args: &Value) -> Result<Option<(String, Vec<u8>)>, String> {
        let name: String = arg(args, "name")?;
        let data: Vec<u8> = arg(args, "bytes")?;
        let first: bool = arg(args, "first")?;
        let last: bool = arg(args, "last")?;
        if data.len() > 32768 {
            return Err("Image chunk too large".into());
        }
        if first {
            if self.uploads.len() >= 8 && !self.uploads.contains_key(&name) {
                return Err("Too many uploads".into());
            }
            self.uploads
                .insert(name.clone(), (Vec::new(), std::time::Instant::now()));
        }
        let (upload, touched) = self.uploads.get_mut(&name).ok_or("Unknown upload")?;
        *touched = std::time::Instant::now();
        if upload.len() + data.len() > 5 * 1024 * 1024 {
            self.uploads.remove(&name);
            return Err("Image too large".into());
        }
        upload.extend(data);
        if last {
            let (data, _) = self.uploads.remove(&name).unwrap();
            Ok(Some((name, data)))
        } else {
            Ok(None)
        }
    }
    fn expire(&mut self) {
        let ttl = std::time::Duration::from_secs(60);
        self.chunks
            .retain(|_, (_, _, touched)| touched.elapsed() < ttl);
        self.uploads
            .retain(|_, (_, touched)| touched.elapsed() < ttl);
    }
    fn chunk(&mut self, token: &str) -> Value {
        let (data, offset, touched) = self.chunks.get_mut(token).unwrap();
        *touched = std::time::Instant::now();
        let end = (*offset + 256 * 1024).min(data.len());
        let encoded = STANDARD.encode(&data[*offset..end]);
        *offset = end;
        let total = data.len();
        let more = end < total;
        if !more {
            self.chunks.remove(token);
        }
        json!({"bytes": encoded, "next": if more { Some(token) } else { None }, "total": total})
    }
    async fn cleanup(self) {
        for id in self.git_files {
            crate::git::git_unwatch_file(id).await;
        }
        for root in self.repos {
            crate::git::git_unwatch_repo(root).await;
        }
        for id in self.files {
            crate::unwatch_text_file(id);
        }
        for id in self.runs {
            crate::run_unwatch_manifests(id);
        }
    }
}
fn take<T: PartialEq>(list: &mut Vec<T>, item: &T) -> bool {
    if let Some(index) = list.iter().position(|v| v == item) {
        list.remove(index);
        true
    } else {
        false
    }
}

fn queue_event(tx: &tokio::sync::mpsc::Sender<Value>, lost: &tokio::sync::Notify, event: Value) {
    if tx.try_send(event).is_err() {
        lost.notify_one();
    }
}

async fn read_managed(
    watches: &tokio::sync::Mutex<std::collections::HashMap<u64, Watches>>,
    session: u64,
    read: impl std::future::Future<Output = Result<Vec<u8>, String>>,
) -> Result<Value, String> {
    let data = read.await?;
    let mut all = watches.lock().await;
    let watch = all.entry(session).or_default();
    watch.expire();
    watch.insert_bytes(data)
}

async fn dispatch_request(
    app: tauri::AppHandle,
    watches: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<u64, Watches>>>,
    session: u64,
    cmd: &str,
    args: &Value,
) -> Result<Value, String> {
    let managed = matches!(
        cmd,
        "read_file_bytes"
            | "git_blob"
            | "read_temp_image"
            | "read_bytes_chunk"
            | "upload_image_chunk"
            | "git_watch_file"
            | "git_watch_repo"
            | "watch_text_file"
            | "run_watch_manifests"
            | "git_unwatch_file"
            | "git_unwatch_repo"
            | "unwatch_text_file"
            | "run_unwatch_manifests"
    );
    if cmd == "release" {
        let watch = watches.lock().await.remove(&session);
        if let Some(watch) = watch {
            watch.cleanup().await;
        }
        Ok(Value::Null)
    } else if matches!(cmd, "read_file_bytes" | "read_temp_image" | "git_blob") {
        read_managed(&watches, session, async {
            if cmd == "git_blob" {
                git_blob_bytes(args).await
            } else {
                let path: String = arg(args, "path")?;
                let image = cmd == "read_temp_image";
                tokio::task::spawn_blocking(move || {
                    if image {
                        crate::read_temp_image(path)
                    } else {
                        crate::read_file_bytes(path)
                    }
                    .and_then(bytes)
                })
                .await
                .map_err(|e| e.to_string())?
            }
        })
        .await
    } else if cmd == "upload_image_chunk" {
        let completed = watches
            .lock()
            .await
            .entry(session)
            .or_default()
            .upload_image(args)?;
        match completed {
            Some((name, data)) => {
                tokio::task::spawn_blocking(move || value(crate::save_temp_image(&name, &data)?))
                    .await
                    .map_err(|e| e.to_string())?
            }
            None => Ok(Value::Null),
        }
    } else if cmd == "read_bytes_chunk" {
        watches
            .lock()
            .await
            .entry(session)
            .or_default()
            .data(cmd, args)
            .await
            .unwrap()
    } else if managed {
        let mut temporary = Watches::default();
        if cmd.starts_with("git_unwatch_")
            || cmd == "unwatch_text_file"
            || cmd == "run_unwatch_manifests"
        {
            let mut all = watches.lock().await;
            let watch = all.entry(session).or_default();
            match cmd {
                "git_unwatch_file" => {
                    let id = arg(args, "id")?;
                    if !take(&mut watch.git_files, &id) {
                        return Err("Unknown watch".into());
                    }
                    temporary.git_files.push(id);
                }
                "git_unwatch_repo" => {
                    let root = arg(args, "root")?;
                    if !take(&mut watch.repos, &root) {
                        return Err("Unknown watch".into());
                    }
                    temporary.repos.push(root);
                }
                "unwatch_text_file" => {
                    let id = arg(args, "id")?;
                    if !take(&mut watch.files, &id) {
                        return Err("Unknown watch".into());
                    }
                    temporary.files.push(id);
                }
                _ => {
                    let id = arg(args, "id")?;
                    if !take(&mut watch.runs, &id) {
                        return Err("Unknown watch".into());
                    }
                    temporary.runs.push(id);
                }
            }
        }
        let result = temporary.dispatch(&app, cmd, args).await.unwrap();
        let mut all = watches.lock().await;
        let watch = all.entry(session).or_default();
        watch.git_files.append(&mut temporary.git_files);
        watch.repos.append(&mut temporary.repos);
        watch.files.append(&mut temporary.files);
        watch.runs.append(&mut temporary.runs);
        result
    } else {
        dispatch(cmd, args).await
    }
}

#[cfg(test)]
async fn release(watches: &mut std::collections::HashMap<u64, Watches>, session: u64) {
    if let Some(watch) = watches.remove(&session) {
        watch.cleanup().await;
    }
}

async fn connect_provider() -> Option<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let secret = std::fs::read_to_string(secret_path()?).ok()?;
    let url = format!("ws://127.0.0.1:{}/host-service", crate::sidecar_port());
    let mut request = url.into_client_request().ok()?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", secret.trim()).parse().ok()?,
    );
    tokio_tungstenite::connect_async_with_config(request, None, true)
        .await
        .ok()
        .map(|(ws, _)| ws)
}

async fn serve_provider(
    app: &tauri::AppHandle,
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> bool {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(64);
    let mut listeners = Vec::new();
    let watches = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        u64,
        Watches,
    >::new()));
    let mut tasks = tokio::task::JoinSet::new();
    let lost = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut replaced = false;
    for &event in HOST_EVENTS {
        let tx = tx.clone();
        let lost = lost.clone();
        listeners.push(app.listen(event, move |e| {
            let payload = serde_json::from_str::<Value>(e.payload()).unwrap_or(Value::Null);
            queue_event(&tx, &lost, json!({"event": event, "payload": payload}));
        }));
    }
    let mut expiry = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = expiry.tick() => { for watch in watches.lock().await.values_mut() { watch.expire(); } },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(s))) => if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if let (Some(id), Some(cmd)) = (v.get("id"), v.get("command").and_then(Value::as_str)) {
                        let id = id.clone();
                        let cmd = cmd.to_owned();
                        let args = v["args"].clone();
                        let session = v["session"].as_u64().unwrap_or(0);
                        let app = app.clone();
                        let watches = watches.clone();
                        let tx = tx.clone();
                        tasks.spawn(async move {
                            let result = dispatch_request(app, watches, session, &cmd, &args).await;
                            let response = match result {
                                Ok(r) => json!({"id": id, "result": r}),
                                Err(e) => json!({"id": id, "error": e}),
                            };
                            let _ = tx.send(response).await;
                        });
                    }
                },
                Some(Ok(Message::Close(frame))) => {
                    replaced = frame.as_ref().is_some_and(|f| f.reason == "provider replaced");
                    break;
                },
                Some(Err(_)) | None => break,
                _ => {},
            },
            event = rx.recv() => match event {
                Some(v) => if sink.send(Message::Text(v.to_string().into())).await.is_err() { break; },
                None => break,
            },
            _ = lost.notified() => {
                let event = json!({"event":"resync"}).to_string();
                if sink.send(Message::Text(event.into())).await.is_err() { break; }
            },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
        }
    }
    for id in listeners {
        app.unlisten(id);
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    if let Ok(watches) = std::sync::Arc::try_unwrap(watches) {
        for (_, watch) in watches.into_inner() {
            watch.cleanup().await;
        }
    }
    replaced
}

pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(ws) = connect_provider().await {
                if serve_provider(&app, ws).await {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn slow_read_does_not_hold_managed_lock() {
        let watches = tokio::sync::Mutex::new(std::collections::HashMap::new());
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finish, waiting) = tokio::sync::oneshot::channel();
        let slow = read_managed(&watches, 1, async {
            started.send(()).unwrap();
            waiting.await.unwrap();
            Ok(vec![1])
        });
        let fast = async {
            ready.await.unwrap();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                read_managed(&watches, 2, async { Ok(vec![2]) }),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result["bytes"], "Ag==");
            finish.send(()).unwrap();
        };
        let (result, ()) = tokio::join!(slow, fast);
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn release_only_removes_the_ended_session() {
        let mut sessions = std::collections::HashMap::new();
        let mut first = Watches::default();
        first
            .chunks
            .insert("first".into(), (vec![1], 0, std::time::Instant::now()));
        let mut second = Watches::default();
        second
            .chunks
            .insert("second".into(), (vec![2], 0, std::time::Instant::now()));
        sessions.insert(1, first);
        sessions.insert(2, second);
        release(&mut sessions, 1).await;
        assert!(!sessions.contains_key(&1));
        assert!(sessions[&2].chunks.contains_key("second"));
    }
    #[tokio::test]
    async fn event_overflow_signals_resync() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let lost = tokio::sync::Notify::new();
        queue_event(&tx, &lost, json!({"event":"first"}));
        queue_event(&tx, &lost, json!({"event":"lost"}));
        tokio::time::timeout(std::time::Duration::from_millis(100), lost.notified())
            .await
            .unwrap();
    }
    #[test]
    fn watch_ownership_removes_one_registration() {
        let mut watches = vec![1, 1, 2];
        assert!(take(&mut watches, &1));
        assert_eq!(watches, vec![1, 2]);
        assert!(!take(&mut watches, &3));
    }
    #[tokio::test]
    async fn chunked_bytes_and_image_upload() {
        let mut watches = Watches::default();
        let name = format!("test-paste-{}.png", std::process::id());
        let data = vec![42u8; 270000];
        let mut path = Value::Null;
        let count = data.chunks(32768).len();
        for (index, chunk) in data.chunks(32768).enumerate() {
            path = watches.data("upload_image_chunk", &json!({
                "name": name, "bytes": chunk, "first": index == 0, "last": index + 1 == count
            })).await.unwrap().unwrap();
        }
        let path = path.as_str().unwrap();
        let first = watches
            .data("read_temp_image", &json!({"path":path}))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            STANDARD.decode(first["bytes"].as_str().unwrap()).unwrap(),
            data[..256 * 1024]
        );
        let token = first["next"].as_str().unwrap();
        let last = watches
            .data("read_bytes_chunk", &json!({"token":token}))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            STANDARD.decode(last["bytes"].as_str().unwrap()).unwrap(),
            data[256 * 1024..]
        );
        assert!(last["next"].is_null());
        assert!(watches
            .data("read_bytes_chunk", &json!({"token":token}))
            .await
            .unwrap()
            .is_err());
        assert!(watches
            .data(
                "upload_image_chunk",
                &json!({"name":"../bad", "bytes":[], "first":true, "last":true})
            )
            .await
            .unwrap()
            .is_err());
        assert!(watches
            .data(
                "read_temp_image",
                &json!({"path":format!("{path}/../other")})
            )
            .await
            .unwrap()
            .is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn empty_byte_stream_finishes_immediately() {
        let mut watches = Watches::default();
        watches
            .chunks
            .insert("empty".into(), (Vec::new(), 0, std::time::Instant::now()));
        assert_eq!(watches.chunk("empty"), json!({"bytes":"", "next":null, "total":0}));
        assert!(watches.chunks.is_empty());
    }
    #[tokio::test]
    async fn abandoned_transfers_expire_and_stream_count_is_bounded() {
        let mut watches = Watches::default();
        let old = std::time::Instant::now() - std::time::Duration::from_secs(61);
        watches.chunks.insert("old".into(), (vec![1], 0, old));
        watches.uploads.insert("old".into(), (vec![1], old));
        watches.expire();
        assert!(watches.chunks.is_empty());
        assert!(watches.uploads.is_empty());
        for i in 0..8 {
            watches
                .chunks
                .insert(i.to_string(), (vec![1], 0, std::time::Instant::now()));
        }
        let file =
            std::env::temp_dir().join(format!("panorama-chunk-limit-{}", std::process::id()));
        std::fs::write(&file, [1]).unwrap();
        assert_eq!(
            watches
                .data("read_file_bytes", &json!({"path": file}))
                .await
                .unwrap()
                .unwrap_err(),
            "Byte stream limit exceeded"
        );
        std::fs::remove_file(file).unwrap();
    }

    #[tokio::test]
    async fn real_git_and_bytes() {
        let dir = std::env::temp_dir().join(format!("panorama-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status()
            .unwrap()
            .success());
        let path = dir.to_string_lossy().to_string();
        assert!(dispatch("git_status", &json!({"path": path})).await.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn rejects_unlisted_and_validates_args() {
        assert_eq!(
            dispatch("open_url", &json!({})).await.unwrap_err(),
            "Command not allowed"
        );
        assert!(dispatch("store_read", &json!({})).await.is_err());
    }
}
