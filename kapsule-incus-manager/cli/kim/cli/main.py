"""KIM CLI — thin client over the KIM daemon REST API."""

from __future__ import annotations

import sys
from typing import Any

import click
from .client import DaemonClient


@click.group()
@click.option("--daemon", default="http://127.0.0.1:8765", envvar="KIM_DAEMON",
              show_default=True, help="KIM daemon base URL.")
@click.pass_context
def cli(ctx: click.Context, daemon: str) -> None:
    """Kapsule-Incus-Manager command-line interface."""
    ctx.ensure_object(dict)
    ctx.obj["client"] = DaemonClient(daemon)


# ── Containers ────────────────────────────────────────────────────────────────

@cli.group()
def container() -> None:
    """Manage containers."""


@container.command("list")
@click.option("--project", default="", help="Project name.")
@click.option("--remote",  default="", help="Remote name.")
@click.pass_context
def container_list(ctx: click.Context, project: str, remote: str) -> None:
    """List containers."""
    ctx.obj["client"].get("/api/v1/instances",
                          params={"type": "container", "project": project,
                                  "remote": remote})


@container.command("create")
@click.argument("name")
@click.option("--image",   required=True, help="Image (e.g. images:ubuntu/24.04).")
@click.option("--profile", multiple=True, help="Profile(s) to apply.")
@click.option("--project", default="",   help="Project name.")
@click.pass_context
def container_create(ctx: click.Context, name: str, image: str,
                     profile: tuple[str, ...], project: str) -> None:
    """Create a container."""
    ctx.obj["client"].post("/api/v1/instances", json={
        "name": name, "image": image,
        "profiles": list(profile), "type": "container",
        "project": project,
    })


@container.command("start")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def container_start(ctx: click.Context, name: str, project: str) -> None:
    """Start a container."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "start", "project": project})


@container.command("stop")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def container_stop(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Stop a container."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "stop", "force": force, "project": project})


@container.command("restart")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def container_restart(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Restart a container."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "restart", "force": force, "project": project})


@container.command("freeze")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def container_freeze(ctx: click.Context, name: str, project: str) -> None:
    """Freeze a container."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "freeze", "project": project})


@container.command("unfreeze")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def container_unfreeze(ctx: click.Context, name: str, project: str) -> None:
    """Unfreeze a container."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "unfreeze", "project": project})


@container.command("rename")
@click.argument("name")
@click.argument("new_name")
@click.option("--project", default="")
@click.pass_context
def container_rename(ctx: click.Context, name: str, new_name: str, project: str) -> None:
    """Rename a container."""
    ctx.obj["client"].post(f"/api/v1/instances/{name}/rename",
                           json={"new_name": new_name, "project": project})


@container.command("delete")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def container_delete(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Delete a container."""
    ctx.obj["client"].delete(f"/api/v1/instances/{name}",
                             params={"force": str(force).lower(), "project": project})


@container.command("logs")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def container_logs(ctx: click.Context, name: str, project: str) -> None:
    """Fetch container logs."""
    ctx.obj["client"].get_text(f"/api/v1/instances/{name}/logs",
                               params={"project": project})


@container.command("exec")
@click.argument("name")
@click.option("--command", "cmd", default="/bin/bash", show_default=True,
              help="Command to run inside the container.")
@click.option("--project", default="")
@click.pass_context
def container_exec(ctx: click.Context, name: str, cmd: str, project: str) -> None:
    """Open an interactive shell inside a container."""
    ctx.obj["client"].exec_session(name, command=cmd, project=project)


@container.command("file-pull")
@click.argument("name")
@click.argument("remote_path")
@click.argument("local_path")
@click.option("--project", default="")
@click.pass_context
def container_file_pull(ctx: click.Context, name: str, remote_path: str,
                        local_path: str, project: str) -> None:
    """Pull a file from a container to the local filesystem."""
    ctx.obj["client"].download_file(
        f"/api/v1/instances/{name}/files",
        params={"path": remote_path, "project": project},
        dest=local_path,
    )


