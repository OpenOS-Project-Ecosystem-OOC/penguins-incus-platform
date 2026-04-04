"""Waydroid container provisioning — waydroid-toolkit feature set.

Covers the full waydroid-toolkit lifecycle as daemon operations:
  create        → provision an Incus container with Waydroid installed
  extensions    → install / remove Waydroid extensions (GApps, MicroG, Magisk, …)
  backup        → export Waydroid data to a tarball
  restore       → import Waydroid data from a tarball
  snapshot      → snapshot / restore / schedule (delegates to generic)
  gpu           → GPU passthrough for hardware acceleration
  usb           → USB passthrough (ADB, controllers)
  net           → port-forward management (ADB over TCP, etc.)
  fleet         → bulk list / start / stop of Waydroid containers
  publish       → export container as reusable Incus image
  cloud-sync    → configure rclone remote for backup sync

The daemon does not run waydroid commands directly — it provisions the
Incus container and injects cloud-init / exec commands that drive waydroid
inside the container.  Host-side waydroid binder setup is handled by the
cloud-init runcmd sequence.
"""

from __future__ import annotations

from typing import Any

from ._base import base_instance_config, build_cloud_init, gpu_device, proxy_device

# ── Known extensions ──────────────────────────────────────────────────────────

KNOWN_EXTENSIONS = {
    "gapps", "microg", "magisk", "libhoudini", "libndk", "widevine", "keymapper",
}

# ── Container create ──────────────────────────────────────────────────────────

async def create_waydroid_container(incus: Any,
                                    config: dict[str, Any]) -> dict[str, Any]:
    """Provision an Incus container with Waydroid pre-installed.

    Config keys:
      name            Container name (required)
      image           Base image (default: images:ubuntu/22.04/cloud)
      image_type      VANILLA | GAPPS (default: VANILLA)
      arch            x86_64 | arm64 (default: x86_64)
      gpu             bool — attach GPU for hardware acceleration
      gpu_type        physical | virtio (default: physical)
      profiles        list of Incus profiles (default: [default, waydroid])
      disk_size       root disk size (default: 40GB)
      project         Incus project
    """
    name = config["name"]
    image = config.get("image", "images:ubuntu/22.04/cloud")
    image_type = config.get("image_type", "VANILLA").upper()
    arch = config.get("arch", "x86_64")
    disk_size = config.get("disk_size", "40GB")
    project = config.get("project", "")
    enable_gpu = config.get("gpu", False)

    profiles = list(config.get("profiles", ["default", "waydroid"]))
    if enable_gpu and "gpu" not in profiles:
        profiles.append("gpu")

    devices: dict[str, Any] = {}
    if enable_gpu:
        devices["gpu0"] = gpu_device(gpu_type=config.get("gpu_type", "physical"))

    # ADB over TCP proxy so the host can reach ADB inside the container
    devices["adb"] = proxy_device(host_port=5555, guest_port=5555)

    runcmd = [
        # Install Waydroid from the official repo
        "apt-get install -y curl gnupg2",
        "curl -s https://repo.waydro.id/waydroid.gpg | gpg --dearmor"
        " -o /usr/share/keyrings/waydroid.gpg",
        "echo 'deb [signed-by=/usr/share/keyrings/waydroid.gpg]"
        " https://repo.waydro.id/ focal main'"
        " > /etc/apt/sources.list.d/waydroid.list",
        "apt-get update -qq",
        "apt-get install -y waydroid",
        # Initialise Waydroid with the requested image type
        f"waydroid init -s {image_type} -a {arch}",
        "systemctl enable --now waydroid-container",
    ]

    user_data = build_cloud_init(
        packages=["binder-control", "ashmem-control"],
        runcmd=runcmd,
    )

    inst_config: dict[str, str] = {
        "user.user-data": user_data,
        "security.nesting": "true",
        "linux.kernel_modules": "binder_linux,ashmem_linux",
    }

    payload = base_instance_config(
        name=name,
        image=image,
        instance_type="container",
        profiles=profiles,
        config=inst_config,
        devices=devices,
        disk_size=disk_size,
        project=project,
    )
    return await incus.create_instance(payload)


# ── Extensions ────────────────────────────────────────────────────────────────

async def install_extension(incus: Any, name: str,
                             config: dict[str, Any]) -> dict[str, Any]:
    """Install a Waydroid extension inside a running container.

    Config keys:
      extension   Extension ID (gapps | microg | magisk | libhoudini |
                  libndk | widevine | keymapper)
      project     Incus project
    """
    ext = config.get("extension", "")
    if ext not in KNOWN_EXTENSIONS:
        raise ValueError(
            f"Unknown extension {ext!r}. Known: {sorted(KNOWN_EXTENSIONS)}"
        )
    project = config.get("project", "")

    # Each extension is installed by running wdt inside the container.
    # The container must have waydroid-toolkit installed (via pip or package).
    result = await incus.exec_instance(
        name,
        ["wdt", "extensions", "install", ext],
        project=project,
    )
    return {"extension": ext, "operation": result}


