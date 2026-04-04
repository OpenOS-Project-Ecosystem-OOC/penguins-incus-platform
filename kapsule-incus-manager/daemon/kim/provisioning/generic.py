"""Generic container provisioning — incusbox feature set.

Covers the full incusbox lifecycle:
  create     → launch a container with cloud-init user setup
  assemble   → post-create package install + hook execution
  snapshot   → create / list / restore / delete snapshots
  snapshot-auto → manage automatic snapshot schedules
  backup     → export container to tarball
  restore    → import container from tarball
  gpu        → attach / detach / list GPU devices
  usb        → attach / detach / list USB devices
  net        → port-forward management
  fleet      → bulk start / stop / list
  publish    → export container as reusable Incus image
"""

from __future__ import annotations

import textwrap
from typing import Any

from ._base import (
    base_instance_config,
    build_cloud_init,
    disk_device,
    gpu_device,
    proxy_device,
    snapshot_schedule_config,
    usb_device,
)

# ── Container create ──────────────────────────────────────────────────────────

async def create_container(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    """Create an incusbox-style container.

    Config keys (all optional except *name*):
      name, image, hostname, user_name, user_uid, user_gid, home,
      additional_packages, init_hooks, pre_init_hooks, nvidia,
      profiles, disk_size, volumes, project
    """
    name = config["name"]
    image = config.get("image", "images:ubuntu/24.04/cloud")
    user_name = config.get("user_name", "user")
    user_uid = config.get("user_uid", 1000)
    user_gid = config.get("user_gid", 1000)
    home = config.get("home", f"/home/{user_name}")
    hostname = config.get("hostname", name)
    packages = config.get("additional_packages", [])
    init_hooks = config.get("init_hooks", [])
    pre_init_hooks = config.get("pre_init_hooks", [])
    nvidia = config.get("nvidia", False)
    disk_size = config.get("disk_size", "20GB")
    project = config.get("project", "")

    profiles = config.get("profiles", ["default"])
    if nvidia and "gpu" not in profiles:
        profiles = list(profiles) + ["gpu"]

    runcmd: list[Any] = []
    if pre_init_hooks:
        runcmd.extend(pre_init_hooks if isinstance(pre_init_hooks, list)
                      else [pre_init_hooks])

    # Create the user inside the container
    runcmd += [
        f"groupadd -g {user_gid} {user_name} || true",
        f"useradd -u {user_uid} -g {user_gid} -m -d {home} -s /bin/bash {user_name} || true",
        f"echo '{user_name} ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers.d/{user_name}",
    ]

    if packages:
        pkg_list = packages if isinstance(packages, list) else packages.split()
        runcmd.append(["apt-get", "install", "-y", "--no-install-recommends"] + pkg_list)

    if init_hooks:
        runcmd.extend(init_hooks if isinstance(init_hooks, list)
                      else [init_hooks])

    user_data = build_cloud_init(
        packages=["sudo", "bash-completion"],
        runcmd=runcmd,
    )

    devices: dict[str, Any] = {}
    for i, vol in enumerate(config.get("volumes", [])):
        parts = str(vol).split(":")
        if len(parts) >= 2:
            ro = len(parts) == 3 and "ro" in parts[2]
            devices[f"vol{i}"] = disk_device(parts[0], parts[1], read_only=ro)

    inst_config: dict[str, str] = {
        "user.user-data": user_data,
        "linux.hostname": hostname,
    }
    if nvidia:
        inst_config["nvidia.runtime"] = "true"

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


# ── Assemble (post-create package + hook runner) ──────────────────────────────

async def assemble_container(incus: Any, name: str,
                              config: dict[str, Any]) -> dict[str, Any]:
    """Run post-create assembly steps inside a running container.

    Mirrors incusbox-assemble: installs packages, runs hooks, sets up
    shell integration.
    """
    project = config.get("project", "")
    packages = config.get("packages", [])
    hooks = config.get("hooks", [])
    shell_integration = config.get("shell_integration", True)

    results: list[dict[str, Any]] = []

    if packages:
        pkg_list = packages if isinstance(packages, list) else packages.split()
        res = await incus.exec_instance(
            name,
            ["apt-get", "install", "-y", "--no-install-recommends"] + pkg_list,
            project=project,
        )
        results.append({"step": "packages", "operation": res})

    for i, hook in enumerate(hooks):
        cmd = hook if isinstance(hook, list) else ["bash", "-c", hook]
        res = await incus.exec_instance(name, cmd, project=project)
        results.append({"step": f"hook_{i}", "operation": res})

    if shell_integration:
        bashrc_snippet = textwrap.dedent("""\
            # incusbox shell integration
            export INCUSBOX=1
            export CONTAINER_ID=$(hostname)
            alias host-exec='nsenter -t 1 -m -u -i -n -p --'
        """)
        await incus.push_file(
            name, "/etc/profile.d/incusbox.sh",
            bashrc_snippet, mode="0644",
            project=project,
        )
        results.append({"step": "shell_integration", "status": "ok"})

    return {"name": name, "steps": results}


# ── Snapshot management ───────────────────────────────────────────────────────

async def list_snapshots(incus: Any, name: str,
                         project: str = "") -> list[dict[str, Any]]:
    return await incus.list_snapshots(name, project=project)


async def create_snapshot(incus: Any, name: str, snapshot: str,
                           stateful: bool = False,
                           project: str = "") -> dict[str, Any]:
    return await incus.create_snapshot(name, snapshot, stateful=stateful,
                                       project=project)


async def restore_snapshot(incus: Any, name: str, snapshot: str,
                            project: str = "") -> dict[str, Any]:
    return await incus.restore_snapshot(name, snapshot, project=project)


async def delete_snapshot(incus: Any, name: str, snapshot: str,
                           project: str = "") -> dict[str, Any]:
    return await incus.delete_snapshot(name, snapshot, project=project)


async def set_snapshot_schedule(incus: Any, name: str, schedule: str,
                                 expiry: str = "",
                                 project: str = "") -> dict[str, Any]:
    """Set automatic snapshot schedule on an instance."""
    inst = await incus.get_instance(name, project=project)
    cfg: dict[str, str] = dict(inst.get("config", {}))
    cfg.update(snapshot_schedule_config(schedule, expiry))
    params = {"project": project} if project else {}
    return await incus.put(
        f"/1.0/instances/{name}",
        json={**inst, "config": cfg},
        params=params,
    )


async def disable_snapshot_schedule(incus: Any, name: str,
                                     project: str = "") -> dict[str, Any]:
    return await set_snapshot_schedule(incus, name, "", project=project)


# ── GPU management ────────────────────────────────────────────────────────────

async def list_host_gpus(incus: Any) -> list[dict[str, Any]]:
    resources = await incus.get_host_resources()
    return resources.get("gpu", {}).get("cards", [])


async def list_instance_gpus(incus: Any, name: str,
                              project: str = "") -> dict[str, Any]:
    devices = await incus.list_devices(name, project=project)
    return {k: v for k, v in devices.items() if v.get("type") == "gpu"}


async def attach_gpu(incus: Any, name: str, config: dict[str, Any]) -> dict[str, Any]:
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


# ── USB management ────────────────────────────────────────────────────────────

async def list_host_usb(incus: Any) -> list[dict[str, Any]]:
    resources = await incus.get_host_resources()
    return resources.get("usb", {}).get("devices", [])


async def list_instance_usb(incus: Any, name: str,
                             project: str = "") -> dict[str, Any]:
    devices = await incus.list_devices(name, project=project)
    return {k: v for k, v in devices.items() if v.get("type") == "usb"}


async def attach_usb(incus: Any, name: str, config: dict[str, Any]) -> dict[str, Any]:
    dev_name = config.get("dev_name", "usb0")
    device = usb_device(
        vendor_id=config["vendor_id"],
        product_id=config["product_id"],
        dev_name=dev_name,
    )
    return await incus.add_device(name, dev_name, device,
                                   project=config.get("project", ""))


async def detach_usb(incus: Any, name: str, dev_name: str,
                     project: str = "") -> dict[str, Any]:
    return await incus.remove_device(name, dev_name, project=project)


# ── Network port forwarding ───────────────────────────────────────────────────

async def list_forwards(incus: Any, name: str,
                         project: str = "") -> dict[str, Any]:
    devices = await incus.list_devices(name, project=project)
    return {k: v for k, v in devices.items() if v.get("type") == "proxy"}


async def add_forward(incus: Any, name: str, config: dict[str, Any]) -> dict[str, Any]:
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


# ── Fleet operations ──────────────────────────────────────────────────────────

async def fleet_list(incus: Any, project: str = "",
                     status_filter: str = "") -> list[dict[str, Any]]:
    instances = await incus.list_instances(project=project,
                                            type_filter="container")
    if status_filter:
        instances = [i for i in instances
                     if i.get("status", "").lower() == status_filter.lower()]
    return instances


async def fleet_start(incus: Any, names: list[str],
                       project: str = "") -> list[dict[str, Any]]:
    import asyncio
    tasks = [incus.change_instance_state(n, "start", project=project)
             for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


async def fleet_stop(incus: Any, names: list[str],
                      project: str = "") -> list[dict[str, Any]]:
    import asyncio
    tasks = [incus.change_instance_state(n, "stop", force=True, project=project)
             for n in names]
    return list(await asyncio.gather(*tasks, return_exceptions=True))


# ── Publish ───────────────────────────────────────────────────────────────────

async def publish_container(incus: Any, config: dict[str, Any]) -> dict[str, Any]:
    """Publish a stopped container as a reusable Incus image."""
    name = config["name"]
    alias = config.get("alias", f"incusbox/{name}")
    description = config.get("description", f"incusbox container: {name}")
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
