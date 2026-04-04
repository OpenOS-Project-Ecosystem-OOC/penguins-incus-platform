"""Remote server management.

Remotes are persisted to ~/.config/penguins-incus/remotes.json.
Adding/removing a remote also updates the live IncusClient pool so
subsequent API calls can target the new remote immediately.
"""

from __future__ import annotations

import json
import pathlib
from typing import Any

from fastapi import APIRouter, HTTPException, Request

router = APIRouter(tags=["remotes"])

_REMOTES_FILE = pathlib.Path.home() / ".config" / "penguins-incus" / "remotes.json"


def _load() -> dict[str, Any]:
    if _REMOTES_FILE.exists():
        return json.loads(_REMOTES_FILE.read_text())  # type: ignore[return-value]
    return {}


def _save(remotes: dict[str, Any]) -> None:
    _REMOTES_FILE.parent.mkdir(parents=True, exist_ok=True)
    _REMOTES_FILE.write_text(json.dumps(remotes, indent=2))


def _incus(req: Request) -> Any:
    return req.app.state.incus


@router.get("/remotes")
async def list_remotes(req: Request) -> Any:
    remotes = _load()
    # Merge with live client state so "local" always appears
    live = _incus(req).list_remote_names()
    for name in live:
        if name not in remotes:
            remotes[name] = {"name": name, "url": "unix://", "protocol": "incus"}
    return list(remotes.values())


@router.post("/remotes", status_code=201)
async def add_remote(req: Request, body: dict[str, Any]) -> Any:
    remotes = _load()
    name = body.get("name", "")
    if not name:
        raise HTTPException(400, "name is required")
    if name in remotes:
        raise HTTPException(409, f"Remote '{name}' already exists")
    remotes[name] = body
    _save(remotes)
    # Register with the live IncusClient
    _incus(req).add_remote(
        name,
        url=body["url"],
        tls_cert=body.get("tls_cert"),
        tls_key=body.get("tls_key"),
    )
    return body


@router.get("/remotes/{name}")
async def get_remote(name: str) -> dict[str, Any]:
    remotes = _load()
    if name == "local":
        return {"name": "local", "url": "unix://", "protocol": "incus"}
    if name not in remotes:
        raise HTTPException(404, f"Remote '{name}' not found")
    return remotes[name]  # type: ignore[return-value]


@router.delete("/remotes/{name}", status_code=204)
async def remove_remote(req: Request, name: str) -> None:
    if name == "local":
        raise HTTPException(400, "Cannot remove the local remote")
    remotes = _load()
    if name not in remotes:
        raise HTTPException(404, f"Remote '{name}' not found")
    del remotes[name]
    _save(remotes)
    _incus(req).remove_remote(name)


@router.put("/remotes/{name}/activate", status_code=200)
async def activate_remote(req: Request, name: str) -> Any:
    """Switch the active remote for all subsequent API calls."""
    incus = _incus(req)
    if name not in incus.list_remote_names():
        raise HTTPException(404, f"Remote '{name}' not configured")
    incus.set_remote(name)
    return {"active": name}
