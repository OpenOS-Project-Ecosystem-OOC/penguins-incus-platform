//! Unit tests for OCI layer packing, image layout assembly, and format conversion.
//! These run without any Incus daemon.

use incus_oci_builder::definition::Definition;
use incus_oci_builder::oci::commit::commit_layered_rootfs;
use incus_oci_builder::oci::commit::commit_rootfs;
use incus_oci_builder::oci::convert::{convert, OutputFormat};
use incus_oci_builder::oci::layer::pack_layer;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn minimal_def() -> Definition {
    let yaml = r#"
image:
  distribution: test
  release: "1.0"
  tag: "latest"
source:
  downloader: incus
  image: "images:test/1.0"
"#;
    serde_yaml::from_str(yaml).unwrap()
}

fn make_rootfs(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("etc")).unwrap();
    std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
    std::fs::write(dir.join("etc/hostname"), b"test-host").unwrap();
    std::fs::write(dir.join("etc/os-release"), b"ID=test\nVERSION_ID=1.0\n").unwrap();
    std::fs::write(dir.join("usr/bin/hello"), b"#!/bin/sh\necho hello\n").unwrap();
}

fn make_rootfs_v2(dir: &std::path::Path) {
    // Same as v1 but with an added file and a modified file — simulates a
    // second snapshot after package installation.
    make_rootfs(dir);
    std::fs::create_dir_all(dir.join("usr/lib")).unwrap();
    std::fs::write(dir.join("usr/lib/libfoo.so"), b"ELF...").unwrap();
    std::fs::write(dir.join("etc/hostname"), b"test-host-v2").unwrap(); // modified
}

// ── Layer packing ─────────────────────────────────────────────────────────────

#[test]
fn pack_layer_produces_valid_blob() {
    let rootfs = tempfile::TempDir::new().unwrap();
    make_rootfs(rootfs.path());

    let dest = tempfile::NamedTempFile::new().unwrap();
    let blob = pack_layer(rootfs.path(), dest.path()).expect("pack_layer should succeed");

    // Digest must be sha256: prefixed hex.
    assert!(
        blob.compressed_digest.starts_with("sha256:"),
        "compressed_digest missing sha256: prefix"
    );
    assert!(
        blob.diff_id.starts_with("sha256:"),
        "diff_id missing sha256: prefix"
    );
    assert!(blob.compressed_size > 0, "compressed_size must be > 0");

    // The two digests must differ (compressed ≠ uncompressed).
    assert_ne!(
        blob.compressed_digest, blob.diff_id,
        "compressed and uncompressed digests should differ"
    );

    // The blob file must exist and match the reported size.
    let on_disk = std::fs::metadata(dest.path()).unwrap().len();
    assert_eq!(
        on_disk, blob.compressed_size,
        "file size matches reported size"
    );
}

#[test]
fn pack_layer_empty_rootfs() {
    let rootfs = tempfile::TempDir::new().unwrap();
    let dest = tempfile::NamedTempFile::new().unwrap();
    // An empty rootfs should still produce a valid (tiny) layer.
    let blob = pack_layer(rootfs.path(), dest.path()).expect("empty rootfs should not fail");
    assert!(blob.compressed_size > 0);
}

// ── OCI layout assembly ───────────────────────────────────────────────────────

#[test]
fn commit_rootfs_produces_valid_layout() {
    let rootfs = tempfile::TempDir::new().unwrap();
    make_rootfs(rootfs.path());

    let output = tempfile::TempDir::new().unwrap();
    let def = minimal_def();

    commit_rootfs(rootfs.path(), output.path(), &def).expect("commit_rootfs should succeed");

    // Required OCI layout files.
    assert!(
        output.path().join("oci-layout").exists(),
        "oci-layout missing"
    );
    assert!(
        output.path().join("index.json").exists(),
        "index.json missing"
    );
    assert!(
        output.path().join("blobs").join("sha256").exists(),
        "blobs/sha256 missing"
    );

    // oci-layout must contain the version marker.
    let layout: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("oci-layout")).unwrap())
            .unwrap();
    assert_eq!(layout["imageLayoutVersion"], "1.0.0");

    // index.json must have exactly one manifest entry.
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("index.json")).unwrap())
            .unwrap();
    let manifests = index["manifests"].as_array().unwrap();
    assert_eq!(manifests.len(), 1, "expected exactly one manifest");

    // The manifest blob must exist and be valid JSON.
    let manifest_digest = manifests[0]["digest"].as_str().unwrap();
    let hex = manifest_digest.strip_prefix("sha256:").unwrap();
    let manifest_path = output.path().join("blobs").join("sha256").join(hex);
    assert!(manifest_path.exists(), "manifest blob missing");

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schemaVersion"], 2);

    // Config blob must exist.
    let config_digest = manifest["config"]["digest"].as_str().unwrap();
    let config_hex = config_digest.strip_prefix("sha256:").unwrap();
    assert!(
        output
            .path()
            .join("blobs")
            .join("sha256")
            .join(config_hex)
            .exists(),
        "config blob missing"
    );

    // Layer blob must exist and match the reported size.
    let layer = &manifest["layers"][0];
    let layer_digest = layer["digest"].as_str().unwrap();
    let layer_size = layer["size"].as_u64().unwrap();
    let layer_hex = layer_digest.strip_prefix("sha256:").unwrap();
    let layer_path = output.path().join("blobs").join("sha256").join(layer_hex);
    assert!(layer_path.exists(), "layer blob missing");
    assert_eq!(
        std::fs::metadata(&layer_path).unwrap().len(),
        layer_size,
        "layer blob size mismatch"
    );
}

