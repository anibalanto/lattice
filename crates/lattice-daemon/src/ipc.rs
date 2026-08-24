use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

use crate::lsp_manager::LspManager;
use crate::types::{RpcRequest, RpcResponse};

pub async fn serve(
    manager:     Arc<LspManager>,
    socket_path: &Path,
    shutdown:    Arc<Notify>,
) -> anyhow::Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;

    loop {
        tokio::select! {
            res = listener.accept() => {
                let (stream, _) = res?;
                tokio::spawn(handle_conn(
                    stream,
                    Arc::clone(&manager),
                    Arc::clone(&shutdown),
                ));
            }
            _ = shutdown.notified() => break,
        }
    }
    Ok(())
}

async fn handle_conn(stream: UnixStream, manager: Arc<LspManager>, shutdown: Arc<Notify>) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let (resp, stop) = dispatch(&manager, &shutdown, &line).await;
        let json = match serde_json::to_string(&resp) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if writer.write_all(json.as_bytes()).await.is_err() { break; }
        if writer.write_all(b"\n").await.is_err() { break; }
        if stop { break; }
    }
}

async fn dispatch(
    manager:  &LspManager,
    shutdown: &Notify,
    line:     &str,
) -> (RpcResponse, bool) {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => return (RpcResponse::parse_error(e.to_string()), false),
    };

    let id = req.id.clone();

    match req.method.as_str() {
        "ping" => (RpcResponse::ok(id, serde_json::json!("pong")), false),

        "callees" => {
            #[derive(serde::Deserialize)]
            struct P { file: String, line: u32, col: u32 }
            let p: P = match serde_json::from_value(req.params) {
                Ok(v) => v,
                Err(e) => return (RpcResponse::invalid_params(id, e.to_string()), false),
            };
            let resp = match manager.callees(&p.file, p.line, p.col).await {
                Ok(v)  => RpcResponse::ok(id, serde_json::json!(v)),
                Err(e) => RpcResponse::server_error(id, e.to_string()),
            };
            (resp, false)
        }

        "callers" => {
            #[derive(serde::Deserialize)]
            struct P { file: String, line: u32, col: u32 }
            let p: P = match serde_json::from_value(req.params) {
                Ok(v) => v,
                Err(e) => return (RpcResponse::invalid_params(id, e.to_string()), false),
            };
            let resp = match manager.callers(&p.file, p.line, p.col).await {
                Ok(v)  => RpcResponse::ok(id, serde_json::json!(v)),
                Err(e) => RpcResponse::server_error(id, e.to_string()),
            };
            (resp, false)
        }

        "symbol_at" => {
            #[derive(serde::Deserialize)]
            struct P { file: String, line: u32, col: u32 }
            let p: P = match serde_json::from_value(req.params) {
                Ok(v) => v,
                Err(e) => return (RpcResponse::invalid_params(id, e.to_string()), false),
            };
            let resp = match manager.symbol_at(&p.file, p.line, p.col).await {
                Ok(v)  => RpcResponse::ok(id, serde_json::json!(v)),
                Err(e) => RpcResponse::server_error(id, e.to_string()),
            };
            (resp, false)
        }

        "status" => {
            let status = manager.status().await;
            (RpcResponse::ok(id, serde_json::json!(status)), false)
        }

        "shutdown" => {
            shutdown.notify_one();
            (RpcResponse::ok(id, serde_json::Value::Null), true)
        }

        method => (RpcResponse::method_not_found(id, method), false),
    }
}
