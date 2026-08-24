//! El grafo compuesto y su traversal.
//!
//! La contención es lo que hace útil a este traversal: sin ella el grafo son dos
//! grafos disjuntos —lo que bilinker declaró por un lado, lo que el LSP infiere
//! por el otro— porque dos proveedores nunca nombran el mismo fragmento con el
//! mismo rango.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use crate::model::{Edge, Guarantee, NodeId};
use crate::provider::ProviderStatus;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    /// Sigue las aristas dirigidas en sentido inverso: de callee a caller.
    Up,
    /// En sentido directo: de caller a callee.
    Down,
    Both,
}

pub struct TraverseOpts {
    pub direction: Direction,
    pub depth:     Option<usize>,
    /// Cortar una rama al alcanzar una arista `accepted`.
    ///
    /// Sin este corte el traversal seguiría más allá del límite del subgrafo que
    /// alguien documentó, que es justamente el borde que interesa encontrar.
    pub stop_at_accepted: bool,
}

impl Default for TraverseOpts {
    fn default() -> Self {
        Self { direction: Direction::Both, depth: None, stop_at_accepted: true }
    }
}

pub struct Graph {
    pub edges:     Vec<Edge>,
    pub providers: Vec<ProviderStatus>,
    /// nodo → índices de aristas incidentes
    incident: HashMap<NodeId, Vec<usize>>,
}

