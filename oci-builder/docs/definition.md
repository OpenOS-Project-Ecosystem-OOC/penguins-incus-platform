# Definition file reference

A definition file is a YAML document that describes how to build an OCI image.
Pass it to the build command:

```
incus-oci-builder build myimage.yaml
```

---

## `image`

Metadata written into the OCI image config and manifest annotations.

| Field | Type | Required | Description |
|---|---|---|---|
| `distribution` | string | yes | OS distribution name (e.g. `ubuntu`, `fedora`) |
| `release` | string | no | Distribution release (e.g. `noble`, `40`) |
| `architecture` | string | no | Target CPU architecture. Defaults to host arch. See [architectures](#architectures). |
| `variant` | string | no | Distribution variant (e.g. `cloud`, `minimal`) |
| `description` | string | no | Human-readable description written into OCI annotations |
| `name` | string | no | OCI image name (e.g. `myorg/ubuntu-noble`). Defaults to `distribution/release`. |
| `tag` | string | no | OCI image tag. Defaults to today's date stamp `YYYYMMDD`. |

### Architectures

Common values for `image.architecture`:

| Value | CPU |
|---|---|
| `x86_64` | Intel/AMD 64-bit |
| `aarch64` | ARM 64-bit |
| `armv7l` | ARM 32-bit |
| `i686` | Intel 32-bit |
| `ppc64le` | POWER 64-bit LE |
| `s390x` | IBM Z |
| `riscv64` | RISC-V 64-bit |

---

## `source`

Controls how the base rootfs is obtained before the build container starts.

| Field | Type | Required | Description |
|---|---|---|---|
| `downloader` | enum | yes | One of `incus`, `debootstrap`, `rpmbootstrap`, `rootfs-http` |
| `image` | string | incus | Incus image reference, e.g. `images:ubuntu/noble` |
| `url` | string | rootfs-http | URL of a rootfs tarball (`.tar.gz`, `.tar.xz`, or `.tar`) |
| `checksum` | string | no | Expected digest of the downloaded file: `sha256:<hex>` |
| `suite` | string | debootstrap | Debian/Ubuntu suite name, e.g. `noble`, `bookworm` |
| `components` | list | no | APT components, e.g. `[main, universe]` |
| `seed_packages` | list | no | RPM packages to install during bootstrap. Defaults to `[basesystem, bash, coreutils, dnf]`. |
| `http_auth` | object | no | Authentication for `rootfs-http`. See [HTTP authentication](#http-authentication). |

### Downloader: `incus`

Pulls a base image from an Incus image server and uses it as the starting point.

```yaml
source:
  downloader: incus
  image: "images:ubuntu/noble"
```

### Downloader: `debootstrap`

Bootstraps a Debian/Ubuntu rootfs on the host using `debootstrap`, then imports
it into Incus. Requires `debootstrap` and `mksquashfs` to be installed.

```yaml
source:
  downloader: debootstrap
  suite: noble
  components: [main, universe]
  url: "http://archive.ubuntu.com/ubuntu"  # optional mirror
```

### Downloader: `rpmbootstrap`

Bootstraps an RPM-based rootfs using `dnf`/`dnf5`, then imports it into Incus.
Requires `dnf` (or `dnf5`) and `mksquashfs` to be installed.

```yaml
source:
  downloader: rpmbootstrap
  # image.release is used as --releasever (e.g. "40" for Fedora 40)
  seed_packages: [basesystem, bash, coreutils, dnf]
```

### Downloader: `rootfs-http`

Downloads a rootfs tarball over HTTP/HTTPS, verifies its checksum (optional),
and imports it into Incus.

```yaml
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
  checksum: "sha256:abc123..."
```

### HTTP authentication

The `http_auth` field supports three authentication types.

**Bearer token:**

```yaml
source:
  http_auth:
    type: bearer
    token: "$MY_TOKEN"   # $VAR and ${VAR} are expanded from the environment
```

**Basic authentication:**

```yaml
source:
  http_auth:
    type: basic
    username: myuser
    password: "$MY_PASSWORD"
```

**Custom header:**

```yaml
source:
  http_auth:
    type: header
    name: "X-Auth-Token"
    value: "$MY_TOKEN"
```

Credential values support `$VAR` and `${VAR}` environment variable expansion.
Secrets are resolved at download time and never stored in the definition.

---

## `packages`

Package installation and removal inside the build container.

| Field | Type | Default | Description |
|---|---|---|---|
| `manager` | enum | — | Package manager: `apt`, `dnf`, `apk`, `pacman`, `zypper`, `xbps` |
| `update` | bool | `false` | Run a full package upgrade before installing |
| `cleanup` | bool | `false` | Remove package caches after installation |
| `sets` | list | — | List of package sets to apply |

Each entry in `sets`:

| Field | Type | Description |
|---|---|---|
| `action` | enum | `install` or `remove` |
| `packages` | list | Package names |
| `releases` | list | Only apply on these `image.release` values |
| `architectures` | list | Only apply on these `image.architecture` values |

```yaml
packages:
  manager: apt
  update: true
  cleanup: true
  sets:
    - action: install
      packages: [ca-certificates, curl]
    - action: remove
      packages: [snapd]
      releases: [noble, jammy]
```

---

## `actions`

Shell scripts run at specific points in the pipeline.

| Field | Type | Description |
|---|---|---|
| `trigger` | enum | When to run: `post-unpack`, `post-packages`, `post-files` |
| `action` | string | Shell script body (run with `/bin/sh -c`) |
| `releases` | list | Only run on these `image.release` values |
| `architectures` | list | Only run on these `image.architecture` values |

```yaml
actions:
  - trigger: post-unpack
    action: |
      echo "container is ready"
  - trigger: post-packages
    action: |
      find /usr/share/doc -type f -delete
```

---

## `files`

File generators write files into the container before the rootfs is exported.

| Field | Type | Description |
|---|---|---|
| `generator` | enum | `hostname`, `hosts`, `dump`, `copy` |
| `path` | string | Destination path inside the container |
| `content` | string | File content (for `dump`) |
| `source` | string | Host path to copy from (for `copy`) |
| `uid` | int | File owner UID (default: 0) |
| `gid` | int | File owner GID (default: 0) |
| `mode` | int | File permissions in octal (default: 0644) |

### Generator: `hostname`

Writes `/etc/hostname` with the value of `content`.

```yaml
files:
  - generator: hostname
    content: "my-container"
```

### Generator: `hosts`

Writes a minimal `/etc/hosts` file.

```yaml
files:
  - generator: hosts
```

### Generator: `dump`

Writes arbitrary content to `path`.

```yaml
files:
  - generator: dump
    path: /etc/motd
    content: "Built with incus-oci-builder\n"
    mode: 0644
```

### Generator: `copy`

Copies a file from the host into the container.

```yaml
files:
  - generator: copy
    source: ./configs/my-app.conf
    path: /etc/my-app/my-app.conf
    mode: 0600
```

---

## `oci`

OCI image output configuration.

| Field | Type | Default | Description |
|---|---|---|---|
| `registry` | string | — | Registry to push to after build (e.g. `registry.example.com`) |
| `cmd` | list | — | Default command (`CMD`) for the image |
| `entrypoint` | list | — | Entrypoint for the image |
| `env` | list | — | Environment variables (`KEY=VALUE`) |
| `labels` | map | — | OCI image labels (e.g. `org.opencontainers.image.vendor`) |
| `layered` | bool | `false` | Enable snapshot-based layered builds |

```yaml
oci:
  registry: registry.example.com
  cmd: ["/bin/bash"]
  entrypoint: []
  env:
    - "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  labels:
    org.opencontainers.image.vendor: "My Org"
    org.opencontainers.image.source: "https://github.com/myorg/myimage"
  layered: false
```

### Layered builds

When `oci.layered: true`, the builder takes Incus snapshots at the end of each
pipeline stage (post-unpack, post-packages, post-files). Each snapshot becomes
a separate OCI layer. This produces smaller incremental layers and enables
layer cache reuse when only later stages change.

---

## Complete example

```yaml
image:
  distribution: ubuntu
  release: noble
  architecture: x86_64
  description: "Ubuntu 24.04 LTS minimal"
  name: myorg/ubuntu-noble
  tag: "24.04"

source:
  downloader: incus
  image: "images:ubuntu/noble"

packages:
  manager: apt
  update: true
  cleanup: true
  sets:
    - action: install
      packages: [ca-certificates, curl, wget]
    - action: remove
      packages: [snapd]

actions:
  - trigger: post-packages
    action: |
      find /usr/share/doc -type f -delete
      find /usr/share/man -type f -delete

files:
  - generator: hostname
    content: "ubuntu-noble"
  - generator: hosts
  - generator: dump
    path: /etc/motd
    content: "Built with incus-oci-builder\n"

oci:
  registry: registry.example.com
  cmd: ["/bin/bash"]
  labels:
    org.opencontainers.image.vendor: "My Org"
```
