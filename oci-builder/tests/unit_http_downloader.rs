//! Unit tests for the rootfs-http downloader.
//!
//! Uses `wiremock` to spin up a local HTTP server so no real network access
//! is required. Tests cover:
//!
//! - Successful download + extraction of gzip and xz tarballs
//! - Checksum verification (pass and fail)
//! - HTTP error responses
//! - Redirect following
//! - Bearer token, Basic, and custom header authentication

use std::io::Write;

use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use incus_oci_builder::incus::bootstrap::rootfs_http;

// ── Tarball builders ──────────────────────────────────────────────────────────

fn make_gzip_rootfs_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut raw = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut raw);
        for (p, data) in entries {
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(data.len() as u64);
            hdr.set_mode(0o644);
            hdr.set_cksum();
            ar.append_data(&mut hdr, p, *data).unwrap();
        }
        ar.finish().unwrap();
    }
    let mut gz = Vec::new();
    let mut enc = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    gz
}

fn make_xz_rootfs_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut raw = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut raw);
        for (p, data) in entries {
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(data.len() as u64);
            hdr.set_mode(0o644);
            hdr.set_cksum();
            ar.append_data(&mut hdr, p, *data).unwrap();
        }
        ar.finish().unwrap();
    }
    let mut xz = Vec::new();
    let mut enc = xz2::write::XzEncoder::new(&mut xz, 1);
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    xz
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn download_gzip_tar_no_checksum() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[
        ("etc/hostname", b"http-host"),
        ("etc/os-release", b"ID=test\n"),
    ]);

    Mock::given(method("GET"))
        .and(path("/rootfs.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/rootfs.tar.gz", server.uri());

    rootfs_http(&url, dest.path(), None, None)
        .await
        .expect("download should succeed");

    assert!(
        dest.path().join("etc/hostname").exists(),
        "hostname missing"
    );
    assert_eq!(
        std::fs::read(dest.path().join("etc/hostname")).unwrap(),
        b"http-host"
    );
}

#[tokio::test]
async fn download_xz_tar_no_checksum() {
    let server = MockServer::start().await;
    let tarball = make_xz_rootfs_tar(&[("usr/bin/hello", b"#!/bin/sh\necho hi\n")]);

    Mock::given(method("GET"))
        .and(path("/rootfs.tar.xz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/rootfs.tar.xz", server.uri());

    rootfs_http(&url, dest.path(), None, None)
        .await
        .expect("xz download should succeed");

    assert!(dest.path().join("usr/bin/hello").exists());
}

#[tokio::test]
async fn download_with_correct_checksum() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/motd", b"welcome\n")]);
    let checksum = format!("sha256:{}", sha256_hex(&tarball));

    Mock::given(method("GET"))
        .and(path("/rootfs.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/rootfs.tar.gz", server.uri());

    rootfs_http(&url, dest.path(), Some(&checksum), None)
        .await
        .expect("correct checksum should pass");

    assert!(dest.path().join("etc/motd").exists());
}

#[tokio::test]
async fn download_with_wrong_checksum_is_rejected() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"host")]);
    let bad_checksum = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    Mock::given(method("GET"))
        .and(path("/rootfs.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/rootfs.tar.gz", server.uri());

    let err = rootfs_http(&url, dest.path(), Some(bad_checksum), None)
        .await
        .expect_err("wrong checksum should be rejected");

    // Walk the full error chain — the root cause is wrapped by with_context.
    let chain = format!("{err:#}");
    assert!(
        chain.contains("checksum mismatch"),
        "error chain should mention checksum mismatch, got: {chain}"
    );
}

#[tokio::test]
async fn checksum_missing_prefix_is_rejected() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"host")]);

    Mock::given(method("GET"))
        .and(path("/rootfs.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/rootfs.tar.gz", server.uri());

    // Checksum without "sha256:" prefix should be rejected.
    let err = rootfs_http(&url, dest.path(), Some("abcdef1234"), None)
        .await
        .expect_err("malformed checksum should be rejected");

    let chain = format!("{err:#}");
    assert!(
        chain.contains("sha256:"),
        "error chain should mention expected format, got: {chain}"
    );
}

