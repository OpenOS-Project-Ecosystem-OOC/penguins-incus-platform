//! Host-side rootfs bootstrappers.
//!
//! These run on the host before an Incus container is created, producing a
//! plain rootfs directory that is then imported into Incus as a local image
//! and used as the container's source.
//!
//! Supported bootstrappers:
//! - `debootstrap`  — Debian/Ubuntu
//! - `rpmbootstrap` — Fedora/RHEL/CentOS via `dnf --installroot`
//! - `rootfs-http`  — Download a pre-built rootfs tarball over HTTP/HTTPS

use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tracing::info;

/// Bootstrap a Debian/Ubuntu rootfs into `dest` using `debootstrap`.
///
/// `suite` is the release codename (e.g. `noble`, `bookworm`).
/// `mirror` is the APT mirror URL; defaults to the official archive.
/// `components` are the APT components (e.g. `["main", "universe"]`).
pub async fn debootstrap(
    suite: &str,
    dest: &Path,
    mirror: Option<&str>,
    components: &[String],
) -> Result<()> {
    let debootstrap = which::which("debootstrap")
        .context("debootstrap not found — install it with: apt-get install debootstrap")?;

    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating rootfs dir {}", dest.display()))?;

    let mirror_url = mirror.unwrap_or("http://deb.debian.org/debian");
    let components_str = if components.is_empty() {
        "main".to_string()
    } else {
        components.join(",")
    };

    info!(suite, dest = %dest.display(), mirror = mirror_url, "running debootstrap");

    let status = tokio::process::Command::new(&debootstrap)
        .arg("--components")
        .arg(&components_str)
        .arg("--variant=minbase")
        .arg(suite)
        .arg(dest)
        .arg(mirror_url)
        .status()
        .await
        .context("running debootstrap")?;

    if !status.success() {
        anyhow::bail!("debootstrap failed with status {status}");
    }

    info!(suite, "debootstrap complete");
    Ok(())
}

/// Bootstrap a Fedora/RHEL rootfs into `dest` using `dnf --installroot`.
///
/// `release` is the Fedora release number (e.g. `"40"`) or RHEL version.
/// `mirror` is an optional repo URL override.
/// `seed_packages` overrides the default minimal package set. When empty,
/// defaults to `["basesystem", "bash", "coreutils", "dnf"]`.
pub async fn rpmbootstrap(
    release: &str,
    dest: &Path,
    mirror: Option<&str>,
    seed_packages: &[String],
) -> Result<()> {
    let dnf = which::which("dnf")
        .or_else(|_| which::which("dnf5"))
        .context("dnf/dnf5 not found — install it with your system package manager")?;

    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating rootfs dir {}", dest.display()))?;

    let packages: Vec<&str> = if seed_packages.is_empty() {
        vec!["basesystem", "bash", "coreutils", "dnf"]
    } else {
        seed_packages.iter().map(String::as_str).collect()
    };

    info!(
        release,
        dest = %dest.display(),
        ?packages,
        "running dnf installroot"
    );

    let mut cmd = tokio::process::Command::new(&dnf);
    cmd.args([
        "install",
        "--installroot",
        dest.to_str().unwrap(),
        "--releasever",
        release,
        "--setopt=install_weak_deps=False",
        "-y",
    ]);
    cmd.args(&packages);

    if let Some(repo) = mirror {
        cmd.arg(format!("--repofrompath=bootstrap,{repo}"));
        cmd.arg("--repo=bootstrap");
    }

    let status = cmd.status().await.context("running dnf installroot")?;
    if !status.success() {
        anyhow::bail!("dnf installroot failed with status {status}");
    }

    info!(release, "rpmbootstrap complete");
    Ok(())
}

/// Download a pre-built rootfs tarball from `url` and unpack it into `dest`.
///
/// Supports the same compression formats as the Incus export path:
/// uncompressed tar, gzip-compressed tar, and xz-compressed tar.
/// The archive may contain a `rootfs/` prefix (Incus-style) or bare paths
/// (most third-party rootfs tarballs) — both are handled transparently.
///
/// `checksum` is an optional `sha256:<hex>` string. When provided the
/// downloaded bytes are verified before unpacking; the download is rejected
/// if the digest does not match.
pub async fn rootfs_http(
    url: &str,
    dest: &Path,
    checksum: Option<&str>,
    auth: Option<&crate::definition::HttpAuth>,
) -> Result<()> {
    info!(url, dest = %dest.display(), "downloading rootfs tarball");

    // Stream the download into a temp file so we can verify the checksum
    // and detect the compression format before unpacking.
    let tmp = tempfile::NamedTempFile::new().context("creating temp file for download")?;
    download_to_file(url, tmp.path(), auth).await?;

    // Verify checksum if provided.
    if let Some(expected) = checksum {
        verify_checksum(tmp.path(), expected)
            .with_context(|| format!("checksum verification failed for {url}"))?;
    }

    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating rootfs dir {}", dest.display()))?;

    // Reuse the same extraction logic as the Incus export path.
    crate::incus::export::unpack_rootfs_tar(tmp.path(), dest)
        .context("unpacking downloaded rootfs tarball")?;

    info!(url, "rootfs download and extraction complete");
    Ok(())
}

/// Stream `url` into `dest`, following redirects, with optional authentication.
async fn download_to_file(
    url: &str,
    dest: &Path,
    auth: Option<&crate::definition::HttpAuth>,
) -> Result<()> {
    use crate::definition::HttpAuth;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("building HTTP client for download")?;

    let mut req = client.get(url);

    // Apply authentication if configured. Resolve env vars at call time so
    // secrets are never stored in the definition struct.
    if let Some(auth) = auth {
        let resolved = auth.resolve_env();
        req = match &resolved {
            HttpAuth::Bearer { token } => {
                req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            }
            HttpAuth::Basic { username, password } => req.basic_auth(username, Some(password)),
            HttpAuth::Header { name, value } => req.header(name.as_str(), value.as_str()),
        };
    }

    let mut resp = req.send().await.with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        anyhow::bail!("download failed: HTTP {} for {url}", resp.status());
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("creating download file {}", dest.display()))?;

    let mut downloaded: u64 = 0;
    while let Some(chunk) = resp.chunk().await.context("reading download stream")? {
        file.write_all(&chunk).await.context("writing chunk")?;
        downloaded += chunk.len() as u64;
    }
    file.flush().await.context("flushing download file")?;

    info!(url, downloaded, "download complete");
    Ok(())
}

/// Verify that `file` matches `expected`, which must be `sha256:<hex>`.
fn verify_checksum(file: &Path, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};

    let expected_hex = expected
        .strip_prefix("sha256:")
        .with_context(|| format!("checksum must be in sha256:<hex> format, got: {expected}"))?;

    let data = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let actual_hex = format!("{:x}", Sha256::digest(&data));

    if actual_hex != expected_hex {
        anyhow::bail!(
            "checksum mismatch:\n  expected: sha256:{expected_hex}\n  actual:   sha256:{actual_hex}"
        );
    }

    info!("checksum verified: sha256:{actual_hex}");
    Ok(())
}
