//! Rootfs extraction from an Incus container into a local directory.
//!
//! Incus unified backup exports (`GET /1.0/instances/<name>/backups/export`)
//! return a tarball that may be:
//!
//! - Uncompressed tar
//! - gzip-compressed tar (.tar.gz)
//! - xz-compressed tar (.tar.xz)
//!
//! The archive contains:
//!   backup.yaml          — instance metadata
//!   rootfs/              — the actual filesystem tree (containers)
//!   rootfs.img           — qcow2 disk image (VMs, not handled here)
//!
//! We detect the compression format from the magic bytes, decompress, and
//! extract only entries under `rootfs/`, stripping that prefix so the output
//! directory represents `/` of the container.

use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::client::IncusClient;

/// Export the rootfs of `instance` into the directory at `dest`.
///
/// `dest` must exist. On return it contains the full filesystem tree of the
/// container (equivalent to `/` inside the container).
pub async fn export_rootfs_to_dir(client: &IncusClient, instance: &str, dest: &Path) -> Result<()> {
    info!(instance, dest = %dest.display(), "exporting rootfs");

    let tmp = tempfile::NamedTempFile::new().context("creating temp file for export")?;
    client
        .export_rootfs(instance, tmp.path())
        .await
        .context("streaming rootfs export from Incus")?;

    unpack_rootfs_tar(tmp.path(), dest).context("unpacking rootfs tarball")?;
    Ok(())
}

/// Detect compression and unpack the rootfs portion of an Incus export tarball.
pub(crate) fn unpack_rootfs_tar(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)
        .with_context(|| format!("opening archive {}", archive.display()))?;
    let mut reader = BufReader::new(file);

    // Peek at the first 6 bytes to detect compression format.
    let mut magic = [0u8; 6];
    reader
        .read_exact(&mut magic)
        .context("reading magic bytes")?;

    // Reopen — BufReader doesn't support seeking back easily.
    let file = std::fs::File::open(archive)
        .with_context(|| format!("reopening archive {}", archive.display()))?;

    match magic {
        // xz magic: 0xFD 0x37 0x7A 0x58 0x5A 0x00
        [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] => {
            debug!("detected xz-compressed export");
            let decoder = xz2::read::XzDecoder::new(file);
            extract_tar(decoder, dest)
        }
        // gzip magic: 0x1F 0x8B
        [0x1F, 0x8B, ..] => {
            debug!("detected gzip-compressed export");
            let decoder = flate2::read::GzDecoder::new(file);
            extract_tar(decoder, dest)
        }
        // Uncompressed tar: starts with a filename in the first 100 bytes,
        // magic at offset 257 is "ustar". We just try it directly.
        _ => {
            debug!("assuming uncompressed tar export");
            extract_tar(file, dest)
        }
    }
}

/// Extract entries under `rootfs/` from a tar stream into `dest`.
fn extract_tar<R: Read>(reader: R, dest: &Path) -> Result<()> {
    let mut ar = tar::Archive::new(reader);
    // Preserve permissions from the archive. Ownership preservation is
    // best-effort — skip it when running as non-root or when the archive
    // has malformed uid/gid fields (common in synthetic test archives).
    ar.set_preserve_permissions(true);
    ar.set_preserve_ownerships(false);
    ar.set_unpack_xattrs(false);

    let mut extracted = 0usize;

    for entry in ar.entries().context("reading tar entries")? {
        let mut entry = entry.context("reading tar entry")?;
        let raw_path = entry.path().context("reading entry path")?.to_path_buf();

        // Accept both "rootfs/..." and bare paths (some Incus versions omit
        // the prefix when exporting a stopped container's filesystem directly).
        let rel = if let Ok(p) = raw_path.strip_prefix("rootfs") {
            p.to_path_buf()
        } else if raw_path.starts_with("backup.yaml") || raw_path.starts_with("index.yaml") {
            debug!(path = %raw_path.display(), "skipping metadata entry");
            continue;
        } else {
            // Bare path — treat as rootfs-relative.
            raw_path.clone()
        };

        if rel.as_os_str().is_empty() {
            continue; // skip the rootfs/ directory entry itself
        }

        // Guard against path traversal.
        let target = dest.join(&rel);
        if !target.starts_with(dest) {
            warn!(path = %rel.display(), "skipping path traversal entry");
            continue;
        }

        // Create parent directories as needed.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating dir {}", parent.display()))?;
        }

        entry
            .unpack(&target)
            .with_context(|| format!("unpacking {}", rel.display()))?;
        extracted += 1;
    }

    info!(extracted, dest = %dest.display(), "rootfs extraction complete");
    Ok(())
}
