mod ipc;
mod language;
mod lsp_client;
mod lsp_manager;
mod types;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Notify;

#[derive(Parser)]
#[command(name = "lattice-daemon", about = "daemon LSP de lattice: call graph en vivo")]
struct Args {
    /// Workspace root to index
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let workspace = args.workspace.canonicalize().unwrap_or(args.workspace);

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let lattice_dir = PathBuf::from(&home).join(".lattice");
    std::fs::create_dir_all(&lattice_dir)?;

    let socket_path = lattice_dir.join("daemon.sock");
    let pid_path    = lattice_dir.join("daemon.pid");

    std::fs::write(&pid_path, std::process::id().to_string())?;

    let manager  = lsp_manager::LspManager::new(workspace);
    let shutdown = Arc::new(Notify::new());

    let result = tokio::select! {
        r = ipc::serve(Arc::clone(&manager), &socket_path, Arc::clone(&shutdown)) => r,
        _ = tokio::signal::ctrl_c() => Ok(()),
    };

    manager.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pid_path);

    result
}
