//! LSP client for Aurora — connection pool, JSON-RPC transport, debounced routing.

pub mod bridge;
pub mod client;
pub mod connection;
pub mod pool;
pub mod transport;

pub use bridge::LspBridge;
pub use client::{LspClient, LspServerConfig};
pub use connection::{ConnectionError, LspConnection};
pub use pool::{ConnectionKey, ConnectionPool};
