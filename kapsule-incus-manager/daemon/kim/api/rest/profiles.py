from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

from ...profiles.library import list_presets

router = APIRouter(tags=["profiles"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/profiles")
async def list_profiles(req: Request, project: str = "", remote: str = "") -> Any:
    return await _incus(req).list_profiles(project=project)

@router.post("/profiles", status_code=202)
async def create_profile(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).create_profile(body)

@router.get("/profiles/presets")
async def get_presets() -> Any:
    return list_presets()

@router.get("/profiles/{name}")
async def get_profile(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).get_profile(name, project=project)

@router.put("/profiles/{name}")
async def update_profile(req: Request, name: str, body: dict[str, Any], project: str = "") -> Any:
    await _incus(req).update_profile(name, body, project=project)
    return {"status": "ok"}

@router.delete("/profiles/{name}", status_code=202)
async def delete_profile(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).delete_profile(name, project=project)
