//! Compress a rootfs directory into an OCI layer blob (gzip-compressed tar).
//!
//! Returns the uncompressed and compressed digests plus the compressed size,
//! which are all required fields in the OCI manifest layer descriptor.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::info;

pub struct LayerBlob {
    /// SHA-256 digest of the compressed layer (prefixed with "sha256:").
    pub compressed_digest: String,
    /// SHA-256 digest of the uncompressed tar (the OCI DiffID).
    pub diff_id: String,
    /// Compressed size in bytes.
    pub compressed_size: u64,
}

/// Pack `rootfs_dir` into a gzip-compressed tar and write it to `dest`.
///
/// Returns metadata needed to populate the OCI manifest and image config.
pub fn pack_layer(rootfs_dir: &Path, dest: &Path) -> Result<LayerBlob> {
    info!(rootfs = %rootfs_dir.display(), dest = %dest.display(), "packing OCI layer");

    // We need both the uncompressed digest (DiffID) and the compressed digest.
    // Build the uncompressed tar in memory first, hash it, then compress.
    let mut uncompressed = Vec::new();
    build_tar(rootfs_dir, &mut uncompressed).context("building tar archive from rootfs")?;

    let diff_id = format!("sha256:{:x}", Sha256::digest(&uncompressed));

    // Compress with gzip.
    let mut compressed = Vec::new();
    {
        let mut encoder =
            flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default());
        encoder
            .write_all(&uncompressed)
            .context("compressing layer")?;
        encoder.finish().context("finalising gzip stream")?;
    }

    let compressed_digest = format!("sha256:{:x}", Sha256::digest(&compressed));
    let compressed_size = compressed.len() as u64;

    std::fs::write(dest, &compressed)
        .with_context(|| format!("writing layer blob to {}", dest.display()))?;

    Ok(LayerBlob {
        compressed_digest,
        diff_id,
        compressed_size,
    })
}

/// Build an uncompressed tar archive from `dir` into `writer`.
///
/// Paths inside the archive are relative to `dir` (i.e. the archive root
/// represents `/` of the container filesystem).
fn build_tar<W: Write>(dir: &Path, writer: W) -> Result<()> {
    let mut ar = tar::Builder::new(writer);
    ar.follow_symlinks(false);
    ar.append_dir_all(".", dir)
        .with_context(|| format!("appending {} to tar", dir.display()))?;
    ar.finish().context("finalising tar archive")?;
    Ok(())
}
