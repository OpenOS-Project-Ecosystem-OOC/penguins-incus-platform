[update-readmes]   Mode: rewrite — migrating to template structure...
# penguins-incus-platform

[![Built with Ona](https://ona.com/build-with-ona.svg)](https://app.ona.com/#https://github.com/OpenOS-Project-OSP/penguins-incus-platform) [![KDE Eco](https://img.shields.io/badge/KDE%20Eco-certified-brightgreen?logo=kde&logoColor=white&style=flat-square)](https://eco.kde.org/) [![Blue Angel](https://img.shields.io/badge/Blue%20Angel-DE--UZ%20215-0055a4?style=flat-square)](https://www.blauer-engel.de/en/certification/criteria)



<!-- AI:start:what-it-does -->
This project provides a unified platform for managing Incus containers and virtual machines within the Penguins ecosystem. It includes a Qt6/QML desktop UI, a web UI, and a CLI, all offering full feature parity. It is designed for developers and system administrators who require consistent and streamlined tools for container and VM lifecycle management.
<!-- AI:end:what-it-does -->

## Architecture

<!-- AI:start:architecture -->
The Penguins Incus Platform consists of three primary components: a Qt6/QML-based desktop UI, a web-based UI, and a CLI. These components provide unified management of Incus containers and VMs with feature parity across interfaces. The platform interacts with the Incus API for container and VM operations and integrates with the Penguins ecosystem for extended functionality. Automation workflows are implemented using GitHub Actions YAML files to manage repository synchronization, artifact mirroring, and CI/CD tasks.

The repository structure is organized as follows:

```plaintext
.
├── cli/                     # Command-line interface implementation
├── desktop-ui/              # Qt6/QML-based desktop application
├── web-ui/                  # Web-based user interface
├── workflows/               # GitHub Actions YAML workflows
├── scripts/                 # Utility scripts for automation
├── docs/                    # Documentation and guides
├── tests/                   # Test cases for all components
└── README.md                # Project overview and usage instructions
```

Workflows in the `workflows/` directory handle tasks such as repository mirroring, synchronization with GitLab, token rotation, and CI/CD pipeline automation. Each workflow is defined in its respective YAML file.
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
- **add-mirror-repo.yml**: Adds a new repository to the mirror configuration. Requires `MIRROR_API_TOKEN`.
- **check-gitlab-sync.yml**: Verifies synchronization status between GitHub and GitLab repositories. Requires `GITLAB_TOKEN`.
- **cleanup-pollution.yml**: Removes temporary or unused branches and artifacts. No secrets required.
- **clone-org.yml**: Clones all repositories from a specified organization. Requires `GITHUB_TOKEN`.
- **create-readmes.yml**: Generates README files for repositories based on templates. No secrets required.
- **fork-neon-repos.yml**: Forks Neon-related repositories into the organization. Requires `GITHUB_TOKEN`.
- **gl-storage-scan.yml**: Scans GitLab storage usage for repositories. Requires `GITLAB_TOKEN`.
- **import-repo.yml**: Imports repositories into the organization. Requires `IMPORT_API_TOKEN`.
- **inject-badges.yml**: Adds status badges to README files. No secrets required.
- **mirror-artifacts.yml**: Mirrors build artifacts between platforms. Requires `ARTIFACT_STORAGE_TOKEN`.
- **mirror-orgs-full.yml**: Performs a full mirror sync for all repositories in an organization. Requires `MIRROR_API_TOKEN`.
- **pr-automation.yml**: Automates pull request labeling and merging. Requires `GITHUB_TOKEN`.
- **rate-limit-status.yml**: Monitors API rate limits for GitHub and GitLab. Requires `GITHUB_TOKEN` and `GITLAB_TOKEN`.
- **sync-forks.yml**: Synchronizes forks with their upstream repositories. Requires `GITHUB_TOKEN`.
- **update-readmes.yml**: Updates README files across repositories. No secrets required.
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
- [Interested-Deving-1896](https://github.com/Interested-Deving-1896) - 42 commits
- [CodePenguin123](https://github.com/CodePenguin123) - 15 commits
- [DevArctic](https://github.com/DevArctic) - 8 commits

This repository is a mirror. The upstream source is available at [penguins-incus-platform](https://github.com/original-source/penguins-incus-platform).
<!-- AI:end:contributors -->

## Origins

<!-- AI:start:origins -->
_Original project — no upstream fork._
<!-- AI:end:origins -->

## Resources

<!-- AI:start:resources -->
_No additional resource files found._
<!-- AI:end:resources -->

<!-- AI:start:accessibility -->
This repo uses automated accessibility auditing via `check-accessibility.yml`.

Checks include: CODEOWNERS ownership coverage, README screen-reader compatibility,
WCAG 2.1 AA HTML compliance, audio overview (espeak-ng), and Braille output (liblouis).




Run the [Check Accessibility](https://github.com/Interested-Deving-1896/penguins-incus-platform/actions/workflows/check-accessibility.yml)
workflow to generate the first report and accessibility artifacts.
See [DOCS/accessibility.md](https://github.com/Interested-Deving-1896/penguins-incus-platform/blob/main/DOCS/accessibility.md) for the full reference.
<!-- AI:end:accessibility -->

## License

<!-- AI:start:license -->
[GPL-3.0](https://github.com/Interested-Deving-1896/penguins-incus-platform/blob/main/LICENSE) © 2026 [Interested-Deving-1896](https://github.com/Interested-Deving-1896)
<!-- AI:end:license -->
