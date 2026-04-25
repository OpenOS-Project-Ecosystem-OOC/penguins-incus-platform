# penguins-incus-platform

Unified [Incus](https://linuxcontainers.org/incus/) platform covering container
and VM management, image building, image serving, and penguins-eggs integration.

## Repository layout

```
penguins-incus-platform/   Core platform: daemon, CLI, web UI, QML desktop UI
oci-builder/               OCI image builder using Incus containers as build sandbox (Rust)
distrobuilder/             LXC/Incus rootfs image builder + TUI menu (Go + Python)
unified-image-server/      Simplestreams image server + multi-distro build manifests (Elixir)
integration/               penguins-eggs and recovery hook scripts for all components
```

---

## penguins-incus-platform

Incus container and VM management with full feature parity across three
frontends: a Qt6/QML desktop app, a React web UI, and a CLI. A single daemon
is the control plane for all guest types.

| Guest type | CLI entry point |
|---|---|
| Generic Linux containers | `penguins-incus provision generic` |
| Waydroid (Android) containers | `penguins-incus provision waydroid` |
| macOS KVM VMs | `penguins-incus provision macos` |
| Windows VMs | `penguins-incus provision windows` |

```
penguins-incus-platform/
├── api/schema/        OpenAPI schema + D-Bus XML
├── daemon/            FastAPI + dasbus daemon
├── cli/               Click CLI (thin HTTP client)
├── profiles/          Bundled Incus profile presets
├── ui-web/            React/TypeScript web UI (Vite)
├── ui-qml/            Qt6/QML desktop UI + libpenguins-incus-qt
├── bin/               penguins-incus-hub management helper
├── lib/               Shared shell helpers
└── INTEGRATIONS.md    penguins-eggs integration details
```

See [`penguins-incus-platform/README.md`](penguins-incus-platform/README.md)
and [`ARCHITECTURE.md`](ARCHITECTURE.md) for full documentation.

### Quick start

```bash
# Daemon
cd penguins-incus-platform/daemon && pip install -e ".[dev]"
penguins-incus-daemon

# CLI
cd penguins-incus-platform/cli && pip install -e ".[dev]"
penguins-incus container list

# Web UI
cd penguins-incus-platform/ui-web && npm install && npm run dev

# QML app
cmake -B penguins-incus-platform/ui-qml/build \
      -S penguins-incus-platform/ui-qml -G Ninja
cmake --build penguins-incus-platform/ui-qml/build
```

### Prerequisites

| Component | Requirement |
|---|---|
| Incus | ≥ 6.0 |
| Python | ≥ 3.11 |
| Node.js | ≥ 20 (web UI) |
| Qt6 | ≥ 6.5 with DBus, Network, WebSockets, Quick, QuickControls2 |
| CMake | ≥ 3.22 (QML app) |

---

## oci-builder

Builds OCI-compliant container images using Incus ephemeral system containers
as the build environment. Gives builds access to a real init system and full
cgroup isolation — useful for images that require `systemd` or complex package
manager interactions during build.

Upstream: [incus-oci-builder](https://gitlab.com/openos-project/incus_deving/incus-oci-builder)

```
oci-builder/
├── src/       Rust source (CLI, Incus client, OCI layer packer, registry push)
├── tests/     Unit and integration tests
├── docs/
├── Cargo.toml
└── DESIGN.md  Architecture and design decisions
```

```bash
cargo build --manifest-path oci-builder/Cargo.toml
cargo test  --manifest-path oci-builder/Cargo.toml

# Build an OCI image from a definition file
incus-oci-builder build my-image.yaml
```

See [`oci-builder/DESIGN.md`](oci-builder/DESIGN.md) for the build pipeline,
definition file format, and module layout.

---

## distrobuilder

[`lxc/distrobuilder`](https://github.com/lxc/distrobuilder) (Go) and
[`distrobuilder-menu`](https://github.com/itoffshore/distrobuilder-menu)
(Python TUI) in a single tree, with YAML templates for common distributions.

Upstream: [penguins-distrobuilder](https://gitlab.com/openos-project/penguins-eggs_deving/penguins-distrobuilder)

```
distrobuilder/
├── distrobuilder/   Go source — builds LXC/Incus rootfs images from YAML templates
├── menu/            Python TUI frontend (dbmenu)
├── templates/       Distrobuilder YAML templates
└── scripts/         Install helpers
```

```bash
# Build distrobuilder binary
make -C distrobuilder build

# Install binary + dbmenu
make -C distrobuilder install

# Launch the TUI
dbmenu           # Incus/LXD mode
dbmenu --lxc     # LXC mode
```

---

## unified-image-server

Simplestreams image server for LXC/LXD/Incus with a multi-distro build
pipeline and live-ISO remastering support.

```
unified-image-server/
├── server/             Elixir/Phoenix simplestreams server
├── manifests/          Distrobuilder YAMLs for Debian, Ubuntu, Fedora, Arch, Gentoo, …
├── chromiumos-stage3/  ChromiumOS stage3 builder (amd64 + arm64)
└── penguins-eggs/      ChromiumOS family support for penguins-eggs
```

See [`unified-image-server/README.md`](unified-image-server/README.md).

---

## integration

Hook scripts that connect all components to
[penguins-eggs](https://gitlab.com/openos-project/penguins-eggs_deving/penguins-eggs)
and [penguins-recovery](https://gitlab.com/openos-project/penguins-eggs_deving/penguins-recovery).

```
integration/
├── eggs-plugin/
│   ├── pip-hook.sh               Embeds PIP daemon + CLI into produced ISOs
│   └── distrobuilder-hook.sh     Builds a distrobuilder image alongside the ISO
├── recovery-plugin/
│   ├── pip-recovery-plugin.sh    Snapshots Incus instances before reset
│   └── distrobuilder-recovery-hook.sh  Snapshots rootfs via distrobuilder before reset
└── conf/
    └── pip-eggs-hooks.conf.default
```

See [`integration/README.md`](integration/README.md).

---

## Licenses

| Component | License |
|---|---|
| penguins-incus-platform daemon, CLI, web UI | GPL-3.0-or-later |
| libpenguins-incus-qt | LGPL-2.1-or-later |
| oci-builder | Apache-2.0 |
| distrobuilder (Go) | Apache-2.0 |
| distrobuilder-menu (Python) | GPL-3.0 |
| profiles | MIT |