@container.command("file-push")
@click.argument("name")
@click.argument("local_path")
@click.argument("remote_path")
@click.option("--project", default="")
@click.pass_context
def container_file_push(ctx: click.Context, name: str, local_path: str,
                        remote_path: str, project: str) -> None:
    """Push a local file into a container."""
    ctx.obj["client"].upload_file(
        f"/api/v1/instances/{name}/files",
        params={"path": remote_path, "project": project},
        src=local_path,
    )


# ── Snapshots ─────────────────────────────────────────────────────────────────

@cli.group()
def snapshot() -> None:
    """Manage instance snapshots."""


@snapshot.command("list")
@click.argument("instance")
@click.option("--project", default="")
@click.pass_context
def snapshot_list(ctx: click.Context, instance: str, project: str) -> None:
    """List snapshots of an instance."""
    ctx.obj["client"].get(f"/api/v1/instances/{instance}/snapshots",
                          params={"project": project})


@snapshot.command("create")
@click.argument("instance")
@click.argument("snap_name")
@click.option("--stateful/--no-stateful", default=False,
              help="Include instance state in snapshot.")
@click.option("--project", default="")
@click.pass_context
def snapshot_create(ctx: click.Context, instance: str, snap_name: str,
                    stateful: bool, project: str) -> None:
    """Create a snapshot."""
    ctx.obj["client"].post(f"/api/v1/instances/{instance}/snapshots",
                           json={"name": snap_name, "stateful": stateful,
                                 "project": project})


@snapshot.command("restore")
@click.argument("instance")
@click.argument("snap_name")
@click.option("--project", default="")
@click.pass_context
def snapshot_restore(ctx: click.Context, instance: str, snap_name: str,
                     project: str) -> None:
    """Restore an instance to a snapshot."""
    ctx.obj["client"].post(
        f"/api/v1/instances/{instance}/snapshots/{snap_name}",
        params={"project": project},
        json={},
    )


@snapshot.command("delete")
@click.argument("instance")
@click.argument("snap_name")
@click.option("--project", default="")
@click.pass_context
def snapshot_delete(ctx: click.Context, instance: str, snap_name: str,
                    project: str) -> None:
    """Delete a snapshot."""
    ctx.obj["client"].delete(
        f"/api/v1/instances/{instance}/snapshots/{snap_name}",
        params={"project": project},
    )


# ── VMs ───────────────────────────────────────────────────────────────────────

@cli.group()
def vm() -> None:
    """Manage virtual machines."""


@vm.command("list")
@click.option("--project", default="")
@click.option("--remote",  default="")
@click.pass_context
def vm_list(ctx: click.Context, project: str, remote: str) -> None:
    """List virtual machines."""
    ctx.obj["client"].get("/api/v1/instances",
                          params={"type": "virtual-machine",
                                  "project": project, "remote": remote})


@vm.command("create")
@click.argument("name")
@click.option("--image",   required=True, help="Image (e.g. images:ubuntu/24.04).")
@click.option("--profile", multiple=True, help="Profile(s) to apply.")
@click.option("--project", default="")
@click.pass_context
def vm_create(ctx: click.Context, name: str, image: str,
              profile: tuple[str, ...], project: str) -> None:
    """Create a virtual machine."""
    ctx.obj["client"].post("/api/v1/instances", json={
        "name": name, "image": image,
        "profiles": list(profile), "type": "virtual-machine",
        "project": project,
    })


@vm.command("start")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def vm_start(ctx: click.Context, name: str, project: str) -> None:
    """Start a VM."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "start", "project": project})


@vm.command("stop")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def vm_stop(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Stop a VM."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "stop", "force": force, "project": project})


@vm.command("restart")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def vm_restart(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Restart a VM."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "restart", "force": force, "project": project})


@vm.command("delete")
@click.argument("name")
@click.option("--force/--no-force", default=False)
@click.option("--project", default="")
@click.pass_context
def vm_delete(ctx: click.Context, name: str, force: bool, project: str) -> None:
    """Delete a VM."""
    ctx.obj["client"].delete(f"/api/v1/instances/{name}",
                             params={"force": str(force).lower(), "project": project})


@vm.command("freeze")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def vm_freeze(ctx: click.Context, name: str, project: str) -> None:
    """Freeze a VM."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "freeze", "project": project})