#[test]
fn commit_rootfs_image_name_and_tag() {
    let rootfs = tempfile::TempDir::new().unwrap();
    make_rootfs(rootfs.path());
    let output = tempfile::TempDir::new().unwrap();
    let def = minimal_def();

    commit_rootfs(rootfs.path(), output.path(), &def).unwrap();

    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("index.json")).unwrap())
            .unwrap();

    let ref_name = index["manifests"][0]["annotations"]["org.opencontainers.image.ref.name"]
        .as_str()
        .unwrap();

    // effective_name() = "test/1.0", effective_tag() = "latest"
    assert_eq!(ref_name, "test/1.0:latest");
}

#[test]
fn commit_rootfs_with_oci_labels() {
    let rootfs = tempfile::TempDir::new().unwrap();
    make_rootfs(rootfs.path());
    let output = tempfile::TempDir::new().unwrap();

    let yaml = r#"
image:
  distribution: myapp
  release: "2.0"
  tag: "stable"
source:
  downloader: incus
  image: "images:myapp/2.0"
oci:
  cmd: ["/usr/bin/myapp"]
  labels:
    com.example.team: "platform"
"#;
    let def: Definition = serde_yaml::from_str(yaml).unwrap();
    commit_rootfs(rootfs.path(), output.path(), &def).unwrap();

    // Read config blob and verify label is present.
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("index.json")).unwrap())
            .unwrap();
    let manifest_digest = index["manifests"][0]["digest"].as_str().unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(manifest_digest.strip_prefix("sha256:").unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    let config_digest = manifest["config"]["digest"].as_str().unwrap();
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(config_digest.strip_prefix("sha256:").unwrap()),
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        config["config"]["Labels"]["com.example.team"], "platform",
        "custom label missing from image config"
    );
    assert_eq!(
        config["config"]["Cmd"][0], "/usr/bin/myapp",
        "cmd missing from image config"
    );
}

// ── Layered OCI commit ────────────────────────────────────────────────────────

#[test]
fn commit_layered_two_snapshots_produces_multi_layer_manifest() {
    let snap0 = tempfile::TempDir::new().unwrap();
    let snap1 = tempfile::TempDir::new().unwrap();
    make_rootfs(snap0.path());
    make_rootfs_v2(snap1.path());

    let output = tempfile::TempDir::new().unwrap();
    let def = minimal_def();

    commit_layered_rootfs(&[snap0.path(), snap1.path()], output.path(), &def)
        .expect("layered commit should succeed");

    // Verify layout files exist.
    assert!(output.path().join("oci-layout").exists());
    assert!(output.path().join("index.json").exists());

    // Manifest must have exactly 2 layers.
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("index.json")).unwrap())
            .unwrap();
    let manifest_digest = index["manifests"][0]["digest"].as_str().unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(manifest_digest.strip_prefix("sha256:").unwrap()),
        )
        .unwrap(),
    )
    .unwrap();

    let layers = manifest["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "expected 2 layers for 2 snapshots");

    // Both layer blobs must exist on disk.
    for layer in layers {
        let digest = layer["digest"].as_str().unwrap();
        let hex = digest.strip_prefix("sha256:").unwrap();
        assert!(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(hex)
                .exists(),
            "layer blob {hex} missing"
        );
    }

    // Config must have 2 diff_ids.
    let config_digest = manifest["config"]["digest"].as_str().unwrap();
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(config_digest.strip_prefix("sha256:").unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        config["rootfs"]["diff_ids"].as_array().unwrap().len(),
        2,
        "expected 2 diff_ids"
    );
}

