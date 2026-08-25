//! Proveedores de aristas.
//!
//! Lattice no produce aristas: descubre proveedores, les pregunta por las suyas
//! en un scope, y compone. La contención sí la calcula lattice, y no es una
//! arista.

use std::path::Path;
use anyhow::Result;
use crate::model::{Edge, Guarantee, NodeId};

/// Si un proveedor puede responder ahora.
///
/// Se consulta **antes** de componer el grafo, no al primer error: un proveedor
/// que falla a mitad de un traversal deja un grafo incompleto que ya se reportó
/// como completo.
#[derive(Debug, Clone, PartialEq)]
pub enum Availability {
    Available,
    Unavailable { reason: String },
}

impl Availability {
    pub fn is_available(&self) -> bool { matches!(self, Self::Available) }
}

pub trait Provider {
    fn name(&self) -> &str;

    /// Qué emite y con qué garantía. La garantía de un `kind` es fija: no existe
    /// un `call` aceptado ni un `bilink` derivado.
    fn kinds(&self) -> Vec<(&'static str, Guarantee)>;

    fn available(&self, scope: &Path) -> Availability;

    /// Sus aristas en el scope. Un proveedor que no enumera —el LSP, donde
    /// listar todas las llamadas del proyecto es inviable— devuelve vacío y se
    /// expande bajo demanda.
    fn edges(&self, scope: &Path) -> Result<Vec<Edge>>;

    /// Aristas incidentes a un nodo, para proveedores que expanden bajo demanda.
    ///
    /// El default vacío es correcto para los que enumeran: sus aristas ya
    /// salieron por `edges`.
    fn edges_from(&self, _scope: &Path, _node: &crate::model::NodeId) -> Result<Vec<Edge>> {
        Ok(Vec::new())
    }
}

// ─── proveedor bilink ─────────────────────────────────────────────────────────

/// Toma las aristas de bilinker vía `bilinker graph --format json`.
///
/// Bilinker entrega los nodos en forma canónica y con la topología de cadena ya
/// resuelta: componer eso con aristas ajenas es tarea de lattice, pero resolver
/// una cadena es conocimiento de su formato.
pub struct BilinkProvider {
    pub binary: String,
    /// Recolectar también de las capas descendientes.
    ///
    /// Se delega en bilinker en vez de que lattice recorra `.stratum/`: dónde
    /// vive una capa es conocimiento de Stratum y del formato bilink, no del
    /// grafo agregado.
    pub recursive: bool,
}

impl Default for BilinkProvider {
    fn default() -> Self { Self { binary: "bilinker".into(), recursive: false } }
}

impl BilinkProvider {
    pub fn recursive(mut self, yes: bool) -> Self { self.recursive = yes; self }
}

impl Provider for BilinkProvider {
    fn name(&self) -> &str { "bilink" }

    fn kinds(&self) -> Vec<(&'static str, Guarantee)> {
        vec![("bilink",  Guarantee::Accepted),
             ("governs", Guarantee::Accepted),
             ("task",    Guarantee::Accepted)]
    }

    fn available(&self, scope: &Path) -> Availability {
        if !scope.join(".bilink").exists() {
            return Availability::Unavailable { reason: "la capa no tiene .bilink/".into() };
        }
        match std::process::Command::new(&self.binary).arg("--version").output() {
            Ok(_)  => Availability::Available,
            Err(_) => Availability::Unavailable {
                reason: format!("no se encontró el ejecutable '{}'", self.binary),
            },
        }
    }

