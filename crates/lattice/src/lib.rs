//! Lattice — el grafo agregado de las conexiones del proyecto.
//!
//! Por ahora solo el cliente del daemon LSP, migrado desde bilinker. El modelo
//! de nodos y aristas, el registry de proveedores y el traversal están
//! especificados en `subsystems/lattice/` y todavía no implementados.

pub mod daemon_client;
pub mod model;
pub mod provider;
