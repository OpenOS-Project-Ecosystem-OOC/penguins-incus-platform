"""Tests for the guest-type provisioning plugins.

All tests use a mock IncusClient so no live Incus daemon is required.
"""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock

import pytest


# ── Helpers ───────────────────────────────────────────────────────────────────

def make_incus(**overrides: object) -> MagicMock:
    """Return a mock IncusClient with async methods pre-configured."""
    m = MagicMock()
    m.create_instance = AsyncMock(return_value={"id": "op-1"})
    m.get_instance = AsyncMock(return_value={
        "name": "test", "config": {}, "devices": {}, "profiles": [],
    })
    m.change_instance_state = AsyncMock(return_value={"id": "op-2"})
    m.list_instances = AsyncMock(return_value=[])
    m.list_snapshots = AsyncMock(return_value=[])
    m.create_snapshot = AsyncMock(return_value={"id": "op-3"})
    m.restore_snapshot = AsyncMock(return_value={"id": "op-4"})
    m.delete_snapshot = AsyncMock(return_value={"id": "op-5"})
    m.add_device = AsyncMock(return_value={"id": "op-6"})
    m.remove_device = AsyncMock(return_value={"id": "op-7"})
    m.list_devices = AsyncMock(return_value={})
    m.get_host_resources = AsyncMock(return_value={"gpu": {"cards": []}, "usb": {"devices": []}})
    m.exec_instance = AsyncMock(return_value={"id": "op-8"})
    m.push_file = AsyncMock(return_value=None)
    m.post = AsyncMock(return_value={"id": "op-9"})
    m.put = AsyncMock(return_value={"id": "op-10"})
    m.get = AsyncMock(return_value=[])
    m.get_profile = AsyncMock(return_value={"name": "macos-kvm"})
    m.create_profile = AsyncMock(return_value={"id": "op-11"})
    for k, v in overrides.items():
        setattr(m, k, v)
    return m


# ── _base helpers ─────────────────────────────────────────────────────────────

def test_base_build_cloud_init_includes_header() -> None:
    from penguins_incus.provisioning._base import build_cloud_init
    result = build_cloud_init(packages=["curl"])
    assert result.startswith("#cloud-config")
    assert "curl" in result


def test_base_build_cloud_init_empty() -> None:
    from penguins_incus.provisioning._base import build_cloud_init
    result = build_cloud_init()
    assert result.startswith("#cloud-config")


def test_base_instance_config_structure() -> None:
    from penguins_incus.provisioning._base import base_instance_config
    cfg = base_instance_config("mybox", "images:ubuntu/24.04")
    assert cfg["name"] == "mybox"
    assert cfg["type"] == "container"
    assert cfg["source"]["alias"] == "images:ubuntu/24.04"
    assert "root" in cfg["devices"]


def test_base_proxy_device() -> None:
    from penguins_incus.provisioning._base import proxy_device
    d = proxy_device(8080, 80)
    assert d["type"] == "proxy"
    assert "8080" in d["listen"]
    assert "80" in d["connect"]


def test_base_gpu_device() -> None:
    from penguins_incus.provisioning._base import gpu_device
    d = gpu_device(gpu_type="physical", pci="0000:01:00.0")
    assert d["type"] == "gpu"
    assert d["pci"] == "0000:01:00.0"


def test_base_usb_device() -> None:
    from penguins_incus.provisioning._base import usb_device
    d = usb_device("046d", "c52b")
    assert d["type"] == "usb"
    assert d["vendorid"] == "046d"
    assert d["productid"] == "c52b"


def test_base_snapshot_schedule_config() -> None:
    from penguins_incus.provisioning._base import snapshot_schedule_config
    cfg = snapshot_schedule_config("@daily", "7d")
    assert cfg["snapshots.schedule"] == "@daily"
    assert cfg["snapshots.expiry"] == "7d"


# ── generic provisioning ──────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_generic_create_container_calls_create_instance() -> None:
    from penguins_incus.provisioning.generic import create_container
    incus = make_incus()
    await create_container(incus, {"name": "mybox"})
    incus.create_instance.assert_called_once()
    payload = incus.create_instance.call_args[0][0]
    assert payload["name"] == "mybox"
    assert payload["type"] == "container"


