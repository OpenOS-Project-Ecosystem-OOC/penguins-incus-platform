from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["storage"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/storage-pools")
async def list_storage_pools(req: Request, remote: str = "") -> Any:
    return await _incus(req).list_storage_pools()

@router.post("/storage-pools", status_code=202)
async def create_storage_pool(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).create_storage_pool(body)

@router.get("/storage-pools/{name}")
async def get_storage_pool(req: Request, name: str) -> Any:
    return await _incus(req).get_storage_pool(name)

@router.put("/storage-pools/{name}")
async def update_storage_pool(req: Request, name: str, body: dict[str, Any]) -> Any:
    incus = _incus(req)
    await incus.put(f"/1.0/storage-pools/{name}", json=body)
    return {"status": "ok"}

@router.delete("/storage-pools/{name}", status_code=202)
async def delete_storage_pool(req: Request, name: str) -> Any:
    return await _incus(req).delete_storage_pool(name)

@router.get("/storage-pools/{pool}/volumes")
async def list_volumes(req: Request, pool: str, project: str = "") -> Any:
    return await _incus(req).list_storage_volumes(pool, project=project)

@router.post("/storage-pools/{pool}/volumes", status_code=202)
async def create_volume(req: Request, pool: str, body: dict[str, Any]) -> Any:
    return await _incus(req).create_storage_volume(pool, body)

@router.delete("/storage-pools/{pool}/volumes/{name}", status_code=202)
async def delete_volume(req: Request, pool: str, name: str, project: str = "") -> Any:
    return await _incus(req).delete_storage_volume(pool, name, project=project)
