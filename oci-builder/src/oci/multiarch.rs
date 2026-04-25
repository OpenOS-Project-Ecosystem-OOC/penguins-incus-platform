//! OCI multi-architecture image index assembly.
//!
//! After building per-architecture OCI layouts (one directory per arch),
//! this module merges them into a single OCI Image Layout with a top-level
//! `index.json` that is an OCI Image Index (mediaType
//! `application/vnd.oci.image.index.v1+json`).
//!
//! The resulting layout can be pushed to a registry as a multi-arch manifest
//! or loaded by tools that understand OCI image indexes.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing::info;

/// Merge multiple per-arch OCI layouts into a single multi-arch OCI layout.
///
/// `arch_dirs` is a list of `(oci_arch, layout_dir)` pairs, e.g.:
/// ```text
/// [("amd64", "/tmp/build-amd64"), ("arm64", "/tmp/build-arm64")]
/// ```
///
/// The blobs from each layout are copied into `output_dir/blobs/sha256/`.
/// A new `index.json` is written that references each arch's manifest.
pub fn assemble_index(arch_dirs: &[(&str, &Path)], output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir.join("blobs").join("sha256"))
        .context("creating output blobs dir")?;

    // Write oci-layout marker.
    fs::write(
        output_dir.join("oci-layout"),
        r#"{"imageLayoutVersion":"1.0.0"}"#,
    )
    .context("writing oci-layout")?;

    let mut manifests: Vec<Value> = Vec::new();

    for (oci_arch, src_dir) in arch_dirs {
        // Copy all blobs from the per-arch layout.
        let src_blobs = src_dir.join("blobs").join("sha256");
        if src_blobs.exists() {
            for entry in fs::read_dir(&src_blobs)
                .with_context(|| format!("reading blobs from {}", src_dir.display()))?
            {
                let entry = entry?;
                let dst = output_dir
                    .join("blobs")
                    .join("sha256")
                    .join(entry.file_name());
                if !dst.exists() {
                    fs::copy(entry.path(), &dst)
                        .with_context(|| format!("copying blob {}", entry.path().display()))?;
                }
            }
        }

        // Read the per-arch index.json to find its manifest descriptor.
        let index_path = src_dir.join("index.json");
        let index_bytes =
            fs::read(&index_path).with_context(|| format!("reading {}", index_path.display()))?;
        let index: Value =
            serde_json::from_slice(&index_bytes).context("parsing per-arch index.json")?;

        let manifest_desc = index["manifests"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .with_context(|| {
                format!(
                    "no manifests in index.json for arch {oci_arch} at {}",
                    src_dir.display()
                )
            })?;

        // The manifest blob is already copied; read it to get its size/digest.
        let manifest_digest = manifest_desc["digest"]
            .as_str()
            .context("manifest descriptor missing digest")?;
        let manifest_blob = output_dir
            .join("blobs")
            .join("sha256")
            .join(manifest_digest.trim_start_matches("sha256:"));
        let manifest_bytes = fs::read(&manifest_blob)
            .with_context(|| format!("reading manifest blob {manifest_digest}"))?;

        // Build the index entry with platform annotation.
        let entry = json!({
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": manifest_digest,
            "size": manifest_bytes.len(),
            "platform": {
                "os": "linux",
                "architecture": oci_arch,
            }
        });
        manifests.push(entry);
        info!(
            arch = oci_arch,
            digest = manifest_digest,
            "added arch to index"
        );
    }

    // Write the top-level image index.
    let index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": manifests,
    });
    let index_bytes = serde_json::to_vec_pretty(&index).context("serialising image index")?;

    // Write index.json with its own digest as a blob (for registry push).
    let digest = hex::encode(Sha256::digest(&index_bytes));
    fs::write(
        output_dir.join("blobs").join("sha256").join(&digest),
        &index_bytes,
    )
    .context("writing index blob")?;
    fs::write(output_dir.join("index.json"), &index_bytes).context("writing index.json")?;

    info!(
        output = %output_dir.display(),
        arches = arch_dirs.len(),
        "multi-arch index assembled"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Build a minimal OCI layout in `dir` for the given arch.
    fn make_fake_layout(dir: &Path, arch: &str) -> Result<()> {
        fs::create_dir_all(dir.join("blobs").join("sha256"))?;
        fs::write(dir.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#)?;

        // Fake config blob.
        let config = json!({"architecture": arch, "os": "linux"});
        let config_bytes = serde_json::to_vec(&config)?;
        let config_digest = hex::encode(Sha256::digest(&config_bytes));
        fs::write(
            dir.join("blobs").join("sha256").join(&config_digest),
            &config_bytes,
        )?;

        // Fake layer blob.
        let layer_bytes = b"fake layer data";
        let layer_digest = hex::encode(Sha256::digest(layer_bytes));
        fs::write(
            dir.join("blobs").join("sha256").join(&layer_digest),
            layer_bytes,
        )?;

        // Manifest.
        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": format!("sha256:{config_digest}"),
                "size": config_bytes.len(),
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "digest": format!("sha256:{layer_digest}"),
                "size": layer_bytes.len(),
            }]
        });
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let manifest_digest = hex::encode(Sha256::digest(&manifest_bytes));
        fs::write(
            dir.join("blobs").join("sha256").join(&manifest_digest),
            &manifest_bytes,
        )?;

        // index.json pointing at the manifest.
        let index = json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": format!("sha256:{manifest_digest}"),
                "size": manifest_bytes.len(),
            }]
        });
        fs::write(dir.join("index.json"), serde_json::to_vec(&index)?)?;
        Ok(())
    }

    #[test]
    fn assemble_two_arch_index() {
        let amd64_dir = TempDir::new().unwrap();
        let arm64_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        make_fake_layout(amd64_dir.path(), "amd64").unwrap();
        make_fake_layout(arm64_dir.path(), "arm64").unwrap();

        assemble_index(
            &[("amd64", amd64_dir.path()), ("arm64", arm64_dir.path())],
            output_dir.path(),
        )
        .unwrap();

        // index.json must exist and have 2 manifests.
        let index_bytes = fs::read(output_dir.path().join("index.json")).unwrap();
        let index: Value = serde_json::from_slice(&index_bytes).unwrap();
        let manifests = index["manifests"].as_array().unwrap();
        assert_eq!(manifests.len(), 2);

        // Each manifest entry must have a platform field.
        let arches: Vec<&str> = manifests
            .iter()
            .map(|m| m["platform"]["architecture"].as_str().unwrap())
            .collect();
        assert!(arches.contains(&"amd64"));
        assert!(arches.contains(&"arm64"));

        // All blobs must be present in the output.
        let blobs_dir = output_dir.path().join("blobs").join("sha256");
        let blob_count = fs::read_dir(&blobs_dir).unwrap().count();
        // amd64: config + layer + manifest = 3
        // arm64: config + layer + manifest = 3 (layer is same fake data, deduped)
        // index blob = 1
        // Total unique = at most 7 (layer deduped → 6), at least 6.
        assert!(
            blob_count >= 6,
            "expected at least 6 blobs, got {blob_count}"
        );
    }

    #[test]
    fn assemble_single_arch_index() {
        let amd64_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();

        make_fake_layout(amd64_dir.path(), "amd64").unwrap();

        assemble_index(&[("amd64", amd64_dir.path())], output_dir.path()).unwrap();

        let index_bytes = fs::read(output_dir.path().join("index.json")).unwrap();
        let index: Value = serde_json::from_slice(&index_bytes).unwrap();
        assert_eq!(index["manifests"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn missing_index_json_is_error() {
        let empty_dir = TempDir::new().unwrap();
        let output_dir = TempDir::new().unwrap();
        // No index.json in empty_dir — should fail.
        let result = assemble_index(&[("amd64", empty_dir.path())], output_dir.path());
        assert!(result.is_err());
    }
}
