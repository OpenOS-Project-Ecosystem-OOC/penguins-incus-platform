//! Integration tests for the full build pipeline.
//!
//! These tests require a running Incus daemon. They are marked `#[ignore]`
//! so they are skipped by default in CI and during `cargo test`.
//!
//! Run manually on a host with Incus installed:
//!   sudo cargo test --test integration_build -- --ignored --nocapture
//!
//! Or run a single test:
//!   sudo cargo test --test integration_build test_build_ubuntu_noble_minimal -- --ignored --nocapture

use std::path::Path;

use incus_oci_builder::builder::{build, BuildOptions};
use incus_oci_builder::definition::Definition;

const SOCKET: &str = "/var/lib/incus/unix.socket";

/// Panic with a clear message if the Incus socket is not present.
fn require_incus() {
    if !Path::new(SOCKET).exists() {
        panic!(
            "Incus socket not found at {SOCKET}. \
             Is Incus installed and running? Try: sudo incus admin init"
        );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires a running Incus daemon (run with --ignored)"]
async fn test_build_ubuntu_noble_minimal() {
    require_incus();

    let def_yaml = r#"
image:
  distribution: ubuntu
  release: noble
  architecture: x86_64

source:
  downloader: incus
  image: "images:ubuntu/noble"

packages:
  manager: apt
  update: false
  cleanup: true
  sets:
    - action: install
      packages:
        - ca-certificates

actions:
  - trigger: post-unpack
    action: "echo 'post-unpack OK' > /tmp/iob-test"
  - trigger: post-packages
    action: "test -f /usr/share/ca-certificates/mozilla/ISRG_Root_X1.crt"

files:
  - generator: hostname
    content: "iob-test"
  - generator: hosts
  - generator: dump
    path: /etc/iob-marker
    content: "built-by-incus-oci-builder"
"#;

    let def: Definition = serde_yaml::from_str(def_yaml).expect("parse definition");
    let output_dir = tempfile::TempDir::new().expect("temp output dir");
    let opts = BuildOptions {
        output_dir: output_dir.path().to_path_buf(),
        ..BuildOptions::default()
    };

    build(&def, &opts).await.expect("build should succeed");

    assert!(
        output_dir.path().join("oci-layout").exists(),
        "oci-layout marker missing"
    );
    assert!(
        output_dir.path().join("index.json").exists(),
        "index.json missing"
    );
    assert!(
        output_dir.path().join("blobs").join("sha256").exists(),
        "blobs/sha256 missing"
    );

    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(output_dir.path().join("index.json")).unwrap(),
    )
    .expect("index.json is valid JSON");
    assert!(
        index["manifests"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "index.json has no manifests"
    );
}

#[tokio::test]
#[ignore = "requires a running Incus daemon (run with --ignored)"]
async fn test_build_alpine_minimal() {
    require_incus();

    let def_yaml = r#"
image:
  distribution: alpine
  release: "3.19"

source:
  downloader: incus
  image: "images:alpine/3.19"

packages:
  manager: apk
  update: true
  cleanup: true
  sets:
    - action: install
      packages:
        - curl
"#;

    let def: Definition = serde_yaml::from_str(def_yaml).expect("parse definition");
    let output_dir = tempfile::TempDir::new().expect("temp output dir");
    let opts = BuildOptions {
        output_dir: output_dir.path().to_path_buf(),
        ..BuildOptions::default()
    };

    build(&def, &opts)
        .await
        .expect("alpine build should succeed");
    assert!(output_dir.path().join("oci-layout").exists());
}

#[tokio::test]
#[ignore = "requires a running Incus daemon (run with --ignored)"]
async fn test_copy_file_generator() {
    require_incus();

    let host_file = tempfile::NamedTempFile::new().expect("temp host file");
    std::fs::write(host_file.path(), b"hello from host").expect("write host file");

    let def_yaml = format!(
        r#"
image:
  distribution: ubuntu
  release: noble

source:
  downloader: incus
  image: "images:ubuntu/noble"

files:
  - generator: copy
    source: "{}"
    path: /etc/iob-copied
    mode: "0644"
"#,
        host_file.path().display()
    );

    let def: Definition = serde_yaml::from_str(&def_yaml).expect("parse definition");
    let output_dir = tempfile::TempDir::new().expect("temp output dir");
    let opts = BuildOptions {
        output_dir: output_dir.path().to_path_buf(),
        ..BuildOptions::default()
    };

    build(&def, &opts)
        .await
        .expect("copy file build should succeed");
    assert!(output_dir.path().join("oci-layout").exists());
}
