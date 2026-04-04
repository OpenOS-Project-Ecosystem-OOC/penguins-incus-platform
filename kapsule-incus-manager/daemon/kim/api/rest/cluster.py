from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["cluster"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/cluster/members")
async def list_cluster_members(req: Request, remote: str = "") -> Any:
    return await _incus(req).list_cluster_members()

@router.get("/cluster/members/{name}")
async def get_cluster_member(req: Request, name: str) -> Any:
    return await _incus(req).get(f"/1.0/cluster/members/{name}")

@router.delete("/cluster/members/{name}", status_code=202)
async def remove_cluster_member(req: Request, name: str) -> Any:
    return await _incus(req).delete_cluster_member(name)

@router.post("/cluster/members/{name}/evacuate", status_code=202)
async def evacuate_cluster_member(req: Request, name: str) -> Any:
    return await _incus(req).evacuate_cluster_member(name)

@router.post("/cluster/members/{name}/restore", status_code=202)
async def restore_cluster_member(req: Request, name: str) -> Any:
    return await _incus(req).restore_cluster_member(name)
