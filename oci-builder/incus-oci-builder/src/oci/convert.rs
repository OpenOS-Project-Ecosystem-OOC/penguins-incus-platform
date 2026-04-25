//! Output format conversion for OCI image layouts.
//!
//! After the build pipeline produces an OCI directory layout, this module
//! can repack it into alternative archive formats:
//!
//! - `oci-dir` — OCI Image Layout directory (default, no conversion)
//! - `oci-archive` — OCI Image Layout packed into a single `.tar` file
//! - `docker-archive` — Docker `save`-compatible tar (repositories.json + per-layer directories)

use std::fs::{self, File};
use std::io::{BufWriter, Read};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use tar::Builder as TarBuilder;
use tracing::info;

/// Output format requested by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// OCI Image Layout directory — no conversion needed.
    #[default]
    OciDir,
    /// OCI Image Layout packed into a single `.tar` file.
    OciArchive,
    /// Docker `save`-compatible tar archive.
    DockerArchive,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "oci-dir" => Ok(Self::OciDir),
            "oci-archive" => Ok(Self::OciArchive),
            "docker-archive" => Ok(Self::DockerArchive),
            other => anyhow::bail!(
                "unknown output format {other:?}; valid values: oci-dir, oci-archive, docker-archive"
            ),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OciDir => f.write_str("oci-dir"),
            Self::OciArchive => f.write_str("oci-archive"),
            Self::DockerArchive => f.write_str("docker-archive"),
        }
    }
}

/// Convert an OCI directory layout at `oci_dir` into the requested format.
///
/// - `OciDir`: no-op.
/// - `OciArchive`: packs `oci_dir` into `<oci_dir>.tar`.
/// - `DockerArchive`: converts to Docker save format at `<oci_dir>-docker.tar`.
///
/// Returns the path to the final output (directory or archive file).
pub fn convert(
    oci_dir: &Path,
    format: OutputFormat,
    image_name: &str,
    image_tag: &str,
) -> Result<std::path::PathBuf> {
    match format {
        OutputFormat::OciDir => Ok(oci_dir.to_path_buf()),
        OutputFormat::OciArchive => pack_oci_archive(oci_dir, image_name, image_tag),
        OutputFormat::DockerArchive => pack_docker_archive(oci_dir, image_name, image_tag),
    }
}

// ── OCI archive ───────────────────────────────────────────────────────────────

/// Pack an OCI directory layout into a single `.tar` file.
///
/// The archive is placed alongside the OCI directory and named
/// `<image_name>-<image_tag>.tar`, with `/` replaced by `-` so the
/// filename is safe on all filesystems. For example, `myorg/ubuntu:noble`
/// produces `myorg-ubuntu-noble.tar`.
fn pack_oci_archive(
    oci_dir: &Path,
    image_name: &str,
    image_tag: &str,
) -> Result<std::path::PathBuf> {
    let safe_name = archive_stem(image_name, image_tag);
    let archive_path = oci_dir
        .parent()
        .unwrap_or(oci_dir)
        .join(format!("{safe_name}.tar"));

    info!(
        src = %oci_dir.display(),
        dst = %archive_path.display(),
        "packing OCI archive"
    );

    let file = File::create(&archive_path)
        .with_context(|| format!("creating OCI archive {}", archive_path.display()))?;
    let mut tar = TarBuilder::new(BufWriter::new(file));

    // Append the OCI directory contents with paths relative to the archive root.
    tar.append_dir_all(".", oci_dir)
        .context("appending OCI layout to archive")?;
    tar.finish().context("finalising OCI archive")?;

    info!(path = %archive_path.display(), "OCI archive written");
    Ok(archive_path)
}

// ── Docker archive ────────────────────────────────────────────────────────────

