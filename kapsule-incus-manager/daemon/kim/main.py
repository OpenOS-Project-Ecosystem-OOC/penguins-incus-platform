"""KIM daemon entry point.

Starts the Incus event subscriber, D-Bus service, and HTTP/WebSocket server
concurrently under a single asyncio event loop.
"""

from __future__ import annotations

import asyncio
import logging
import signal
import sys

import uvicorn

from .api.dbus.service import DBusService
from .api.rest.app import build_app
from .events import EventBus
from .incus.client import IncusClient
from .resources import poll_resource_usage

logger = logging.getLogger(__name__)


async def _run(host: str, port: int) -> None:
    incus = IncusClient()
    bus = EventBus()

    # Wire Incus event stream → EventBus fan-out
    async def _forward_incus_events() -> None:
        async for event in incus.stream_events():
            await bus.publish(event)

    app = build_app(incus=incus, bus=bus)
    dbus_svc = DBusService(incus=incus, bus=bus)

    config = uvicorn.Config(
        app,
        host=host,
        port=port,
        log_level="info",
        ws="websockets",
    )
    server = uvicorn.Server(config)

    async with asyncio.TaskGroup() as tg:
        tg.create_task(_forward_incus_events(), name="incus-events")
        tg.create_task(poll_resource_usage(incus, bus), name="resource-poll")
        tg.create_task(dbus_svc.run(), name="dbus-service")
        tg.create_task(server.serve(), name="http-server")


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )

    host = "127.0.0.1"
    port = 8765

    loop = asyncio.new_event_loop()

    def _shutdown() -> None:
        logger.info("Shutting down")
        for task in asyncio.all_tasks(loop):
            task.cancel()

    loop.add_signal_handler(signal.SIGTERM, _shutdown)
    loop.add_signal_handler(signal.SIGINT, _shutdown)

    try:
        loop.run_until_complete(_run(host, port))
    except asyncio.CancelledError:
        pass
    finally:
        loop.close()
        sys.exit(0)
