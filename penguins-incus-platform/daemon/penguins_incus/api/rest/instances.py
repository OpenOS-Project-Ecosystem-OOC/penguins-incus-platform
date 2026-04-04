"""REST routes for instances (containers + VMs)."""

from __future__ import annotations

import asyncio
from typing import Any

from fastapi import APIRouter, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import JSONResponse, PlainTextResponse, Response

router = APIRouter(tags=["instances"])


def _incus(req: Request) -> Any:
    return req.app.state.incus


@router.get("/instances")
async def list_instances(
    req: Request,
    project: str = "",
    remote: str = "",
    type: str = "",
) -> Any:
    return await _incus(req).list_instances(project=project, remote=remote,
                                             type_filter=type)


@router.post("/instances", status_code=202)
async def create_instance(req: Request, body: dict[str, Any]) -> Any:
    return await _incus(req).create_instance(body)


@router.get("/instances/{name}")
async def get_instance(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).get_instance(name, project=project)


@router.delete("/instances/{name}", status_code=202)
async def delete_instance(
    req: Request, name: str, project: str = "", force: bool = False
) -> Any:
    return await _incus(req).delete_instance(name, project=project, force=force)


@router.put("/instances/{name}/state", status_code=202)
async def change_instance_state(
    req: Request, name: str, body: dict[str, Any]
) -> Any:
    return await _incus(req).change_instance_state(
        name,
        action=body["action"],
        force=body.get("force", False),
        timeout=body.get("timeout", 30),
        project=body.get("project", ""),
    )


@router.post("/instances/{name}/rename", status_code=202)
async def rename_instance(
    req: Request, name: str, body: dict[str, Any]
) -> Any:
    return await _incus(req).rename_instance(
        name, body["new_name"], project=body.get("project", "")
    )


@router.get("/instances/{name}/snapshots")
async def list_snapshots(req: Request, name: str, project: str = "") -> Any:
    return await _incus(req).list_snapshots(name, project=project)


@router.post("/instances/{name}/snapshots", status_code=202)
async def create_snapshot(
    req: Request, name: str, body: dict[str, Any]
) -> Any:
    return await _incus(req).create_snapshot(
        name, body["name"],
        stateful=body.get("stateful", False),
        project=body.get("project", ""),
    )


@router.post("/instances/{name}/snapshots/{snapshot}", status_code=202)
async def restore_snapshot(
    req: Request, name: str, snapshot: str, project: str = ""
) -> Any:
    incus = _incus(req)
    return await incus.post(
        f"/1.0/instances/{name}/snapshots/{snapshot}",
        json={"restore": snapshot},
        params={"project": project} if project else {},
    )


@router.delete("/instances/{name}/snapshots/{snapshot}", status_code=202)
async def delete_snapshot(
    req: Request, name: str, snapshot: str, project: str = ""
) -> Any:
    return await _incus(req).delete_snapshot(name, snapshot, project=project)


@router.get("/instances/{name}/logs", response_class=PlainTextResponse)
async def get_instance_logs(req: Request, name: str, project: str = "") -> str:
    return str(await _incus(req).get_instance_logs(name, project=project))


@router.get("/instances/{name}/files")
async def pull_file(req: Request, name: str, path: str, project: str = "") -> Response:
    incus = _incus(req)
    resp = await incus._http.get(
        f"/1.0/instances/{name}/files",
        params={"path": path, **({"project": project} if project else {})},
    )
    return Response(content=resp.content, media_type="application/octet-stream")


@router.post("/instances/{name}/files", status_code=200)
async def push_file(
    req: Request, name: str, path: str, project: str = ""
) -> JSONResponse:
    body = await req.body()
    incus = _incus(req)
    await incus._http.post(
        f"/1.0/instances/{name}/files",
        content=body,
        params={"path": path, **({"project": project} if project else {})},
        headers={"Content-Type": "application/octet-stream"},
    )
    return JSONResponse({"status": "ok"})


@router.websocket("/instances/{name}/console/ws")
async def console_ws(
    ws: WebSocket,
    name: str,
    project: str = "",
    type: str = "console",
    width: int = 80,
    height: int = 24,
) -> None:
    """Proxy a console session (serial or VGA) between the client and Incus.

    type="console"  — serial console, works for containers and VMs.
    type="vga"      — VGA framebuffer, VMs only (requires SPICE/VNC client).

    The client receives raw PTY bytes over the WebSocket binary frames.
    """
    import websockets

    await ws.accept()
    incus = ws.app.state.incus

    # Start a console operation on Incus
    resp = await incus.post(
        f"/1.0/instances/{name}/console",
        json={
            "type":   type,
            "width":  width,
            "height": height,
        },
        params={"project": project} if project else {},
    )
    op_id  = resp.get("id", "")
    fds    = resp.get("metadata", {}).get("fds", {})
    secret = fds.get("0", "")

    incus_ws_url = (
        f"ws+unix:///var/lib/incus/unix.socket:"
        f"/1.0/operations/{op_id}/websocket?secret={secret}"
    )

    try:
        async with websockets.connect(incus_ws_url) as incus_ws:
            async def _client_to_incus() -> None:
                try:
                    while True:
                        data = await ws.receive_bytes()
                        await incus_ws.send(data)
                except (WebSocketDisconnect, Exception):
                    pass

            async def _incus_to_client() -> None:
                try:
                    async for msg in incus_ws:
                        if isinstance(msg, bytes):
                            await ws.send_bytes(msg)
                        else:
                            await ws.send_text(msg)
                except Exception:
                    pass

            await asyncio.gather(_client_to_incus(), _incus_to_client())
    except Exception:
        pass
    finally:
        await ws.close()


@router.websocket("/instances/{name}/exec/ws")
async def exec_ws(
    ws: WebSocket,
    name: str,
    project: str = "",
    command: str = "/bin/bash",
    width: int = 80,
    height: int = 24,
) -> None:
    """Proxy an interactive PTY session between the client and Incus."""
    import websockets

    await ws.accept()
    incus = ws.app.state.incus

    # Ask Incus to start an exec operation and get the websocket URLs
    resp = await incus.post(
        f"/1.0/instances/{name}/exec",
        json={
            "command": [command],
            "environment": {},
            "wait-for-websocket": True,
            "interactive": True,
            "width": width,
            "height": height,
        },
        params={"project": project} if project else {},
    )
    op_id = resp.get("id", "")
    fds = resp.get("metadata", {}).get("fds", {})
    secret = fds.get("0", "")
    # ctrl_secret reserved for future control-channel use
    _ctrl_secret = fds.get("control", "")

    # Build Incus websocket URL (UDS-based — use http+unix scheme via httpx)
    incus_ws_url = f"ws+unix:///var/lib/incus/unix.socket:/1.0/operations/{op_id}/websocket?secret={secret}"

    try:
        async with websockets.connect(incus_ws_url) as incus_ws:
            async def _client_to_incus() -> None:
                try:
                    while True:
                        data = await ws.receive_bytes()
                        await incus_ws.send(data)
                except (WebSocketDisconnect, Exception):
                    pass

            async def _incus_to_client() -> None:
                try:
                    async for msg in incus_ws:
                        if isinstance(msg, bytes):
                            await ws.send_bytes(msg)
                        else:
                            await ws.send_text(msg)
                except Exception:
                    pass

            await asyncio.gather(_client_to_incus(), _incus_to_client())
    except Exception:
        pass
    finally:
        await ws.close()
