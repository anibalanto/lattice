use std::path::{Path, PathBuf};
use clap::{Parser, Subcommand};

mod html;
mod render;

use lattice::graph::{Direction, Graph, TraverseOpts};
use lattice::model::{Guarantee, NodeId};
use lattice::provider::{Availability, BilinkProvider, LspProvider, Registry};

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
        /// Sigue las aristas dirigidas hacia los llamadores
        #[arg(long, conflicts_with_all = ["down", "both"])]
        up: bool,
        /// Sigue las aristas dirigidas hacia los llamados
        #[arg(long, conflicts_with = "both")]
        down: bool,
        /// Ambos sentidos (default)
        #[arg(long)]
        both: bool,
        /// Profundidad máxima del traversal
        #[arg(long)]
        depth: Option<usize>,
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
        Command::Graph { selector, up, down, both, depth, via, guarantee, state, format } => {
            let direction = if up { Direction::Up }
                       else if down { Direction::Down }
                       else { let _ = both; Direction::Both };
            cmd_graph(&cwd, &selector, direction, depth, via.as_deref(),
                      guarantee.as_deref(), state.as_deref(), &format)
        }

        Command::Daemon { sub } => match sub {
            DaemonCommand::Start { workspace } => daemon_start(&workspace.unwrap_or(cwd)),
            DaemonCommand::Stop   => daemon_stop(),
            DaemonCommand::Status => daemon_status(),
        },
    }
}

// ─── graph ────────────────────────────────────────────────────────────────────

/// Nodos de partida del traversal, según la forma del selector.
///
/// `.` y `*` no arrancan un traversal: piden el grafo entero de la capa.
fn resolve_selector(g: &Graph, cwd: &Path, selector: &str) -> Option<Vec<NodeId>> {
    if selector == "." || selector == "*" { return None; }

    // archivo:línea:col → nodos que cubren esa posición
    let parts: Vec<&str> = selector.rsplitn(3, ':').collect();
    if parts.len() == 3 {
        if let (Ok(col), Ok(line)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
            let file = parts[2];
            if let Ok(source) = std::fs::read_to_string(cwd.join(file)) {
                let pos = line_col_to_byte(&source, line, col);
                // La capa del nodo es la de cwd relativa a la raíz más externa:
                // los nodos se nombran contra esa raíz, no contra el directorio
                // desde el que se invoca.
                let base  = lattice::provider::outermost_root(cwd);
                let rel   = cwd.strip_prefix(&base).ok()
                    .map(|p| p.display().to_string()).unwrap_or_default();
                let layer = if rel.is_empty() { ".".to_string() } else { rel };

                // El nodo de la posición misma, además de los que la cubren.
                //
                // El LSP razona sobre posiciones, no sobre los rangos que
                // declaró bilinker: preguntarle por el byte 0 de un archivo
                // entero no significa nada. Este nodo sintético es su punto de
                // entrada, y la contención lo conecta con lo que haya declarado
                // encima.
                let mut starts = vec![NodeId(format!("{layer}::{file}#{pos}~{pos}"))];
                starts.extend(g.covering(&layer, file, pos).into_iter().cloned());
                return Some(starts);
            }
            // Sin nodos que la cubran, el selector no resuelve.
            return Some(vec![]);
        }
    }

    // UUID de un vínculo → los dos extremos de esa arista
    let by_ref: Vec<NodeId> = g.edges.iter()
        .filter(|e| e.r#ref.starts_with(selector))
        .flat_map(|e| [e.from.clone(), e.to.clone()])
        .collect();
    if !by_ref.is_empty() { return Some(by_ref); }

    // archivo → todos los nodos de ese archivo
    Some(g.nodes().into_iter()
        .filter(|n| n.as_fragment().is_some_and(|(_, p, _)| p == selector || p.ends_with(selector)))
        .cloned().collect())
}

fn line_col_to_byte(source: &str, line: usize, col: usize) -> usize {
    let mut cur = 1;
    for (i, c) in source.char_indices() {
        if cur == line { return i + (col - 1).min(source.len() - i); }
        if c == '\n' { cur += 1; }
    }
    source.len()
}

#[allow(clippy::too_many_arguments)]
fn cmd_graph(
    cwd: &Path, selector: &str,
    direction: Direction, depth: Option<usize>,
    via: Option<&str>, guarantee: Option<&str>, state: Option<&str>,
    format: &str,
) -> anyhow::Result<()> {
    let registry = Registry::new()
        .register(Box::new(BilinkProvider::default()))
        .register(Box::new(LspProvider));
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
    // El traversal va después de los filtros: `--via` y `--guarantee` definen
    // por qué aristas se puede caminar, no solo cuáles se muestran.
    let graph  = Graph::new(edges, status);
    let starts = resolve_selector(&graph, cwd, selector);

    let (graph, mut edges) = match starts {
        // `.` y `*` no recorren: piden el grafo entero de la capa.
        None => { let all = graph.edges.clone(); (graph, all) }
        Some(starts) => {
            // Los proveedores que expanden bajo demanda —el LSP— aportan sus
            // aristas antes de recorrer: si no, el traversal solo vería lo que
            // los que enumeran ya habían puesto.
            let extra: Vec<_> = starts.iter()
                .flat_map(|n| registry.expand(cwd, n)).collect();
            let graph = if extra.is_empty() { graph } else {
                let mut all = graph.edges.clone();
                all.extend(extra);
                Graph::new(lattice::provider::dedup(all), graph.providers)
            };
            let opts = TraverseOpts { direction, depth, stop_at_accepted: true };
            let reached = graph.traverse(&starts, &opts).into_iter()
                .map(|i| graph.edges[i].clone()).collect();
            (graph, reached)
        }
    };
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    let status = &graph.providers;

    let degraded = status.iter().any(|s| !s.status.is_available());

    match format {
        "json" => render::json(&edges, status)?,
        "flat" => render::flat(&edges),
        "dot"  => render::dot(&edges),
        "html" => print!("{}", html::render(&lattice::provider::outermost_root(cwd), &edges)),
        _      => {
            eprintln!("{}
", render::providers_line(status));
            render::tree(&edges);
            if edges.is_empty() { eprintln!("(sin aristas)"); }
        }
    }

    for s in status {
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
