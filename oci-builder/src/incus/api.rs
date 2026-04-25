//! Incus REST API request/response types.
//!
//! Only the subset of the API used by the build pipeline is modelled here.
//! Full API reference: https://linuxcontainers.org/incus/docs/main/rest-api/

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Generic response envelope ─────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Response<T> {
    #[serde(rename = "type")]
    pub response_type: String,
    pub status: String,
    pub status_code: u16,
    pub metadata: T,
}

#[derive(Debug, Deserialize)]
pub struct Operation {
    pub id: String,
    pub status: String,
    pub status_code: u16,
    #[serde(default)]
    pub err: String,
    /// Arbitrary metadata returned by the operation on completion.
    /// For exec: contains `"return"` (exit code) and `"output"` (log URLs).
    /// For image import: contains `"fingerprint"`.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

// ── Instance creation ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct InstanceSource {
    #[serde(rename = "type")]
    pub source_type: String,
    /// e.g. "images:ubuntu/noble" or a local image fingerprint
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alias: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    /// Remote server URL for pulling images (e.g. "https://images.linuxcontainers.org")
    #[serde(skip_serializing_if = "String::is_empty")]
    pub server: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub protocol: String,
}

impl InstanceSource {
    /// Pull a named image from the default Incus image server.
    pub fn from_remote_alias(alias: &str) -> Self {
        // Strip the "images:" prefix if present — the server field handles routing.
        let (server, alias) = if let Some(a) = alias.strip_prefix("images:") {
            (
                "https://images.linuxcontainers.org".to_string(),
                a.to_string(),
            )
        } else {
            (String::new(), alias.to_string())
        };
        let protocol = if server.is_empty() {
            String::new()
        } else {
            "simplestreams".to_string()
        };
        Self {
            source_type: "image".to_string(),
            alias,
            fingerprint: String::new(),
            server,
            protocol,
        }
    }

    /// Use a locally imported image by alias (no remote server needed).
    pub fn from_local_alias(alias: &str) -> Self {
        Self {
            source_type: "image".to_string(),
            alias: alias.to_string(),
            fingerprint: String::new(),
            server: String::new(),
            protocol: String::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InstanceConfig {
    /// Instance name — must be unique on the daemon.
    pub name: String,
    /// "container" or "virtual-machine"
    #[serde(rename = "type")]
    pub instance_type: String,
    pub source: InstanceSource,
    /// Ephemeral instances are deleted automatically when stopped.
    pub ephemeral: bool,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config: HashMap<String, String>,
    /// Target architecture for the container (e.g. "x86_64", "aarch64").
    /// When set, Incus will use the appropriate kernel/emulation layer.
    /// Empty string means "use the host architecture".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub architecture: String,
}

// ── Instance state ────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct InstanceState {
    pub status: String,
    pub status_code: u16,
}

#[derive(Debug, Serialize)]
pub struct InstanceStatePut {
    pub action: String, // "start" | "stop"
    pub timeout: i32,
    pub force: bool,
}

// ── Exec ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub environment: HashMap<String, String>,
    /// Wait for the command to finish and return its exit code.
    #[serde(rename = "wait-for-websocket")]
    pub wait_for_websocket: bool,
    /// Run without a TTY so stdout/stderr can be captured as streams.
    pub interactive: bool,
    #[serde(rename = "record-output")]
    pub record_output: bool,
}

// ── Snapshot ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SnapshotRequest {
    pub name: String,
    pub stateful: bool,
}

// ── Image import ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImageAliasRequest {
    pub name: String,
    pub description: String,
    pub target: String,
}