    fn edges(&self, scope: &Path) -> Result<Vec<Edge>> {
        let mut args = vec!["graph", ".", "--format", "json"];
        if self.recursive { args.push("--recursive"); }
        let out = std::process::Command::new(&self.binary)
            .args(&args)
            .current_dir(scope)
            .output()?;
        if !out.status.success() {
            anyhow::bail!("bilinker graph falló: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(serde_json::from_slice(&out.stdout)?)
    }
}

// ─── proveedor LSP ────────────────────────────────────────────────────────────

/// Aristas de llamada, consultadas al daemon en el momento.
///
/// No enumera: listar todas las llamadas de un proyecto es inviable, y además
/// el call graph no se persiste — el ecosistema ya intentó cachearlo en archivos
/// y lo revirtió. Se expande nodo por nodo durante el traversal.
pub struct LspProvider;

/// Info de un caller/callee tal como la devuelve el daemon.
#[derive(serde::Deserialize)]
struct CallInfo {
    name: String,
    file: String,
    line: u32,
    #[allow(dead_code)]
    col:  u32,
}

impl LspProvider {
    /// Posición `(línea, columna)` 0-based del anchor de un nodo.
    ///
    /// El nodo trae bytes; `callHierarchy` necesita la posición del
    /// identificador. La conversión byte → línea es exacta; la columna la
    /// aporta bilinker en el campo `anchor` del contrato, porque es quien tiene
    /// la query y puede resolver la captura del nombre.
    fn anchor_of(scope: &Path, node: &NodeId) -> Option<(String, u32, u32)> {
        let (layer, path, range) = node.as_fragment()?;
        let (start, _) = range?;

        // Las capas de un nodo son relativas a la raíz más externa del
        // ecosistema, no al directorio de invocación: si no, un nodo de la capa
        // de specs no se resolvería al correr desde impl.
        let base = outermost_root(scope);
        let abs  = base.join(if layer == "." { "" } else { layer }).join(path);

        let source = std::fs::read_to_string(&abs).ok()?;
        if start > source.len() { return None; }
        let line = source[..start].chars().filter(|&c| c == '\n').count() as u32;

        // `callHierarchy` necesita caer sobre el identificador. La columna se
        // mide desde el inicio de la línea, no desde el byte del fragmento.
        let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_text  = source[line_start..].lines().next().unwrap_or("");
        let col = identifier_col(line_text);
        Some((abs.to_string_lossy().into_owned(), line, col))
    }

    fn call_edges(scope: &Path, node: &NodeId, method: &str) -> Result<Vec<Edge>> {
        let Some((file, line, col)) = Self::anchor_of(scope, node) else {
            return Ok(Vec::new());
        };
        let val = crate::daemon_client::rpc(method,
            serde_json::json!({ "file": file, "line": line, "col": col }))?;
        let calls: Vec<CallInfo> = serde_json::from_value(val).unwrap_or_default();

        let base = outermost_root(scope);
        Ok(calls.into_iter().filter_map(|c| {
            // El LSP habla de (archivo, línea); el grafo, de bytes en una capa.
            // Sin convertir, estos nodos quedarían fuera de la forma canónica y
            // no participarían de contención — que es exactamente lo que hace
            // falta para descubrir qué bilink cubre al caller.
            let other = canonical_node(&base, &c.file, c.line)?;
            let (from, to) = if method == "callers" {
                (other, node.clone())
            } else {
                (node.clone(), other)
            };
            Some(Edge {
                from, to,
                kind: "call".into(),
                guarantee: Guarantee::Derived,
                provider: "lsp".into(),
                directed: true,
                r#ref: c.name,
                state: None, commit: None,
            })
        }).collect())
    }
}

impl Provider for LspProvider {
    fn name(&self) -> &str { "lsp" }

    fn kinds(&self) -> Vec<(&'static str, Guarantee)> {
        vec![("call", Guarantee::Derived)]
    }

    fn available(&self, _scope: &Path) -> Availability {
        match crate::daemon_client::rpc("ping", serde_json::json!({})) {
            Ok(v) if v == serde_json::json!("pong") => Availability::Available,
            _ => Availability::Unavailable { reason: "el daemon no responde".into() },
        }
    }

    /// Vacío a propósito: el call graph se expande, no se enumera.
    fn edges(&self, _scope: &Path) -> Result<Vec<Edge>> { Ok(Vec::new()) }

    fn edges_from(&self, scope: &Path, node: &NodeId) -> Result<Vec<Edge>> {
        let mut out = Self::call_edges(scope, node, "callers")?;
        out.extend(Self::call_edges(scope, node, "callees")?);
        Ok(out)
    }
}

/// Convierte `(archivo absoluto, línea)` a la forma canónica del grafo.
///
/// El rango es de ancho cero en el byte donde arranca la línea: el LSP no dice
/// hasta dónde llega la función, y un punto alcanza para que la contención lo
/// ubique dentro de lo que se haya declarado encima.
fn canonical_node(base: &Path, file_abs: &str, line: u32) -> Option<NodeId> {
    let abs = Path::new(file_abs);
    let rel = abs.strip_prefix(base).ok()?;

    // La capa es el ancestro de `rel` que tiene `.bilink/`; lo que sigue es el
    // path dentro de esa capa.
    let mut layer = std::path::PathBuf::new();
    let mut best: Option<(String, String)> = None;
    let comps: Vec<_> = rel.components().collect();
    for i in 0..comps.len() {
        layer.push(comps[i]);
        if base.join(&layer).join(".bilink").exists() {
            let inner: std::path::PathBuf = comps[i + 1..].iter().collect();
            best = Some((layer.display().to_string(), inner.display().to_string()));
        }
    }
    let (layer, inner) = best.unwrap_or_else(|| (".".into(), rel.display().to_string()));

    let source = std::fs::read_to_string(abs).ok()?;
    let start  = source.split_inclusive('\n').take(line as usize).map(str::len).sum::<usize>();
    Some(NodeId(format!("{layer}::{inner}#{start}~{start}")))
}

/// La raíz más externa que sea repo o capa, desde `start` hacia arriba.
pub fn outermost_root(start: &Path) -> std::path::PathBuf {
    let mut best = start.to_path_buf();
    let mut cur  = start;
    while let Some(parent) = cur.parent() {
        if parent.join(".git").exists() || parent.join(".bilink").exists() {
            best = parent.to_path_buf();
        }
        cur = parent;
    }
    best
}

/// Columna del identificador declarado en la línea.
///
/// Busca la palabra clave de declaración y devuelve el identificador que la
/// sigue. Mirar "el primer token que no sea keyword" no alcanza: `pub(crate) fn
/// foo` daría `crate`, y la lista de modificadores no tiene fin (`pub(super)`,
/// `pub(in ruta)`, anotaciones Java, decoradores). El nombre, en cambio, siempre
/// viene después de la palabra que declara.
///
/// Es una heurística y solo se usa acá. Las aristas que produce son `derived`,
/// así que fallar significa no encontrar una llamada, nunca afirmar algo falso.
fn identifier_col(line: &str) -> u32 {
    const DECL: &[&str] = &[
        "fn", "def", "function", "class", "struct", "trait", "interface", "enum",
        "impl", "type", "void",
    ];
    let mut toks = Vec::new();
    let b = line.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if !b[i].is_ascii_alphanumeric() && b[i] != b'_' { i += 1; continue; }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
        toks.push((start, &line[start..i]));
    }
    for (n, (_, tok)) in toks.iter().enumerate() {
        if DECL.contains(tok) {
            if let Some((col, _)) = toks.get(n + 1) { return *col as u32; }
        }
    }
    // Sin palabra clave de declaración —una firma Java sin tipo, por ejemplo—
    // queda el primer token, que suele ser el nombre.
    toks.first().map(|(c, _)| *c as u32).unwrap_or(0)
}

#[cfg(test)]
mod anchor_tests {
    use super::identifier_col;