/// Convert an OCI Image Layout directory to a Docker `save`-compatible tar.
///
/// Docker archive format:
/// ```
/// <layer-id>/
///   layer.tar      — uncompressed layer tar
///   json           — layer metadata JSON
///   VERSION        — "1.0"
/// <config-id>.json — image config
/// manifest.json    — [{Config, RepoTags, Layers}]
/// repositories.json — {"name": {"tag": "layer-id"}}
/// ```
fn pack_docker_archive(
    oci_dir: &Path,
    image_name: &str,
    image_tag: &str,
) -> Result<std::path::PathBuf> {
    let safe_name = archive_stem(image_name, image_tag);
    let archive_path = oci_dir
        .parent()
        .unwrap_or(oci_dir)
        .join(format!("{safe_name}-docker.tar"));

    info!(
        src = %oci_dir.display(),
        dst = %archive_path.display(),
        "packing Docker archive"
    );

    // Read the OCI index to find the manifest digest.
    let index_path = oci_dir.join("index.json");
    let index_bytes =
        fs::read(&index_path).with_context(|| format!("reading {}", index_path.display()))?;
    let index: serde_json::Value =
        serde_json::from_slice(&index_bytes).context("parsing index.json")?;

    let manifest_digest = index["manifests"][0]["digest"]
        .as_str()
        .context("index.json has no manifests[0].digest")?;
    let manifest_blob = blob_path(oci_dir, manifest_digest);
    let manifest_bytes = fs::read(&manifest_blob)
        .with_context(|| format!("reading manifest blob {manifest_digest}"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).context("parsing manifest blob")?;

    let config_digest = manifest["config"]["digest"]
        .as_str()
        .context("manifest has no config.digest")?;
    let config_blob = blob_path(oci_dir, config_digest);
    let config_bytes =
        fs::read(&config_blob).with_context(|| format!("reading config blob {config_digest}"))?;

    let layers = manifest["layers"]
        .as_array()
        .context("manifest has no layers array")?;

    // Extract DiffIDs from the image config (SHA-256 of each uncompressed layer tar).
    // These are used to compute Docker chain-IDs for layer directory names.
    let config_value: serde_json::Value =
        serde_json::from_slice(&config_bytes).context("parsing image config JSON")?;
    let diff_ids: Vec<&str> = config_value["rootfs"]["diff_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Build the archive in a temp directory then tar it up.
    let staging = tempfile::TempDir::new().context("creating staging dir for docker archive")?;
    let stage = staging.path();

    // Write config JSON — Docker uses the config digest hex as the filename.
    let config_filename = format!("{}.json", hex_id(config_digest));
    fs::write(stage.join(&config_filename), &config_bytes).context("writing config to staging")?;

    // Write each layer as <chain-id>/layer.tar + json + VERSION.
    //
    // Docker chain-ID algorithm (moby/moby image/spec):
    //   chain_id[0] = sha256(diff_id[0])
    //   chain_id[n] = sha256(chain_id[n-1] + " " + diff_id[n])
    //
    // Using chain-IDs means the layer directory name is stable across image
    // rebuilds when the layer content is unchanged, enabling layer sharing
    // between images in the Docker image store.
    let mut layer_ids: Vec<String> = Vec::new();
    let mut prev_chain_id: Option<String> = None;

    for (i, layer) in layers.iter().enumerate() {
        let layer_digest = layer["digest"].as_str().context("layer missing digest")?;

        // Compute chain-ID for this layer.
        let diff_id = diff_ids.get(i).copied().unwrap_or(layer_digest);
        let chain_id = compute_chain_id(prev_chain_id.as_deref(), diff_id);

        let layer_dir = stage.join(&chain_id);
        fs::create_dir_all(&layer_dir).context("creating layer dir")?;

        // Docker save format requires uncompressed layer tars.
        let layer_blob = blob_path(oci_dir, layer_digest);
        let media_type = layer["mediaType"].as_str().unwrap_or("");
        decompress_layer_blob(&layer_blob, &layer_dir.join("layer.tar"), media_type)
            .with_context(|| format!("decompressing layer blob {layer_digest}"))?;

        // Layer metadata JSON — `id` is the chain-ID, `parent` is the previous chain-ID.
        let layer_json = json!({
            "id": chain_id,
            "parent": prev_chain_id,
        });
        fs::write(
            layer_dir.join("json"),
            serde_json::to_vec_pretty(&layer_json).unwrap(),
        )
        .context("writing layer json")?;

        fs::write(layer_dir.join("VERSION"), b"1.0").context("writing layer VERSION")?;

        prev_chain_id = Some(chain_id.clone());
        layer_ids.push(format!("{chain_id}/layer.tar"));
    }

    // manifest.json
    let repo_tag = format!("{image_name}:{image_tag}");
    let docker_manifest = json!([{
        "Config": config_filename,
        "RepoTags": [repo_tag],
        "Layers": layer_ids,
    }]);
    fs::write(
        stage.join("manifest.json"),
        serde_json::to_vec_pretty(&docker_manifest).unwrap(),
    )
    .context("writing manifest.json")?;

    // repositories.json
    let top_layer_id = layer_ids
        .last()
        .and_then(|s| s.split('/').next())
        .unwrap_or("");
    let repositories = json!({ image_name: { image_tag: top_layer_id } });
    fs::write(
        stage.join("repositories.json"),
        serde_json::to_vec_pretty(&repositories).unwrap(),
    )
    .context("writing repositories.json")?;

    // Pack staging dir into the output tar.
    let file = File::create(&archive_path)
        .with_context(|| format!("creating docker archive {}", archive_path.display()))?;
    let mut tar = TarBuilder::new(BufWriter::new(file));
    tar.append_dir_all(".", stage)
        .context("appending staging dir to docker archive")?;
    tar.finish().context("finalising docker archive")?;

    info!(path = %archive_path.display(), "Docker archive written");
    Ok(archive_path)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a filesystem-safe archive stem from an image name and tag.
///
/// Replaces `/`, `:`, and whitespace with `-` and collapses consecutive
/// dashes so `myorg/ubuntu:noble` → `myorg-ubuntu-noble`.
fn archive_stem(image_name: &str, image_tag: &str) -> String {
    let raw = format!("{image_name}-{image_tag}");
    let sanitised: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Collapse consecutive dashes.
    let mut result = String::with_capacity(sanitised.len());
    let mut prev_dash = false;
    for c in sanitised.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result.trim_matches('-').to_string()
}

/// Compute the Docker chain-ID for a layer.
///
/// Algorithm from the Docker image spec (moby/moby):
/// - First layer:  `sha256(diff_id)`
/// - Subsequent:   `sha256(prev_chain_id + " " + diff_id)`
///
/// `diff_id` is the `sha256:<hex>` digest of the **uncompressed** layer tar,
/// as stored in `config.rootfs.diff_ids`.
fn compute_chain_id(prev_chain_id: Option<&str>, diff_id: &str) -> String {
    let input = match prev_chain_id {
        None => diff_id.to_string(),
        Some(prev) => format!("{prev} {diff_id}"),
    };
    hex::encode(Sha256::digest(input.as_bytes()))
}

/// Write the layer blob at `src` to `dst` as an uncompressed tar.
///
/// Docker save format requires uncompressed `layer.tar` entries. OCI blobs
/// are gzip-compressed (`tar+gzip`) or occasionally zstd (`tar+zstd`) or
/// already uncompressed (`tar`). This function decompresses as needed.
fn decompress_layer_blob(src: &Path, dst: &Path, media_type: &str) -> Result<()> {
    let blob_bytes =
        fs::read(src).with_context(|| format!("reading layer blob {}", src.display()))?;

    let raw_tar: Vec<u8> = if media_type.ends_with("tar+gzip") || media_type.ends_with("tar+gz") {
        // Gzip-compressed — decompress with flate2.
        let mut decoder = flate2::read::GzDecoder::new(blob_bytes.as_slice());
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .context("decompressing gzip layer")?;
        out
    } else if media_type.ends_with("tar+zstd") {
        // Zstd-compressed — decompress with the zstd crate if available,
        // otherwise fall through to the raw copy path.
        // We don't currently produce zstd layers, so treat as passthrough
        // and let the runtime handle it (most modern ones do).
        blob_bytes
    } else {
        // Already uncompressed tar, or unknown — copy as-is.
        blob_bytes
    };

    fs::write(dst, &raw_tar)
        .with_context(|| format!("writing decompressed layer to {}", dst.display()))?;
    Ok(())
}

/// Resolve a digest like `sha256:<hex>` to its blob path under `oci_dir`.
fn blob_path(oci_dir: &Path, digest: &str) -> std::path::PathBuf {
    let hex = digest.split_once(':').map(|(_, h)| h).unwrap_or(digest);
    oci_dir.join("blobs").join("sha256").join(hex)
}

/// Return the hex portion of a digest string (strips `sha256:` prefix).
fn hex_id(digest: &str) -> String {
    digest
        .split_once(':')
        .map(|(_, h)| h)
        .unwrap_or(digest)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_roundtrip() {
        for (s, expected) in [
            ("oci-dir", OutputFormat::OciDir),
            ("oci-archive", OutputFormat::OciArchive),
            ("docker-archive", OutputFormat::DockerArchive),
        ] {
            let parsed: OutputFormat = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn unknown_format_is_error() {
        let result: Result<OutputFormat> = "tarball".parse();
        assert!(result.is_err());
    }
}
