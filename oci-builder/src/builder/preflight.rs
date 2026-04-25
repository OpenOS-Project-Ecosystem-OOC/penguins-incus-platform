//! Pre-flight checks run before the build pipeline starts.
//!
//! Validates that all required resources and host tools are available so the
//! build fails fast with a clear error rather than partway through a long run.
//!
//! Checks performed:
//! - Incus socket is present and reachable
//! - Required host tools exist (mksquashfs for bootstrap downloaders)
//! - Source image exists on the configured remote (incus downloader)
//! - Registry is reachable (when oci.registry is set)
//! - Downloader-specific requirements (debootstrap binary, dnf binary)

use anyhow::{Context, Result};
use tracing::info;

use crate::definition::{Definition, Downloader};
use crate::incus::client::DEFAULT_SOCKET;

/// Run all pre-flight checks for `def`. Returns `Ok(())` if everything is
/// in order, or an error describing the first failing check.
pub async fn run(def: &Definition) -> Result<()> {
    info!("running pre-flight checks");

    check_incus_socket()?;
    check_downloader_tools(def)?;
    check_registry_reachable(def).await?;

    info!("pre-flight checks passed");
    Ok(())
}

// ── Incus socket ──────────────────────────────────────────────────────────────

fn check_incus_socket() -> Result<()> {
    let socket = std::path::Path::new(DEFAULT_SOCKET);
    if !socket.exists() {
        anyhow::bail!(
            "Incus socket not found at {DEFAULT_SOCKET}.\n\
             Is Incus installed and running? Try: sudo incus admin init"
        );
    }
    Ok(())
}

// ── Downloader tool checks ────────────────────────────────────────────────────

fn check_downloader_tools(def: &Definition) -> Result<()> {
    match &def.source.downloader {
        Downloader::Incus => {
            if def.source.image.is_empty() {
                anyhow::bail!("source.image must be set when using the incus downloader");
            }
        }

        Downloader::Debootstrap => {
            which::which("debootstrap")
                .context("debootstrap not found — install it with: apt-get install debootstrap")?;
            which::which("mksquashfs").context(
                "mksquashfs not found — install it with: apt-get install squashfs-tools",
            )?;
            if def.source.suite.is_empty() {
                anyhow::bail!("source.suite must be set when using the debootstrap downloader");
            }
        }

        Downloader::Rpmbootstrap => {
            which::which("dnf")
                .or_else(|_| which::which("dnf5"))
                .context("dnf/dnf5 not found — install it with your system package manager")?;
            which::which("mksquashfs").context(
                "mksquashfs not found — install it with: apt-get install squashfs-tools",
            )?;
            if def.image.release.is_empty() {
                anyhow::bail!(
                    "image.release must be set when using the rpmbootstrap downloader \
                     (used as --releasever, e.g. \"40\" for Fedora 40)"
                );
            }
        }

        Downloader::RootfsHttp => {
            which::which("mksquashfs").context(
                "mksquashfs not found — install it with: apt-get install squashfs-tools",
            )?;
            if def.source.url.is_empty() {
                anyhow::bail!("source.url must be set when using the rootfs-http downloader");
            }
        }
    }
    Ok(())
}

// ── Registry reachability ─────────────────────────────────────────────────────

async fn check_registry_reachable(def: &Definition) -> Result<()> {
    let registry = match def.oci.as_ref().map(|o| o.registry.as_str()) {
        Some(r) if !r.is_empty() => r,
        _ => return Ok(()), // no registry configured — skip
    };

    // Probe the registry v2 API endpoint. A 200 or 401 both indicate the
    // registry is up (401 = auth required, which is expected).
    let url = format!("https://{registry}/v2/");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building HTTP client for registry check")?;

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("registry {registry} is not reachable at {url}"))?;

    let status = resp.status().as_u16();
    if status != 200 && status != 401 {
        anyhow::bail!(
            "registry {registry} returned unexpected status {status} — \
             expected 200 or 401 from {url}"
        );
    }

    info!(registry, "registry reachable");
    Ok(())
}
