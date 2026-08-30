//! Lattice — el grafo agregado de las conexiones del proyecto.
//!
//! El modelo de nodos y aristas, el registry de proveedores, el traversal y el
//! cliente del daemon LSP. La especificación vive en `subsystems/lattice/`.
//!
//! Lo que falta no es esto: el daemon con sus language servers vive en el crate
//! `lattice-daemon`, y la CLI en `lattice-cli`.

pub mod daemon_client;
pub mod graph;
pub mod model;
pub mod provider;
