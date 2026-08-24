//! Proveedores de aristas.
//!
//! Lattice no produce aristas: descubre proveedores, les pregunta por las suyas
//! en un scope, y compone. La contención sí la calcula lattice, y no es una
//! arista.

use std::path::Path;
use anyhow::Result;
use crate::model::{Edge, Guarantee};

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
}

// ─── proveedor bilink ─────────────────────────────────────────────────────────

/// Toma las aristas de bilinker vía `bilinker graph --format json`.
///
/// Bilinker entrega los nodos en forma canónica y con la topología de cadena ya
/// resuelta: componer eso con aristas ajenas es tarea de lattice, pero resolver
/// una cadena es conocimiento de su formato.
pub struct BilinkProvider {
    pub binary: String,
}

impl Default for BilinkProvider {
    fn default() -> Self { Self { binary: "bilinker".into() } }
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
        let out = std::process::Command::new(&self.binary)
            .args(["graph", ".", "--format", "json"])
            .current_dir(scope)
            .output()?;
        if !out.status.success() {
            anyhow::bail!("bilinker graph falló: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(serde_json::from_slice(&out.stdout)?)
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