async def remove_extension(incus: Any, name: str,
                            config: dict[str, Any]) -> dict[str, Any]:
    ext = config.get("extension", "")
    project = config.get("project", "")
    result = await incus.exec_instance(
        name,
        ["wdt", "extensions", "remove", ext],
        project=project,
    )
    return {"extension": ext, "operation": result}


async def list_extensions(incus: Any, name: str,
                           project: str = "") -> dict[str, Any]:
    result = await incus.exec_instance(
        name,
        ["wdt", "extensions", "list", "--json"],
        project=project,
    )
    return {"operation": result}


# ── Backup / restore ──────────────────────────────────────────────────────────

async def backup_waydroid(incus: Any, name: str,
                           config: dict[str, Any]) -> dict[str, Any]:
    """Trigger a Waydroid data backup inside the container.

    Config keys:
      dest      Path inside the container to write the archive (optional)
      project   Incus project
    """
    project = config.get("project", "")
    dest = config.get("dest", "/var/lib/waydroid-backups")

    result = await incus.exec_instance(
        name,
        ["wdt", "backup", "create", "--dest", dest],
        project=project,
    )
    return {"operation": result, "dest": dest}


async def restore_waydroid(incus: Any, name: str,
                            config: dict[str, Any]) -> dict[str, Any]:
    """Restore a Waydroid backup inside the container.

    Config keys:
      archive   Path to the backup archive inside the container (required)
      project   Incus project
    """
    archive = config["archive"]
    project = config.get("project", "")

    result = await incus.exec_instance(
        name,
        ["wdt", "backup", "restore", archive],
        project=project,
    )
    return {"operation": result, "archive": archive}


async def list_backups(incus: Any, name: str,
                        project: str = "") -> dict[str, Any]:
    result = await incus.exec_instance(
        name,
        ["wdt", "backup", "list", "--json"],
        project=project,
    )
    return {"operation": result}


# ── Cloud sync ────────────────────────────────────────────────────────────────

async def configure_cloud_sync(incus: Any, name: str,
                                config: dict[str, Any]) -> dict[str, Any]:
    """Configure rclone cloud sync for Waydroid backups inside the container.

    Config keys:
      remote_name   rclone remote name (required)
      remote_path   destination path on the remote (required)
      schedule      cron schedule for automatic sync (optional)
      project       Incus project
    """
    remote_name = config["remote_name"]
    remote_path = config["remote_path"]
    schedule = config.get("schedule", "")
    project = config.get("project", "")

    cmd = ["wdt", "cloud-sync", "setup",
           "--remote", remote_name,
           "--path", remote_path]
    if schedule:
        cmd += ["--schedule", schedule]

    result = await incus.exec_instance(name, cmd, project=project)
    return {"operation": result}


# ── GPU passthrough ───────────────────────────────────────────────────────────

async def attach_gpu(incus: Any, name: str,
                     config: dict[str, Any]) -> dict[str, Any]:
    dev_name = config.get("dev_name", "gpu0")
    device = gpu_device(
        dev_name=dev_name,
        gpu_type=config.get("gpu_type", "physical"),
        pci=config.get("pci", ""),
        vendor=config.get("vendor", ""),
        gid=config.get("gid", 44),
    )
    return await incus.add_device(name, dev_name, device,
                                   project=config.get("project", ""))


async def detach_gpu(incus: Any, name: str, dev_name: str,
                     project: str = "") -> dict[str, Any]:
    return await incus.remove_device(name, dev_name, project=project)


# ── Fleet ─────────────────────────────────────────────────────────────────────

async def fleet_list(incus: Any, project: str = "",
                     status_filter: str = "") -> list[dict[str, Any]]:
    """List Waydroid containers (identified by the 'waydroid' profile)."""
    instances = await incus.list_instances(project=project,
                                            type_filter="container")
    waydroid = [i for i in instances
                if "waydroid" in i.get("profiles", [])]
    if status_filter:
        waydroid = [i for i in waydroid
                    if i.get("status", "").lower() == status_filter.lower()]
    return waydroid


# ── Publish ───────────────────────────────────────────────────────────────────

async def publish_container(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    name = config["name"]
    alias = config.get("alias", f"waydroid/{name}")
    description = config.get("description", f"Waydroid container: {name}")
    project = config.get("project", "")

    payload: dict[str, Any] = {
        "source": {
            "type": "instance",
            "name": name,
            **({"project": project} if project else {}),
        },
        "aliases": [{"name": alias}],
        "properties": {"description": description},
        "public": config.get("public", False),
    }
    return await incus.post("/1.0/images", json=payload)
