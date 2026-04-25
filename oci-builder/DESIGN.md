# incus-oci-builder — Design

## Goal

Build OCI-compliant container images using Incus system containers as the
build environment, rather than the chroot/overlay approach used by Dockerfile
builders. This gives builds access to a real init system, full cgroup
isolation, and proper kernel namespace separation — useful for images that
require `systemd`, complex package manager interactions, or kernel module
setup during build.

## Source projects

| Project | Role in this tool |
|---|---|
| [Incus](https://github.com/lxc/incus) | Build sandbox runtime; REST API client |
| [distrobuilder](https://github.com/lxc/distrobuilder) | Definition file schema and build lifecycle model |
| [Buildah](https://github.com/containers/buildah) | OCI layer/manifest model (implemented natively in Rust here) |
| [Blincus](https://github.com/ublue-os/blincus) | UX inspiration for template-driven container management |

## Architecture

```
Definition YAML
      │
      ▼
┌─────────────┐   REST API    ┌──────────────────────┐
│   builder   │ ────────────► │  Incus daemon        │
│ (pipeline)  │               │  (ephemeral container)│
└──────┬──────┘               └──────────┬───────────┘
       │                                 │ rootfs export
       │                                 ▼
       │                      ┌──────────────────────┐
       │                      │  rootfs directory    │
       │                      └──────────┬───────────┘
       │                                 │
       ▼                                 ▼
┌─────────────┐               ┌──────────────────────┐
│  OCI commit │ ◄─────────────│  layer packer        │
│  (manifest, │               │  (tar + gzip + sha256)│
│   index)    │               └──────────────────────┘
└──────┬──────┘
       │
       ▼
OCI image layout  ──► registry push (skopeo / native)
```

## Build pipeline stages

1. **Create container** — spawn an ephemeral Incus container from the source
   image specified in the definition (`source.downloader: incus`,
   `source.image: images:ubuntu/noble`).

2. **post-unpack actions** — run user-defined shell scripts inside the
   container immediately after it starts.

3. **Package management** — install/remove packages via the configured
   manager (`apt`, `dnf`, `apk`, `pacman`, `zypper`, `xbps`). Optionally
   run a full upgrade first and clean caches afterwards.

4. **post-packages actions** — run scripts after package changes.

5. **File generators** — write files into the container (`dump`, `remove`,
   `hostname`, `hosts`, `copy`).

6. **post-files actions** — run scripts after file generation.

7. **Export rootfs** — stop the container and stream its filesystem out via
   the Incus backup export API (`GET /1.0/instances/<name>/backups/export`).
   The `rootfs/` prefix is stripped from the archive.

8. **OCI commit** — pack the rootfs directory into a gzip-compressed tar
   layer, compute SHA-256 digests, and write a valid OCI Image Layout:
   ```
   oci-output/
     oci-layout
     index.json
     blobs/sha256/
       <config-digest>
       <layer-digest>
       <manifest-digest>
   ```

9. **Push** (optional) — push the layout to a registry via `skopeo copy`
   (preferred) or the built-in OCI Distribution API client.

10. **Cleanup** — delete the ephemeral container. Runs even on failure.

## Definition file format

The YAML schema is a subset of distrobuilder's, extended with an `oci`
section for OCI-specific output options. See `src/definition/mod.rs` for the
full type definitions and `incus-oci-builder example` for an annotated
example.

Key sections:

```yaml
image:          # distribution, release, architecture, name, tag
source:         # downloader (incus | debootstrap | rpmbootstrap | rootfs-http), image/url
packages:       # manager, update, cleanup, sets[], repositories[]
actions:        # trigger (post-unpack | post-packages | post-files), action (shell script)
files:          # generator (dump | copy | remove | hostname | hosts), path, content
oci:            # registry, cmd, entrypoint, labels, exposed_ports, layered
```

## Module layout

```
src/
  main.rs              CLI entry point
  cli/mod.rs           clap argument definitions
  definition/mod.rs    Definition YAML types + validation
  incus/
    mod.rs             Re-exports
    api.rs             Incus REST API request/response types
    client.rs          HTTP client over Unix socket
    exec.rs            Package manager and script helpers
    export.rs          Rootfs extraction
  oci/
    mod.rs             Re-exports
    layer.rs           Rootfs → gzip tar layer blob
    commit.rs          OCI image layout assembly
    push.rs            Registry push (skopeo + native fallback)
  builder/mod.rs       Pipeline orchestrator
```

## Key design decisions

**Incus REST API over Unix socket** — all communication with Incus goes
through its Unix socket (`/var/lib/incus/unix.socket`) using the standard
REST API. No Incus CLI binary is required at runtime.

**Ephemeral containers** — build containers are created with `ephemeral:
true` so Incus deletes them automatically on stop, even if the builder
crashes before the explicit cleanup step.

**Single-layer OCI images** — the initial implementation produces a single
uncompressed rootfs layer. The `oci.layered` flag in the definition is
reserved for a future mode that uses Incus snapshots between build stages to
produce diff layers, enabling layer cache reuse.

**No daemon** — the builder is a single stateless binary. There is no
persistent build daemon; each `incus-oci-builder build` invocation is
independent.

**skopeo for push** — registry push delegates to `skopeo copy` when
available, which handles authentication, retries, and cross-repo blob mounts
correctly. The native push implementation covers environments without skopeo.

## Known limitations / future work

- Only the `incus` downloader is implemented. `debootstrap` and
  `rpmbootstrap` require running those tools on the host before launching the
  container.
- The `copy` file generator (copying host-side files into the container) is
  not yet implemented; it requires the Incus file push API.
- Layered builds (snapshot-based diff layers) are not yet implemented.
- Authentication for the Incus image server is not handled; public images
  only.
- Windows image support (`repack-windows` equivalent) is out of scope.
