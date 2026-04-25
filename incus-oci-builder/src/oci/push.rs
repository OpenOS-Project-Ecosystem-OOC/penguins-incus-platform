//! Push an OCI image layout to a remote registry.
//!
//! Strategy:
//! 1. If `skopeo` is available, delegate to it — it handles auth, retries,
//!    cross-repo mounts, and all edge cases correctly.
//! 2. Otherwise use the built-in native push which reads credentials from
//!    `~/.docker/config.json` (same format used by Docker, Podman, etc.).
//!
//! Auth lookup order (native path):
//!   a. `REGISTRY_USERNAME` / `REGISTRY_PASSWORD` environment variables
//!   b. `~/.docker/config.json` auths / credHelpers entries
//!   c. Unauthenticated (public registries only)

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use tracing::{debug, info};

// ── Public entry point ────────────────────────────────────────────────────────

/// Push the OCI layout at `layout_dir` to `registry/name:tag`.
pub async fn push(layout_dir: &Path, registry: &str, name: &str, tag: &str) -> Result<()> {
    if which_skopeo() {
        push_via_skopeo(layout_dir, registry, name, tag).await
    } else {
        push_native(layout_dir, registry, name, tag).await
    }
}

// ── skopeo path ───────────────────────────────────────────────────────────────

fn which_skopeo() -> bool {
    which::which("skopeo").is_ok()
}

async fn push_via_skopeo(layout_dir: &Path, registry: &str, name: &str, tag: &str) -> Result<()> {
    let src = format!("oci:{}", layout_dir.display());
    let dest = format!("docker://{registry}/{name}:{tag}");
    info!(src, dest, "pushing via skopeo");

    let status = tokio::process::Command::new("skopeo")
        .args(["copy", &src, &dest])
        .status()
        .await
        .context("running skopeo")?;

    if !status.success() {
        anyhow::bail!("skopeo copy failed with status {status}");
    }
    Ok(())
}

// ── Native push ───────────────────────────────────────────────────────────────

async fn push_native(layout_dir: &Path, registry: &str, name: &str, tag: &str) -> Result<()> {
    info!(registry, name, tag, "pushing via native OCI client");

    let creds = load_credentials(registry);
    let client = build_client(&creds)?;

    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(layout_dir.join("index.json")).context("reading index.json")?,
    )
    .context("parsing index.json")?;

    let manifest_digest = index["manifests"][0]["digest"]
        .as_str()
        .context("missing manifest digest in index")?;

    let manifest_bytes = std::fs::read(blob_path(layout_dir, manifest_digest))
        .with_context(|| format!("reading manifest blob {manifest_digest}"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).context("parsing manifest")?;

    let base = format!("https://{registry}/v2/{name}");

    // Upload config blob.
    let config_digest = manifest["config"]["digest"]
        .as_str()
        .context("missing config digest")?;
    upload_blob(&client, &base, layout_dir, config_digest).await?;

    // Upload layer blobs.
    for layer in manifest["layers"].as_array().context("missing layers")? {
        let digest = layer["digest"].as_str().context("missing layer digest")?;
        upload_blob(&client, &base, layout_dir, digest).await?;
    }

    // PUT manifest.
    let url = format!("{base}/manifests/{tag}");
    info!(url, "PUT manifest");
    let mut req = client
        .put(&url)
        .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
        .body(manifest_bytes);
    if let Some(auth) = auth_header(&creds) {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.context("PUT manifest")?;
    if !resp.status().is_success() {
        anyhow::bail!("PUT manifest failed: {}", resp.status());
    }

    info!(registry, name, tag, "push complete");
    Ok(())
}

async fn upload_blob(
    client: &reqwest::Client,
    base: &str,
    layout_dir: &Path,
    digest: &str,
) -> Result<()> {
    // HEAD — check if blob already exists.
    let url = format!("{base}/blobs/{digest}");
    let head = client.head(&url).send().await.context("HEAD blob")?;
    if head.status().is_success() {
        debug!(digest, "blob already exists");
        return Ok(());
    }

    let data = std::fs::read(blob_path(layout_dir, digest))
        .with_context(|| format!("reading blob {digest}"))?;

    // POST — initiate upload session.
    let upload_url = format!("{base}/blobs/uploads/");
    let resp = client
        .post(&upload_url)
        .send()
        .await
        .context("initiating blob upload")?;

    // Handle 401 WWW-Authenticate bearer token challenge.
    let client = if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        let www_auth = resp
            .headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let token = fetch_bearer_token(client, &www_auth).await?;
        // Rebuild client with bearer token baked in as a default header.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid bearer token")?,
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("building authenticated client")?
    } else {
        client.clone()
    };

    // Retry the upload initiation with auth.
    let resp = client
        .post(&upload_url)
        .send()
        .await
        .context("initiating blob upload (authenticated)")?;

    let location = resp
        .headers()
        .get("Location")
        .context("missing Location header")?
        .to_str()
        .context("Location not UTF-8")?
        .to_string();

    // PUT — upload blob data.
    let put_url = format!("{location}&digest={digest}");
    let resp = client
        .put(&put_url)
        .header("Content-Type", "application/octet-stream")
        .body(data)
        .send()
        .await
        .context("PUT blob")?;

    if !resp.status().is_success() {
        anyhow::bail!("blob upload failed for {digest}: {}", resp.status());
    }
    Ok(())
}