#[test]
fn commit_layered_single_snapshot_produces_one_layer() {
    let snap0 = tempfile::TempDir::new().unwrap();
    make_rootfs(snap0.path());
    let output = tempfile::TempDir::new().unwrap();
    let def = minimal_def();

    commit_layered_rootfs(&[snap0.path()], output.path(), &def)
        .expect("single-snapshot layered commit should succeed");

    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.path().join("index.json")).unwrap())
            .unwrap();
    let manifest_digest = index["manifests"][0]["digest"].as_str().unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            output
                .path()
                .join("blobs")
                .join("sha256")
                .join(manifest_digest.strip_prefix("sha256:").unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["layers"].as_array().unwrap().len(), 1);
}

#[test]
fn commit_layered_empty_dirs_is_error() {
    let output = tempfile::TempDir::new().unwrap();
    let def = minimal_def();
    let err =
        commit_layered_rootfs(&[], output.path(), &def).expect_err("empty dirs should be an error");
    assert!(err.to_string().contains("at least one"));
}

// ── docker-archive conversion tests ──────────────────────────────────────────

#[test]
fn docker_archive_layer_is_uncompressed_tar() {
    let rootfs = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(rootfs.path().join("etc")).unwrap();
    std::fs::write(rootfs.path().join("etc/hostname"), b"docker-test").unwrap();

    let oci_dir = tempfile::TempDir::new().unwrap();
    let def = minimal_def();
    commit_rootfs(rootfs.path(), oci_dir.path(), &def).unwrap();

    let archive_path = convert(oci_dir.path(), OutputFormat::DockerArchive, "test", "1.0").unwrap();
    assert!(archive_path.exists(), "docker archive should exist");

    // Collect all entries from the outer tar.
    let archive_file = std::fs::File::open(&archive_path).unwrap();
    let mut outer = tar::Archive::new(archive_file);
    let entries: Vec<(String, Vec<u8>)> = outer
        .entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let path = e.path().unwrap().to_string_lossy().to_string();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut data).unwrap();
            (path, data)
        })
        .collect();

    // Find the layer.tar entry.
    let (layer_path, layer_bytes) = entries
        .iter()
        .find(|(p, _)| p.ends_with("/layer.tar"))
        .expect("docker archive should contain a layer.tar entry");

    // Layer directory name must be a 64-char hex chain-ID (not the compressed blob digest).
    let layer_dir = layer_path.trim_end_matches("/layer.tar");
    assert_eq!(
        layer_dir.len(),
        64,
        "layer dir should be a 64-char chain-ID hex, got: {layer_dir}"
    );
    assert!(
        layer_dir.chars().all(|c| c.is_ascii_hexdigit()),
        "layer dir should be hex, got: {layer_dir}"
    );

    // Must NOT start with gzip magic (0x1f 0x8b).
    assert!(
        !(layer_bytes.len() >= 2 && layer_bytes[0] == 0x1f && layer_bytes[1] == 0x8b),
        "layer.tar must be uncompressed, but starts with gzip magic"
    );

    // Must be a valid tar containing etc/hostname.
    let mut inner = tar::Archive::new(layer_bytes.as_slice());
    let has_hostname = inner.entries().unwrap().any(|e| {
        e.unwrap()
            .path()
            .unwrap()
            .to_string_lossy()
            .contains("hostname")
    });
    assert!(has_hostname, "layer.tar should contain etc/hostname");

    // manifest.json Layers entry must reference the chain-ID directory.
    let (_, manifest_bytes) = entries
        .iter()
        .find(|(p, _)| p == "manifest.json" || p == "./manifest.json")
        .expect("docker archive must contain manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes).unwrap();
    let manifest_layer = manifest[0]["Layers"][0].as_str().unwrap();
    assert_eq!(
        manifest_layer,
        format!("{layer_dir}/layer.tar"),
        "manifest.json Layers entry should match chain-ID directory"
    );
}

#[test]
fn oci_archive_is_valid_tar_with_index_json() {
    let rootfs = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(rootfs.path().join("etc")).unwrap();
    std::fs::write(rootfs.path().join("etc/hostname"), b"oci-test").unwrap();

    let oci_dir = tempfile::TempDir::new().unwrap();
    let def = minimal_def();
    commit_rootfs(rootfs.path(), oci_dir.path(), &def).unwrap();

    let archive_path = convert(
        oci_dir.path(),
        OutputFormat::OciArchive,
        "myorg/ubuntu",
        "noble",
    )
    .unwrap();
    assert!(archive_path.exists());

    // Filename must be derived from image name + tag, not the OCI dir name.
    let filename = archive_path.file_name().unwrap().to_string_lossy();
    assert_eq!(
        filename, "myorg-ubuntu-noble.tar",
        "oci-archive filename should be <name>-<tag>.tar, got: {filename}"
    );

    // The archive must contain index.json and oci-layout.
    let file = std::fs::File::open(&archive_path).unwrap();
    let mut archive = tar::Archive::new(file);
    let entries: Vec<String> = archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(entries
        .iter()
        .any(|p| p == "index.json" || p == "./index.json"));
    assert!(entries
        .iter()
        .any(|p| p == "oci-layout" || p == "./oci-layout"));
}