@vm.command("unfreeze")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def vm_unfreeze(ctx: click.Context, name: str, project: str) -> None:
    """Unfreeze a VM."""
    ctx.obj["client"].put(f"/api/v1/instances/{name}/state",
                          json={"action": "unfreeze", "project": project})


@vm.command("rename")
@click.argument("name")
@click.argument("new_name")
@click.option("--project", default="")
@click.pass_context
def vm_rename(ctx: click.Context, name: str, new_name: str, project: str) -> None:
    """Rename a VM."""
    ctx.obj["client"].post(f"/api/v1/instances/{name}/rename",
                           json={"new_name": new_name, "project": project})


@vm.command("logs")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def vm_logs(ctx: click.Context, name: str, project: str) -> None:
    """Fetch VM logs."""
    ctx.obj["client"].get_text(f"/api/v1/instances/{name}/logs",
                               params={"project": project})


@vm.command("exec")
@click.argument("name")
@click.option("--command", "cmd", default="/bin/bash", show_default=True,
              help="Command to run inside the VM.")
@click.option("--project", default="")
@click.pass_context
def vm_exec(ctx: click.Context, name: str, cmd: str, project: str) -> None:
    """Open an interactive shell inside a VM."""
    ctx.obj["client"].exec_session(name, command=cmd, project=project)


@vm.command("file-pull")
@click.argument("name")
@click.argument("remote_path")
@click.argument("local_path")
@click.option("--project", default="")
@click.pass_context
def vm_file_pull(ctx: click.Context, name: str, remote_path: str,
                 local_path: str, project: str) -> None:
    """Pull a file from a VM to the local filesystem."""
    ctx.obj["client"].download_file(
        f"/api/v1/instances/{name}/files",
        params={"path": remote_path, "project": project},
        dest=local_path,
    )


@vm.command("file-push")
@click.argument("name")
@click.argument("local_path")
@click.argument("remote_path")
@click.option("--project", default="")
@click.pass_context
def vm_file_push(ctx: click.Context, name: str, local_path: str,
                 remote_path: str, project: str) -> None:
    """Push a local file into a VM."""
    ctx.obj["client"].upload_file(
        f"/api/v1/instances/{name}/files",
        params={"path": remote_path, "project": project},
        src=local_path,
    )


# ── Networks ──────────────────────────────────────────────────────────────────

@cli.group()
def network() -> None:
    """Manage networks."""


@network.command("list")
@click.option("--project", default="")
@click.pass_context
def network_list(ctx: click.Context, project: str) -> None:
    """List networks."""
    ctx.obj["client"].get("/api/v1/networks", params={"project": project})


@network.command("create")
@click.argument("name")
@click.option("--type", "net_type", default="bridge",
              type=click.Choice(["bridge", "macvlan", "sriov", "ovn", "physical"]),
              show_default=True, help="Network type.")
@click.option("--description", default="")
@click.option("--project", default="")
@click.pass_context
def network_create(ctx: click.Context, name: str, net_type: str,
                   description: str, project: str) -> None:
    """Create a network."""
    ctx.obj["client"].post("/api/v1/networks",
                           json={"name": name, "type": net_type,
                                 "description": description, "project": project})


@network.command("delete")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def network_delete(ctx: click.Context, name: str, project: str) -> None:
    """Delete a network."""
    ctx.obj["client"].delete(f"/api/v1/networks/{name}",
                             params={"project": project})


# ── Storage ───────────────────────────────────────────────────────────────────

@cli.group()
def storage() -> None:
    """Manage storage pools."""


@storage.command("list")
@click.pass_context
def storage_list(ctx: click.Context) -> None:
    """List storage pools."""
    ctx.obj["client"].get("/api/v1/storage-pools")


@storage.command("create")
@click.argument("name")
@click.option("--driver", default="dir",
              type=click.Choice(["dir", "btrfs", "lvm", "zfs", "ceph"]),
              show_default=True, help="Storage driver.")
@click.option("--description", default="")
@click.pass_context
def storage_create(ctx: click.Context, name: str, driver: str,
                   description: str) -> None:
    """Create a storage pool."""
    ctx.obj["client"].post("/api/v1/storage-pools",
                           json={"name": name, "driver": driver,
                                 "description": description})