// ── Credential helpers ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct Credentials {
    username: Option<String>,
    password: Option<String>,
}

/// Load credentials for `registry` from env vars or docker config.
fn load_credentials(registry: &str) -> Credentials {
    // 1. Environment variables take priority.
    if let (Ok(u), Ok(p)) = (
        std::env::var("REGISTRY_USERNAME"),
        std::env::var("REGISTRY_PASSWORD"),
    ) {
        return Credentials {
            username: Some(u),
            password: Some(p),
        };
    }

    // 2. ~/.docker/config.json
    if let Some(creds) = load_docker_config(registry) {
        return creds;
    }

    Credentials::default()
}

fn load_docker_config(registry: &str) -> Option<Credentials> {
    let config_path = dirs_next::home_dir()?.join(".docker").join("config.json");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Check `auths` map for a base64-encoded "user:pass" entry.
    let auths = config.get("auths")?.as_object()?;
    for key in &[
        registry,
        &format!("https://{registry}"),
        &format!("https://{registry}/v1/"),
    ] {
        if let Some(entry) = auths.get(*key) {
            if let Some(auth_b64) = entry.get("auth").and_then(|v| v.as_str()) {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(auth_b64)
                    .ok()?;
                let s = String::from_utf8(decoded).ok()?;
                let (user, pass) = s.split_once(':')?;
                return Some(Credentials {
                    username: Some(user.to_string()),
                    password: Some(pass.to_string()),
                });
            }
        }
    }
    None
}

fn auth_header(creds: &Credentials) -> Option<String> {
    let u = creds.username.as_deref()?;
    let p = creds.password.as_deref()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
    Some(format!("Basic {encoded}"))
}

fn build_client(creds: &Credentials) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(auth) = auth_header(creds) {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth).context("invalid auth header")?,
        );
        builder = builder.default_headers(headers);
    }
    builder.build().context("building HTTP client")
}

/// Parse a `WWW-Authenticate: Bearer realm=...,service=...,scope=...` header
/// and fetch a short-lived token from the auth server.
async fn fetch_bearer_token(client: &reqwest::Client, www_auth: &str) -> Result<String> {
    let params = parse_www_authenticate(www_auth);
    let realm = params
        .get("realm")
        .context("missing realm in WWW-Authenticate")?;

    let mut url = reqwest::Url::parse(realm).context("parsing auth realm URL")?;
    if let Some(service) = params.get("service") {
        url.query_pairs_mut().append_pair("service", service);
    }
    if let Some(scope) = params.get("scope") {
        url.query_pairs_mut().append_pair("scope", scope);
    }

    let resp = client
        .get(url)
        .send()
        .await
        .context("fetching bearer token")?;
    let body: serde_json::Value = resp.json().await.context("parsing token response")?;
    body["token"]
        .as_str()
        .or_else(|| body["access_token"].as_str())
        .map(str::to_string)
        .context("missing token in auth response")
}

fn parse_www_authenticate(header: &str) -> HashMap<String, String> {
    // Strip "Bearer " prefix.
    let s = header.trim_start_matches("Bearer ").trim();
    let mut map = HashMap::new();
    for part in s.split(',') {
        if let Some((k, v)) = part.trim().split_once('=') {
            map.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    map
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn blob_path(layout_dir: &Path, digest: &str) -> std::path::PathBuf {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    layout_dir.join("blobs").join("sha256").join(hex)
}
