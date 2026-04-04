# Kapsule Incus Manager

Unified [Incus](https://linuxcontainers.org/incus/) container and VM management
with full feature parity across three frontends: a Qt6/QML desktop app, a React
web UI, and a CLI.

KIM is the central control plane for all Incus guest types — generic Linux
containers, Waydroid (Android) containers, macOS KVM VMs, and Windows VMs.
Four previously independent toolkits have been merged into the daemon as
provisioning plugins:

| Source project | Guest type | CLI entry point |
|---|---|---|
| [incusbox](https://github.com/Interested-Deving-1896/incusbox) | Generic Linux containers | `kim provision generic` |
| [waydroid-toolkit](https://github.com/Interested-Deving-1896/waydroid-toolkit) | Waydroid (Android) containers | `kim provision waydroid` |
| [Incus-MacOS-Toolkit](https://github.com/Interested-Deving-1896/Incus-MacOS-Toolkit) | macOS KVM VMs | `kim provision macos` |
| [incus-windows-toolkit](https://github.com/Interested-Deving-1896/incus-windows-toolkit) | Windows VMs | `kim provision windows` |

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
           │                      │
           │  provisioning/       │
           │    generic.py        │  ← incusbox
           │    waydroid.py       │  ← waydroid-toolkit
           │    macos.py          │  ← Incus-MacOS-Toolkit
           │    windows.py        │  ← incus-windows-toolkit
           └──────────┬───────────┘
                      │ Unix socket
                      ▼
           ┌──────────────────────┐
           │       Incus          │
           │  (containers + VMs)  │
           └──────────────────────┘
```

The daemon is the single control plane. No frontend or plugin calls the `incus`
CLI directly — all operations go through the Incus REST API. Every action
available in the GUI is also available in the CLI and REST API.

## Repository layout

```
├── ARCHITECTURE.md                    # Design decisions and component boundaries
├── kapsule-incus-manager/
│   ├── api/schema/                    # OpenAPI schema (143 operations) + D-Bus XML
│   ├── daemon/
│   │   └── kim/
│   │       ├── provisioning/          # Guest-type provisioning plugins
│   │       │   ├── generic.py         # incusbox feature set
│   │       │   ├── waydroid.py        # waydroid-toolkit feature set
│   │       │   ├── macos.py           # Incus-MacOS-Toolkit feature set
│   │       │   └── windows.py         # incus-windows-toolkit feature set
│   │       └── incus/client.py        # Async Incus REST client
│   ├── cli/                           # Python CLI (Click + httpx + rich)
│   ├── profiles/                      # Bundled Incus profile presets (16 profiles)
│   │   ├── generic/                   # incusbox profiles
│   │   ├── macos/                     # macOS KVM profile
│   │   ├── windows/                   # Windows VM profiles + GPU overlays
│   │   └── waydroid/                  # Waydroid container profile
│   ├── ui-web/                        # React/TypeScript web UI (Vite)
│   └── ui-qml/                        # Qt6/QML desktop UI + libkim-qt
```

Full documentation is in [`kapsule-incus-manager/README.md`](kapsule-incus-manager/README.md).

## Quick start

### Daemon

```bash
cd kapsule-incus-manager/daemon
pip install -e ".[dev]"
kim-daemon
```

### CLI

```bash
cd kapsule-incus-manager/cli
pip install -e ".[dev]"

# Generic containers (incusbox)
kim provision generic create mybox --image images:ubuntu/24.04/cloud

# Waydroid (Android) container
kim provision waydroid create my-android --image-type GAPPS

# macOS VM
kim provision macos image firmware
kim provision macos image fetch --version sonoma
kim provision macos create my-mac --version sonoma

# Windows VM
kim provision windows create my-win --image /path/to/win11.iso

# Standard instance management
kim container list
kim vm list
```

### Web UI

```bash
cd kapsule-incus-manager/ui-web
npm install && npm run dev
# Open http://localhost:5173
```

### QML desktop app

```bash
cmake -B build -S kapsule-incus-manager/ui-qml -G Ninja
cmake --build build
./build/kim-app
```

## Prerequisites

| Component | Requirement |
|---|---|
| Incus | ≥ 6.0 |
| Python | ≥ 3.11 |
| Node.js | ≥ 20 (web UI) |
| Qt6 | ≥ 6.5 with DBus, Network, WebSockets, Quick, QuickControls2 |
| CMake | ≥ 3.22 (QML app) |

## License

- Daemon, CLI, web UI: GPL-3.0-or-later
- `libkim-qt`: LGPL-2.1-or-later
