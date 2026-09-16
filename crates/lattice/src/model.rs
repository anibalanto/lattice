//! Nodos y aristas del grafo agregado.
//!
//! El punto del modelo es que **la procedencia nunca se aplana**: toda arista
//! declara de dónde viene y qué garantiza, y un consumidor puede filtrar por
//! garantía pero nunca recibe una arista sin ella.

use std::fmt;
use serde::{Deserialize, Serialize};

/// Qué se puede afirmar a partir de una arista.
///
/// El orden importa: al deduplicar, gana la garantía más fuerte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Guarantee {
    /// Escrita en el contenido, sin verificación. "Alguien escribió que se relaciona."
    Asserted,
    /// Calculada por una herramienta desde el contenido actual. Heurística.
    Derived,
    /// Declarada por un humano y verificada por su dueño. La única sobre la que
    /// se puede afirmar que hubo drift, porque es la única con estado anterior.
    Accepted,
}

impl fmt::Display for Guarantee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asserted => write!(f, "asserted"),
            Self::Derived  => write!(f, "derived"),
            Self::Accepted => write!(f, "accepted"),
        }
    }
}

impl Guarantee {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "asserted" => Some(Self::Asserted),
            "derived"  => Some(Self::Derived),
            "accepted" => Some(Self::Accepted),
            _ => None,
        }
    }
}

/// Un fragmento direccionable, en forma canónica.
///
/// `<layer-root>::<path>#<start>~<end>[,<start>~<end>…]` · `<layer-root>::<path>` · `issue:<id>` · `<uri>`
///
/// La identidad es igualdad exacta de la forma canónica. No se intenta unificar
/// nodos con rangos parecidos: la relación entre ellos se expresa por contención,
/// no por identidad, porque cualquier tolerancia sería arbitraria.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.0) }
}

impl NodeId {
    /// `(layer, path, range)` si el nodo es un fragmento de archivo.
    ///
    /// El rango va del primer tramo al último: sirve para leer el fragmento y
    /// ubicarlo, no para contener, que se hace con [`NodeId::spans`]. Los nodos
    /// `issue:` y las URIs no participan de contención y devuelven `None`.
    pub fn as_fragment(&self) -> Option<(&str, &str, Option<(usize, usize)>)> {
        let (layer, path, spans) = self.parts()?;
        let hull = spans.map(|s| (s[0].0, s[s.len() - 1].1));
        Some((layer, path, hull))
    }

    /// Los tramos de un fragmento, en orden de archivo. `None` en un archivo
    /// completo, un `issue:` o una URI.
    pub fn spans(&self) -> Option<Vec<(usize, usize)>> {
        self.parts()?.2
    }

    fn parts(&self) -> Option<(&str, &str, Option<Vec<(usize, usize)>>)> {
        if self.0.starts_with("issue:") || self.0.contains("://") { return None; }
        let (layer, rest) = self.0.split_once("::")?;
        match rest.split_once('#') {
            None => Some((layer, rest, None)),
            Some((path, ranges)) => Some((layer, path, Some(parse_spans(ranges)?))),
        }
    }

    /// ¿Este nodo contiene al otro?
    ///
    /// Misma capa, mismo archivo, y cada tramo del otro adentro de alguno de los
    /// suyos. Es la operación que permite cruzar de una garantía a otra:
    /// preguntar si hay un endpoint aceptado que cubre la función que el LSP
    /// acaba de señalar. Un punto entre dos tramos no está contenido.
    pub fn contains(&self, other: &NodeId) -> bool {
        let (Some((l1, p1, r1)), Some((l2, p2, r2))) =
            (self.parts(), other.parts()) else { return false };
        if l1 != l2 || p1 != p2 { return false; }
        match (r1, r2) {
            // Un nodo de archivo completo contiene a todo fragmento de ese archivo.
            // Es lo que permite que un link markdown —que apunta al archivo—
            // alcance los bilinks declarados sobre sus fragmentos.
            (None, _) => self != other,
            (Some(_), None) => false,
            (Some(outer), Some(inner)) => inner.iter()
                .all(|&t| outer.iter().any(|&o| within(o, t))),
        }
    }

    /// ¿La declaración de este nodo contiene al otro?
    ///
    /// Un fragmento de varias partes deja afuera lo que hay entre ellas: en un
    /// endpoint de Spring, el nombre y el cuerpo del método. La declaración es el
    /// tramo que las envuelve, y alcanza a lo que cae ahí sin caer en una parte.
    pub fn contains_within(&self, declaration: (usize, usize), other: &NodeId) -> bool {
        let (Some((l1, p1, _)), Some((l2, p2, Some(inner)))) =
            (self.parts(), other.parts()) else { return false };
        l1 == l2 && p1 == p2 && inner.iter().all(|&t| within(declaration, t))
    }