#[tokio::test]
async fn http_error_response_is_propagated() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.tar.gz"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/missing.tar.gz", server.uri());

    let err = rootfs_http(&url, dest.path(), None, None)
        .await
        .expect_err("404 should be an error");

    assert!(
        err.to_string().contains("404"),
        "error should mention HTTP 404, got: {err}"
    );
}

#[tokio::test]
async fn redirect_is_followed() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"redirected-host")]);

    // /redirect -> 301 -> /final
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(301)
                .insert_header("Location", format!("{}/final", server.uri()).as_str()),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/redirect", server.uri());

    rootfs_http(&url, dest.path(), None, None)
        .await
        .expect("redirect should be followed");

    assert!(dest.path().join("etc/hostname").exists());
    assert_eq!(
        std::fs::read(dest.path().join("etc/hostname")).unwrap(),
        b"redirected-host"
    );
}

// ── Authentication tests ──────────────────────────────────────────────────────

use incus_oci_builder::definition::HttpAuth;
use wiremock::matchers::header;

#[tokio::test]
async fn bearer_token_is_sent_in_authorization_header() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"auth-host")]);

    Mock::given(method("GET"))
        .and(path("/secure"))
        .and(header("authorization", "Bearer secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/secure", server.uri());
    let auth = HttpAuth::Bearer {
        token: "secret-token".to_string(),
    };

    rootfs_http(&url, dest.path(), None, Some(&auth))
        .await
        .expect("bearer auth download should succeed");

    assert!(dest.path().join("etc/hostname").exists());
}

#[tokio::test]
async fn basic_auth_is_sent_correctly() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"basic-host")]);

    // Basic auth for user:pass encodes to dXNlcjpwYXNz
    Mock::given(method("GET"))
        .and(path("/basic"))
        .and(header("authorization", "Basic dXNlcjpwYXNz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/basic", server.uri());
    let auth = HttpAuth::Basic {
        username: "user".to_string(),
        password: "pass".to_string(),
    };

    rootfs_http(&url, dest.path(), None, Some(&auth))
        .await
        .expect("basic auth download should succeed");

    assert!(dest.path().join("etc/hostname").exists());
}

#[tokio::test]
async fn custom_header_auth_is_sent() {
    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"header-host")]);

    Mock::given(method("GET"))
        .and(path("/header-auth"))
        .and(header("x-auth-token", "my-token-value"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/header-auth", server.uri());
    let auth = HttpAuth::Header {
        name: "x-auth-token".to_string(),
        value: "my-token-value".to_string(),
    };

    rootfs_http(&url, dest.path(), None, Some(&auth))
        .await
        .expect("custom header auth download should succeed");

    assert!(dest.path().join("etc/hostname").exists());
}

#[tokio::test]
async fn env_var_expansion_in_bearer_token() {
    // Set a temporary env var and verify it gets expanded.
    std::env::set_var("IOB_TEST_TOKEN", "expanded-token");

    let server = MockServer::start().await;
    let tarball = make_gzip_rootfs_tar(&[("etc/hostname", b"env-host")]);

    Mock::given(method("GET"))
        .and(path("/env-auth"))
        .and(header("authorization", "Bearer expanded-token"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
        .mount(&server)
        .await;

    let dest = tempfile::TempDir::new().unwrap();
    let url = format!("{}/env-auth", server.uri());
    let auth = HttpAuth::Bearer {
        token: "$IOB_TEST_TOKEN".to_string(),
    };

    rootfs_http(&url, dest.path(), None, Some(&auth))
        .await
        .expect("env-expanded bearer token should work");

    std::env::remove_var("IOB_TEST_TOKEN");
    assert!(dest.path().join("etc/hostname").exists());
}
