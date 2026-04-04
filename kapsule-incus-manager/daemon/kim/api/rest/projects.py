from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["projects"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/projects")
async def list_projects(req: Request, remote: str = "") -> Any:
    return await _incus(req).list_projects()

@router.post("/projects", status_code=202)
async def create_project(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).create_project(body)

@router.get("/projects/{name}")
async def get_project(req: Request, name: str) -> Any:
    return await _incus(req).get(f"/1.0/projects/{name}")

@router.put("/projects/{name}")
async def update_project(req: Request, name: str, body: dict[str, Any]) -> Any:
    await _incus(req).put(f"/1.0/projects/{name}", json=body)
    return {"status": "ok"}

@router.delete("/projects/{name}", status_code=202)
async def delete_project(req: Request, name: str) -> Any:
    return await _incus(req).delete_project(name)
