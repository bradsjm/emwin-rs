//! Live CLI modes backed by real EMWIN or Weather Wire connections.
//!
//! This subtree adapts the shared ingest runtime into CLI behaviors such as optional file
//! persistence and the HTTP/SSE server mode. It owns command-level orchestration and leaves
//! protocol details in `emwin-protocol`.

pub mod server;
mod server_support;
