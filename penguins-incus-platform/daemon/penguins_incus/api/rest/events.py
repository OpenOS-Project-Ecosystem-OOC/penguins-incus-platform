"""SSE and WebSocket event stream endpoints."""

from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncGenerator
from typing import Any

from fastapi import APIRouter, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import StreamingResponse

router = APIRouter(tags=["events"])


@router.get("/events")
async def stream_events_sse(
    req: Request,
    type: str = "",
    project: str = "",
) -> StreamingResponse:
    """Server-Sent Events stream. Each event is a JSON-encoded PIP Event."""
    bus = req.app.state.bus

    async def _generator() -> AsyncGenerator[str, None]:
        async for event in bus.iter_events(type_filter=type, project_filter=project):
            yield f"data: {json.dumps(event)}\n\n"

    return StreamingResponse(
        _generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


@router.websocket("/events/ws")
async def stream_events_ws(
    ws: WebSocket,
    type: str = "",
    project: str = "",
) -> None:
    """WebSocket event stream. Each message is a JSON-encoded PIP Event."""
    await ws.accept()
    bus = ws.app.state.bus
    q = bus.subscribe()
    try:
        while True:
            event: dict[str, Any] = await q.get()
            if type and event.get("type") != type:
                continue
            if project and event.get("project") != project:
                continue
            await ws.send_text(json.dumps(event))
    except (WebSocketDisconnect, asyncio.CancelledError):
        pass
    finally:
        bus.unsubscribe(q)
        await ws.close()
