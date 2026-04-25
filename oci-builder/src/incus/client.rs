//! HTTP client wired to the Incus Unix socket.
//!
//! All communication with the Incus daemon goes through its Unix socket via
//! `hyperlocal`. The client covers:
//!
//! - Instance lifecycle (create / start / stop / delete)
//! - Exec with real exit-code extraction and live stdout/stderr streaming
//! - File push via the Incus file API
//! - Image import via multipart POST (no CLI dependency)
//! - Image alias management and deletion
//! - Snapshots
//! - Rootfs export

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Method;
use hyper_util::client::legacy::Client as HyperClient;
use hyperlocal::{UnixClientExt, UnixConnector, Uri as UnixUri};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, instrument};

use super::api::{
    ExecRequest, ImageAliasRequest, InstanceConfig, InstanceStatePut, Operation, Response,
    SnapshotRequest,
};

pub const DEFAULT_SOCKET: &str = "/var/lib/incus/unix.socket";
const API_VERSION: &str = "1.0";
const OP_TIMEOUT_SECS: u64 = 300;

type UnixHyperClient = HyperClient<UnixConnector, Full<Bytes>>;

pub struct IncusClient {
    inner: UnixHyperClient,
    socket_path: PathBuf,
}

impl IncusClient {
    pub fn new() -> Result<Self> {
        Self::with_socket(Path::new(DEFAULT_SOCKET))
    }

    pub fn with_socket(socket: &Path) -> Result<Self> {
        if !socket.exists() {
            anyhow::bail!(
                "Incus socket not found at {}. Is incus running?",
                socket.display()
            );
        }
        let inner = HyperClient::unix();
        Ok(Self {
            inner,
            socket_path: socket.to_path_buf(),
        })
    }

    #[allow(dead_code)]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    // ── URL / request helpers ─────────────────────────────────────────────────

    fn uri(&self, path: &str) -> hyper::Uri {
        let p = format!("/{}/{}", API_VERSION, path.trim_start_matches('/'));
        UnixUri::new(&self.socket_path, &p).into()
    }