    /// ¿Algún tramo de este nodo cubre `pos` (byte absoluto en `path` de `layer`)?
    pub fn covers(&self, layer: &str, path: &str, pos: usize) -> bool {
        self.covering_len(layer, path, pos, None).is_some()
    }

    /// El largo de lo que cubre `pos`: el tramo que la contiene o, si ninguno, la
    /// declaración. Es lo que ordena de más específico a más general.
    pub fn covering_len(
        &self, layer: &str, path: &str, pos: usize, declaration: Option<(usize, usize)>,
    ) -> Option<usize> {
        let (l, p, Some(spans)) = self.parts()? else { return None };
        if l != layer || p != path { return None; }
        let covers = |(s, e): (usize, usize)| s <= pos && pos < e;
        spans.into_iter().find(|&t| covers(t))
            .or(declaration.filter(|&d| covers(d)))
            .map(|(s, e)| e - s)
    }
}

/// `a~b,c~d`, ordenados. `None` si alguno no es `inicio~fin`.
pub fn parse_spans(ranges: &str) -> Option<Vec<(usize, usize)>> {
    let mut spans = ranges.split(',').map(parse_span).collect::<Option<Vec<_>>>()?;
    spans.sort();
    Some(spans)
}

fn parse_span(range: &str) -> Option<(usize, usize)> {
    let (a, b) = range.split_once('~')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn within((s1, e1): (usize, usize), (s2, e2): (usize, usize)) -> bool {
    s1 <= s2 && e2 <= e1
}

/// Una conexión entre dos nodos, con su procedencia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to:   NodeId,
    /// `bilink` · `governs` · `task` · `call` · `doclink` · `external`
    pub kind: String,
    pub guarantee: Guarantee,
    pub provider:  String,
    /// Si el orden `from → to` tiene significado semántico.
    #[serde(default)]
    pub directed: bool,
    /// Identificador en la fuente: UUID del bilink, símbolo LSP, path + anchor.
    #[serde(default)]
    pub r#ref: String,
    /// Estado que reporta el proveedor. Solo para `accepted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<[String; 2]>,
    /// Commit en que se aceptó cada extremo. Baseline de `git log <commit>..HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<[String; 2]>,
    /// El destino no se pudo resolver.
    ///
    /// No usa `state`, que es el estado que reporta el dueño de una arista
    /// `accepted`. Un link muerto en un documento es información —una arista que
    /// existe y apunta a la nada— y no un estado de aceptación.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub broken: bool,
    /// El tramo que envuelve a cada extremo de varias partes, `inicio~fin`.
    ///
    /// No es parte de la identidad del nodo: la manda el proveedor para que la
    /// contención alcance lo que cae entre las partes, como el cuerpo de un método
    /// cuya captura es sólo la firma.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<[Option<String>; 2]>,
}

impl Edge {
    /// Clave de deduplicación: dos proveedores pueden emitir la misma conexión.
    ///
    /// Los extremos se ordenan **solo si la arista no es dirigida**. En una
    /// dirigida el sentido es parte del hecho: dos documentos que se referencian
    /// mutuamente son dos links, no uno, y `a llama a b` no es `b llama a a`.
    pub fn dedup_key(&self) -> (String, String, String) {
        let (a, b) = if self.directed || self.from <= self.to {
            (self.from.0.clone(), self.to.0.clone())
        } else {
            (self.to.0.clone(), self.from.0.clone())
        };
        (a, b, self.kind.clone())
    }

    /// La declaración del extremo `n`, si el proveedor la mandó.
    pub fn declaration_of(&self, n: &NodeId) -> Option<(usize, usize)> {
        let [from, to] = self.declaration.as_ref()?;
        let d = if *n == self.from { from } else if *n == self.to { to } else { return None };
        parse_span(d.as_deref()?)
    }

