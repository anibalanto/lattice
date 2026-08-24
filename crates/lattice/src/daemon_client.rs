//! Cliente del daemon LSP.
//!
//! JSON-RPC 2.0 sobre un socket Unix, con framing newline-delimited. Es el único
//! punto por el que lattice habla con el daemon; el proveedor `lsp` se apoya acá.

use std::path::PathBuf;
use anyhow::Result;

pub fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".lattice").join("daemon.sock")
}

pub fn rpc(method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });
    let line = serde_json::to_string(&req)? + "\n";
    stream.write_all(line.as_bytes())?;

    let mut resp_line = String::new();
    BufReader::new(stream).read_line(&mut resp_line)?;

    let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;
    if let Some(err) = resp.get("error") {
        anyhow::bail!("{}", err["message"].as_str().unwrap_or("unknown error"));
    }
    Ok(resp["result"].clone())
}
