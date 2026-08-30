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
/// `<layer-root>::<path>#<start>~<end>` · `<layer-root>::<path>` · `issue:<id>` · `<uri>`
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
    /// Los nodos `issue:` y las URIs no participan de contención y devuelven `None`.
    pub fn as_fragment(&self) -> Option<(&str, &str, Option<(usize, usize)>)> {
        if self.0.starts_with("issue:") || self.0.contains("://") { return None; }
        let (layer, rest) = self.0.split_once("::")?;
        match rest.split_once('#') {
            None => Some((layer, rest, None)),
            Some((path, range)) => {
                let (a, b) = range.split_once('~')?;
                Some((layer, path, Some((a.parse().ok()?, b.parse().ok()?))))
            }
        }
    }

    /// ¿Este nodo contiene al otro?
    ///
    /// Misma capa, mismo archivo, rango que incluye. Es la operación que permite
    /// cruzar de una garantía a otra: preguntar si hay un endpoint aceptado que
    /// cubre la función que el LSP acaba de señalar.
    pub fn contains(&self, other: &NodeId) -> bool {
        let (Some((l1, p1, r1)), Some((l2, p2, _))) =
            (self.as_fragment(), other.as_fragment()) else { return false };
        if l1 != l2 || p1 != p2 { return false; }
        match (r1, other.as_fragment().and_then(|(_, _, r)| r)) {
            // Un nodo de archivo completo contiene a todo fragmento de ese archivo.
            // Es lo que permite que un link markdown —que apunta al archivo—
            // alcance los bilinks declarados sobre sus fragmentos.
            (None, _) => self != other,
            (Some(_), None) => false,
            (Some((s1, e1)), Some((s2, e2))) => s1 <= s2 && e2 <= e1,
        }
    }

    /// ¿El rango de este nodo cubre `pos` (byte absoluto en `path` de `layer`)?
    pub fn covers(&self, layer: &str, path: &str, pos: usize) -> bool {
        match self.as_fragment() {
            Some((l, p, Some((s, e)))) => l == layer && p == path && s <= pos && pos < e,
            _ => false,
        }
    }

    /// Largo del fragmento, para ordenar de más específico a más general.
    pub fn span(&self) -> Option<usize> {
        match self.as_fragment() {
            Some((_, _, Some((s, e)))) => Some(e.saturating_sub(s)),
            _ => None,
        }
    }
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
    fn directed_edges_keep_their_direction_when_deduping() {
        let mk = |a: &str, b: &str| Edge {
            from: n(a), to: n(b), kind: "doclink".into(),
            guarantee: Guarantee::Asserted, provider: "doc".into(),
            directed: true, r#ref: "x".into(), state: None, commit: None, broken: false,
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
            directed: false, r#ref: "x".into(), state: None, commit: None, broken: false,
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
