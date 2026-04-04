from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

router = APIRouter(tags=["images"])
def _incus(req: Request): return req.app.state.incus  # type: ignore[return]

@router.get("/images")
async def list_images(req: Request, remote: str = "") -> Any:
    return await _incus(req).list_images()

@router.post("/images", status_code=202)
async def pull_image(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).pull_image(
        body["remote"], body["image"], alias=body.get("alias", "")
    )

@router.get("/images/{fingerprint}")
async def get_image(req: Request, fingerprint: str) -> Any:
    return await _incus(req).get(f"/1.0/images/{fingerprint}")

@router.delete("/images/{fingerprint}", status_code=202)
async def delete_image(req: Request, fingerprint: str) -> Any:
    return await _incus(req).delete_image(fingerprint)