impl Graph {
    pub fn new(edges: Vec<Edge>, providers: Vec<ProviderStatus>) -> Self {
        let mut incident: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            incident.entry(e.from.clone()).or_default().push(i);
            incident.entry(e.to.clone()).or_default().push(i);
        }
        Self { edges, providers, incident }
    }

    pub fn nodes(&self) -> BTreeSet<&NodeId> {
        self.incident.keys().collect()
    }

    /// Nodos cuyo rango cubre la posición, **del más específico al más general**.
    ///
    /// El orden importa: si dos bilinks cubren la misma posición, quien pregunta
    /// casi siempre quiere el más ajustado.
    pub fn covering(&self, layer: &str, path: &str, pos: usize) -> Vec<&NodeId> {
        let mut hits: Vec<&NodeId> = self.incident.keys()
            .filter(|n| n.covers(layer, path, pos))
            .collect();
        hits.sort_by_key(|n| n.span().unwrap_or(usize::MAX));
        hits
    }

    /// Nodos relacionados por contención con `n`, en cualquier sentido.
    ///
    /// Es el puente entre garantías: llegar a una función por una arista `call`
    /// y descubrir que hay un endpoint `accepted` que la contiene.
    pub fn related_by_containment(&self, n: &NodeId) -> Vec<&NodeId> {
        self.incident.keys()
            .filter(|m| *m != n && (m.contains(n) || n.contains(m)))
            .collect()
    }

    /// ¿Se puede recorrer esta arista desde `from` en esta dirección?
    ///
    /// Las no dirigidas se recorren siempre en ambos sentidos; `--up` y `--down`
    /// solo afectan a las dirigidas.
    fn passable(e: &Edge, from: &NodeId, dir: Direction) -> bool {
        if !e.directed || dir == Direction::Both { return true; }
        match dir {
            Direction::Down => *from == e.from,
            Direction::Up   => *from == e.to,
            Direction::Both => true,
        }
    }

    /// BFS desde `starts`. Devuelve los índices de las aristas alcanzadas.
    pub fn traverse(&self, starts: &[NodeId], opts: &TraverseOpts) -> Vec<usize> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut reached: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<(NodeId, usize)> =
            starts.iter().cloned().map(|n| (n, 0)).collect();

        while let Some((node, depth)) = queue.pop_front() {
            if !visited.insert(node.clone()) { continue; }

            // El salto por contención no consume profundidad: no es un paso del
            // grafo, es reconocer que dos proveedores nombraron lo mismo.
            let mut frontier: Vec<NodeId> = vec![node.clone()];
            for m in self.related_by_containment(&node) {
                if !visited.contains(m) { frontier.push(m.clone()); }
            }

            for current in frontier {
                visited.insert(current.clone());
                let Some(idxs) = self.incident.get(&current) else { continue };

                for &i in idxs {
                    let e = &self.edges[i];
                    if !Self::passable(e, &current, opts.direction) { continue; }

                    // Una arista que sale del nodo actual está a `depth + 1`
                    // del inicio, así que el límite se evalúa antes de contarla.
                    if opts.depth.is_some_and(|d| depth + 1 > d) { continue; }
                    reached.insert(i);

                    // Corte: alcanzamos el borde de lo documentado.
                    if opts.stop_at_accepted && depth > 0 && e.guarantee == Guarantee::Accepted {
                        continue;
                    }
                    if let Some(other) = e.other(&current) {
                        if !visited.contains(other) {
                            queue.push_back((other.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        reached.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Availability;

    fn n(s: &str) -> NodeId { NodeId(s.into()) }

    fn edge(from: &str, to: &str, kind: &str, g: Guarantee, directed: bool) -> Edge {
        Edge {
            from: n(from), to: n(to), kind: kind.into(), guarantee: g,
            provider: "t".into(), directed, r#ref: format!("{from}->{to}"),
            state: None, commit: None,
        }
    }

    fn graph(edges: Vec<Edge>) -> Graph {
        Graph::new(edges, vec![ProviderStatus {
            name: "t".into(), status: Availability::Available,
        }])
    }

    #[test]
    fn covering_orders_from_most_specific() {
        let g = graph(vec![
            edge(".::a.rs#0~100", ".::x", "bilink", Guarantee::Accepted, false),
            edge(".::a.rs#10~20", ".::y", "bilink", Guarantee::Accepted, false),
        ]);
        let hits: Vec<String> = g.covering(".", "a.rs", 15).iter().map(|n| n.0.clone()).collect();
        assert_eq!(hits, vec![".::a.rs#10~20", ".::a.rs#0~100"]);
    }

    #[test]
    fn containment_bridges_a_call_edge_to_an_accepted_one() {
        // El LSP ve una llamada a una función; un bilink cubre esa función.
        let g = graph(vec![
            edge(".::a.rs#50~60", ".::a.rs#200~210", "call", Guarantee::Derived, true),
            edge(".::a.rs#0~100", ".::spec.md#0~10", "bilink", Guarantee::Accepted, false),
        ]);

        // Partiendo del callee, subir por la llamada y cruzar a la spec.
        let idx = g.traverse(&[n(".::a.rs#200~210")], &TraverseOpts {
            direction: Direction::Up, depth: None, stop_at_accepted: true,
        });
        let kinds: BTreeSet<&str> = idx.iter().map(|&i| g.edges[i].kind.as_str()).collect();
        assert!(kinds.contains("call"),   "debería seguir la llamada hacia arriba");
        assert!(kinds.contains("bilink"), "y cruzar por contención a la spec");
    }

    #[test]
    fn direction_filters_directed_edges_only() {
        let g = graph(vec![
            edge(".::a#0~1", ".::b#0~1", "call",   Guarantee::Derived,  true),
            edge(".::a#0~1", ".::c#0~1", "bilink", Guarantee::Accepted, false),
        ]);

        // Desde `a` hacia arriba: la llamada sale de `a`, así que no se recorre.
        let up: BTreeSet<&str> = g.traverse(&[n(".::a#0~1")], &TraverseOpts {
            direction: Direction::Up, ..Default::default()
        }).iter().map(|&i| g.edges[i].kind.as_str()).collect();
        assert!(!up.contains("call"), "una arista dirigida no se recorre a contramano");
        assert!(up.contains("bilink"), "las no dirigidas se recorren siempre");
    }

    #[test]
    fn stops_at_the_first_accepted_edge() {
        // a --call--> b --bilink--> spec --bilink--> mas_alla
        let g = graph(vec![
            edge(".::a#0~1",    ".::b#0~1",    "call",   Guarantee::Derived,  true),
            edge(".::b#0~1",    ".::spec#0~1", "bilink", Guarantee::Accepted, false),
            edge(".::spec#0~1", ".::mas#0~1",  "bilink", Guarantee::Accepted, false),
        ]);
        let reached: BTreeSet<String> = g.traverse(&[n(".::a#0~1")], &TraverseOpts::default())
            .iter().map(|&i| g.edges[i].r#ref.clone()).collect();

        assert!(reached.contains(".::b#0~1->.::spec#0~1"), "alcanza el bilink");
        assert!(!reached.contains(".::spec#0~1->.::mas#0~1"),
                "y corta ahí: más allá está fuera del subgrafo documentado");
    }

    #[test]
    fn depth_limits_the_walk() {
        let g = graph(vec![
            edge(".::a#0~1", ".::b#0~1", "call", Guarantee::Derived, false),
            edge(".::b#0~1", ".::c#0~1", "call", Guarantee::Derived, false),
        ]);
        let reached = g.traverse(&[n(".::a#0~1")], &TraverseOpts {
            depth: Some(1), ..Default::default()
        });
        assert_eq!(reached.len(), 1, "con --depth 1 solo el primer salto");
    }

    #[test]
    fn cycles_do_not_hang() {
        let g = graph(vec![
            edge(".::a#0~1", ".::b#0~1", "call", Guarantee::Derived, false),
            edge(".::b#0~1", ".::a#0~1", "call", Guarantee::Derived, false),
        ]);
        assert_eq!(g.traverse(&[n(".::a#0~1")], &TraverseOpts::default()).len(), 2);
    }
}