    async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Bytes>,
        content_type: Option<&str>,
    ) -> Result<Bytes> {
        let uri = self.uri(path);
        debug!(%uri, %method, "request");

        let body_bytes = body.unwrap_or_default();
        let mut builder = hyper::Request::builder().method(method).uri(uri);
        if let Some(ct) = content_type {
            builder = builder.header("Content-Type", ct);
        }
        let req = builder
            .body(Full::new(body_bytes))
            .context("building request")?;

        let resp = tokio::time::timeout(
            Duration::from_secs(OP_TIMEOUT_SECS),
            self.inner.request(req),
        )
        .await
        .context("request timed out")?
        .context("sending request")?;

        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .context("reading response body")?
            .to_bytes();

        if !status.is_success() && status.as_u16() != 202 {
            let text = String::from_utf8_lossy(&body);
            anyhow::bail!("HTTP {status}: {text}");
        }
        Ok(body)
    }

    async fn get_raw(&self, path: &str) -> Result<Bytes> {
        self.request_raw(Method::GET, path, None, None).await
    }

    async fn post_raw(&self, path: &str, body: Bytes, content_type: &str) -> Result<Bytes> {
        self.request_raw(Method::POST, path, Some(body), Some(content_type))
            .await
    }

    async fn delete_raw(&self, path: &str) -> Result<Bytes> {
        self.request_raw(Method::DELETE, path, None, None).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let raw = self.get_raw(path).await?;
        let resp: Response<T> = serde_json::from_slice(&raw).context("decoding GET response")?;
        Ok(resp.metadata)
    }

    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let json = serde_json::to_vec(body).context("serialising request body")?;
        let raw = self
            .post_raw(path, Bytes::from(json), "application/json")
            .await?;
        let resp: Response<T> = serde_json::from_slice(&raw).context("decoding POST response")?;
        Ok(resp.metadata)
    }

    async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let raw = self.delete_raw(path).await?;
        let resp: Response<T> = serde_json::from_slice(&raw).context("decoding DELETE response")?;
        Ok(resp.metadata)
    }

    // ── Operation polling ─────────────────────────────────────────────────────

    async fn wait_for_op(&self, op: Operation) -> Result<()> {
        self.wait_for_op_raw(op).await.map(|_| ())
    }

    /// Wait for an async operation and return the completed `Operation` so
    /// callers can inspect its `metadata` field.
    async fn wait_for_op_raw(&self, op: Operation) -> Result<Operation> {
        if op.status == "Success" {
            return Ok(op);
        }
        let path = format!("operations/{}/wait?timeout={OP_TIMEOUT_SECS}", op.id);
        let finished: Operation = self
            .get(&path)
            .await
            .with_context(|| format!("waiting for operation {}", op.id))?;
        if finished.status_code >= 400 || !finished.err.is_empty() {
            anyhow::bail!("operation {} failed: {}", finished.id, finished.err);
        }
        Ok(finished)
    }

    // ── Instance lifecycle ────────────────────────────────────────────────────

    #[instrument(skip(self, config), fields(name = %config.name))]
    pub async fn create_instance(&self, config: &InstanceConfig) -> Result<()> {
        let op: Operation = self
            .post("instances", config)
            .await
            .context("creating instance")?;
        self.wait_for_op(op)
            .await
            .context("waiting for instance creation")
    }

    #[instrument(skip(self), fields(name))]
    pub async fn start_instance(&self, name: &str) -> Result<()> {
        let req = InstanceStatePut {
            action: "start".to_string(),
            timeout: 60,
            force: false,
        };
        let op: Operation = self
            .post(&format!("instances/{name}/state"), &req)
            .await
            .context("starting instance")?;
        self.wait_for_op(op)
            .await
            .context("waiting for instance start")
    }

    #[instrument(skip(self), fields(name))]
    pub async fn stop_instance(&self, name: &str) -> Result<()> {
        let req = InstanceStatePut {
            action: "stop".to_string(),
            timeout: 60,
            force: true,
        };
        let op: Operation = self
            .post(&format!("instances/{name}/state"), &req)
            .await
            .context("stopping instance")?;
        self.wait_for_op(op)
            .await
            .context("waiting for instance stop")
    }

    #[instrument(skip(self), fields(name))]
    pub async fn delete_instance(&self, name: &str) -> Result<()> {
        let op: Operation = self
            .delete(&format!("instances/{name}"))
            .await
            .context("deleting instance")?;
        self.wait_for_op(op)
            .await
            .context("waiting for instance deletion")
    }

    // ── Exec ──────────────────────────────────────────────────────────────────

    /// Run a command inside a container with live stdout/stderr streaming.
    ///
    /// Uses the Incus WebSocket exec API (`wait-for-websocket: true`) to
    /// stream output in real time. Falls back to the record-output path if
    /// the WebSocket handshake fails (e.g. older Incus versions or socket
    /// permission issues).
    #[instrument(skip(self), fields(name, cmd = ?req.command))]
    pub async fn exec(&self, name: &str, req: &ExecRequest) -> Result<i32> {
        // Always request websocket mode for live streaming.
        let ws_req = ExecRequest {
            wait_for_websocket: true,
            interactive: false,
            record_output: false,
            command: req.command.clone(),
            environment: req.environment.clone(),
        };

        let op: Operation = self
            .post(&format!("instances/{name}/exec"), &ws_req)
            .await
            .context("exec in instance")?;

        let op_id = op.id.clone();
        let fds = op.metadata.as_ref().and_then(|m| m.get("fds")).cloned();

        if let Some(fds) = fds {
            let stdout_secret = fds
                .get("1")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let stderr_secret = fds
                .get("2")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ctrl_secret = fds
                .get("control")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let stdout_path = format!("/1.0/operations/{op_id}/websocket?secret={stdout_secret}");
            let stderr_path = format!("/1.0/operations/{op_id}/websocket?secret={stderr_secret}");
            let ctrl_path = format!("/1.0/operations/{op_id}/websocket?secret={ctrl_secret}");

            let sock1 = self.socket_path.clone();
            let sock2 = self.socket_path.clone();
            let sock3 = self.socket_path.clone();

            let stdout_task = tokio::spawn(async move {
                let mut out = tokio::io::stdout();
                crate::incus::ws_exec::stream_ws_to_writer(&sock1, &stdout_path, &mut out).await
            });
            let stderr_task = tokio::spawn(async move {
                let mut err = tokio::io::stderr();
                crate::incus::ws_exec::stream_ws_to_writer(&sock2, &stderr_path, &mut err).await
            });

            let code = crate::incus::ws_exec::read_exit_code(&sock3, &ctrl_path).await?;
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            Ok(code)
        } else {
            // Fallback: poll for completion and fetch recorded output.
            let finished = self.wait_for_op_raw(op).await.context("waiting for exec")?;
            if let Some(meta) = &finished.metadata {
                self.print_exec_output(meta).await;
            }
            Ok(finished
                .metadata
                .as_ref()
                .and_then(|m| m.get("return"))
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
                .unwrap_or(0))
        }
    }

    /// Fetch and print stdout/stderr from Incus operation log URLs (fallback path).
    async fn print_exec_output(&self, meta: &serde_json::Value) {
        let output = match meta.get("output").and_then(|v| v.as_object()) {
            Some(o) => o.clone(),
            None => return,
        };
        for (fd, url_val) in &output {
            let url = match url_val.as_str() {
                Some(u) => u,
                None => continue,
            };
            let path = url.trim_start_matches('/').trim_start_matches("1.0/");
            match self.get_raw(path).await {
                Ok(bytes) if !bytes.is_empty() => {
                    let text = String::from_utf8_lossy(&bytes);
                    if fd == "1" {
                        print!("{text}");
                    } else {
                        eprint!("{text}");
                    }
                }
                _ => {}
            }
        }
    }

    // ── File push ─────────────────────────────────────────────────────────────

    /// Push raw bytes into a file inside the container.
    #[instrument(skip(self, content), fields(name, dest))]
    pub async fn push_file(
        &self,
        name: &str,
        dest: &str,
        content: Bytes,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Result<()> {
        let path = format!("instances/{name}/files?path={dest}");
        let uri = self.uri(&path);
        debug!(%uri, dest, "push file");

        let req = hyper::Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("Content-Type", "application/octet-stream")
            .header("X-Incus-type", "file")
            .header("X-Incus-uid", uid.to_string())
            .header("X-Incus-gid", gid.to_string())
            .header("X-Incus-mode", format!("{mode:04o}"))
            .body(Full::new(content))
            .context("building file push request")?;

        let resp = tokio::time::timeout(
            Duration::from_secs(OP_TIMEOUT_SECS),
            self.inner.request(req),
        )
        .await
        .context("file push timed out")?
        .context("sending file push request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .into_body()
                .collect()
                .await
                .context("reading error body")?
                .to_bytes();
            anyhow::bail!(
                "file push to {dest} failed HTTP {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        Ok(())
    }

    // ── Image import ──────────────────────────────────────────────────────────

    /// Import a squashfs rootfs into the Incus image store via the REST API.
    ///
    /// Sends a multipart/form-data POST to `/1.0/images` with:
    ///   - `metadata`  part: a minimal `metadata.yaml` tarball
    ///   - `rootfs`    part: the squashfs filesystem image
    ///
    /// Returns the fingerprint of the imported image and registers `alias`.
    /// This replaces the previous `incus image import` CLI shell-out.
    #[instrument(skip(self, squashfs_bytes), fields(alias))]
    pub async fn import_image(&self, squashfs_bytes: Bytes, alias: &str) -> Result<String> {
        let metadata_tar = build_metadata_tar(alias)?;
        let boundary = format!("iob-{}", uuid::Uuid::new_v4().simple());
        let body = build_multipart_body(&boundary, &metadata_tar, &squashfs_bytes);
        let content_type = format!("multipart/form-data; boundary={boundary}");

        let raw = self
            .post_raw("images", Bytes::from(body), &content_type)
            .await
            .context("POST /1.0/images")?;

        // Response is an async operation envelope.
        let resp: Response<Operation> =
            serde_json::from_slice(&raw).context("parsing image import response")?;
        let finished = self
            .wait_for_op_raw(resp.metadata)
            .await
            .context("waiting for image import operation")?;

        let fingerprint = finished
            .metadata
            .as_ref()
            .and_then(|m| m.get("fingerprint"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .context("missing fingerprint in image import metadata")?;

        // Register the alias.
        let alias_req = ImageAliasRequest {
            name: alias.to_string(),
            description: "incus-oci-builder bootstrap image".to_string(),
            target: fingerprint.clone(),
        };
        self.post::<_, serde_json::Value>("images/aliases", &alias_req)
            .await
            .context("registering image alias")?;

        info!(alias, fingerprint, "image imported");
        Ok(fingerprint)
    }

    /// Delete an image by fingerprint. Best-effort — logs but does not fail.
    #[instrument(skip(self), fields(fingerprint))]
    pub async fn delete_image(&self, fingerprint: &str) -> Result<()> {
        let op: Operation = self
            .delete(&format!("images/{fingerprint}"))
            .await
            .context("deleting image")?;
        self.wait_for_op(op)
            .await
            .context("waiting for image deletion")
    }

    // ── Snapshots ─────────────────────────────────────────────────────────────

    #[instrument(skip(self), fields(instance, snapshot))]
    pub async fn create_snapshot(&self, instance: &str, snapshot: &str) -> Result<()> {
        let req = SnapshotRequest {
            name: snapshot.to_string(),
            stateful: false,
        };
        let op: Operation = self
            .post(&format!("instances/{instance}/snapshots"), &req)
            .await
            .context("creating snapshot")?;
        self.wait_for_op(op)
            .await
            .context("waiting for snapshot creation")
    }

    #[instrument(skip(self), fields(instance, snapshot))]
    pub async fn delete_snapshot(&self, instance: &str, snapshot: &str) -> Result<()> {
        let op: Operation = self
            .delete(&format!("instances/{instance}/snapshots/{snapshot}"))
            .await
            .context("deleting snapshot")?;
        self.wait_for_op(op)
            .await
            .context("waiting for snapshot deletion")
    }

    // ── Rootfs export ─────────────────────────────────────────────────────────

    #[instrument(skip(self, dest), fields(name))]
    pub async fn export_rootfs(&self, name: &str, dest: &Path) -> Result<()> {
        let raw = self
            .get_raw(&format!("instances/{name}/backups/export"))
            .await
            .with_context(|| format!("exporting rootfs for {name}"))?;

        let mut file = tokio::fs::File::create(dest)
            .await
            .with_context(|| format!("creating export file {}", dest.display()))?;
        file.write_all(&raw)
            .await
            .context("writing export tarball")?;
        file.flush().await.context("flushing export file")?;
        Ok(())
    }
}

impl Default for IncusClient {
    fn default() -> Self {
        Self::new().expect("failed to create default IncusClient")
    }
}

// ── Multipart helpers ─────────────────────────────────────────────────────────

/// Build a minimal `metadata.yaml` inside a tar archive, as required by the
/// Incus image import API. The metadata identifies the image architecture and
/// creation date.
fn build_metadata_tar(alias: &str) -> Result<Vec<u8>> {
    let yaml = format!(
        "architecture: x86_64\ncreation_date: {}\ndescription: {alias}\n",
        chrono::Utc::now().timestamp()
    );
    let mut buf = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut buf);
        let data = yaml.as_bytes();
        let mut hdr = tar::Header::new_gnu();
        hdr.set_size(data.len() as u64);
        hdr.set_mode(0o644);
        hdr.set_cksum();
        ar.append_data(&mut hdr, "metadata.yaml", data)
            .context("building metadata tar")?;
        ar.finish().context("finalising metadata tar")?;
    }
    Ok(buf)
}

/// Assemble a multipart/form-data body with `metadata` and `rootfs` parts.
fn build_multipart_body(boundary: &str, metadata: &[u8], rootfs: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    // metadata part
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"; \
             filename=\"metadata.tar\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(metadata);
    body.extend_from_slice(b"\r\n");
    // rootfs part
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"rootfs\"; \
             filename=\"rootfs.squashfs\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(rootfs);
    body.extend_from_slice(b"\r\n");
    // closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}
