# Kapsule Incus Manager

Unified [Incus](https://linuxcontainers.org/incus/) container and VM management
with full feature parity across three frontends: a Qt6/QML desktop app, a React
web UI, and a CLI.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Frontends                           │
│  Qt6/QML desktop app  │  React web UI  │  kim CLI       │
└──────────┬────────────┴───────┬────────┴────────┬───────┘
           │ D-Bus              │ HTTP/WS/SSE      │ HTTP
           └──────────┬─────────┘                 │
                      ▼                            │
           ┌──────────────────────┐                │
           │    kim-daemon        │◄───────────────┘
           │  (FastAPI + dasbus)  │
           └──────────┬───────────┘
                      │ Unix socket
                      ▼
           ┌──────────────────────┐
           │       Incus          │
           │  (containers + VMs)  │
           └──────────────────────┘
```

The daemon is the single control plane. All three frontends are thin clients —
they never talk to Incus directly. The REST and D-Bus transports expose
identical operations, so every action available in the GUI is also available
in the CLI.

## Repository layout

```
kapsule-incus-manager/
├── api/
│   └── schema/
│       ├── openapi.yaml                       # REST API schema (canonical)
│       └── dbus/org.KapsuleIncusManager.xml   # D-Bus interface
├── daemon/                     # Python daemon (FastAPI + dasbus)
│   └── kim/
│       ├── main.py             # Entry point, TaskGroup
│       ├── events.py           # EventBus fan-out
│       ├── resources.py        # CPU/memory/disk polling (diff-based %)
│       ├── incus/client.py     # Async Incus REST client, multi-remote pool
│       ├── api/rest/           # FastAPI routers (one per resource type)
│       ├── api/dbus/service.py # D-Bus service
│       ├── profiles/library.py # Built-in profile presets
│       └── provisioning/       # Docker Compose → Incus converter
├── cli/                        # Python CLI (Click + httpx + rich)
│   └── kim/cli/
│       ├── main.py             # All command groups
│       └── client.py           # DaemonClient HTTP wrapper
├── ui-web/                     # React/TypeScript web UI (Vite)
│   └── src/
│       ├── api/client.ts       # Typed API client
│       ├── hooks/              # useApi, useEvents (SSE)
│       ├── components/         # StatusBadge, ConfirmDialog, PageHeader
│       └── pages/              # 11 pages (one per resource type)
└── ui-qml/                     # Qt6/QML desktop UI
    ├── lib/src/                # libkim-qt: KimClient, models, EventSource
    └── app/qml/                # QML pages and components
```

## Prerequisites

| Component | Requirement |
|---|---|
| Incus | ≥ 6.0, running locally or on a reachable remote |
| Python | ≥ 3.11 |
| Node.js | ≥ 20 (web UI only) |
| Qt6 | ≥ 6.5 with DBus, Network, WebSockets, Quick, QuickControls2 |
| CMake | ≥ 3.22 (QML app only) |

## Installation

### Daemon

```bash
cd daemon
pip install -e ".[dev]"
```

### CLI

```bash
cd cli
pip install -e ".[dev]"
```

### Web UI

```bash
cd ui-web
npm install
```

### QML app

```bash
cmake -B build -S ui-qml -G Ninja
cmake --build build
```

## Running

### Start the daemon

The daemon needs read/write access to the Incus Unix socket
(`/var/lib/incus/unix.socket`). Add your user to the `incus-admin` group or
run with appropriate permissions.

```bash
kim-daemon
```

The daemon listens on:
- `http://127.0.0.1:8765` — REST API, SSE event stream, WebSocket exec/console
- D-Bus session bus — `org.KapsuleIncusManager` at `/org/KapsuleIncusManager`

### Web UI (development)

```bash
cd ui-web && npm run dev
# Open http://localhost:5173
```

### Web UI (production build)

```bash
cd ui-web && npm run build
# Serve ui-web/dist/ with any static file server
```

### CLI

```bash
# List containers
kim container list

# Create and start a container
kim container create mybox --image images:ubuntu/24.04
kim container start mybox

# Stream live events
kim events --type lifecycle

# All commands
kim --help
```

The CLI connects to `http://127.0.0.1:8765` by default. Override with
`--daemon URL` or the `KIM_DAEMON` environment variable.

### QML desktop app

```bash
./build/kim-app
```

The app connects to the daemon via D-Bus on startup. Ensure the daemon is
running first.

## CLI reference

```
kim container  list / create / start / stop / restart / freeze / unfreeze /
               rename / delete / logs / exec / file-pull / file-push
kim vm         list / create / start / stop / restart / freeze / unfreeze /
               rename / delete / logs / exec / file-pull / file-push
kim snapshot   list / create / restore / delete
kim network    list / create / delete
kim storage    list / create / delete
kim storage volume  list / create / delete
kim image      list / pull / delete
kim profile    list / presets / create / delete
kim project    list / create / delete
kim cluster    list / evacuate / restore / remove
kim remote     list / add / activate / remove
kim operation  list / cancel
kim provision  convert / deploy
kim events
```

## API

The full REST API is documented in
[`api/schema/openapi.yaml`](api/schema/openapi.yaml).
The D-Bus interface is in
[`api/schema/dbus/org.KapsuleIncusManager.xml`](api/schema/dbus/org.KapsuleIncusManager.xml).

Key endpoints:

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/instances` | List containers and VMs |
| `POST` | `/api/v1/instances` | Create an instance |
| `PUT` | `/api/v1/instances/{name}/state` | Start / stop / restart / freeze |
| `WS` | `/api/v1/instances/{name}/exec/ws` | Interactive exec (PTY) |
| `WS` | `/api/v1/instances/{name}/console/ws` | Serial or VGA console |
| `GET` | `/api/v1/events` | SSE event stream |
| `POST` | `/api/v1/provisioning/compose` | Deploy from Docker Compose YAML |

## Multi-remote support

The daemon manages a pool of named Incus remotes. The built-in `local` remote
uses the Unix socket. Additional remotes connect over HTTPS.

```bash
kim remote add prod https://prod.example.com
kim remote activate prod
kim container list   # lists containers on prod
kim remote activate local
```

The active remote is also switchable from the QML and web UIs via the Remotes
page.

## Development

### Run tests

```bash
# Daemon
cd daemon && pytest

# CLI
cd cli && pytest

# Web UI
cd ui-web && npm test
```

### Lint and type-check

```bash
# Python (daemon + CLI)
ruff check .
mypy .

# TypeScript
cd ui-web && npm run typecheck && npm run lint
```

### CI

GitHub Actions runs on every push to `main` and `feat/**` branches:
- Python: ruff, mypy, pytest (daemon + CLI)
- TypeScript: tsc, eslint, vitest, vite build
- C++/QML: cmake configure + ninja build

## License

- Daemon, CLI, web UI: GPL-3.0-or-later
- `libkim-qt` (C++ D-Bus client library): LGPL-2.1-or-later
