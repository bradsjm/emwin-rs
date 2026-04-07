//! Decode and encode the QBT/EMWIN wire protocol.
//!
//! This module holds the protocol-facing pieces that have to agree on framing, checksums,
//! compression, and server-list semantics. The decoder keeps wire recovery state local so the
//! rest of the runtime can work with typed events instead of partial transport details.
//!
//! Module ownership is split by responsibility:
//! - `auth`: downstream relay authentication message formatting and parsing
//! - `checksum`: canonical QBT checksum calculation
//! - `codec`: framed wire decode and encode state machines
//! - `compression`: V2 compression handling helpers
//! - `model`: typed protocol events, warnings, segments, and server-list models
//! - `server_list`: textual server-list parsing
//! - `server_list_wire`: relay-facing server-list frame generation and scanning

pub mod auth;
pub mod checksum;
pub mod codec;
pub mod compression;
pub mod model;
pub mod server_list;
pub mod server_list_wire;
