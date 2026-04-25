//! Unit tests for rootfs tarball extraction.
//! Builds synthetic tarballs in memory and verifies the extractor handles
//! all supported formats and edge cases correctly.

use std::io::Write;

// We test the internal logic by constructing tarballs and calling the
// public export path indirectly via a helper that writes to a temp dir.
// Since `unpack_rootfs_tar` is private, we test it through a thin shim
// that mirrors its logic using the public `export_rootfs_to_dir` signature
// but operates on a pre-written archive file.

fn build_tar_with_rootfs_prefix(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut buf);
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append_data(&mut header, path, *data).unwrap();
        }
        ar.finish().unwrap();
    }
    buf
}

fn build_gzip_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let raw = build_tar_with_rootfs_prefix(entries);
    let mut compressed = Vec::new();
    let mut enc = flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    compressed
}

fn build_xz_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let raw = build_tar_with_rootfs_prefix(entries);
    let mut compressed = Vec::new();
    let mut enc = xz2::write::XzEncoder::new(&mut compressed, 1);
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    compressed
}

fn write_and_extract(archive_bytes: &[u8]) -> tempfile::TempDir {
    let archive_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(archive_file.path(), archive_bytes).unwrap();

    let dest = tempfile::TempDir::new().unwrap();

    // Mirror the logic from export.rs directly.
    incus_oci_builder::export_test_helper::unpack(archive_file.path(), dest.path())
        .expect("extraction should succeed");

    dest
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn extract_uncompressed_tar_with_rootfs_prefix() {
    let entries = &[
        ("backup.yaml", b"metadata: true" as &[u8]),
        ("rootfs/etc/hostname", b"test-host"),
        ("rootfs/etc/os-release", b"ID=test\n"),
    ];
    let archive = build_tar_with_rootfs_prefix(entries);
    let dest = write_and_extract(&archive);

    assert!(
        dest.path().join("etc/hostname").exists(),
        "hostname missing"
    );
    assert!(
        dest.path().join("etc/os-release").exists(),
        "os-release missing"
    );
    assert!(
        !dest.path().join("backup.yaml").exists(),
        "backup.yaml should be excluded"
    );
    assert_eq!(
        std::fs::read(dest.path().join("etc/hostname")).unwrap(),
        b"test-host"
    );
}

#[test]
fn extract_gzip_tar() {
    let entries = &[
        ("rootfs/usr/bin/hello", b"#!/bin/sh\necho hi\n" as &[u8]),
        ("rootfs/etc/motd", b"welcome\n"),
    ];
    let archive = build_gzip_tar(entries);
    let dest = write_and_extract(&archive);

    assert!(dest.path().join("usr/bin/hello").exists());
    assert!(dest.path().join("etc/motd").exists());
}

#[test]
fn extract_xz_tar() {
    let entries = &[("rootfs/etc/hostname", b"xz-host" as &[u8])];
    let archive = build_xz_tar(entries);
    let dest = write_and_extract(&archive);

    assert!(dest.path().join("etc/hostname").exists());
    assert_eq!(
        std::fs::read(dest.path().join("etc/hostname")).unwrap(),
        b"xz-host"
    );
}

#[test]
fn extract_bare_paths_without_rootfs_prefix() {
    // Some Incus versions export without the rootfs/ prefix.
    let entries = &[
        ("etc/hostname", b"bare-host" as &[u8]),
        ("etc/os-release", b"ID=bare\n"),
        ("backup.yaml", b"meta: true"),
    ];
    let archive = build_tar_with_rootfs_prefix(entries);
    let dest = write_and_extract(&archive);

    // bare paths should be extracted as-is (excluding backup.yaml).
    assert!(dest.path().join("etc/hostname").exists());
    assert_eq!(
        std::fs::read(dest.path().join("etc/hostname")).unwrap(),
        b"bare-host"
    );
}