@pytest.mark.asyncio
async def test_generic_create_container_nvidia_adds_profile() -> None:
    from penguins_incus.provisioning.generic import create_container
    incus = make_incus()
    await create_container(incus, {"name": "mybox", "nvidia": True})
    payload = incus.create_instance.call_args[0][0]
    assert "gpu" in payload["profiles"]


@pytest.mark.asyncio
async def test_generic_create_container_volumes_become_devices() -> None:
    from penguins_incus.provisioning.generic import create_container
    incus = make_incus()
    await create_container(incus, {"name": "mybox", "volumes": ["/src:/dst"]})
    payload = incus.create_instance.call_args[0][0]
    assert any(v.get("source") == "/src" for v in payload["devices"].values())


@pytest.mark.asyncio
async def test_generic_assemble_runs_packages() -> None:
    from penguins_incus.provisioning.generic import assemble_container
    incus = make_incus()
    await assemble_container(incus, "mybox", {"packages": ["vim", "git"]})
    calls = [str(c) for c in incus.exec_instance.call_args_list]
    assert any("vim" in c for c in calls)


@pytest.mark.asyncio
async def test_generic_attach_gpu_calls_add_device() -> None:
    from penguins_incus.provisioning.generic import attach_gpu
    incus = make_incus()
    await attach_gpu(incus, "mybox", {"dev_name": "gpu0", "gpu_type": "physical"})
    incus.add_device.assert_called_once()
    args = incus.add_device.call_args
    assert args[0][1] == "gpu0"
    assert args[0][2]["type"] == "gpu"


@pytest.mark.asyncio
async def test_generic_detach_gpu_calls_remove_device() -> None:
    from penguins_incus.provisioning.generic import detach_gpu
    incus = make_incus()
    await detach_gpu(incus, "mybox", "gpu0")
    incus.remove_device.assert_called_once_with("mybox", "gpu0", project="")


@pytest.mark.asyncio
async def test_generic_attach_usb_calls_add_device() -> None:
    from penguins_incus.provisioning.generic import attach_usb
    incus = make_incus()
    await attach_usb(incus, "mybox", {
        "vendor_id": "046d", "product_id": "c52b", "dev_name": "usb0",
    })
    incus.add_device.assert_called_once()
    device = incus.add_device.call_args[0][2]
    assert device["type"] == "usb"
    assert device["vendorid"] == "046d"


@pytest.mark.asyncio
async def test_generic_add_forward_calls_add_device() -> None:
    from penguins_incus.provisioning.generic import add_forward
    incus = make_incus()
    await add_forward(incus, "mybox", {"host_port": 8080, "guest_port": 80})
    incus.add_device.assert_called_once()
    device = incus.add_device.call_args[0][2]
    assert device["type"] == "proxy"
    assert "8080" in device["listen"]


@pytest.mark.asyncio
async def test_generic_fleet_list_filters_by_status() -> None:
    from penguins_incus.provisioning.generic import fleet_list
    incus = make_incus()
    incus.list_instances = AsyncMock(return_value=[
        {"name": "a", "status": "Running", "profiles": []},
        {"name": "b", "status": "Stopped", "profiles": []},
    ])
    result = await fleet_list(incus, status_filter="Running")
    assert len(result) == 1
    assert result[0]["name"] == "a"


@pytest.mark.asyncio
async def test_generic_set_snapshot_schedule_updates_config() -> None:
    from penguins_incus.provisioning.generic import set_snapshot_schedule
    incus = make_incus()
    await set_snapshot_schedule(incus, "mybox", "@daily", expiry="7d")
    incus.put.assert_called_once()
    payload = incus.put.call_args[1]["json"]
    assert payload["config"]["snapshots.schedule"] == "@daily"
    assert payload["config"]["snapshots.expiry"] == "7d"


# ── waydroid provisioning ─────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_waydroid_create_sets_nesting() -> None:
    from penguins_incus.provisioning.waydroid import create_waydroid_container
    incus = make_incus()
    await create_waydroid_container(incus, {"name": "wdroid"})
    payload = incus.create_instance.call_args[0][0]
    assert payload["config"].get("security.nesting") == "true"


