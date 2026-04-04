from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Request

from ...provisioning.compose import convert_compose, deploy_compose

router = APIRouter(tags=["provisioning"])

@router.post("/provisioning/compose", status_code=202)
async def deploy(req: Request, body: dict[str, Any]) -> Any:
    return await deploy_compose(req.app.state.incus, body)

@router.post("/provisioning/compose/convert")
async def convert(body: dict[str, Any]) -> Any:
    return convert_compose(body["compose"])