@storage.command("delete")
@click.argument("name")
@click.pass_context
def storage_delete(ctx: click.Context, name: str) -> None:
    """Delete a storage pool."""
    ctx.obj["client"].delete(f"/api/v1/storage-pools/{name}")


@storage.group("volume")
def storage_volume() -> None:
    """Manage storage volumes."""


@storage_volume.command("list")
@click.argument("pool")
@click.option("--project", default="")
@click.pass_context
def volume_list(ctx: click.Context, pool: str, project: str) -> None:
    """List volumes in a storage pool."""
    ctx.obj["client"].get(f"/api/v1/storage-pools/{pool}/volumes",
                          params={"project": project})


@storage_volume.command("create")
@click.argument("pool")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def volume_create(ctx: click.Context, pool: str, name: str, project: str) -> None:
    """Create a custom storage volume."""
    ctx.obj["client"].post(f"/api/v1/storage-pools/{pool}/volumes",
                           json={"name": name, "project": project})


@storage_volume.command("delete")
@click.argument("pool")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def volume_delete(ctx: click.Context, pool: str, name: str, project: str) -> None:
    """Delete a storage volume."""
    ctx.obj["client"].delete(f"/api/v1/storage-pools/{pool}/volumes/{name}",
                             params={"project": project})


# ── Images ────────────────────────────────────────────────────────────────────

@cli.group()
def image() -> None:
    """Manage images."""


@image.command("list")
@click.option("--remote", default="")
@click.pass_context
def image_list(ctx: click.Context, remote: str) -> None:
    """List images."""
    ctx.obj["client"].get("/api/v1/images", params={"remote": remote})


@image.command("pull")
@click.argument("remote")
@click.argument("image_ref")
@click.option("--alias", default="", help="Local alias for the image.")
@click.pass_context
def image_pull(ctx: click.Context, remote: str, image_ref: str, alias: str) -> None:
    """Pull an image from a remote."""
    ctx.obj["client"].post("/api/v1/images",
                           json={"remote": remote, "image": image_ref, "alias": alias})


@image.command("delete")
@click.argument("fingerprint")
@click.pass_context
def image_delete(ctx: click.Context, fingerprint: str) -> None:
    """Delete a local image."""
    ctx.obj["client"].delete(f"/api/v1/images/{fingerprint}")


# ── Profiles ──────────────────────────────────────────────────────────────────

@cli.group()
def profile() -> None:
    """Manage profiles."""


@profile.command("list")
@click.option("--project", default="")
@click.pass_context
def profile_list(ctx: click.Context, project: str) -> None:
    """List profiles."""
    ctx.obj["client"].get("/api/v1/profiles", params={"project": project})


@profile.command("presets")
@click.pass_context
def profile_presets(ctx: click.Context) -> None:
    """List built-in profile presets."""
    ctx.obj["client"].get("/api/v1/profiles/presets")


@profile.command("create")
@click.argument("name")
@click.option("--description", default="")
@click.option("--project", default="")
@click.pass_context
def profile_create(ctx: click.Context, name: str, description: str,
                   project: str) -> None:
    """Create a profile."""
    ctx.obj["client"].post("/api/v1/profiles",
                           json={"name": name, "description": description,
                                 "project": project})


@profile.command("delete")
@click.argument("name")
@click.option("--project", default="")
@click.pass_context
def profile_delete(ctx: click.Context, name: str, project: str) -> None:
    """Delete a profile."""
    ctx.obj["client"].delete(f"/api/v1/profiles/{name}",
                             params={"project": project})


# ── Projects ──────────────────────────────────────────────────────────────────

@cli.group()
def project() -> None:
    """Manage projects."""


@project.command("list")
@click.pass_context
def project_list(ctx: click.Context) -> None:
    """List projects."""
    ctx.obj["client"].get("/api/v1/projects")


@project.command("create")
@click.argument("name")
@click.option("--description", default="")
@click.pass_context
def project_create(ctx: click.Context, name: str, description: str) -> None:
    """Create a project."""
    ctx.obj["client"].post("/api/v1/projects",
                           json={"name": name, "description": description})


@project.command("delete")
@click.argument("name")
@click.pass_context
def project_delete(ctx: click.Context, name: str) -> None:
    """Delete a project."""
    ctx.obj["client"].delete(f"/api/v1/projects/{name}")


