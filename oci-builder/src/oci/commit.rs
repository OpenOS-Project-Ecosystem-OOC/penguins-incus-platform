//! Assemble a complete OCI image layout from a rootfs directory.
//!
//! Produces the on-disk OCI Image Layout that can be pushed to a registry
//! or loaded with `podman load` / `docker load`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::arch::host_arch;
use crate::definition::{Definition, OciDef};

use super::layer::pack_layer;

/// Write a complete OCI image layout to `output_dir`.
///
/// `rootfs_dir` is the unpacked container filesystem.
/// `def` provides image metadata (name, labels, cmd, etc.).
pub fn commit_rootfs(rootfs_dir: &Path, output_dir: &Path, def: &Definition) -> Result<()> {
    info!(
        rootfs = %rootfs_dir.display(),
        output = %output_dir.display(),
        "committing rootfs to OCI layout"
    );

    std::fs::create_dir_all(output_dir).context("creating OCI output directory")?;
    let blobs_dir = output_dir.join("blobs").join("sha256");
    std::fs::create_dir_all(&blobs_dir).context("creating blobs/sha256 directory")?;

    // 1. Pack the rootfs into a compressed layer blob.
    let layer_tmp = tempfile::NamedTempFile::new().context("creating temp file for layer")?;
    let layer =
        pack_layer(rootfs_dir, layer_tmp.path()).context("packing rootfs into OCI layer")?;

    // Move the layer blob into the content-addressed store.
    let layer_digest_hex = layer.compressed_digest.strip_prefix("sha256:").unwrap();
    let layer_blob_path = blobs_dir.join(layer_digest_hex);
    std::fs::copy(layer_tmp.path(), &layer_blob_path)
        .context("copying layer blob into OCI store")?;

    // 2. Build the image config.
    let oci_cfg = def.oci.as_ref();
    let config_json =
        build_image_config(def, oci_cfg, &layer.diff_id).context("building OCI image config")?;
    let config_digest =
        write_blob(&blobs_dir, config_json.as_bytes()).context("writing image config blob")?;
    let config_size = config_json.len() as u64;

    // 3. Build the manifest.
    let manifest_json = build_manifest(
        &config_digest,
        config_size,
        &layer.compressed_digest,
        layer.compressed_size,
    )
    .context("building OCI manifest")?;
    let manifest_digest =
        write_blob(&blobs_dir, manifest_json.as_bytes()).context("writing manifest blob")?;
    let manifest_size = manifest_json.len() as u64;

    // 4. Write oci-layout marker.
    std::fs::write(
        output_dir.join("oci-layout"),
        r#"{"imageLayoutVersion":"1.0.0"}"#,
    )
    .context("writing oci-layout")?;

    // 5. Write index.json.
    let image_name = def.effective_name();
    let image_tag = def.effective_tag();
    let index = build_index(&manifest_digest, manifest_size, &image_name, &image_tag)
        .context("building OCI index")?;
    std::fs::write(output_dir.join("index.json"), index).context("writing index.json")?;

    info!(
        output = %output_dir.display(),
        manifest = manifest_digest,
        "OCI image layout written"
    );
    Ok(())
}