@pytest.mark.asyncio
async def test_waydroid_create_gpu_adds_device() -> None:
    from penguins_incus.provisioning.waydroid import create_waydroid_container
    incus = make_incus()
    await create_waydroid_container(incus, {"name": "wdroid", "gpu": True})
    payload = incus.create_instance.call_args[0][0]
    assert any(v.get("type") == "gpu" for v in payload["devices"].values())


@pytest.mark.asyncio
async def test_waydroid_create_adds_adb_proxy() -> None:
    from penguins_incus.provisioning.waydroid import create_waydroid_container
    incus = make_incus()
    await create_waydroid_container(incus, {"name": "wdroid"})
    payload = incus.create_instance.call_args[0][0]
    assert "adb" in payload["devices"]
    assert payload["devices"]["adb"]["type"] == "proxy"


@pytest.mark.asyncio
async def test_waydroid_install_extension_rejects_unknown() -> None:
    from penguins_incus.provisioning.waydroid import install_extension
    incus = make_incus()
    with pytest.raises(ValueError, match="Unknown extension"):
        await install_extension(incus, "wdroid", {"extension": "badext"})


@pytest.mark.asyncio
async def test_waydroid_install_extension_known() -> None:
    from penguins_incus.provisioning.waydroid import install_extension
    incus = make_incus()
    result = await install_extension(incus, "wdroid", {"extension": "gapps"})
    assert result["extension"] == "gapps"
    incus.exec_instance.assert_called_once()


@pytest.mark.asyncio
async def test_waydroid_fleet_list_filters_by_profile() -> None:
    from penguins_incus.provisioning.waydroid import fleet_list
    incus = make_incus()
    incus.list_instances = AsyncMock(return_value=[
        {"name": "a", "status": "Running", "profiles": ["waydroid"]},
        {"name": "b", "status": "Running", "profiles": ["default"]},
    ])
    result = await fleet_list(incus)
    assert len(result) == 1
    assert result[0]["name"] == "a"


# ── macOS provisioning ────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_macos_create_vm_creates_instance() -> None:
    from penguins_incus.provisioning.macos import create_macos_vm
    incus = make_incus()
    await create_macos_vm(incus, {"name": "macos-sonoma"})
    incus.create_instance.assert_called_once()
    payload = incus.create_instance.call_args[0][0]
    assert payload["name"] == "macos-sonoma"
    assert payload["type"] == "virtual-machine"


@pytest.mark.asyncio
async def test_macos_create_vm_attaches_three_volumes() -> None:
    from penguins_incus.provisioning.macos import create_macos_vm
    incus = make_incus()
    await create_macos_vm(incus, {"name": "macos-sonoma"})
    # Three add_device calls: macos-disk, opencore, installer
    assert incus.add_device.call_count == 3
    dev_names = {c[0][1] for c in incus.add_device.call_args_list}
    assert dev_names == {"macos-disk", "opencore", "installer"}


@pytest.mark.asyncio
async def test_macos_create_vm_creates_profile_if_missing() -> None:
    from penguins_incus.provisioning.macos import create_macos_vm
    incus = make_incus()
    incus.get_profile = AsyncMock(side_effect=Exception("not found"))
    await create_macos_vm(incus, {"name": "macos-sonoma"})
    incus.create_profile.assert_called_once()


@pytest.mark.asyncio
async def test_macos_fleet_list_filters_by_profile() -> None:
    from penguins_incus.provisioning.macos import fleet_list
    incus = make_incus()
    incus.list_instances = AsyncMock(return_value=[
        {"name": "mac1", "status": "Running", "profiles": ["macos-kvm"]},
        {"name": "win1", "status": "Running", "profiles": ["windows-desktop"]},
    ])
    result = await fleet_list(incus)
    assert len(result) == 1
    assert result[0]["name"] == "mac1"


@pytest.mark.asyncio
async def test_macos_resize_disk_calls_put() -> None:
    from penguins_incus.provisioning.macos import resize_disk
    incus = make_incus()
    await resize_disk(incus, "mac1", {"new_size": "256G"})
    incus.put.assert_called_once()
    payload = incus.put.call_args[1]["json"]
    assert payload["config"]["size"] == "256G"


