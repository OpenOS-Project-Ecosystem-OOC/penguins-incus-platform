//! incus-oci-builder library crate.
//!
//! Exposes the core modules for use in integration tests and as a library.

pub mod arch;
pub mod builder;
pub mod cache;
pub mod definition;
pub mod incus;
pub mod oci;
pub mod progress;

// Re-export preflight for integration tests.
pub use builder::preflight;

/// Exposes internal extraction logic for integration and unit tests.
pub mod export_test_helper {
    use anyhow::Result;
    use std::path::Path;
    pub fn unpack(archive: &Path, dest: &Path) -> Result<()> {
        crate::incus::export::unpack_rootfs_tar(archive, dest)
    }
}