/// Write a multi-layer OCI image layout from a sequence of rootfs snapshots.
///
/// Each consecutive pair of directories is diffed: files present in `dirs[n]`
/// but absent or changed in `dirs[n-1]` become a new layer. The first
/// directory is packed as a full base layer.
///
/// This enables layer cache reuse: if only the last stage changed, only the
/// last layer needs to be re-pushed.
pub fn commit_layered_rootfs(dirs: &[&Path], output_dir: &Path, def: &Definition) -> Result<()> {
    if dirs.is_empty() {
        anyhow::bail!("commit_layered_rootfs requires at least one snapshot directory");
    }

    info!(
        layers = dirs.len(),
        output = %output_dir.display(),
        "committing layered OCI image"
    );

    std::fs::create_dir_all(output_dir).context("creating OCI output directory")?;
    let blobs_dir = output_dir.join("blobs").join("sha256");
    std::fs::create_dir_all(&blobs_dir).context("creating blobs/sha256 directory")?;

    let created = Utc::now().to_rfc3339();
    let mut layer_descriptors: Vec<serde_json::Value> = Vec::new();
    let mut diff_ids: Vec<String> = Vec::new();
    let mut history: Vec<serde_json::Value> = Vec::new();

    for (i, dir) in dirs.iter().enumerate() {
        let layer_tmp = tempfile::NamedTempFile::new().context("creating temp file for layer")?;

        let blob = if i == 0 {
            // Base layer: pack the full rootfs.
            pack_layer(dir, layer_tmp.path())
                .with_context(|| format!("packing base layer from {}", dir.display()))?
        } else {
            // Diff layer: pack only files that changed relative to the previous snapshot.
            pack_diff_layer(dirs[i - 1], dir, layer_tmp.path())
                .with_context(|| format!("packing diff layer {i}"))?
        };

        let hex = blob.compressed_digest.strip_prefix("sha256:").unwrap();
        std::fs::copy(layer_tmp.path(), blobs_dir.join(hex))
            .context("copying layer blob into OCI store")?;

        layer_descriptors.push(serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": blob.compressed_digest,
            "size": blob.compressed_size,
        }));
        diff_ids.push(blob.diff_id);
        history.push(serde_json::json!({
            "created": created,
            "created_by": format!(
                "incus-oci-builder {} layer {i}",
                env!("CARGO_PKG_VERSION")
            ),
        }));
    }

    // Build image config with all diff_ids.
    let oci_cfg = def.oci.as_ref();
    let config_json = build_image_config_multi(def, oci_cfg, &diff_ids, &history, &created)
        .context("building OCI image config")?;
    let config_digest =
        write_blob(&blobs_dir, config_json.as_bytes()).context("writing image config blob")?;
    let config_size = config_json.len() as u64;

    // Build manifest with all layers.
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config_size,
        },
        "layers": layer_descriptors,
    });
    let manifest_json = serde_json::to_string(&manifest).context("serialising layered manifest")?;
    let manifest_digest =
        write_blob(&blobs_dir, manifest_json.as_bytes()).context("writing manifest blob")?;
    let manifest_size = manifest_json.len() as u64;

    std::fs::write(
        output_dir.join("oci-layout"),
        r#"{"imageLayoutVersion":"1.0.0"}"#,
    )
    .context("writing oci-layout")?;

    let index = build_index(
        &manifest_digest,
        manifest_size,
        &def.effective_name(),
        &def.effective_tag(),
    )
    .context("building OCI index")?;
    std::fs::write(output_dir.join("index.json"), index).context("writing index.json")?;

    info!(
        output = %output_dir.display(),
        layers = dirs.len(),
        "layered OCI image written"
    );
    Ok(())
}

