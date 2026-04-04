from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["operations"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/operations")
async def list_operations(req: Request, status: str = "") -> Any:
    ops = await _incus(req).list_operations()
    if status:
        ops = [o for o in ops if o.get("status", "").lower() == status.lower()]
    return ops

@router.get("/operations/{id}")
async def get_operation(req: Request, id: str) -> Any:
    return await _incus(req).get(f"/1.0/operations/{id}")

@router.delete("/operations/{id}", status_code=202)
async def cancel_operation(req: Request, id: str) -> Any:
    await _incus(req).cancel_operation(id)
    return {"status": "cancelled"}
