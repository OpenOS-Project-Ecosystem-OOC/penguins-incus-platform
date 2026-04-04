"""FastAPI application factory.

Builds the HTTP REST + WebSocket + SSE server.
All routes delegate to the same handler functions used by the D-Bus service,
ensuring both transports share identical business logic.
"""

from __future__ import annotations

import pathlib

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from ...events import EventBus
from ...incus.client import IncusClient
from . import (
    cluster,
    events,
    images,
    instances,
    networks,
    operations,
    profiles,
    projects,
    provisioning,
    provisioning_generic,
    provisioning_macos,
    provisioning_waydroid,
    provisioning_windows,
    remotes,
    storage,
)

_WEB_DIST = pathlib.Path(__file__).parents[5] / "ui-web" / "dist"


def build_app(incus: IncusClient, bus: EventBus) -> FastAPI:
    app = FastAPI(
        title="Kapsule-Incus-Manager API",
        version="0.1.0",
        docs_url="/api/docs",
        redoc_url="/api/redoc",
        openapi_url="/api/openapi.json",
    )

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["http://localhost:5173"],  # Vite dev server
        allow_methods=["*"],
        allow_headers=["*"],
    )

    # Attach shared state so routers can access it via request.app.state
    app.state.incus = incus
    app.state.bus = bus

    # Register all routers
    app.include_router(instances.router,              prefix="/api/v1")
    app.include_router(networks.router,               prefix="/api/v1")
    app.include_router(storage.router,                prefix="/api/v1")
    app.include_router(images.router,                 prefix="/api/v1")
    app.include_router(profiles.router,               prefix="/api/v1")
    app.include_router(projects.router,               prefix="/api/v1")
    app.include_router(cluster.router,                prefix="/api/v1")
    app.include_router(remotes.router,                prefix="/api/v1")
    app.include_router(operations.router,             prefix="/api/v1")
    app.include_router(events.router,                 prefix="/api/v1")
    app.include_router(provisioning.router,           prefix="/api/v1")
    app.include_router(provisioning_generic.router,   prefix="/api/v1")
    app.include_router(provisioning_waydroid.router,  prefix="/api/v1")
    app.include_router(provisioning_macos.router,     prefix="/api/v1")
    app.include_router(provisioning_windows.router,   prefix="/api/v1")

    # Serve the web UI from the built dist directory (production)
    if _WEB_DIST.exists():
        app.mount("/", StaticFiles(directory=str(_WEB_DIST), html=True), name="web")

    return app