/// Pack a diff layer containing only files that are new or changed in `new_dir`
/// relative to `old_dir`. Deleted files are represented as whiteout entries.
fn pack_diff_layer(old_dir: &Path, new_dir: &Path, dest: &Path) -> Result<super::layer::LayerBlob> {
    use std::io::Write;

    let mut uncompressed = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut uncompressed);
        ar.follow_symlinks(false);

        // Walk new_dir; include entries that are new or have a different size/mtime.
        for entry in walkdir::WalkDir::new(new_dir).min_depth(1) {
            let entry = entry.context("walking new rootfs")?;
            let rel = entry
                .path()
                .strip_prefix(new_dir)
                .context("stripping new_dir prefix")?;
            let old_path = old_dir.join(rel);

            let new_meta = entry.metadata().context("reading new entry metadata")?;
            let include = if old_path.exists() {
                // Include if size or mtime changed.
                let old_meta =
                    std::fs::metadata(&old_path).context("reading old entry metadata")?;
                new_meta.len() != old_meta.len()
                    || new_meta
                        .modified()
                        .ok()
                        .zip(old_meta.modified().ok())
                        .map(|(n, o)| n != o)
                        .unwrap_or(true)
            } else {
                true // new file
            };

            if include {
                ar.append_path_with_name(entry.path(), rel)
                    .with_context(|| format!("appending {} to diff layer", rel.display()))?;
            }
        }

        // Walk old_dir; emit OCI whiteout entries for deleted files.
        for entry in walkdir::WalkDir::new(old_dir).min_depth(1) {
            let entry = entry.context("walking old rootfs")?;
            let rel = entry
                .path()
                .strip_prefix(old_dir)
                .context("stripping old_dir prefix")?;
            let new_path = new_dir.join(rel);
            if !new_path.exists() {
                // Emit a whiteout: a zero-byte file named .wh.<filename>
                let whiteout_name = rel
                    .file_name()
                    .map(|n| format!(".wh.{}", n.to_string_lossy()))
                    .unwrap_or_default();
                if whiteout_name.is_empty() {
                    continue;
                }
                let whiteout_path = rel
                    .parent()
                    .map(|p| p.join(&whiteout_name))
                    .unwrap_or_else(|| std::path::PathBuf::from(&whiteout_name));
                let mut hdr = tar::Header::new_gnu();
                hdr.set_size(0);
                hdr.set_mode(0o644);
                hdr.set_cksum();
                ar.append_data(&mut hdr, &whiteout_path, std::io::empty())
                    .with_context(|| format!("appending whiteout for {}", rel.display()))?;
            }
        }

        ar.finish().context("finalising diff tar")?;
    }

    // Compute diff_id (uncompressed digest).
    let diff_id = format!("sha256:{:x}", sha2::Sha256::digest(&uncompressed));

    // Compress.
    let mut compressed = Vec::new();
    {
        let mut enc =
            flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default());
        enc.write_all(&uncompressed)
            .context("compressing diff layer")?;
        enc.finish().context("finalising gzip stream")?;
    }

    let compressed_digest = format!("sha256:{:x}", sha2::Sha256::digest(&compressed));
    let compressed_size = compressed.len() as u64;
    std::fs::write(dest, &compressed).context("writing diff layer blob")?;

    Ok(super::layer::LayerBlob {
        compressed_digest,
        diff_id,
        compressed_size,
    })
}

fn build_image_config_multi(
    def: &Definition,
    oci: Option<&OciDef>,
    diff_ids: &[String],
    history: &[serde_json::Value],
    created: &str,
) -> Result<String> {
    let arch = if def.image.architecture.is_empty() {
        map_arch(&host_arch())
    } else {
        map_arch(&def.image.architecture)
    };

    let cmd: serde_json::Value = match oci.map(|o| o.cmd.as_slice()) {
        Some([_, ..]) => serde_json::to_value(oci.unwrap().cmd.clone())?,
        _ => serde_json::json!(["/bin/sh"]),
    };

    let entrypoint: serde_json::Value = match oci.map(|o| o.entrypoint.as_slice()) {
        Some([_, ..]) => serde_json::to_value(oci.unwrap().entrypoint.clone())?,
        _ => serde_json::Value::Null,
    };

    let mut labels = std::collections::HashMap::new();
    labels.insert(
        "org.opencontainers.image.title".to_string(),
        def.effective_name(),
    );
    labels.insert(
        "org.opencontainers.image.created".to_string(),
        created.to_string(),
    );
    labels.insert(
        "org.opencontainers.image.description".to_string(),
        def.image.description.clone(),
    );
    if let Some(o) = oci {
        labels.extend(o.labels.clone());
    }

    let config = serde_json::json!({
        "created": created,
        "architecture": arch,
        "os": "linux",
        "config": {
            "Cmd": cmd,
            "Entrypoint": entrypoint,
            "Labels": labels,
        },
        "rootfs": {
            "type": "layers",
            "diff_ids": diff_ids,
        },
        "history": history,
    });

    serde_json::to_string(&config).context("serialising multi-layer image config")
}

