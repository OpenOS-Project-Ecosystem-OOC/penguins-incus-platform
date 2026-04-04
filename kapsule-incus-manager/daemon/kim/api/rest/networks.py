from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["networks"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/networks")
async def list_networks(req: Request, project: str = "", remote: str = "") -> Any:
    return await _incus(req).list_networks(project=project)

@router.post("/networks", status_code=202)
async def create_network(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).create_network(body)

@router.get("/networks/{name}")
async def get_network(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).get_network(name, project=project)

@router.put("/networks/{name}")
async def update_network(req: Request, name: str, body: dict[str, Any], project: str = "") -> Any:
    await _incus(req).update_network(name, body, project=project)
    return {"status": "ok"}

@router.delete("/networks/{name}", status_code=202)
async def delete_network(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).delete_network(name, project=project)
