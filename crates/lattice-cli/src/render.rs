//! Formatos de salida de `lattice graph`.
//!
//! Todos muestran `kind` y garantía de cada arista: un consumidor tiene que
//! poder distinguir de un vistazo qué parte del resultado es verificable y qué
//! parte es inferencia de un language server.

use std::collections::BTreeMap;
use lattice::model::{Edge, Guarantee, NodeId};
use lattice::provider::{Availability, ProviderStatus};

/// Símbolo de la relación: las no dirigidas se leen en ambos sentidos.
fn arrow(e: &Edge) -> &'static str {
    if e.directed { "↑" } else { "↕" }
}

fn short(r: &str) -> &str { &r[..8.min(r.len())] }

/// El estado, si la arista lo trae. Solo las `accepted` lo tienen.
fn state_tag(e: &Edge) -> String {
    match &e.state {
        Some([a, b]) if a == b => format!("  [{a}]"),
        Some([a, b])           => format!("  [{a} ↔ {b}]"),
        None                   => String::new(),
    }
}

pub fn providers_line(status: &[ProviderStatus]) -> String {
    let parts: Vec<String> = status.iter().map(|s| match &s.status {
        Availability::Available          => format!("{} OK", s.name),
        Availability::Degraded { .. }    => format!("{} incompleto", s.name),
        Availability::Unavailable { .. } => format!("{} no disponible", s.name),
    }).collect();
    format!("proveedores: {}", parts.join(" · "))
}

// ─── tree ─────────────────────────────────────────────────────────────────────

pub fn tree(edges: &[Edge]) {
    for e in edges {
        println!("◆ {}{}", short(&e.r#ref), state_tag(e));
        println!("  {}", e.from);
        let commit = e.commit.as_ref()
            .map(|[c, _]| format!(", commit {}", short(c))).unwrap_or_default();
        println!("  {} {} ({}{commit})", arrow(e), e.kind, e.guarantee);
        println!("  {}", e.to);
        println!();
    }
}

// ─── flat ─────────────────────────────────────────────────────────────────────

/// Una línea por arista, para scripting.
pub fn flat(edges: &[Edge]) {
    for e in edges {
        let st = e.state.as_ref()
            .map(|[a, b]| format!("{a}↔{b}")).unwrap_or_else(|| "-".into());
        println!("{}\t{}\t{}\t{}\t{}\t{}",
                 short(&e.r#ref), e.kind, e.guarantee, st, e.from, e.to);
    }
}

// ─── json ─────────────────────────────────────────────────────────────────────

/// `providers` va **primero y siempre**, incluso cuando todos respondieron: un
/// consumidor no debería tener que inferir la completitud del grafo a partir de
/// su contenido.
pub fn json(edges: &[Edge], status: &[ProviderStatus]) -> anyhow::Result<()> {
    let providers: Vec<serde_json::Value> = status.iter().map(|s| match &s.status {
        Availability::Available =>
            serde_json::json!({"name": s.name, "status": "available"}),
        Availability::Degraded { reason } =>
            serde_json::json!({"name": s.name, "status": "degraded", "reason": reason}),
        Availability::Unavailable { reason } =>
            serde_json::json!({"name": s.name, "status": "unavailable", "reason": reason}),
    }).collect();

    let mut ids: Vec<&NodeId> = edges.iter().flat_map(|e| [&e.from, &e.to]).collect();
    ids.sort();
    ids.dedup();

    let nodes: Vec<serde_json::Value> = ids.iter().map(|n| {
        match n.as_fragment() {
            Some((layer, path, _)) =>
                serde_json::json!({"id": n.0, "layer": layer, "path": path}),
            None => serde_json::json!({"id": n.0}),
        }
    }).collect();

    let mut out = serde_json::Map::new();
    out.insert("providers".into(), serde_json::Value::Array(providers));
    out.insert("nodes".into(),     serde_json::Value::Array(nodes));
    out.insert("edges".into(),     serde_json::to_value(edges)?);
    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out))?);
    Ok(())
}

// ─── dot ──────────────────────────────────────────────────────────────────────

fn dot_id(n: &NodeId) -> String {
    n.0.replace(['"', '\\'], "_")
}

fn dot_label(n: &NodeId) -> String {
    match n.as_fragment() {
        Some((_, path, Some((s, _)))) => format!("{path}\\n@{s}"),
        Some((_, path, None))         => path.to_string(),
        None                          => n.0.clone(),
    }
}

/// Graphviz, con las capas como clusters.
///
/// La garantía se codifica en el trazo: continuo para lo verificado, punteado
/// para lo inferido. Que se distingan a simple vista es el punto — un grafo que
/// mezcla ambas sin marcarlas induce a confiar en la inferencia.
pub fn dot(edges: &[Edge]) {
    let mut by_layer: BTreeMap<&str, Vec<&NodeId>> = BTreeMap::new();
    for n in edges.iter().flat_map(|e| [&e.from, &e.to]) {
        let layer = n.as_fragment().map(|(l, _, _)| l).unwrap_or("externo");
        by_layer.entry(layer).or_default().push(n);
    }

    println!("digraph lattice {{");
    println!("  graph [rankdir=LR newrank=true];");
    println!("  node  [shape=box fontname=\"monospace\" fontsize=10];");

    for (i, (layer, nodes)) in by_layer.iter().enumerate() {
        println!("  subgraph cluster_{i} {{");
        println!("    label=\"{layer}\";");
        let mut seen = std::collections::HashSet::new();
        for n in nodes {
            if seen.insert(&n.0) {
                println!("    \"{}\" [label=\"{}\"];", dot_id(n), dot_label(n));
            }
        }
        println!("  }}");
    }

    for e in edges {
        let (style, dir) = match e.guarantee {
            Guarantee::Accepted => ("solid",  if e.directed { "forward" } else { "none" }),
            Guarantee::Derived  => ("dashed", "forward"),
            Guarantee::Asserted => ("dotted", "forward"),
        };
        let label = e.state.as_ref()
            .map(|[a, b]| format!("{}\\n{a}↔{b}", e.kind))
            .unwrap_or_else(|| e.kind.clone());
        println!("  \"{}\" -> \"{}\" [label=\"{label}\" style={style} dir={dir}];",
                 dot_id(&e.from), dot_id(&e.to));
    }
    println!("}}");
}
