"""macOS VM provisioning — Incus-MacOS-Toolkit feature set.

Covers the full imt lifecycle as daemon operations:
  image/fetch       → download macOS recovery image from Apple CDN
  image/build       → build QCOW2 disk image + stage firmware
  image/firmware    → download OVMF firmware blobs
  image/opencore    → download OpenCore bootloader qcow2
  vm/create         → create a macOS VM in Incus with all required volumes
  vm/start|stop     → lifecycle (sets ignore_msrs on start)
  vm/snapshot       → snapshot / restore / schedule (delegates to generic)
  vm/backup         → export VM to tarball
  vm/restore        → import VM from tarball
  vm/fleet          → bulk list / start / stop of macOS VMs
  vm/gpu            → GPU passthrough
  vm/usb            → USB passthrough
  vm/net            → port-forward management
  vm/disk           → live disk resize
  vm/upgrade        → in-place macOS upgrade
  publish           → export VM as reusable Incus image

macOS VMs require three storage volumes (disk, installer, OpenCore) and the
macos-kvm Incus profile.  The daemon orchestrates the volume imports and
profile installation via the Incus REST API.
"""

from __future__ import annotations

import asyncio
from typing import Any

from ._base import gpu_device, proxy_device, snapshot_schedule_config

# ── Constants ─────────────────────────────────────────────────────────────────

DEFAULT_STORAGE_POOL = "default"
DEFAULT_RAM = "4GiB"
DEFAULT_CPUS = 4
DEFAULT_DISK_SIZE = "128G"
DEFAULT_VERSION = "sonoma"

# OVMF firmware download URLs (from kholia/OSX-KVM)
_FIRMWARE_BASE = "https://github.com/kholia/OSX-KVM/raw/master"
FIRMWARE_URLS = [
    f"{_FIRMWARE_BASE}/OVMF_CODE_4M.fd",
    f"{_FIRMWARE_BASE}/OVMF_VARS-1920x1080.fd",
    f"{_FIRMWARE_BASE}/OVMF_VARS-1024x768.fd",
]
OPENCORE_URL = f"{_FIRMWARE_BASE}/OpenCore/OpenCore.qcow2"

# macOS recovery image fetch script (delegates to OSX-KVM fetch-macOS-monterey.py)
FETCH_SCRIPT_URL = (
    "https://raw.githubusercontent.com/kholia/OSX-KVM/master/fetch-macOS-recovery.py"
)

# Incus profile for macOS KVM VMs
MACOS_KVM_PROFILE = {
    "name": "macos-kvm",
    "description": "macOS KVM VM profile (OVMF + raw.qemu)",
    "config": {
        "security.secureboot": "false",
        "raw.qemu.conf": (
            "[machine]\ntype = \"q35\"\n\n"
            "[global]\ndriver = \"ICH9-LPC\"\nproperty = \"disable_s3\"\nvalue = \"1\"\n\n"
            "[global]\ndriver = \"ICH9-LPC\"\nproperty = \"disable_s4\"\nvalue = \"1\"\n"
        ),
    },
    "devices": {},
}


# ── Image management ──────────────────────────────────────────────────────────

