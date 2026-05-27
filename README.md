[update-readmes]   Mode: rewrite — migrating to template structure...
# penguins-incus-platform

[![Built with Ona](https://ona.com/build-with-ona.svg)](https://app.ona.com/#https://github.com/Interested-Deving-1896/penguins-incus-platform)

<!-- AI:start:what-it-does -->
This project provides unified management for Incus containers and virtual machines within the Penguins ecosystem. It includes a Qt6/QML desktop UI, web UI, and CLI, ensuring feature parity across all interfaces. It is used by developers and system administrators to streamline container and VM operations in environments requiring consistent tooling and workflows.
<!-- AI:end:what-it-does -->

## Architecture

<!-- AI:start:architecture -->
The Penguins Incus Platform consists of three primary components: a Qt6/QML-based desktop UI, a web UI, and a CLI. These components provide unified management for Incus containers and VMs, ensuring feature parity across interfaces. The platform interacts with Incus APIs to manage container and VM lifecycles, networking, and storage. It also integrates with Penguins ecosystem tools for seamless operation.

The repository is organized as follows:

```plaintext
.
├── .devcontainer/          # Development container configuration
├── .github/                # GitHub workflows and CI/CD pipelines
├── config/                 # Configuration files for the platform
├── distrobuilder/          # Tools for building container and VM images
├── integration/            # Integration tests and related scripts
├── oci-builder/            # OCI-compliant image builder
├── penguins-incus-platform/ # Core platform codebase
├── scripts/                # Utility and helper scripts
├── unified-image-server/   # Server for managing unified images
├── ARCHITECTURE.md         # Detailed architecture documentation
├── LICENSE                 # Licensing information
├── README.md               # Project overview and usage instructions
```

The `.github` directory contains workflows for repository automation, including synchronization, artifact mirroring, and CI tasks. Each workflow is defined in YAML files, such as `mirror-orgs-full.yml` and `sync-to-gitlab.yml`.
<!-- AI:end:architecture -->

## Install

<!-- Add installation instructions here. This section is yours — the AI will not modify it. -->

```bash
git clone https://github.com/Interested-Deving-1896/penguins-incus-platform.git
cd penguins-incus-platform
```

## Usage

<!-- Add usage examples here. This section is yours — the AI will not modify it. -->

## Configuration

<!-- Document configuration options here. This section is yours — the AI will not modify it. -->

## CI

<!-- AI:start:ci -->
The repository uses GitHub Actions for continuous integration and automation. Below is a summary of the workflows and their purposes:

- **add-mirror-repo.yml**: Adds new repositories to the mirror configuration. Requires `GITHUB_TOKEN`.
- **check-gitlab-sync.yml**: Verifies synchronization status between GitHub and GitLab repositories.
- **cleanup-branches.yml**: Deletes stale branches across repositories. Requires `GITHUB_TOKEN`.
- **cleanup-pollution.yml**: Removes unnecessary files or configurations from repositories.
- **generate-dep-graph.yml**: Creates a dependency graph for project components.
- **mirror-artifacts.yml**: Syncs build artifacts to external storage. Requires `ARTIFACT_STORAGE_KEY`.
- **mirror-orgs-full.yml**: Performs a full mirror of all repositories in an organization. Requires `GITHUB_TOKEN` and `ORG_MIRROR_KEY`.
- **mirror-orgs-watchdog.yml**: Monitors and ensures the health of organization mirrors.
- **notify-poller.yml**: Sends notifications for polling-based updates. Requires `NOTIFICATION_WEBHOOK`.
- **pr-automation.yml**: Automates pull request labeling and merging. Requires `GITHUB_TOKEN`.
- **rate-limit-status.yml**: Monitors and logs GitHub API rate limits.
- **sync-to-gitlab.yml**: Syncs repositories from GitHub to GitLab. Requires `GITLAB_TOKEN`.
- **token-health.yml**: Validates the health and expiration of API tokens.
- **update-infra-deps.yml**: Updates infrastructure dependencies across repositories.
- **validate-config.yml**: Checks configuration files for syntax and policy compliance.

Secrets required for workflows include `GITHUB_TOKEN`, `GITLAB_TOKEN`, `ARTIFACT_STORAGE_KEY`, `ORG_MIRROR_KEY`, and `NOTIFICATION_WEBHOOK`.
<!-- AI:end:ci -->

## Mirror chain

<!-- AI:start:mirror-chain -->
This repo is maintained in [`Interested-Deving-1896/penguins-incus-platform`](https://github.com/Interested-Deving-1896/penguins-incus-platform) and mirrored through:

```
Interested-Deving-1896/penguins-incus-platform  ──►  OpenOS-Project-OSP/penguins-incus-platform  ──►  OpenOS-Project-Ecosystem-OOC/penguins-incus-platform
```

Changes flow downstream automatically via the hourly mirror chain in
[`fork-sync-all`](https://github.com/Interested-Deving-1896/fork-sync-all).
Direct commits to OSP or OOC are detected and opened as PRs back to `Interested-Deving-1896`.
<!-- AI:end:mirror-chain -->

## Contributors

<!-- AI:start:contributors -->
[@Interested-Deving-1896](https://github.com/Interested-Deving-1896): 182 commits

*Note: This repository is a mirror. Please refer to the upstream source for additional contributions and history.*
<!-- AI:end:contributors -->

## Origins

<!-- AI:start:origins -->
_Original project — no upstream fork._
<!-- AI:end:origins -->

## Resources

<!-- AI:start:resources -->
| File | Description |
|---|---|
| [config/gitlab-subgroups.yml](https://github.com/Interested-Deving-1896/penguins-incus-platform/blob/main/config/gitlab-subgroups.yml) | GitLab subgroup map |
<!-- AI:end:resources -->

## License

<!-- AI:start:license -->
[GPL-3.0](https://github.com/Interested-Deving-1896/penguins-incus-platform/blob/main/LICENSE) © 2026 [Interested-Deving-1896](https://github.com/Interested-Deving-1896)
<!-- AI:end:license -->
