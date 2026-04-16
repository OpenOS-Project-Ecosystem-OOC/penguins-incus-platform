# Kapsule Incus Manager

Unified [Incus](https://linuxcontainers.org/incus/) container and VM management
with full feature parity across three frontends: a Qt6/QML desktop app, a React
web UI, and a CLI.

PIP is the central control plane for all Incus guest types — generic Linux
containers, Waydroid (Android) containers, macOS KVM VMs, and Windows VMs.
Four previously independent toolkits have been merged into the daemon as
provisioning plugins:

| Source project | Guest type | CLI entry point |
|---|---|---|
| [incusbox](https://github.com/OpenOS-Project-Ecosystem-OOC/incusbox) | Generic Linux containers | `penguins-incus provision generic` |
| [waydroid-toolkit](https://github.com/OpenOS-Project-Ecosystem-OOC/waydroid-toolkit) | Waydroid (Android) containers | `penguins-incus provision waydroid` |
| [Incus-MacOS-Toolkit](https://github.com/OpenOS-Project-Ecosystem-OOC/Incus-MacOS-Toolkit) | macOS KVM VMs | `penguins-incus provision macos` |
| [incus-windows-toolkit](https://github.com/OpenOS-Project-Ecosystem-OOC/incus-windows-toolkit) | Windows VMs | `penguins-incus provision windows` |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Frontends                           │
│  Qt6/QML desktop app  │  React web UI  │  penguins-incus CLI       │
└──────────┬────────────┴───────┬────────┴────────┬───────┘
           │ D-Bus              │ HTTP/WS/SSE      │ HTTP
           └──────────┬─────────┘                 │
                      ▼                            │
           ┌──────────────────────┐                │
           │    penguins-incus-daemon        │◄───────────────┘
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
├── penguins-incus-platform/
│   ├── api/schema/                    # OpenAPI schema (143 operations) + D-Bus XML
│   ├── daemon/
│   │   └── penguins_incus/
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
│   └── ui-qml/                        # Qt6/QML desktop UI + libpenguins-incus-qt
```

Full documentation is in [`penguins-incus-platform/README.md`](penguins-incus-platform/README.md).

## Quick start

### Daemon

```bash
cd penguins-incus-platform/daemon
pip install -e ".[dev]"
penguins-incus-daemon
```

### CLI

```bash
cd penguins-incus-platform/cli
pip install -e ".[dev]"

# Generic containers (incusbox)
penguins-incus provision generic create mybox --image images:ubuntu/24.04/cloud

# Waydroid (Android) container
penguins-incus provision waydroid create my-android --image-type GAPPS

# macOS VM
penguins-incus provision macos image firmware
penguins-incus provision macos image fetch --version sonoma
penguins-incus provision macos create my-mac --version sonoma

# Windows VM
penguins-incus provision windows create my-win --image /path/to/win11.iso

# Standard instance management
penguins-incus container list
penguins-incus vm list
```

### Web UI

```bash
cd penguins-incus-platform/ui-web
npm install && npm run dev
# Open http://localhost:5173
```

### QML desktop app

```bash
cmake -B build -S penguins-incus-platform/ui-qml -G Ninja
cmake --build build
./build/penguins-incus-app
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
- `libpenguins-incus-qt`: LGPL-2.1-or-later
