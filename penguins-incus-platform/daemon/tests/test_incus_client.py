"""Tests for IncusClient multi-remote management (no live Incus required)."""

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

import penguins_incus.incus.client as _mod
from penguins_incus.incus.client import IncusClient


def make_client() -> IncusClient:
    """Return a client with _RemoteConnection patched out."""
    with patch.object(_mod, "_RemoteConnection", return_value=MagicMock()):
        return IncusClient(socket_path="/fake.sock")


def test_default_remote_is_local() -> None:
    client = make_client()
    assert client._active == "local"
    assert "local" in client.list_remote_names()


def test_add_remote_registers_name() -> None:
    client = make_client()
    with patch.object(_mod, "_RemoteConnection", return_value=MagicMock()):
        client.add_remote("prod", url="https://prod.example.com")
    assert "prod" in client.list_remote_names()


def test_set_remote_switches_active() -> None:
    client = make_client()
    with patch.object(_mod, "_RemoteConnection", return_value=MagicMock()):
        client.add_remote("prod", url="https://prod.example.com")
    client.set_remote("prod")
    assert client._active == "prod"


def test_set_remote_unknown_raises() -> None:
    client = make_client()
    with pytest.raises(KeyError):
        client.set_remote("nonexistent")


def test_remove_remote_unregisters() -> None:
    client = make_client()
    mock_conn = MagicMock()
    mock_conn.aclose = AsyncMock()
    with patch.object(_mod, "_RemoteConnection", return_value=mock_conn):
        client.add_remote("staging", url="https://staging.example.com")
    client.remove_remote("staging")
    assert "staging" not in client.list_remote_names()


def test_remove_local_raises() -> None:
    client = make_client()
    with pytest.raises(ValueError):
        client.remove_remote("local")


def test_remove_active_remote_falls_back_to_local() -> None:
    client = make_client()
    mock_conn = MagicMock()
    mock_conn.aclose = AsyncMock()
    with patch.object(_mod, "_RemoteConnection", return_value=mock_conn):
        client.add_remote("temp", url="https://temp.example.com")
    client.set_remote("temp")
    client.remove_remote("temp")
    assert client._active == "local"


def test_list_remote_names_includes_all() -> None:
    client = make_client()
    with patch.object(_mod, "_RemoteConnection", return_value=MagicMock()):
        client.add_remote("a", url="https://a.example.com")
        client.add_remote("b", url="https://b.example.com")
    names = client.list_remote_names()
    assert "local" in names
    assert "a" in names
    assert "b" in names