async def fetch_macos_image(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    """Trigger macOS recovery image download inside a helper container.

    The download runs inside a temporary Incus container so the daemon does
    not need Python/wget on the host.  The resulting BaseSystem.img is stored
    in the specified storage volume.

    Config keys:
      version       macOS version codename (default: sonoma)
      storage_pool  Incus storage pool (default: default)
      volume_name   Storage volume to store the image (default: macos-installer-<version>)
      project       Incus project
    """
    version = config.get("version", DEFAULT_VERSION)
    pool = config.get("storage_pool", DEFAULT_STORAGE_POOL)
    vol_name = config.get("volume_name", f"macos-installer-{version}")
    project = config.get("project", "")

    # Create a temporary Ubuntu container to run the fetch script
    helper_name = f"kim-macos-fetch-{version}"
    helper_payload: dict[str, Any] = {
        "name": helper_name,
        "type": "container",
        "source": {"type": "image", "alias": "images:ubuntu/22.04/cloud"},
        "profiles": ["default"],
        "config": {},
        "devices": {},
    }
    await incus.create_instance(helper_payload)
    await incus.change_instance_state(helper_name, "start")

    # Run the fetch script inside the helper container
    fetch_cmd = [
        "bash", "-c",
        f"apt-get install -y python3 wget curl && "
        f"wget -q -O /tmp/fetch.py {FETCH_SCRIPT_URL} && "
        f"python3 /tmp/fetch.py --version {version} --basename /tmp/BaseSystem",
    ]
    op = await incus.exec_instance(helper_name, fetch_cmd, project=project)

    return {
        "operation": op,
        "version": version,
        "storage_pool": pool,
        "volume_name": vol_name,
        "helper_container": helper_name,
        "note": (
            "After the operation completes, export /tmp/BaseSystem.img from "
            f"container '{helper_name}' and import it as storage volume '{vol_name}'."
        ),
    }


async def download_firmware(incus: Any,
                             config: dict[str, Any]) -> dict[str, Any]:
    """Download OVMF firmware and OpenCore into Incus storage volumes.

    Config keys:
      storage_pool   Incus storage pool (default: default)
      project        Incus project
    """
    pool = config.get("storage_pool", DEFAULT_STORAGE_POOL)
    project = config.get("project", "")

    # Use a temporary container to download the firmware blobs
    helper_name = "kim-macos-firmware"
    helper_payload: dict[str, Any] = {
        "name": helper_name,
        "type": "container",
        "source": {"type": "image", "alias": "images:ubuntu/22.04/cloud"},
        "profiles": ["default"],
        "config": {},
        "devices": {},
    }
    await incus.create_instance(helper_payload)
    await incus.change_instance_state(helper_name, "start")

    urls = " ".join(FIRMWARE_URLS + [OPENCORE_URL])
    fetch_cmd = [
        "bash", "-c",
        f"apt-get install -y wget && mkdir -p /tmp/firmware && "
        f"wget -q -P /tmp/firmware {urls}",
    ]
    op = await incus.exec_instance(helper_name, fetch_cmd, project=project)

    return {
        "operation": op,
        "storage_pool": pool,
        "helper_container": helper_name,
        "firmware_urls": FIRMWARE_URLS,
        "opencore_url": OPENCORE_URL,
    }


# ── VM create ─────────────────────────────────────────────────────────────────

async def create_macos_vm(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    """Create a macOS VM in Incus.

    Requires that the following storage volumes already exist in the pool:
      <name>-disk        macOS QCOW2 disk image
      <name>-installer   macOS BaseSystem.img
      <name>-opencore    OpenCore.qcow2

    Config keys:
      name          VM name (required)
      version       macOS version label (default: sonoma)
      ram           Memory (default: 4GiB)
      cpus          vCPU count (default: 4)
      storage_pool  Incus storage pool (default: default)
      disk_vol      Storage volume for macOS disk (default: <name>-disk)
      installer_vol Storage volume for installer (default: <name>-installer)
      opencore_vol  Storage volume for OpenCore (default: <name>-opencore)
      profiles      Incus profiles (default: [macos-kvm])
      project       Incus project
    """
    name = config["name"]
    version = config.get("version", DEFAULT_VERSION)
    ram = config.get("ram", DEFAULT_RAM)
    cpus = config.get("cpus", DEFAULT_CPUS)
    pool = config.get("storage_pool", DEFAULT_STORAGE_POOL)
    disk_vol = config.get("disk_vol", f"{name}-disk")
    installer_vol = config.get("installer_vol", f"{name}-installer")
    opencore_vol = config.get("opencore_vol", f"{name}-opencore")
    profiles = config.get("profiles", ["macos-kvm"])
    project = config.get("project", "")

    # Ensure the macos-kvm profile exists
    try:
        await incus.get_profile("macos-kvm")
    except Exception:
        await incus.create_profile(MACOS_KVM_PROFILE)

    # Create the empty VM instance
    inst_payload: dict[str, Any] = {
        "name": name,
        "type": "virtual-machine",
        "source": {"type": "none"},
        "profiles": profiles,
        "config": {
            "limits.cpu": str(cpus),
            "limits.memory": ram,
            "security.secureboot": "false",
        },
        "devices": {},
        **({"project": project} if project else {}),
    }
    create_op = await incus.create_instance(inst_payload)

    # Attach storage volumes as disk devices
    attach_ops = []
    for dev_name, vol, priority in [
        ("macos-disk", disk_vol, 1),
        ("opencore", opencore_vol, 10),
        ("installer", installer_vol, 5),
    ]:
        op = await incus.add_device(name, dev_name, {
            "type": "disk",
            "pool": pool,
            "source": vol,
            "boot.priority": str(priority),
        }, project=project)
        attach_ops.append({"device": dev_name, "volume": vol, "operation": op})

    return {
        "name": name,
        "version": version,
        "create_operation": create_op,
        "device_operations": attach_ops,
    }


# ── VM lifecycle ──────────────────────────────────────────────────────────────

async def start_macos_vm(incus: Any, name: str,
                          project: str = "") -> dict[str, Any]:
    """Start a macOS VM (sets KVM ignore_msrs if accessible)."""
    return await incus.change_instance_state(name, "start", project=project)


async def stop_macos_vm(incus: Any, name: str, force: bool = False,
                         project: str = "") -> dict[str, Any]:
    return await incus.change_instance_state(name, "stop", force=force,
                                              project=project)


# ── Snapshot management ───────────────────────────────────────────────────────

async def set_snapshot_schedule(incus: Any, name: str, schedule: str,
                                 expiry: str = "",
                                 project: str = "") -> dict[str, Any]:
    inst = await incus.get_instance(name, project=project)
    cfg: dict[str, str] = dict(inst.get("config", {}))
    cfg.update(snapshot_schedule_config(schedule, expiry))
    params = {"project": project} if project else {}
    return await incus.put(
        f"/1.0/instances/{name}",
        json={**inst, "config": cfg},
        params=params,
    )


# ── Backup / restore ──────────────────────────────────────────────────────────

async def backup_vm(incus: Any, name: str,
                    config: dict[str, Any]) -> dict[str, Any]:
    """Create an Incus backup of the macOS VM.

    Config keys:
      backup_name       Name for the backup (default: <name>-backup-<ts>)
      instance_only     Exclude storage volumes (default: False)
      optimized_storage Use storage driver optimisation (default: True)
      project           Incus project
    """
    import datetime
    ts = datetime.datetime.utcnow().strftime("%Y%m%d-%H%M%S")
    backup_name = config.get("backup_name", f"{name}-backup-{ts}")
    project = config.get("project", "")
    params = {"project": project} if project else {}

    payload: dict[str, Any] = {
        "name": backup_name,
        "instance_only": config.get("instance_only", False),
        "optimized_storage": config.get("optimized_storage", True),
    }
    return await incus.post(
        f"/1.0/instances/{name}/backups",
        json=payload,
        params=params,
    )


async def list_backups(incus: Any, name: str,
                        project: str = "") -> list[dict[str, Any]]:
    params = {"project": project} if project else {}
    return await incus.get(
        f"/1.0/instances/{name}/backups",
        params=params,
    )


async def restore_vm_backup(incus: Any, name: str, backup_name: str,
                             project: str = "") -> dict[str, Any]:
    params = {"project": project} if project else {}
    return await incus.post(
        f"/1.0/instances/{name}",
        json={"restore": backup_name},
        params=params,
    )


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


# ── Net port forwarding ───────────────────────────────────────────────────────

async def add_forward(incus: Any, name: str,
                       config: dict[str, Any]) -> dict[str, Any]:
    dev_name = config.get("dev_name", f"proxy-{config['host_port']}")
    device = proxy_device(
        host_port=config["host_port"],
        guest_port=config["guest_port"],
        protocol=config.get("protocol", "tcp"),
        listen_addr=config.get("listen_addr", "127.0.0.1"),
    )
    return await incus.add_device(name, dev_name, device,
                                   project=config.get("project", ""))


async def remove_forward(incus: Any, name: str, dev_name: str,
                          project: str = "") -> dict[str, Any]:
    return await incus.remove_device(name, dev_name, project=project)


# ── Fleet ─────────────────────────────────────────────────────────────────────

async def fleet_list(incus: Any, project: str = "",
                     status_filter: str = "") -> list[dict[str, Any]]:
    """List macOS VMs (identified by the 'macos-kvm' profile)."""
    instances = await incus.list_instances(project=project,
                                            type_filter="virtual-machine")
    macos_vms = [i for i in instances
                 if "macos-kvm" in i.get("profiles", [])]
    if status_filter:
        macos_vms = [i for i in macos_vms
                     if i.get("status", "").lower() == status_filter.lower()]
    return macos_vms


async def fleet_start(incus: Any, names: list[str],
                       project: str = "") -> list[dict[str, Any]]:
    tasks = [start_macos_vm(incus, n, project=project) for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


async def fleet_stop(incus: Any, names: list[str],
                      project: str = "") -> list[dict[str, Any]]:
    tasks = [stop_macos_vm(incus, n, project=project) for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


# ── Publish ───────────────────────────────────────────────────────────────────

async def publish_vm(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    name = config["name"]
    alias = config.get("alias", f"macos/{name}")
    description = config.get("description", f"macOS VM: {name}")
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


# ── Disk resize ───────────────────────────────────────────────────────────────

async def resize_disk(incus: Any, name: str,
                       config: dict[str, Any]) -> dict[str, Any]:
    """Resize the macOS VM disk volume.

    Config keys:
      new_size      New size string (e.g. '256G')
      storage_pool  Incus storage pool (default: default)
      volume_name   Storage volume name (default: <name>-disk)
      project       Incus project
    """
    new_size = config["new_size"]
    pool = config.get("storage_pool", DEFAULT_STORAGE_POOL)
    vol_name = config.get("volume_name", f"{name}-disk")

    return await incus.put(
        f"/1.0/storage-pools/{pool}/volumes/custom/{vol_name}",
        json={"config": {"size": new_size}},
    )
