"""Windows VM provisioning — incus-windows-toolkit feature set.

Covers the full iwt lifecycle as daemon operations:
  image/build       → build a Windows VM image via mkosi / ISO
  image/download    → download Windows ISO
  vm/create         → create a Windows VM in Incus from profile + optional ISO
  vm/start|stop     → lifecycle
  vm/snapshot       → snapshot / restore / schedule
  vm/backup         → Incus backup of the VM
  vm/restore        → restore from Incus backup
  vm/fleet          → bulk list / start / stop of Windows VMs
  vm/gpu            → GPU passthrough (VFIO / Looking Glass)
  vm/usb            → USB passthrough
  vm/net            → port-forward management (RDP, etc.)
  vm/disk           → live disk resize
  vm/guest-tools    → install guest tools (SvcGuest, serviceman, SrvLib)
  vm/remoteapp      → discover / launch Windows apps as seamless Linux windows
  vm/apps           → install Windows app bundles via winget
  vm/upgrade        → in-place Windows upgrade
  publish           → export VM as reusable Incus image
  cloud-sync        → configure rclone remote for backup sync
  security/harden   → apply security hardening to the VM
"""

from __future__ import annotations

import asyncio
from typing import Any

from ._base import gpu_device, proxy_device, snapshot_schedule_config

# ── Constants ─────────────────────────────────────────────────────────────────

DEFAULT_PROFILE = "windows-desktop"
DEFAULT_VM_NAME = "windows"
DEFAULT_RAM = "8GiB"
DEFAULT_CPUS = 4
DEFAULT_DISK_SIZE = "64GB"

# Standard RDP proxy device
RDP_DEVICE = proxy_device(host_port=3389, guest_port=3389)


# ── VM create ─────────────────────────────────────────────────────────────────

