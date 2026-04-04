"""D-Bus service for the PIP daemon.

Exposes every operation from org.PenguinsIncusPlatform.xml.
All methods delegate to the same IncusClient used by the REST API,
ensuring identical behaviour across both transports.
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

from ...events import EventBus
from ...incus.client import IncusClient
from ...profiles.library import list_presets
from ...provisioning.compose import convert_compose, deploy_compose

logger = logging.getLogger(__name__)

_DBUS_SERVICE   = "org.PenguinsIncusPlatform"
_DBUS_PATH      = "/org/KapsuleIncusManager"
_DBUS_INTERFACE = "org.PenguinsIncusPlatform"


class DBusService:
    """Wraps the PIP daemon as a D-Bus service using dasbus."""

    def __init__(self, incus: IncusClient, bus: EventBus) -> None:
        self._incus = incus
        self._bus = bus
        self._connection = None

    async def run(self) -> None:
        """Start the D-Bus service and forward events as signals."""
        try:
            from dasbus.connection import SessionMessageBus  # type: ignore[import-untyped]
        except ImportError:
            logger.warning("dasbus not available — D-Bus service disabled")
            # Block forever so the TaskGroup doesn't exit
            await asyncio.Event().wait()
            return

        # Register service on session bus
        bus = SessionMessageBus()
        bus.publish_object(_DBUS_PATH, _PIPInterface(self._incus))
        bus.register_service(_DBUS_SERVICE)
        logger.info("D-Bus service registered: %s", _DBUS_SERVICE)

        # Forward EventBus events as D-Bus signals
        iface = bus.get_proxy(_DBUS_SERVICE, _DBUS_PATH)
        async for event in self._bus.iter_events():
            try:
                etype    = event.get("type", "")
                project  = event.get("project", "")
                metadata = event.get("metadata", {})

                if etype == "resource_usage":
                    # Emit the typed ResourceUsageUpdated signal
                    iface.ResourceUsageUpdated(
                        metadata.get("name", ""),
                        project,
                        float(metadata.get("cpu_usage", 0.0)),
                        int(metadata.get("memory_usage_bytes", 0)),
                        int(metadata.get("disk_usage_bytes", 0)),
                    )
                elif etype == "lifecycle":
                    # Emit InstanceStateChanged for lifecycle events
                    src  = metadata.get("source", "")
                    name = src.split("/")[-1] if "/" in src else src
                    action = metadata.get("action", "")
                    status_map = {
                        "started": "Running", "stopped": "Stopped",
                        "frozen": "Frozen",   "deleted": "Stopped",
                    }
                    status = status_map.get(action, "Unknown")
                    iface.InstanceStateChanged(name, project, status)
                    iface.EventReceived(etype, project,
                                        event.get("timestamp", ""),
                                        json.dumps(metadata))
                else:
                    iface.EventReceived(etype, project,
                                        event.get("timestamp", ""),
                                        json.dumps(metadata))
            except Exception as exc:
                logger.debug("D-Bus signal emission failed: %s", exc)


class _PIPInterface:
    """Implementation object published at the D-Bus path.

    Each method is a thin wrapper around IncusClient, matching the
    signatures in org.PenguinsIncusPlatform.xml exactly.
    JSON is used for structured arguments to stay within D-Bus type limits.
    """

    def __init__(self, incus: IncusClient) -> None:
        self._incus = incus

    def _run(self, coro: Any) -> Any:
        """Run a coroutine synchronously from a D-Bus method call."""
        return asyncio.get_event_loop().run_until_complete(coro)

    # ── Instances ─────────────────────────────────────────────────────────

    def ListInstances(self, project: str, remote: str, type_: str) -> str:
        result = self._run(self._incus.list_instances(
            project=project, remote=remote, type_filter=type_
        ))
        return json.dumps(result)

    def CreateInstance(self, config: str) -> str:
        op = self._run(self._incus.create_instance(json.loads(config)))
        return str(op.get("id", ""))

    def GetInstance(self, name: str, project: str, remote: str) -> str:
        return json.dumps(self._run(self._incus.get_instance(name, project=project)))

    def DeleteInstance(self, name: str, project: str, force: bool) -> str:
        op = self._run(self._incus.delete_instance(name, project=project, force=force))
        return str(op.get("id", ""))

    def ChangeInstanceState(self, name: str, project: str, action: str,
                             force: bool, timeout: int) -> str:
        op = self._run(self._incus.change_instance_state(
            name, action, force=force, timeout=timeout, project=project
        ))
        return str(op.get("id", ""))

    def RenameInstance(self, name: str, new_name: str, project: str) -> str:
        op = self._run(self._incus.rename_instance(name, new_name, project=project))
        return str(op.get("id", ""))

    def ListSnapshots(self, name: str, project: str) -> str:
        return json.dumps(self._run(self._incus.list_snapshots(name, project=project)))

    def CreateSnapshot(self, name: str, snapshot: str,
                       stateful: bool, project: str) -> str:
        op = self._run(self._incus.create_snapshot(
            name, snapshot, stateful=stateful, project=project
        ))
        return str(op.get("id", ""))

    def RestoreSnapshot(self, name: str, snapshot: str, project: str) -> str:
        op = self._run(self._incus.restore_snapshot(name, snapshot, project=project))
        return str(op.get("id", ""))

    def DeleteSnapshot(self, name: str, snapshot: str, project: str) -> str:
        op = self._run(self._incus.delete_snapshot(name, snapshot, project=project))
        return str(op.get("id", ""))

    def GetInstanceLogs(self, name: str, project: str) -> str:
        return str(self._run(self._incus.get_instance_logs(name, project=project)))

    def ConsoleInstance(self, name: str, project: str, type_: str,
                        width: int, height: int) -> str:
        # Returns a WebSocket URL on the daemon's HTTP server
        return (
            f"ws://127.0.0.1:8765/api/v1/instances/{name}/console/ws"
            f"?type={type_}&width={width}&height={height}&project={project}"
        )

    def ExecInstance(self, name: str, project: str, command: str,
                     width: int, height: int) -> str:
        # Returns a WebSocket URL on the daemon's HTTP server
        return f"ws://127.0.0.1:8765/api/v1/instances/{name}/exec/ws?command={command}&width={width}&height={height}&project={project}"

    def PullFile(self, name: str, project: str, path: str) -> str:
        import base64
        loop = asyncio.get_event_loop()
        incus = self._incus
        async def _pull() -> bytes:
            resp = await incus._http.get(
                f"/1.0/instances/{name}/files",
                params={"path": path, **({"project": project} if project else {})},
            )
            return resp.content
        content = loop.run_until_complete(_pull())
        return base64.b64encode(content).decode()

    def PushFile(self, name: str, project: str, path: str,
                 content_base64: str, mode: str) -> None:
        import base64
        content = base64.b64decode(content_base64)
        loop = asyncio.get_event_loop()
        incus = self._incus
        async def _push() -> None:
            await incus._http.post(
                f"/1.0/instances/{name}/files",
                content=content,
                params={"path": path, **({"project": project} if project else {})},
                headers={"X-Incus-mode": mode, "Content-Type": "application/octet-stream"},
            )
        loop.run_until_complete(_push())

    # ── Networks ──────────────────────────────────────────────────────────

    def ListNetworks(self, project: str, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_networks(project=project)))

    def CreateNetwork(self, config: str) -> str:
        op = self._run(self._incus.create_network(json.loads(config)))
        return str(op.get("id", ""))

    def GetNetwork(self, name: str, project: str) -> str:
        return json.dumps(self._run(self._incus.get_network(name, project=project)))

    def UpdateNetwork(self, name: str, project: str, config: str) -> None:
        self._run(self._incus.update_network(name, json.loads(config), project=project))

    def DeleteNetwork(self, name: str, project: str) -> str:
        op = self._run(self._incus.delete_network(name, project=project))
        return str(op.get("id", ""))

    # ── Storage ───────────────────────────────────────────────────────────

    def ListStoragePools(self, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_storage_pools()))

    def CreateStoragePool(self, config: str) -> str:
        op = self._run(self._incus.create_storage_pool(json.loads(config)))
        return str(op.get("id", ""))

    def GetStoragePool(self, name: str) -> str:
        return json.dumps(self._run(self._incus.get_storage_pool(name)))

    def UpdateStoragePool(self, name: str, config: str) -> None:
        self._run(self._incus.update_storage_pool(name, json.loads(config)))

    def DeleteStoragePool(self, name: str) -> str:
        op = self._run(self._incus.delete_storage_pool(name))
        return str(op.get("id", ""))

    def ListStorageVolumes(self, pool: str, project: str) -> str:
        return json.dumps(self._run(self._incus.list_storage_volumes(pool, project=project)))

    def CreateStorageVolume(self, pool: str, config: str) -> str:
        op = self._run(self._incus.create_storage_volume(pool, json.loads(config)))
        return str(op.get("id", ""))

    def DeleteStorageVolume(self, pool: str, name: str, project: str) -> str:
        op = self._run(self._incus.delete_storage_volume(pool, name, project=project))
        return str(op.get("id", ""))

    # ── Images ────────────────────────────────────────────────────────────

    def ListImages(self, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_images()))

    def PullImage(self, remote: str, image: str, alias: str) -> str:
        op = self._run(self._incus.pull_image(remote, image, alias=alias))
        return str(op.get("id", ""))

    def GetImage(self, fingerprint: str) -> str:
        return json.dumps(self._run(self._incus.get_image(fingerprint)))

    def DeleteImage(self, fingerprint: str) -> str:
        op = self._run(self._incus.delete_image(fingerprint))
        return str(op.get("id", ""))

    # ── Profiles ──────────────────────────────────────────────────────────

    def ListProfiles(self, project: str, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_profiles(project=project)))

    def CreateProfile(self, config: str) -> str:
        op = self._run(self._incus.create_profile(json.loads(config)))
        return str(op.get("id", ""))

    def GetProfile(self, name: str, project: str) -> str:
        return json.dumps(self._run(self._incus.get_profile(name, project=project)))

    def UpdateProfile(self, name: str, project: str, config: str) -> None:
        self._run(self._incus.update_profile(name, json.loads(config), project=project))

    def DeleteProfile(self, name: str, project: str) -> str:
        op = self._run(self._incus.delete_profile(name, project=project))
        return str(op.get("id", ""))

    def ListProfilePresets(self) -> str:
        return json.dumps(list_presets())

    # ── Projects ──────────────────────────────────────────────────────────

    def ListProjects(self, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_projects()))

    def CreateProject(self, config: str) -> str:
        op = self._run(self._incus.create_project(json.loads(config)))
        return str(op.get("id", ""))

    def GetProject(self, name: str) -> str:
        return json.dumps(self._run(self._incus.get_project(name)))

    def UpdateProject(self, name: str, config: str) -> None:
        self._run(self._incus.update_project(name, json.loads(config)))

    def DeleteProject(self, name: str) -> str:
        op = self._run(self._incus.delete_project(name))
        return str(op.get("id", ""))

    # ── Cluster ───────────────────────────────────────────────────────────

    def ListClusterMembers(self, remote: str) -> str:
        return json.dumps(self._run(self._incus.list_cluster_members()))

    def GetClusterMember(self, name: str) -> str:
        return json.dumps(self._run(self._incus.get_cluster_member(name)))

    def RemoveClusterMember(self, name: str) -> str:
        op = self._run(self._incus.delete_cluster_member(name))
        return str(op.get("id", ""))

    def EvacuateClusterMember(self, name: str) -> str:
        op = self._run(self._incus.evacuate_cluster_member(name))
        return str(op.get("id", ""))

    def RestoreClusterMember(self, name: str) -> str:
        op = self._run(self._incus.restore_cluster_member(name))
        return str(op.get("id", ""))

    # ── Operations ────────────────────────────────────────────────────────

    # ── Remotes ───────────────────────────────────────────────────────────

    def ListRemotes(self) -> str:
        import pathlib
        cfg = pathlib.Path.home() / ".config" / "penguins-incus" / "remotes.json"
        stored: dict[str, Any] = {}
        if cfg.exists():
            import json as _json
            stored = _json.loads(cfg.read_text())
        for name in self._incus.list_remote_names():
            if name not in stored:
                stored[name] = {"name": name, "url": "unix://", "protocol": "incus"}
        return json.dumps(list(stored.values()))

    def AddRemote(self, config: str) -> None:
        import pathlib
        body = json.loads(config)
        name = body["name"]
        cfg_path = pathlib.Path.home() / ".config" / "penguins-incus" / "remotes.json"
        cfg_path.parent.mkdir(parents=True, exist_ok=True)
        stored: dict[str, Any] = {}
        if cfg_path.exists():
            stored = json.loads(cfg_path.read_text())
        stored[name] = body
        cfg_path.write_text(json.dumps(stored, indent=2))
        self._incus.add_remote(name, url=body["url"],
                               tls_cert=body.get("tls_cert"),
                               tls_key=body.get("tls_key"))

    def GetRemote(self, name: str) -> str:
        import pathlib
        cfg = pathlib.Path.home() / ".config" / "penguins-incus" / "remotes.json"
        stored: dict[str, Any] = {}
        if cfg.exists():
            stored = json.loads(cfg.read_text())
        if name in stored:
            return json.dumps(stored[name])
        # Fall back to a minimal record for built-in remotes
        return json.dumps({"name": name, "url": "unix://", "protocol": "incus"})

    def RemoveRemote(self, name: str) -> None:
        import pathlib
        cfg_path = pathlib.Path.home() / ".config" / "penguins-incus" / "remotes.json"
        if cfg_path.exists():
            stored = json.loads(cfg_path.read_text())
            stored.pop(name, None)
            cfg_path.write_text(json.dumps(stored, indent=2))
        self._incus.remove_remote(name)

    def ActivateRemote(self, name: str) -> None:
        self._incus.set_remote(name)

    def ListOperations(self, status: str) -> str:
        ops = self._run(self._incus.list_operations())
        if status:
            ops = [o for o in ops if o.get("status", "").lower() == status.lower()]
        return json.dumps(ops)

    def GetOperation(self, id: str) -> str:
        return json.dumps(self._run(self._incus.get_operation(id)))

    def CancelOperation(self, id: str) -> None:
        self._run(self._incus.cancel_operation(id))

    # ── Provisioning ──────────────────────────────────────────────────────

    def DeployCompose(self, config: str) -> str:
        op = self._run(deploy_compose(self._incus, json.loads(config)))
        return str(op.get("id", ""))

    def ConvertCompose(self, compose_yaml: str) -> str:
        return json.dumps(convert_compose(compose_yaml))
