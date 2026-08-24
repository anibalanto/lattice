use std::path::{Path, PathBuf};
use clap::{Parser, Subcommand};

use lattice::model::Guarantee;
use lattice::provider::{Availability, BilinkProvider, Registry};

#[derive(Parser)]
#[command(name = "lattice", about = "El grafo agregado de las conexiones del proyecto")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Recorre el grafo agregado desde un selector
    Graph {
        /// Archivo, posición archivo:línea:col, UUID, o `.` para la capa actual
        #[arg(default_value = ".")]
        selector: String,
        /// Tipos de arista habilitados: bilink,governs,task,call,doclink,external
        #[arg(long)]
        via: Option<String>,
        /// Garantía mínima: accepted, derived, asserted
        #[arg(long)]
        guarantee: Option<String>,
        /// Solo aristas con ese estado. `non-ok` selecciona todo lo distinto de OK
        #[arg(long)]
        state: Option<String>,
        #[arg(long, default_value = "tree")]
        format: String,
    },

    /// Gestiona el daemon que mantiene vivos los language servers
    Daemon {
        #[command(subcommand)]
        sub: DaemonCommand,
    },
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Arranca el daemon en background
    Start {
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    /// Envía shutdown a los language servers y termina el proceso
    Stop,
    /// Estado del daemon y de los language servers activos
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Command::Graph { selector, via, guarantee, state, format } =>
            cmd_graph(&cwd, &selector, via.as_deref(), guarantee.as_deref(),
                      state.as_deref(), &format),

        Command::Daemon { sub } => match sub {
            DaemonCommand::Start { workspace } => daemon_start(&workspace.unwrap_or(cwd)),
            DaemonCommand::Stop   => daemon_stop(),
            DaemonCommand::Status => daemon_status(),
        },
    }
}

// ─── graph ────────────────────────────────────────────────────────────────────

fn cmd_graph(
    cwd: &Path, selector: &str,
    via: Option<&str>, guarantee: Option<&str>, state: Option<&str>,
    format: &str,
) -> anyhow::Result<()> {
    let registry = Registry::new().register(Box::new(BilinkProvider::default()));
    let (mut edges, status) = registry.collect(cwd);

    // Los filtros se aplican después de componer, no antes: el estado de los
    // proveedores tiene que reflejar el grafo completo que se pudo ver.
    if let Some(kinds) = via {
        let allowed: Vec<&str> = kinds.split(',').map(str::trim).collect();
        edges.retain(|e| allowed.contains(&e.kind.as_str()));
    }
    if let Some(g) = guarantee {
        let min = Guarantee::parse(g)
            .ok_or_else(|| anyhow::anyhow!("garantía desconocida: '{g}'"))?;
        edges.retain(|e| e.guarantee >= min);
    }
    if let Some(f) = state {
        edges.retain(|e| match &e.state {
            None => false,
            Some([a, b]) => match f {
                "non-ok" => a != "OK" || b != "OK",
                other    => a == other || b == other,
            },
        });
    }
    if selector != "." && selector != "*" {
        edges.retain(|e| e.from.0.contains(selector) || e.to.0.contains(selector)
                      || e.r#ref.starts_with(selector));
    }

    let degraded = status.iter().any(|s| !s.status.is_available());

    match format {
        "json" => {
            let providers: Vec<serde_json::Value> = status.iter().map(|s| match &s.status {
                Availability::Available => serde_json::json!({"name": s.name, "status": "available"}),
                Availability::Unavailable { reason } =>
                    serde_json::json!({"name": s.name, "status": "unavailable", "reason": reason}),
            }).collect();
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "providers": providers,
                "edges": edges,
            }))?);
        }
        _ => {
            let names: Vec<String> = status.iter().map(|s| match &s.status {
                Availability::Available => format!("{} OK", s.name),
                Availability::Unavailable { .. } => format!("{} no disponible", s.name),
            }).collect();
            eprintln!("proveedores: {}\n", names.join(" · "));

            for e in &edges {
                let st = e.state.as_ref()
                    .map(|[a, b]| format!("  [{a} ↔ {b}]")).unwrap_or_default();
                println!("{}…  {} ({}){st}",
                    &e.r#ref[..8.min(e.r#ref.len())], e.kind, e.guarantee);
                println!("  {}", e.from);
                println!("  {}", e.to);
            }
            if edges.is_empty() { eprintln!("(sin aristas)"); }
        }
    }

    for s in &status {
        if let Availability::Unavailable { reason } = &s.status {
            eprintln!("warn: proveedor {} no disponible ({reason})", s.name);
        }
    }

    // 3 distingue "no hay conexiones" de "no pude verlas": sin eso, un pipeline
    // de CI con un proveedor caído reporta verde.
    if degraded { std::process::exit(3); }
    if edges.is_empty() { std::process::exit(1); }
    Ok(())
}

// ─── daemon ───────────────────────────────────────────────────────────────────

fn lattice_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".lattice")
}

fn daemon_pid() -> u32 {
    std::fs::read_to_string(lattice_dir().join("daemon.pid"))
        .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn daemon_alive() -> bool {
    lattice::daemon_client::rpc("ping", serde_json::json!({}))
        .map(|v| v == serde_json::json!("pong")).unwrap_or(false)
}

fn daemon_start(workspace: &Path) -> anyhow::Result<()> {
    if daemon_alive() {
        eprintln!("el daemon ya está corriendo  pid={}", daemon_pid());
        std::process::exit(1);
    }
    let bin = std::env::current_exe()?.parent()
        .map(|d| d.join("lattice-daemon"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("lattice-daemon"));

    let child = std::process::Command::new(&bin)
        .arg("--workspace").arg(workspace)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    // El daemon escribe su pid al arrancar; esperar a que responda evita
    // reportar "started" sobre un proceso que murió en el handshake.
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if daemon_alive() {
            println!("daemon started  pid={}  socket={}",
                     child.id(), lattice_dir().join("daemon.sock").display());
            return Ok(());
        }
    }
    anyhow::bail!("el daemon no respondió en 5s")
}

fn daemon_stop() -> anyhow::Result<()> {
    if !daemon_alive() {
        eprintln!("el daemon no está corriendo");
        std::process::exit(1);
    }
    lattice::daemon_client::rpc("shutdown", serde_json::json!({}))?;
    println!("daemon stopped");
    Ok(())
}

fn daemon_status() -> anyhow::Result<()> {
    if !daemon_alive() {
        eprintln!("el daemon no está corriendo");
        std::process::exit(1);
    }
    println!("daemon  pid={}  socket={}", daemon_pid(),
             lattice_dir().join("daemon.sock").display());

    let servers = lattice::daemon_client::rpc("status", serde_json::json!({}))?;
    println!("\nlanguage servers:");
    match servers.as_array() {
        Some(list) if !list.is_empty() => {
            for s in list {
                println!("  {:<28}{:<9}queries={}",
                    s["name"].as_str().unwrap_or("?"),
                    s["state"].as_str().unwrap_or("?"),
                    s["queries"]);
            }
        }
        _ => println!("  (ninguno arrancado todavía)"),
    }
    Ok(())
}
