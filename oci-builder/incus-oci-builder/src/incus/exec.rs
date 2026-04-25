//! Helpers for running build steps inside an Incus container.

use std::collections::HashMap;

use anyhow::Result;
use tracing::info;

use super::api::ExecRequest;
use super::client::IncusClient;

/// Run a shell script inside the named container.
///
/// The script is passed as a string and executed via `/bin/sh -c`.
/// Returns an error if the command exits non-zero.
pub async fn run_script(client: &IncusClient, instance: &str, script: &str) -> Result<()> {
    info!(instance, "running script");
    let req = ExecRequest {
        command: vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
        environment: HashMap::new(),
        wait_for_websocket: false,
        interactive: false,
        record_output: true,
    };
    let code = client.exec(instance, &req).await?;
    if code != 0 {
        anyhow::bail!(
            "script exited with code {code} in instance {instance}:\n  {}",
            script.lines().next().unwrap_or("(empty)")
        );
    }
    Ok(())
}

/// Install packages using the specified package manager.
pub async fn install_packages(
    client: &IncusClient,
    instance: &str,
    manager: &str,
    packages: &[String],
) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }
    let pkg_list = packages.join(" ");
    let script = match manager {
        "apt" => format!(
            "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends {pkg_list}"
        ),
        "dnf" => format!("dnf install -y {pkg_list}"),
        "apk" => format!("apk add --no-cache {pkg_list}"),
        "pacman" => format!("pacman -Sy --noconfirm {pkg_list}"),
        "zypper" => format!("zypper install -y {pkg_list}"),
        "xbps" => format!("xbps-install -Sy {pkg_list}"),
        other => anyhow::bail!("unsupported package manager: {other}"),
    };
    run_script(client, instance, &script).await
}

/// Remove packages using the specified package manager.
pub async fn remove_packages(
    client: &IncusClient,
    instance: &str,
    manager: &str,
    packages: &[String],
) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }
    let pkg_list = packages.join(" ");
    let script = match manager {
        "apt" => format!("apt-get remove -y {pkg_list}"),
        "dnf" => format!("dnf remove -y {pkg_list}"),
        "apk" => format!("apk del {pkg_list}"),
        "pacman" => format!("pacman -R --noconfirm {pkg_list}"),
        "zypper" => format!("zypper remove -y {pkg_list}"),
        "xbps" => format!("xbps-remove -y {pkg_list}"),
        other => anyhow::bail!("unsupported package manager: {other}"),
    };
    run_script(client, instance, &script).await
}

/// Run a full package upgrade.
pub async fn upgrade_packages(client: &IncusClient, instance: &str, manager: &str) -> Result<()> {
    let script = match manager {
        "apt" => "DEBIAN_FRONTEND=noninteractive apt-get update && apt-get upgrade -y".to_string(),
        "dnf" => "dnf upgrade -y".to_string(),
        "apk" => "apk update && apk upgrade".to_string(),
        "pacman" => "pacman -Syu --noconfirm".to_string(),
        "zypper" => "zypper update -y".to_string(),
        "xbps" => "xbps-install -Su".to_string(),
        other => anyhow::bail!("unsupported package manager: {other}"),
    };
    run_script(client, instance, &script).await
}

/// Clean package manager caches to reduce image size.
pub async fn cleanup_packages(client: &IncusClient, instance: &str, manager: &str) -> Result<()> {
    let script = match manager {
        "apt" => "apt-get clean && rm -rf /var/lib/apt/lists/*".to_string(),
        "dnf" => "dnf clean all".to_string(),
        "apk" => "rm -rf /var/cache/apk/*".to_string(),
        "pacman" => "pacman -Sc --noconfirm".to_string(),
        "zypper" => "zypper clean".to_string(),
        "xbps" => "xbps-remove -O".to_string(),
        other => anyhow::bail!("unsupported package manager: {other}"),
    };
    run_script(client, instance, &script).await
}