async def create_windows_vm(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    """Create a Windows VM in Incus.

    Config keys:
      name          VM name (default: windows)
      profile       Incus profile (default: windows-desktop)
      image         Path to Windows ISO or Incus image alias (optional)
      disk          Path to additional disk image (optional)
      ram           Memory (default: 8GiB)
      cpus          vCPU count (default: 4)
      disk_size     Root disk size (default: 64GB)
      gpu_overlay   GPU overlay profile name (e.g. vfio, looking-glass)
      rdp           bool — add RDP proxy device (default: True)
      boot_autostart bool — set boot.autostart (default: False)
      project       Incus project
    """
    name = config.get("name", DEFAULT_VM_NAME)
    profile = config.get("profile", DEFAULT_PROFILE)
    ram = config.get("ram", DEFAULT_RAM)
    cpus = config.get("cpus", DEFAULT_CPUS)
    disk_size = config.get("disk_size", DEFAULT_DISK_SIZE)
    gpu_overlay = config.get("gpu_overlay", "")
    add_rdp = config.get("rdp", True)
    project = config.get("project", "")

    profiles = [profile]
    if gpu_overlay:
        profiles.append(f"gpu-{gpu_overlay}")

    inst_config: dict[str, str] = {
        "limits.cpu": str(cpus),
        "limits.memory": ram,
        "security.secureboot": "false",
    }
    if config.get("boot_autostart"):
        inst_config["boot.autostart"] = "true"

    devices: dict[str, Any] = {
        "root": {"type": "disk", "path": "/", "size": disk_size},
    }
    if add_rdp:
        devices["rdp"] = RDP_DEVICE

    inst_payload: dict[str, Any] = {
        "name": name,
        "type": "virtual-machine",
        "source": {"type": "none"},
        "profiles": profiles,
        "config": inst_config,
        "devices": devices,
        **({"project": project} if project else {}),
    }
    create_op = await incus.create_instance(inst_payload)

    attach_ops = []

    # Attach Windows ISO if provided
    iso_path = config.get("image", "")
    if iso_path:
        op = await incus.add_device(name, "install", {
            "type": "disk",
            "source": iso_path,
        }, project=project)
        attach_ops.append({"device": "install", "source": iso_path, "operation": op})

    # Attach additional disk if provided
    extra_disk = config.get("disk", "")
    if extra_disk:
        op = await incus.add_device(name, "data", {
            "type": "disk",
            "source": extra_disk,
        }, project=project)
        attach_ops.append({"device": "data", "source": extra_disk, "operation": op})

    return {
        "name": name,
        "create_operation": create_op,
        "device_operations": attach_ops,
    }


# ── VM lifecycle ──────────────────────────────────────────────────────────────

async def start_windows_vm(incus: Any, name: str,
                            project: str = "") -> dict[str, Any]:
    return await incus.change_instance_state(name, "start", project=project)


async def stop_windows_vm(incus: Any, name: str, force: bool = False,
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
    return await incus.get(f"/1.0/instances/{name}/backups", params=params)


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


# ── Guest tools ───────────────────────────────────────────────────────────────

async def install_guest_tools(incus: Any, name: str,
                               config: dict[str, Any]) -> dict[str, Any]:
    """Install Windows guest tools inside the VM via exec.

    Requires the VM to be running and have PowerShell accessible via
    the Incus exec API (needs qemu-guest-agent or similar).

    Config keys:
      tools     list of tool IDs: svcguest | serviceman | srvlib | winbtrfs | winfsp
      project   Incus project
    """
    tools = config.get("tools", ["svcguest", "serviceman"])
    project = config.get("project", "")

    _tool_scripts = {
        "svcguest":   "setup-svcguest.ps1",
        "serviceman": "setup-serviceman.ps1",
        "srvlib":     "setup-srvlib.ps1",
        "winbtrfs":   "setup-winbtrfs.ps1",
        "winfsp":     "setup-winfsp.ps1",
    }

    results = []
    for tool in tools:
        script = _tool_scripts.get(tool)
        if not script:
            results.append({"tool": tool, "error": "unknown tool"})
            continue
        op = await incus.exec_instance(
            name,
            ["powershell.exe", "-ExecutionPolicy", "Bypass",
             "-File", f"C:\\iwt\\guest\\{script}"],
            project=project,
        )
        results.append({"tool": tool, "operation": op})

    return {"name": name, "tools": results}


# ── RemoteApp ─────────────────────────────────────────────────────────────────

async def discover_remoteapps(incus: Any, name: str,
                               project: str = "") -> dict[str, Any]:
    """List installed Windows applications via PowerShell exec."""
    op = await incus.exec_instance(
        name,
        ["powershell.exe", "-Command",
         "Get-ItemProperty HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\* "
         "| Select-Object DisplayName, DisplayVersion, Publisher "
         "| ConvertTo-Json"],
        project=project,
    )
    return {"operation": op}


async def launch_remoteapp(incus: Any, name: str,
                            config: dict[str, Any]) -> dict[str, Any]:
    """Launch a Windows application inside the VM.

    Config keys:
      app       Executable name or full path (required)
      args      List of arguments (optional)
      project   Incus project
    """
    app = config["app"]
    args = config.get("args", [])
    project = config.get("project", "")

    cmd = ["powershell.exe", "-Command", f"Start-Process '{app}'"]
    if args:
        args_str = " ".join(f"'{a}'" for a in args)
        cmd = ["powershell.exe", "-Command",
               f"Start-Process '{app}' -ArgumentList {args_str}"]

    op = await incus.exec_instance(name, cmd, project=project)
    return {"app": app, "operation": op}


# ── App store (winget) ────────────────────────────────────────────────────────

async def install_apps(incus: Any, name: str,
                        config: dict[str, Any]) -> dict[str, Any]:
    """Install Windows applications via winget inside the VM.

    Config keys:
      apps      list of winget package IDs (required)
      project   Incus project
    """
    apps = config.get("apps", [])
    project = config.get("project", "")

    results = []
    for app_id in apps:
        op = await incus.exec_instance(
            name,
            ["powershell.exe", "-Command",
             f"winget install --id {app_id} --silent --accept-package-agreements"
             " --accept-source-agreements"],
            project=project,
        )
        results.append({"app_id": app_id, "operation": op})

    return {"name": name, "apps": results}


# ── Cloud sync ────────────────────────────────────────────────────────────────

async def configure_cloud_sync(incus: Any, name: str,
                                config: dict[str, Any]) -> dict[str, Any]:
    """Store rclone cloud sync configuration in the VM's Incus metadata.

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

    inst = await incus.get_instance(name, project=project)
    cfg: dict[str, str] = dict(inst.get("config", {}))
    cfg["user.iwt.cloud_sync.remote"] = remote_name
    cfg["user.iwt.cloud_sync.path"] = remote_path
    if schedule:
        cfg["user.iwt.cloud_sync.schedule"] = schedule

    params = {"project": project} if project else {}
    return await incus.put(
        f"/1.0/instances/{name}",
        json={**inst, "config": cfg},
        params=params,
    )


# ── Security hardening ────────────────────────────────────────────────────────

async def harden_vm(incus: Any, name: str,
                    config: dict[str, Any]) -> dict[str, Any]:
    """Apply security hardening to a Windows VM via PowerShell exec.

    Config keys:
      level     basic | standard | strict (default: standard)
      project   Incus project
    """
    level = config.get("level", "standard")
    project = config.get("project", "")

    op = await incus.exec_instance(
        name,
        ["powershell.exe", "-ExecutionPolicy", "Bypass",
         "-File", "C:\\iwt\\security\\harden-vm.ps1", "-Level", level],
        project=project,
    )
    return {"level": level, "operation": op}


# ── Fleet ─────────────────────────────────────────────────────────────────────

async def fleet_list(incus: Any, project: str = "",
                     status_filter: str = "") -> list[dict[str, Any]]:
    """List Windows VMs (identified by windows-* profiles)."""
    instances = await incus.list_instances(project=project,
                                            type_filter="virtual-machine")
    windows_vms = [
        i for i in instances
        if any(p.startswith("windows") for p in i.get("profiles", []))
    ]
    if status_filter:
        windows_vms = [i for i in windows_vms
                       if i.get("status", "").lower() == status_filter.lower()]
    return windows_vms


async def fleet_start(incus: Any, names: list[str],
                       project: str = "") -> list[dict[str, Any]]:
    tasks = [start_windows_vm(incus, n, project=project) for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


async def fleet_stop(incus: Any, names: list[str],
                      project: str = "") -> list[dict[str, Any]]:
    tasks = [stop_windows_vm(incus, n, project=project) for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


# ── Publish ───────────────────────────────────────────────────────────────────

async def publish_vm(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    name = config["name"]
    alias = config.get("alias", f"windows/{name}")
    description = config.get("description", f"Windows VM: {name}")
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
    """Resize the root disk of a Windows VM."""
    new_size = config["new_size"]
    project = config.get("project", "")
    return await incus.add_device(name, "root", {
        "type": "disk",
        "path": "/",
        "size": new_size,
    }, project=project)