// ── Blob helpers ──────────────────────────────────────────────────────────────

/// Write `data` into `blobs_dir/<sha256-digest>` and return the full digest
/// string (`sha256:<hex>`).
fn write_blob(blobs_dir: &Path, data: &[u8]) -> Result<String> {
    let digest_hex = format!("{:x}", Sha256::digest(data));
    let path = blobs_dir.join(&digest_hex);
    std::fs::write(&path, data).with_context(|| format!("writing blob {digest_hex}"))?;
    Ok(format!("sha256:{digest_hex}"))
}

// ── Image config ──────────────────────────────────────────────────────────────

fn build_image_config(def: &Definition, oci: Option<&OciDef>, diff_id: &str) -> Result<String> {
    let created = Utc::now().to_rfc3339();
    let arch = if def.image.architecture.is_empty() {
        map_arch(&host_arch())
    } else {
        map_arch(&def.image.architecture)
    };

    let cmd: serde_json::Value = match oci.map(|o| o.cmd.as_slice()) {
        Some([_, ..]) => serde_json::to_value(oci.unwrap().cmd.clone())?,
        _ => serde_json::json!(["/bin/sh"]),
    };

    let entrypoint: serde_json::Value = match oci.map(|o| o.entrypoint.as_slice()) {
        Some([_, ..]) => serde_json::to_value(oci.unwrap().entrypoint.clone())?,
        _ => serde_json::Value::Null,
    };

    let mut labels: HashMap<String, String> = HashMap::new();
    labels.insert(
        "org.opencontainers.image.title".to_string(),
        def.effective_name(),
    );
    labels.insert(
        "org.opencontainers.image.created".to_string(),
        created.clone(),
    );
    labels.insert(
        "org.opencontainers.image.description".to_string(),
        def.image.description.clone(),
    );
    if let Some(o) = oci {
        labels.extend(o.labels.clone());
    }

    let exposed_ports: HashMap<String, serde_json::Value> = oci
        .map(|o| {
            o.exposed_ports
                .iter()
                .map(|p| (p.clone(), serde_json::json!({})))
                .collect()
        })
        .unwrap_or_default();

    let config = serde_json::json!({
        "created": created,
        "architecture": arch,
        "os": "linux",
        "config": {
            "Cmd": cmd,
            "Entrypoint": entrypoint,
            "Labels": labels,
            "ExposedPorts": exposed_ports,
        },
        "rootfs": {
            "type": "layers",
            "diff_ids": [diff_id],
        },
        "history": [{
            "created": created,
            "created_by": format!("incus-oci-builder {}", env!("CARGO_PKG_VERSION")),
            "comment": format!("built from {}", def.effective_name()),
        }],
    });

    serde_json::to_string(&config).context("serialising image config")
}

// ── Manifest ──────────────────────────────────────────────────────────────────

fn build_manifest(
    config_digest: &str,
    config_size: u64,
    layer_digest: &str,
    layer_size: u64,
) -> Result<String> {
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config_size,
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": layer_digest,
            "size": layer_size,
        }],
    });
    serde_json::to_string(&manifest).context("serialising manifest")
}

// ── Index ─────────────────────────────────────────────────────────────────────

fn build_index(manifest_digest: &str, manifest_size: u64, name: &str, tag: &str) -> Result<String> {
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": manifest_digest,
            "size": manifest_size,
            "annotations": {
                "org.opencontainers.image.ref.name": format!("{name}:{tag}"),
            },
        }],
    });
    serde_json::to_string(&index).context("serialising index")
}

// ── Architecture mapping ──────────────────────────────────────────────────────

/// Map distrobuilder/Incus architecture names to OCI/Go architecture strings.
fn map_arch(arch: &str) -> String {
    match arch {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        "armv7l" | "armhf" => "arm",
        "i686" | "i386" => "386",
        "ppc64le" => "ppc64le",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        other => other,
    }
    .to_string()
}
