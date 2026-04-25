//! Incus REST API client.
//!
//! Communicates with the local Incus daemon over its Unix socket
//! (`/var/lib/incus/unix.socket`). All operations needed for the build
//! pipeline are covered:
//!
//! - Create / start / stop / delete ephemeral containers
//! - Execute commands inside a running container
//! - Export the container rootfs as a tar stream
//! - Take and delete snapshots (for layered OCI builds)

pub mod api;
pub mod bootstrap;
pub mod client;
pub mod exec;
pub mod export;
pub mod ws_exec;

pub use client::IncusClient;