#[test]
fn docker_archive_filename_derives_from_image_tag() {
    let rootfs = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(rootfs.path().join("etc")).unwrap();
    std::fs::write(rootfs.path().join("etc/hostname"), b"docker-name-test").unwrap();

    let oci_dir = tempfile::TempDir::new().unwrap();
    let def = minimal_def();
    commit_rootfs(rootfs.path(), oci_dir.path(), &def).unwrap();

    let archive_path = convert(
        oci_dir.path(),
        OutputFormat::DockerArchive,
        "myorg/ubuntu",
        "noble",
    )
    .unwrap();
    assert!(archive_path.exists());

    let filename = archive_path.file_name().unwrap().to_string_lossy();
    assert_eq!(
        filename, "myorg-ubuntu-noble-docker.tar",
        "docker-archive filename should be <name>-<tag>-docker.tar, got: {filename}"
    );
}

// ── Architecture propagation tests ───────────────────────────────────────────

#[test]
fn commit_rootfs_arch_written_to_image_config() {
    let rootfs = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(rootfs.path().join("etc")).unwrap();
    std::fs::write(rootfs.path().join("etc/hostname"), b"arch-test").unwrap();

    let oci_dir = tempfile::TempDir::new().unwrap();

    // Build a definition with an explicit non-host architecture.
    let yaml = r#"
image:
  distribution: test
  release: "1.0"
  architecture: aarch64
source:
  downloader: incus
  image: "images:test/1.0"
"#;
    let def: Definition = serde_yaml::from_str(yaml).unwrap();
    commit_rootfs(rootfs.path(), oci_dir.path(), &def).unwrap();

    // Read the manifest to find the config blob.
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(oci_dir.path().join("index.json")).unwrap()).unwrap();
    let config_digest = index["manifests"][0]["digest"].as_str().unwrap();
    let hex = config_digest.trim_start_matches("sha256:");

    // Read the manifest blob to find the config digest.
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(oci_dir.path().join("blobs").join("sha256").join(hex)).unwrap(),
    )
    .unwrap();
    let cfg_digest = manifest["config"]["digest"].as_str().unwrap();
    let cfg_hex = cfg_digest.trim_start_matches("sha256:");

    // Read the image config and check the architecture field.
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(oci_dir.path().join("blobs").join("sha256").join(cfg_hex)).unwrap(),
    )
    .unwrap();

    assert_eq!(
        config["architecture"].as_str().unwrap(),
        "arm64",
        "OCI config architecture should be 'arm64' (OCI name for aarch64)"
    );
}

#[test]
fn commit_rootfs_empty_arch_uses_host_arch() {
    let rootfs = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(rootfs.path().join("etc")).unwrap();
    std::fs::write(rootfs.path().join("etc/hostname"), b"host-arch-test").unwrap();

    let oci_dir = tempfile::TempDir::new().unwrap();

    // Definition with no architecture set.
    let yaml = r#"
image:
  distribution: test
  release: "1.0"
source:
  downloader: incus
  image: "images:test/1.0"
"#;
    let def: Definition = serde_yaml::from_str(yaml).unwrap();
    commit_rootfs(rootfs.path(), oci_dir.path(), &def).unwrap();

    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(oci_dir.path().join("index.json")).unwrap()).unwrap();
    let config_digest = index["manifests"][0]["digest"].as_str().unwrap();
    let hex = config_digest.trim_start_matches("sha256:");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(oci_dir.path().join("blobs").join("sha256").join(hex)).unwrap(),
    )
    .unwrap();
    let cfg_digest = manifest["config"]["digest"].as_str().unwrap();
    let cfg_hex = cfg_digest.trim_start_matches("sha256:");
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(oci_dir.path().join("blobs").join("sha256").join(cfg_hex)).unwrap(),
    )
    .unwrap();

    // Should be a non-empty, valid OCI arch string (not "amd64" hardcoded).
    let arch = config["architecture"].as_str().unwrap();
    assert!(!arch.is_empty(), "architecture must not be empty");
    // On the CI runner (x86_64) this will be "amd64"; on arm64 it will be "arm64".
    // Either way it must not be the old hardcoded "amd64" on non-x86 hosts.
    assert!(
        ["amd64", "arm64", "arm", "386", "ppc64le", "s390x", "riscv64"].contains(&arch),
        "unexpected architecture value: {arch}"
    );
}
