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
    /// nodo → la declaración que mandó su proveedor
    declarations: HashMap<NodeId, (usize, usize)>,
}

impl Graph {
    pub fn new(edges: Vec<Edge>, providers: Vec<ProviderStatus>) -> Self {
        let mut incident: HashMap<NodeId, Vec<usize>> = HashMap::new();
        let mut declarations = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            for n in [&e.from, &e.to] {
                incident.entry(n.clone()).or_default().push(i);
                if let Some(d) = e.declaration_of(n) { declarations.insert(n.clone(), d); }
            }
        }
        Self { edges, providers, incident, declarations }
    }

    /// Suma aristas que no estaban, sin mover los índices de las que ya estaban.
    ///
    /// Una arista cuya clave de deduplicación ya está no entra: el recorrido
    /// guarda índices, y recomponer el grafo entero los invalidaría.
    pub fn add_edges(&mut self, edges: Vec<Edge>) {
        let mut known: HashSet<(String, String, String)> =
            self.edges.iter().map(Edge::dedup_key).collect();
        for e in edges {
            if !known.insert(e.dedup_key()) { continue; }
            let i = self.edges.len();
            for n in [&e.from, &e.to] {
                self.incident.entry(n.clone()).or_default().push(i);
                if let Some(d) = e.declaration_of(n) { self.declarations.insert(n.clone(), d); }
            }
            self.edges.push(e);
        }
    }

    pub fn nodes(&self) -> BTreeSet<&NodeId> {
        self.incident.keys().collect()
    }

    /// Nodos cuyo rango cubre la posición, **del más específico al más general**.
    ///
    /// El orden importa: si dos bilinks cubren la misma posición, quien pregunta
    /// casi siempre quiere el más ajustado.
    ///
    /// Un nodo la cubre por un tramo o por su declaración, y se ordena por el largo
    /// de lo que la cubre.
    pub fn covering(&self, layer: &str, path: &str, pos: usize) -> Vec<&NodeId> {
        let mut hits: Vec<(&NodeId, usize)> = self.incident.keys()
            .filter_map(|n| n.covering_len(layer, path, pos, self.declarations.get(n).copied())
                .map(|len| (n, len)))
            .collect();
        hits.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
        hits.into_iter().map(|(n, _)| n).collect()
    }

    /// ¿`a` contiene a `b`, por sus tramos o por su declaración?
    pub fn contains(&self, a: &NodeId, b: &NodeId) -> bool {
        a.contains(b) || self.declarations.get(a).is_some_and(|&d| a.contains_within(d, b))
    }

    /// Nodos relacionados por contención con `n`, en cualquier sentido.
    ///
    /// Es el puente entre garantías: llegar a una función por una arista `call`
    /// y descubrir que hay un endpoint `accepted` que la contiene.
    pub fn related_by_containment(&self, n: &NodeId) -> Vec<&NodeId> {
        self.incident.keys()
            .filter(|m| *m != n && (self.contains(m, n) || self.contains(n, m)))
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

    /// El recorrido de impacto: expande en cada nodo y cruza las aristas `accepted`.
    ///
    /// `expand` da las aristas incidentes a un nodo que el grafo todavía no tiene,
    /// y se pide en cada nodo que sale de la cola. Una arista `accepted` se cruza,
    /// pero desde el nodo al que se llegó por ella no se toma otra: sería volver, o
    /// saltar a otro bilink del mismo fragmento. Por eso un nodo se marca visitado
    /// junto con cómo se llegó a él.
    pub fn traverse_crossing(
        &mut self, starts: &[NodeId], opts: &TraverseOpts,
        mut expand: impl FnMut(&NodeId) -> Vec<Edge>,
    ) -> Vec<usize> {
        let mut visited: HashSet<(NodeId, bool)> = HashSet::new();
        let mut reached: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<(NodeId, usize, bool)> =
            starts.iter().cloned().map(|n| (n, 0, false)).collect();

        while let Some((node, depth, crossed)) = queue.pop_front() {
            if !visited.insert((node.clone(), crossed)) { continue; }
            if opts.depth.is_some_and(|d| depth >= d) { continue; }
            self.add_edges(expand(&node));

            // Como en `traverse`, el salto por contención no consume profundidad.
            let mut frontier: Vec<NodeId> = vec![node.clone()];
            for m in self.related_by_containment(&node) {
                if !visited.contains(&(m.clone(), crossed)) { frontier.push(m.clone()); }
            }

            for current in frontier {
                visited.insert((current.clone(), crossed));
                let Some(idxs) = self.incident.get(&current) else { continue };

                for &i in idxs {
                    let e = &self.edges[i];
                    if !Self::passable(e, &current, opts.direction) { continue; }
                    let accepted = e.guarantee == Guarantee::Accepted;
                    if crossed && accepted { continue; }
                    reached.insert(i);
                    if let Some(other) = e.other(&current) {
                        if !visited.contains(&(other.clone(), accepted)) {
                            queue.push_back((other.clone(), depth + 1, accepted));
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
            state: None, commit: None, broken: false, declaration: None,
        }
    }

    fn graph(edges: Vec<Edge>) -> Graph {
        Graph::new(edges, vec![ProviderStatus {
            name: "t".into(), status: Availability::Available,
        }])
    }

    fn with_declaration(mut e: Edge, to: &str) -> Edge {
        e.declaration = Some([None, Some(to.into())]);
        e
    }

    /// Tres endpoints de un controller: la ruta de la clase en `10~30`, y cada
    /// método con sus partes y su declaración.
    fn controller() -> Graph {
        graph(vec![
            with_declaration(edge(".::front.ts#0~50",   ".::C.java#10~30,100~120,140~160", "bilink", Guarantee::Accepted, false), "95~260"),
            with_declaration(edge(".::front.ts#60~90",  ".::C.java#10~30,300~320,340~360", "bilink", Guarantee::Accepted, false), "295~460"),
            with_declaration(edge(".::front.ts#95~120", ".::C.java#10~30,500~520,540~560", "bilink", Guarantee::Accepted, false), "495~660"),
        ])
    }

    #[test]
    fn from_one_endpoint_the_others_of_the_controller_are_not_reached() {
        let g = controller();
        let reached = g.traverse(&[n(".::front.ts#0~50"), n(".::C.java#10~30,100~120,140~160")],
                                 &TraverseOpts::default());
        let refs: Vec<&str> = reached.iter().map(|&i| g.edges[i].r#ref.as_str()).collect();
        assert_eq!(refs, vec![".::front.ts#0~50->.::C.java#10~30,100~120,140~160"],
                   "compartir la ruta de la clase no es contenerse");
    }

    #[test]
    fn a_call_on_the_name_line_reaches_only_the_endpoint_of_its_method() {
        let mut edges = controller().edges;
        // El LSP nombra al método por el comienzo de la línea de su nombre, entre dos partes.
        edges.push(edge(".::C.java#330~330", ".::Service.java#0~0", "call", Guarantee::Derived, true));
        let g = graph(edges);

        let reached = g.traverse(&[n(".::Service.java#0~0")], &TraverseOpts {
            direction: Direction::Up, ..Default::default()
        });
        let bilinks: Vec<&str> = reached.iter().map(|&i| &g.edges[i])
            .filter(|e| e.kind == "bilink").map(|e| e.to.0.as_str()).collect();
        assert_eq!(bilinks, vec![".::C.java#10~30,300~320,340~360"]);
    }

    #[test]
    fn covering_a_body_line_finds_the_endpoint_of_its_method() {
        let g = controller();
        let hits: Vec<&str> = g.covering(".", "C.java", 400).iter().map(|n| n.0.as_str()).collect();
        assert_eq!(hits, vec![".::C.java#10~30,300~320,340~360"]);
        assert!(g.covering(".", "C.java", 50).is_empty(), "entre la clase y el primer método no hay nadie");
        assert_eq!(g.covering(".", "C.java", 20).len(), 3, "la ruta de la clase es de los tres");
    }

    #[test]
    fn a_span_is_more_specific_than_a_declaration() {
        let g = graph(vec![
            with_declaration(edge(".::x#0~1", ".::C.java#100~120,140~160", "bilink", Guarantee::Accepted, false), "95~260"),
            edge(".::y#0~1", ".::C.java#130~200", "bilink", Guarantee::Accepted, false),
        ]);
        let hits: Vec<&str> = g.covering(".", "C.java", 145).iter().map(|n| n.0.as_str()).collect();
        assert_eq!(hits, vec![".::C.java#100~120,140~160", ".::C.java#130~200"],
                   "lo cubre el tramo 140~160, más corto que 130~200");
        let hits: Vec<&str> = g.covering(".", "C.java", 135).iter().map(|n| n.0.as_str()).collect();
        assert_eq!(hits, vec![".::C.java#130~200", ".::C.java#100~120,140~160"],
                   "y a 135, entre las partes, lo cubre la declaración, más larga");
    }

    /// La cadena de sge, con las llamadas sólo disponibles expandiendo:
    ///
    /// servicio ←call— endpoint ⊂ bilink back —accepted— front ←call— componente
    /// ⊂ bilink de flujo —accepted— flujo
    fn impact_chain() -> (Graph, HashMap<&'static str, Vec<Edge>>) {
        let g = graph(vec![
            with_declaration(edge(".::svc.ts#500~800", ".::Ctrl.java#10~30,100~120,140~160", "bilink", Guarantee::Accepted, false), "95~260"),
            edge(".::flujos.feature#0~40", ".::comp.ts#0~900", "bilink", Guarantee::Accepted, false),
            edge(".::flujos.feature#50~90", ".::otro.ts#0~900", "bilink", Guarantee::Accepted, false),
        ]);
        let mut calls = HashMap::new();
        calls.insert(".::Impl.java#40~40", vec![
            edge(".::Ctrl.java#125~125", ".::Impl.java#40~40", "call", Guarantee::Derived, true)]);
        calls.insert(".::svc.ts#500~800", vec![
            edge(".::comp.ts#300~300", ".::svc.ts#500~800", "call", Guarantee::Derived, true)]);
        (g, calls)
    }

    fn crossing(g: &mut Graph, calls: &HashMap<&'static str, Vec<Edge>>, depth: usize) -> BTreeSet<String> {
        let opts = TraverseOpts { direction: Direction::Up, depth: Some(depth), stop_at_accepted: false };
        let reached = g.traverse_crossing(&[n(".::Impl.java#40~40")], &opts,
            |node| calls.get(node.0.as_str()).cloned().unwrap_or_default());
        reached.iter().map(|&i| g.edges[i].r#ref.clone()).collect()
    }

    #[test]
    fn crossing_reaches_the_flow_through_calls_and_bilinks() {
        let (mut g, calls) = impact_chain();
        let reached = crossing(&mut g, &calls, 6);
        assert!(reached.contains(".::flujos.feature#0~40->.::comp.ts#0~900"),
                "llega al flujo del componente: {reached:?}");
        assert!(!reached.contains(".::flujos.feature#50~90->.::otro.ts#0~900"),
                "y a ningún otro: {reached:?}");
    }

    #[test]
    fn crossing_expands_every_reached_node() {
        let (mut g, calls) = impact_chain();
        let reached = crossing(&mut g, &calls, 6);
        assert!(reached.contains(".::comp.ts#300~300->.::svc.ts#500~800"),
                "la llamada al servicio del front sólo sale expandiendo del otro lado del bilink: {reached:?}");
    }

    #[test]
    fn crossing_does_not_take_an_accepted_edge_from_a_node_reached_by_one() {
        let (g, calls) = impact_chain();
        let mut edges = g.edges;
        // Otro bilink sobre el mismo fragmento del front: tomarlo sería saltar de bilink en bilink.
        edges.push(edge(".::svc.ts#500~800", ".::otra-spec.md#0~10", "bilink", Guarantee::Accepted, false));
        let mut g = graph(edges);
        let reached = crossing(&mut g, &calls, 6);
        assert!(!reached.contains(".::svc.ts#500~800->.::otra-spec.md#0~10"), "{reached:?}");
    }

    #[test]
    fn crossing_stops_at_the_depth_cap() {
        let (mut g, calls) = impact_chain();
        let reached = crossing(&mut g, &calls, 3);
        assert!(reached.contains(".::comp.ts#300~300->.::svc.ts#500~800"), "tres pasos: {reached:?}");
        assert!(!reached.contains(".::flujos.feature#0~40->.::comp.ts#0~900"), "el cuarto no: {reached:?}");
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