# ── Cluster ───────────────────────────────────────────────────────────────────

@cli.group()
def cluster() -> None:
    """Manage cluster members."""


@cluster.command("list")
@click.pass_context
def cluster_list(ctx: click.Context) -> None:
    """List cluster members."""
    ctx.obj["client"].get("/api/v1/cluster/members")


@cluster.command("evacuate")
@click.argument("member")
@click.pass_context
def cluster_evacuate(ctx: click.Context, member: str) -> None:
    """Evacuate workloads from a cluster member."""
    ctx.obj["client"].post(f"/api/v1/cluster/members/{member}/evacuate", json={})


@cluster.command("restore")
@click.argument("member")
@click.pass_context
def cluster_restore(ctx: click.Context, member: str) -> None:
    """Restore a previously evacuated cluster member."""
    ctx.obj["client"].post(f"/api/v1/cluster/members/{member}/restore", json={})


@cluster.command("remove")
@click.argument("member")
@click.pass_context
def cluster_remove(ctx: click.Context, member: str) -> None:
    """Remove a member from the cluster."""
    ctx.obj["client"].delete(f"/api/v1/cluster/members/{member}")


# ── Remotes ───────────────────────────────────────────────────────────────────

@cli.group()
def remote() -> None:
    """Manage remote servers."""


@remote.command("list")
@click.pass_context
def remote_list(ctx: click.Context) -> None:
    """List configured remotes."""
    ctx.obj["client"].get("/api/v1/remotes")


@remote.command("add")
@click.argument("name")
@click.argument("url")
@click.pass_context
def remote_add(ctx: click.Context, name: str, url: str) -> None:
    """Add a remote server."""
    ctx.obj["client"].post("/api/v1/remotes", json={"name": name, "url": url})


@remote.command("activate")
@click.argument("name")
@click.pass_context
def remote_activate(ctx: click.Context, name: str) -> None:
    """Switch the active remote."""
    ctx.obj["client"].put(f"/api/v1/remotes/{name}/activate", json={})


@remote.command("remove")
@click.argument("name")
@click.pass_context
def remote_remove(ctx: click.Context, name: str) -> None:
    """Remove a remote server."""
    ctx.obj["client"].delete(f"/api/v1/remotes/{name}")


# ── Operations ────────────────────────────────────────────────────────────────

@cli.group()
def operation() -> None:
    """Manage operations."""


@operation.command("list")
@click.pass_context
def operation_list(ctx: click.Context) -> None:
    """List running and recent operations."""
    ctx.obj["client"].get("/api/v1/operations")


@operation.command("cancel")
@click.argument("id")
@click.pass_context
def operation_cancel(ctx: click.Context, id: str) -> None:
    """Cancel a running operation."""
    ctx.obj["client"].delete(f"/api/v1/operations/{id}")


# ── Provisioning ──────────────────────────────────────────────────────────────

@cli.group()
def provision() -> None:
    """Compose-based provisioning."""


@provision.command("convert")
@click.argument("compose_file", type=click.Path(exists=True, dir_okay=False))
@click.pass_context
def provision_convert(ctx: click.Context, compose_file: str) -> None:
    """Convert a Docker Compose file to KIM instance config."""
    with open(compose_file) as f:
        compose_yaml = f.read()
    ctx.obj["client"].post("/api/v1/provisioning/compose/convert",
                           json={"compose": compose_yaml})


@provision.command("deploy")
@click.argument("compose_file", type=click.Path(exists=True, dir_okay=False))
@click.option("--project", default="")
@click.pass_context
def provision_deploy(ctx: click.Context, compose_file: str, project: str) -> None:
    """Deploy a Docker Compose file as Incus instances."""
    with open(compose_file) as f:
        compose_yaml = f.read()
    ctx.obj["client"].post("/api/v1/provisioning/compose",
                           json={"compose": compose_yaml, "project": project})


# ── Events ────────────────────────────────────────────────────────────────────

@cli.command("events")
@click.option("--type", "event_type", default="", help="Filter by event type.")
@click.pass_context
def events(ctx: click.Context, event_type: str) -> None:
    """Stream live events from the daemon."""
    ctx.obj["client"].stream_events(event_type)
