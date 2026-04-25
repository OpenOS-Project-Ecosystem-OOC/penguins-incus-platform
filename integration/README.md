# integration

Hook scripts that connect penguins-incus-platform components to
[penguins-eggs](https://gitlab.com/openos-project/penguins-eggs_deving/penguins-eggs)
(ISO producer) and
[penguins-recovery](https://gitlab.com/openos-project/penguins-eggs_deving/penguins-recovery)
(factory reset tool).

## Layout

```
integration/
├── eggs-plugin/
│   ├── pip-hook.sh                   PIP daemon + CLI embed into ISOs
│   └── distrobuilder-hook.sh         distrobuilder image build after ISO creation
├── recovery-plugin/
│   ├── pip-recovery-plugin.sh        Incus instance snapshot before/after reset
│   └── distrobuilder-recovery-hook.sh  rootfs snapshot via distrobuilder before reset
└── conf/
    └── pip-eggs-hooks.conf.default   default config for pip-hook.sh
```

## Hook points

### eggs-plugin — triggered by `eggs produce`

| Script | Action |
|---|---|
| `pip-hook.sh` | Copies `penguins-incus-daemon`, the CLI, bundled profiles, and a systemd unit into the ISO so the daemon auto-starts in the live environment |
| `distrobuilder-hook.sh` | After ISO assembly, optionally builds a distrobuilder LXC/Incus image of the produced system for container distribution alongside the ISO |

### recovery-plugin — triggered by `penguins-powerwash`

| Script | Trigger | Action |
|---|---|---|
| `pip-recovery-plugin.sh` | pre-reset (any mode) | Snapshots all running Incus instances via `penguins-incus snapshot create` |
| `pip-recovery-plugin.sh` | post-hard-reset | Restarts the PIP daemon; re-applies default Incus profiles |
| `distrobuilder-recovery-hook.sh` | pre-reset | Snapshots the current rootfs via `distrobuilder pack-incus` (or `pack-lxc`) |

## Configuration

### pip-hook.sh

Install the default config and edit as needed:

```bash
sudo mkdir -p /etc/penguins-incus-hub
sudo cp integration/conf/pip-eggs-hooks.conf.default \
        /etc/penguins-incus-hub/eggs-hooks.conf
```

Key options:

```bash
PIP_ROOT="/usr/lib/penguins-incus-platform"  # PIP installation root
EMBED_DAEMON=1       # embed penguins-incus-daemon into ISOs
EMBED_CLI=1          # embed penguins-incus CLI into ISOs
EMBED_PROFILES=1     # embed bundled Incus profiles into ISOs
PRE_RESET_SNAPSHOT=1      # snapshot Incus instances before reset
POST_HARD_RESET_RESTART=1 # restart daemon after hard reset
```

### distrobuilder-hook.sh

Config is read from `/etc/penguins-distrobuilder/eggs-hooks.conf`:

```bash
DISTROBUILDER_ENABLED=0          # set to 1 to activate
DISTROBUILDER_TYPE=incus         # incus | lxc | both
DISTROBUILDER_OUTPUT=/var/lib/eggs/distrobuilder
DISTROBUILDER_TEMPLATE=          # path to template YAML (auto-detected if empty)
```

## Registration

Register hooks with penguins-eggs by symlinking into its plugin directory:

```bash
# PIP hook
sudo ln -s "$(pwd)/integration/eggs-plugin/pip-hook.sh" \
           /usr/share/penguins-eggs/plugins/pip-hook.sh

# distrobuilder hook
sudo ln -s "$(pwd)/integration/eggs-plugin/distrobuilder-hook.sh" \
           /usr/share/penguins-eggs/plugins/distrobuilder-hook.sh
```

Register recovery hooks:

```bash
sudo ln -s "$(pwd)/integration/recovery-plugin/pip-recovery-plugin.sh" \
           /usr/share/penguins-recovery/plugins/pip-recovery-plugin.sh

sudo ln -s "$(pwd)/integration/recovery-plugin/distrobuilder-recovery-hook.sh" \
           /usr/share/penguins-recovery/plugins/distrobuilder-recovery-hook.sh
```

## Acceptance criteria

- [ ] `eggs produce` on a system with PIP installed produces an ISO where
      `penguins-incus-daemon` starts automatically in the live environment
- [ ] `penguins-incus container list` works inside the live ISO without
      additional setup
- [ ] `eggs produce` with `DISTROBUILDER_ENABLED=1` produces a valid
      LXC/Incus image alongside the ISO
- [ ] Pre-reset hook creates an Incus snapshot for each running instance
- [ ] Post-hard-reset hook restarts the daemon and re-applies profiles
- [ ] Pre-reset distrobuilder hook produces a restorable rootfs snapshot