# ── Windows provisioning ──────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_windows_create_vm_creates_instance() -> None:
    from penguins_incus.provisioning.windows import create_windows_vm
    incus = make_incus()
    await create_windows_vm(incus, {"name": "win11"})
    incus.create_instance.assert_called_once()
    payload = incus.create_instance.call_args[0][0]
    assert payload["name"] == "win11"
    assert payload["type"] == "virtual-machine"


@pytest.mark.asyncio
async def test_windows_create_vm_rdp_proxy_added() -> None:
    from penguins_incus.provisioning.windows import create_windows_vm
    incus = make_incus()
    await create_windows_vm(incus, {"name": "win11", "rdp": True})
    payload = incus.create_instance.call_args[0][0]
    assert "rdp" in payload["devices"]
    assert payload["devices"]["rdp"]["type"] == "proxy"


@pytest.mark.asyncio
async def test_windows_create_vm_no_rdp() -> None:
    from penguins_incus.provisioning.windows import create_windows_vm
    incus = make_incus()
    await create_windows_vm(incus, {"name": "win11", "rdp": False})
    payload = incus.create_instance.call_args[0][0]
    assert "rdp" not in payload["devices"]


@pytest.mark.asyncio
async def test_windows_create_vm_attaches_iso() -> None:
    from penguins_incus.provisioning.windows import create_windows_vm
    incus = make_incus()
    await create_windows_vm(incus, {"name": "win11", "image": "/tmp/win11.iso"})
    assert incus.add_device.call_count >= 1
    dev_names = {c[0][1] for c in incus.add_device.call_args_list}
    assert "install" in dev_names


@pytest.mark.asyncio
async def test_windows_create_vm_gpu_overlay_adds_profile() -> None:
    from penguins_incus.provisioning.windows import create_windows_vm
    incus = make_incus()
    await create_windows_vm(incus, {"name": "win11", "gpu_overlay": "vfio"})
    payload = incus.create_instance.call_args[0][0]
    assert "gpu-vfio" in payload["profiles"]


@pytest.mark.asyncio
async def test_windows_fleet_list_filters_windows_vms() -> None:
    from penguins_incus.provisioning.windows import fleet_list
    incus = make_incus()
    incus.list_instances = AsyncMock(return_value=[
        {"name": "win1", "status": "Running", "profiles": ["windows-desktop"]},
        {"name": "mac1", "status": "Running", "profiles": ["macos-kvm"]},
    ])
    result = await fleet_list(incus)
    assert len(result) == 1
    assert result[0]["name"] == "win1"


@pytest.mark.asyncio
async def test_windows_install_guest_tools_unknown_tool() -> None:
    from penguins_incus.provisioning.windows import install_guest_tools
    incus = make_incus()
    result = await install_guest_tools(incus, "win11", {"tools": ["badtool"]})
    assert result["tools"][0]["error"] == "unknown tool"


@pytest.mark.asyncio
async def test_windows_install_guest_tools_known() -> None:
    from penguins_incus.provisioning.windows import install_guest_tools
    incus = make_incus()
    result = await install_guest_tools(incus, "win11", {"tools": ["svcguest"]})
    assert result["tools"][0]["tool"] == "svcguest"
    incus.exec_instance.assert_called_once()


@pytest.mark.asyncio
async def test_windows_harden_calls_exec() -> None:
    from penguins_incus.provisioning.windows import harden_vm
    incus = make_incus()
    result = await harden_vm(incus, "win11", {"level": "strict"})
    assert result["level"] == "strict"
    incus.exec_instance.assert_called_once()
    cmd = incus.exec_instance.call_args[0][1]
    assert "-Level" in cmd
    assert "strict" in cmd


@pytest.mark.asyncio
async def test_windows_cloud_sync_stores_in_config() -> None:
    from penguins_incus.provisioning.windows import configure_cloud_sync
    incus = make_incus()
    await configure_cloud_sync(incus, "win11", {
        "remote_name": "b2", "remote_path": "bucket/backups",
    })
    incus.put.assert_called_once()
    payload = incus.put.call_args[1]["json"]
    assert payload["config"]["user.iwt.cloud_sync.remote"] == "b2"
    assert payload["config"]["user.iwt.cloud_sync.path"] == "bucket/backups"