    #[test]
    fn finds_the_name_after_the_declaration_keyword() {
        assert_eq!(identifier_col("fn foo() {"), 3);
        assert_eq!(identifier_col("pub fn foo() {"), 7);
    }

    #[test]
    fn survives_visibility_modifiers_with_parens() {
        // El caso que rompe "primer token que no sea keyword": daría `crate`.
        let line = "pub(crate) fn check_structural(";
        assert_eq!(&line[identifier_col(line) as usize..][..16], "check_structural");
    }

    #[test]
    fn handles_java_style_declarations() {
        let line = "    public void run() {}";
        assert_eq!(&line[identifier_col(line) as usize..][..3], "run");
    }
}

// ─── registry ─────────────────────────────────────────────────────────────────

/// El estado de un proveedor en una consulta, tal como viaja al resultado.
pub struct ProviderStatus {
    pub name:   String,
    pub status: Availability,
}

#[derive(Default)]
pub struct Registry {
    providers: Vec<Box<dyn Provider>>,
}

impl Registry {
    /// Aristas incidentes a un nodo, pidiéndoselas a los proveedores que
    /// expanden bajo demanda. Los que enumeran devuelven vacío.
    pub fn expand(&self, scope: &Path, node: &NodeId) -> Vec<Edge> {
        self.providers.iter()
            .filter(|p| p.available(scope).is_available())
            .filter_map(|p| p.edges_from(scope, node).ok())
            .flatten()
            .collect()
    }
}

impl Registry {
    pub fn new() -> Self { Self::default() }