    pub fn other(&self, n: &NodeId) -> Option<&NodeId> {
        if *n == self.from { Some(&self.to) } else if *n == self.to { Some(&self.from) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> NodeId { NodeId(s.into()) }

    #[test]
    fn parses_a_fragment_node() {
        let id = n(".stratum/impl::src/a.rs#10~50");
        let (layer, path, range) = id.as_fragment().unwrap();
        assert_eq!((layer, path, range), (".stratum/impl", "src/a.rs", Some((10, 50))));
    }

    #[test]
    fn parses_a_whole_file_node() {
        let id = n(".::docs/a.md");
        let (_, path, range) = id.as_fragment().unwrap();
        assert_eq!((path, range), ("docs/a.md", None));
    }

    #[test]
    fn task_and_uri_nodes_have_no_fragment() {
        assert!(n("issue:3a").as_fragment().is_none());
        assert!(n("https://example.com/x").as_fragment().is_none());
    }

    #[test]
    fn whole_file_contains_its_fragments() {
        let file = n(".::a.rs");
        assert!(file.contains(&n(".::a.rs#10~50")));
        assert!(!file.contains(&n(".::b.rs#10~50")), "otro archivo");
        assert!(!file.contains(&file), "no se contiene a sí mismo");
        assert!(!n(".::a.rs#10~50").contains(&file), "un fragmento no contiene al archivo");
    }

    #[test]
    fn containment_requires_same_layer_and_file() {
        let outer = n(".::a.rs#0~100");
        assert!(outer.contains(&n(".::a.rs#10~50")));
        assert!(!outer.contains(&n(".::b.rs#10~50")), "otro archivo");
        assert!(!outer.contains(&n("impl::a.rs#10~50")), "otra capa");
        assert!(!outer.contains(&n(".::a.rs#50~150")), "se sale del rango");
    }

    #[test]
    fn covers_is_half_open() {
        let node = n(".::a.rs#10~20");
        assert!(node.covers(".", "a.rs", 10));
        assert!(node.covers(".", "a.rs", 19));
        assert!(!node.covers(".", "a.rs", 20), "el fin es exclusivo");
        assert!(!node.covers(".", "a.rs", 9));
    }

    #[test]
    fn a_fragment_of_several_spans_parses_each_one() {
        let id = n(".::C.java#10~20,50~60,70~90");
        assert_eq!(id.spans(), Some(vec![(10, 20), (50, 60), (70, 90)]));
        let (_, path, hull) = id.as_fragment().unwrap();
        assert_eq!((path, hull), ("C.java", Some((10, 90))), "el rango de lectura va del primero al último");
    }

    #[test]
    fn a_point_between_two_spans_is_not_contained() {
        let endpoint = n(".::C.java#10~20,50~60");
        assert!(endpoint.contains(&n(".::C.java#15~15")));
        assert!(endpoint.contains(&n(".::C.java#55~55")));
        assert!(!endpoint.contains(&n(".::C.java#30~30")), "entre dos tramos no hay fragmento");
        assert!(!endpoint.covers(".", "C.java", 30));
        assert!(endpoint.covers(".", "C.java", 55));
    }

    #[test]
    fn every_span_of_the_contained_node_falls_in_some_span() {
        let outer = n(".::C.java#0~30,50~90");
        assert!(outer.contains(&n(".::C.java#10~20,60~70")));
        assert!(!outer.contains(&n(".::C.java#10~20,40~45")), "un tramo afuera basta para no contener");
        assert!(!outer.contains(&n(".::C.java#10~60")), "un tramo que cruza el hueco no está contenido");
    }

    #[test]
    fn two_endpoints_of_the_same_controller_do_not_contain_each_other() {
        // Los dos empiezan en la anotación de ruta de la clase.
        let a = n(".::C.java#10~30,100~200");
        let b = n(".::C.java#10~30,300~400");
        assert!(!a.contains(&b) && !b.contains(&a));
    }

    #[test]
    fn a_declaration_contains_what_falls_inside_it() {
        let endpoint = n(".::C.java#10~30,100~120,140~160");
        let method = (95, 260);
        assert!(endpoint.contains_within(method, &n(".::C.java#125~125")), "el nombre, entre dos partes");
        assert!(endpoint.contains_within(method, &n(".::C.java#200~200")), "una línea del cuerpo");
        assert!(!endpoint.contains_within(method, &n(".::C.java#50~50")), "entre la clase y el método");
        assert!(!endpoint.contains_within(method, &n(".::C.java#10~30,300~400")),
                "otro endpoint comparte la clase, que está fuera del método");
        assert!(!endpoint.contains_within(method, &n(".::D.java#200~200")), "otro archivo");
        assert!(!endpoint.contains_within(method, &n(".::C.java")), "un archivo entero no cae en una declaración");
    }

    #[test]
    fn directed_edges_keep_their_direction_when_deduping() {
        let mk = |a: &str, b: &str| Edge {
            from: n(a), to: n(b), kind: "doclink".into(),
            guarantee: Guarantee::Asserted, provider: "doc".into(),
            directed: true, r#ref: "x".into(), state: None, commit: None, broken: false, declaration: None,
        };
        assert_ne!(mk(".::a.md", ".::b.md").dedup_key(),
                   mk(".::b.md", ".::a.md").dedup_key(),
                   "dos documentos que se referencian mutuamente son dos links");
    }

    #[test]
    fn dedup_key_is_order_independent() {
        let mk = |a: &str, b: &str| Edge {
            from: n(a), to: n(b), kind: "bilink".into(),
            guarantee: Guarantee::Accepted, provider: "bilinker".into(),
            directed: false, r#ref: "x".into(), state: None, commit: None, broken: false, declaration: None,
        };
        assert_eq!(mk(".::a#0~1", ".::b#0~1").dedup_key(),
                   mk(".::b#0~1", ".::a#0~1").dedup_key());
    }

    #[test]
    fn guarantee_orders_by_strength() {
        assert!(Guarantee::Accepted > Guarantee::Derived);
        assert!(Guarantee::Derived  > Guarantee::Asserted);
    }
}
