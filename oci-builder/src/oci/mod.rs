//! OCI image assembly.
//!
//! Takes a rootfs directory and produces a valid OCI image layout on disk,
//! which can then be pushed to a registry or loaded into a container runtime.
//!
//! The layout follows the OCI Image Layout Specification:
//!   https://github.com/opencontainers/image-spec/blob/main/image-layout.md
//!
//!   <output>/
//!     oci-layout          — version marker
//!     index.json          — top-level index
//!     blobs/
//!       sha256/
//!         <digest>        — config blob
//!         <digest>        — layer blob (compressed tar)
//!         <digest>        — manifest blob

pub mod commit;
pub mod convert;
pub mod layer;
pub mod multiarch;
pub mod push;