    pub fn register(mut self, p: Box<dyn Provider>) -> Self {
        self.providers.push(p);
        self
    }

    /// Compone las aristas de los proveedores disponibles.
    ///
    /// Devuelve también el estado de **todos** los registrados, disponibles o no:
    /// un consumidor no debería tener que inferir la completitud del grafo a
    /// partir de su contenido.
    pub fn collect(&self, scope: &Path) -> (Vec<Edge>, Vec<ProviderStatus>) {
        let mut edges  = Vec::new();
        let mut status = Vec::new();

        for p in &self.providers {
            let av = p.available(scope);
            if av.is_available() {
                match p.edges(scope) {
                    Ok(mut e) => edges.append(&mut e),
                    Err(err)  => {
                        status.push(ProviderStatus {
                            name: p.name().into(),
                            status: Availability::Unavailable { reason: err.to_string() },
                        });
                        continue;
                    }
                }
            }
            status.push(ProviderStatus { name: p.name().into(), status: av });
        }

        (dedup(edges), status)
    }
}

/// Deduplica por `(from, to, kind)`, conservando la garantía más fuerte.
///
/// Dos proveedores pueden emitir la misma conexión. Que la manual gane a la
/// derivada es deliberado: la declarada tiene un estado aceptado detrás.
pub fn dedup(edges: Vec<Edge>) -> Vec<Edge> {
    use std::collections::HashMap;
    let mut best: HashMap<(String, String, String), Edge> = HashMap::new();
    for e in edges {
        let k = e.dedup_key();
        match best.get(&k) {
            Some(prev) if prev.guarantee >= e.guarantee => {}
            _ => { best.insert(k, e); }
        }
    }
    let mut out: Vec<Edge> = best.into_values().collect();
    out.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeId;

    fn edge(from: &str, to: &str, kind: &str, g: Guarantee, provider: &str) -> Edge {
        Edge {
            from: NodeId(from.into()), to: NodeId(to.into()), kind: kind.into(),
            guarantee: g, provider: provider.into(), directed: false,
            r#ref: String::new(), state: None, commit: None,
        }
    }

    #[test]
    fn dedup_keeps_the_strongest_guarantee() {
        let out = dedup(vec![
            edge(".::a#0~1", ".::b#0~1", "bilink", Guarantee::Derived,  "lsp"),
            edge(".::a#0~1", ".::b#0~1", "bilink", Guarantee::Accepted, "bilinker"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].guarantee, Guarantee::Accepted);
        assert_eq!(out[0].provider, "bilinker");
    }

    #[test]
    fn dedup_does_not_merge_different_kinds() {
        let out = dedup(vec![
            edge(".::a#0~1", ".::b#0~1", "bilink", Guarantee::Accepted, "bilinker"),
            edge(".::a#0~1", ".::b#0~1", "call",   Guarantee::Derived,  "lsp"),
        ]);
        assert_eq!(out.len(), 2, "kinds distintos son conexiones distintas");
    }

    #[test]
    fn unavailable_provider_still_reports_status() {
        struct Caido;
        impl Provider for Caido {
            fn name(&self) -> &str { "caido" }
            fn kinds(&self) -> Vec<(&'static str, Guarantee)> { vec![("call", Guarantee::Derived)] }
            fn available(&self, _: &Path) -> Availability {
                Availability::Unavailable { reason: "daemon no responde".into() }
            }
            fn edges(&self, _: &Path) -> Result<Vec<Edge>> { unreachable!("no debe consultarse") }
        }
        let (edges, status) = Registry::new().register(Box::new(Caido)).collect(Path::new("."));
        assert!(edges.is_empty());
        assert_eq!(status.len(), 1, "un proveedor caído igual aparece en el reporte");
        assert!(!status[0].status.is_available());
    }
}
