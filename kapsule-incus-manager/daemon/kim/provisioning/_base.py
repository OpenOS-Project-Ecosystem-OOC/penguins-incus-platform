"""Shared helpers for all provisioning plugins."""

from __future__ import annotations

import textwrap
from typing import Any

# ── Cloud-init builder ────────────────────────────────────────────────────────

def build_cloud_init(
    *,
    packages: list[str] | None = None,
    runcmd: list[str | list[str]] | None = None,
    write_files: list[dict[str, Any]] | None = None,
    users: list[dict[str, Any]] | None = None,
    extra: dict[str, Any] | None = None,
) -> str:
    """Return a #cloud-config YAML string from structured inputs.

    Only keys with non-empty values are emitted so the output stays minimal.
    """
    import yaml  # local import — yaml is a daemon dependency

    doc: dict[str, Any] = {"package_update": True}
    if packages:
        doc["packages"] = packages
    if write_files:
        doc["write_files"] = write_files
    if runcmd:
        doc["runcmd"] = runcmd
    if users:
        doc["users"] = users
    if extra:
        doc.update(extra)

    return "#cloud-config\n" + yaml.dump(doc, default_flow_style=False)


# ── Instance config builder ───────────────────────────────────────────────────

def base_instance_config(
    name: str,
    image: str,
    instance_type: str = "container",
    profiles: list[str] | None = None,
    config: dict[str, str] | None = None,
    devices: dict[str, Any] | None = None,
    disk_size: str = "20GB",
    project: str = "",
) -> dict[str, Any]:
    """Return a minimal Incus instance creation payload."""
    payload: dict[str, Any] = {
        "name": name,
        "type": instance_type,
        "source": {"type": "image", "alias": image},
        "profiles": profiles or ["default"],
        "config": config or {},
        "devices": {
            "root": {"type": "disk", "path": "/", "size": disk_size},
            **(devices or {}),
        },
    }
    if project:
        payload["project"] = project
    return payload


# ── Proxy device helper ───────────────────────────────────────────────────────

def proxy_device(host_port: int, guest_port: int,
                 protocol: str = "tcp",
                 listen_addr: str = "127.0.0.1") -> dict[str, str]:
    """Return an Incus proxy device config dict."""
    return {
        "type": "proxy",
        "listen": f"{protocol}:{listen_addr}:{host_port}",
        "connect": f"{protocol}:0.0.0.0:{guest_port}",
    }


# ── GPU device helper ─────────────────────────────────────────────────────────

def gpu_device(
    dev_name: str = "gpu0",
    gpu_type: str = "physical",
    pci: str = "",
    vendor: str = "",
    gid: int = 44,
) -> dict[str, str]:
    """Return an Incus GPU device config dict."""
    cfg: dict[str, str] = {"type": "gpu", "gputype": gpu_type, "gid": str(gid)}
    if pci:
        cfg["pci"] = pci
    if vendor:
        cfg["vendor"] = vendor
    return cfg


# ── USB device helper ─────────────────────────────────────────────────────────

def usb_device(vendor_id: str, product_id: str,
               dev_name: str = "usb0") -> dict[str, str]:
    """Return an Incus USB device config dict."""
    return {
        "type": "usb",
        "vendorid": vendor_id,
        "productid": product_id,
    }


# ── Disk device helper ────────────────────────────────────────────────────────

def disk_device(source: str, path: str,
                pool: str = "", read_only: bool = False) -> dict[str, str]:
    """Return an Incus disk device config dict."""
    cfg: dict[str, str] = {"type": "disk", "source": source, "path": path}
    if pool:
        cfg["pool"] = pool
    if read_only:
        cfg["readonly"] = "true"
    return cfg


# ── Snapshot schedule helper ──────────────────────────────────────────────────

def snapshot_schedule_config(schedule: str, expiry: str = "") -> dict[str, str]:
    """Return instance config keys for automatic snapshots."""
    cfg: dict[str, str] = {"snapshots.schedule": schedule}
    if expiry:
        cfg["snapshots.expiry"] = expiry
    return cfg


# ── Indent helper ─────────────────────────────────────────────────────────────

def indent(text: str, prefix: str = "  ") -> str:
    return textwrap.indent(text, prefix)
