//! Lattice — el grafo agregado de las conexiones del proyecto.
//!
//! El modelo de nodos y aristas, el registry de proveedores y el traversal. La
//! especificación vive en `subsystems/lattice/`.
//!
//! Lo que falta no es esto: la CLI vive en `lattice-cli`, y los language servers en
//! [`lspd`](https://github.com/anibalanto/lspd) — **que no es de lattice**. Su crate
//! nunca dependió de éste, y desde que bilinker también le pregunta, tenerlo adentro
//! se leería como una inversión de capas que no está ocurriendo. Se le habla con
//! `lspd_client`, igual que le hablaría cualquiera.

pub mod graph;
pub mod model;
pub mod provider;
