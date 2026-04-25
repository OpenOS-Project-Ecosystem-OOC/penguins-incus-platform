//! Build pipeline orchestrator.
//!
//! Ties together the Incus client, exec helpers, rootfs export, and OCI
//! commit into a single `build` function that the CLI calls.
//!
//! Pipeline stages:
//!   1. Spawn an ephemeral Incus container from the source image.
//!   2. Run post-unpack actions.
//!   3. Install / remove packages.
//!   4. Run post-packages actions.
//!   5. Apply file generators.
//!   6. Run post-files actions.
//!   7. Stop the container and export its rootfs.
//!   8. Commit the rootfs to an OCI image layout.
//!   9. Optionally push to a registry.
//!  10. Clean up the container (always, even on failure).

pub mod preflight;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{error, info};
use uuid::Uuid;

use crate::arch::{host_arch, Platform};
use crate::cache::{self, Stage};
use crate::definition::{ActionTrigger, Definition, Downloader, FileGenerator, PackageAction};
use crate::incus::api::{InstanceConfig, InstanceSource};
use crate::incus::bootstrap::{debootstrap, rpmbootstrap};
use crate::incus::exec::{
    cleanup_packages, install_packages, remove_packages, run_script, upgrade_packages,
};
use crate::incus::export::export_rootfs_to_dir;
use crate::incus::IncusClient;
use crate::oci::commit::{commit_layered_rootfs, commit_rootfs};
use crate::oci::convert::{convert, OutputFormat};
use crate::oci::multiarch::assemble_index;
use crate::oci::push::push;

pub struct BuildOptions {
    /// Where to write the OCI image layout directory.
    pub output_dir: PathBuf,
    /// If true, keep the Incus container after the build (for debugging).
    pub keep_container: bool,
    /// Output format: oci-dir (default), oci-archive, or docker-archive.
    pub output_format: OutputFormat,
    /// Target platforms for multi-arch builds (e.g. `linux/amd64,linux/arm64`).
    /// Empty means single-arch using `def.image.architecture` (or host arch).
    pub platforms: Vec<Platform>,
    /// If true, bypass the build cache and always run the full pipeline.
    pub no_cache: bool,
    /// Override the push registry from the CLI (`--push registry.example.com`).
    /// Takes precedence over `oci.registry` in the definition file.
    /// `None` means use the definition's registry (which may also be empty).
    pub push_registry: Option<String>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./oci-output"),
            keep_container: false,
            output_format: OutputFormat::OciDir,
            platforms: Vec::new(),
            no_cache: false,
            push_registry: None,
        }
    }
}

/// Run the full build pipeline for `def`.
///
/// When `opts.platforms` is non-empty, the pipeline runs once per platform
/// and the results are merged into a multi-arch OCI image index.
pub async fn build(def: &Definition, opts: &BuildOptions) -> Result<()> {
    // Fail fast before touching Incus.
    preflight::run(def)
        .await
        .context("pre-flight checks failed")?;

    if opts.platforms.len() > 1 {
        build_multiarch(def, opts).await
    } else {
        // Single-arch path (original behaviour).
        // Resolve the target architecture: explicit platform > definition > host.
        let arch = opts
            .platforms
            .first()
            .map(|p| p.incus_arch())
            .or_else(|| {
                if def.image.architecture.is_empty() {
                    None
                } else {
                    Some(def.image.architecture.clone())
                }
            })
            .unwrap_or_else(host_arch);

        let mut arch_def = def.clone();
        arch_def.image.architecture = arch;

        build_single(&arch_def, opts, &opts.output_dir).await
    }
}

/// Build for each platform in `opts.platforms` and assemble a multi-arch index.
async fn build_multiarch(def: &Definition, opts: &BuildOptions) -> Result<()> {
    let mut arch_dirs: Vec<(String, PathBuf)> = Vec::new();

    for platform in &opts.platforms {
        let oci_arch = platform.oci_arch();
        let incus_arch = platform.incus_arch();
        let arch_output = opts.output_dir.join(&oci_arch);

        info!(arch = oci_arch, "building for platform {platform}");

        let mut arch_def = def.clone();
        arch_def.image.architecture = incus_arch;

        let arch_opts = BuildOptions {
            output_dir: arch_output.clone(),
            keep_container: opts.keep_container,
            output_format: OutputFormat::OciDir, // always oci-dir per-arch; convert at the end
            platforms: Vec::new(),
            no_cache: opts.no_cache,
            push_registry: None, // push happens at the index level, not per-arch
        };

        // Preflight already ran; skip it for per-arch runs.
        build_single(&arch_def, &arch_opts, &arch_output).await?;
        arch_dirs.push((oci_arch, arch_output));
    }

    // Assemble the multi-arch index into the top-level output dir.
    let pairs: Vec<(&str, &Path)> = arch_dirs
        .iter()
        .map(|(a, p)| (a.as_str(), p.as_path()))
        .collect();
    assemble_index(&pairs, &opts.output_dir).context("assembling multi-arch index")?;

    // Apply output format conversion to the assembled index.
    let final_output = convert(
        &opts.output_dir,
        opts.output_format,
        &def.effective_name(),
        &def.effective_tag(),
    )
    .context("converting multi-arch output format")?;

    // Push the multi-arch index if a registry is configured.
    let effective_registry = opts
        .push_registry
        .as_deref()
        .filter(|r| !r.is_empty())
        .or_else(|| {
            def.oci
                .as_ref()
                .map(|o| o.registry.as_str())
                .filter(|r| !r.is_empty())
        });
    if let Some(registry) = effective_registry {
        push(
            &opts.output_dir,
            registry,
            &def.effective_name(),
            &def.effective_tag(),
        )
        .await
        .context("pushing multi-arch OCI index")?;
    }

    info!(output = %final_output.display(), arches = opts.platforms.len(), "multi-arch build complete");
    Ok(())
}

/// Run the single-arch pipeline, writing the OCI layout to `output_dir`.
///
/// Checks the build cache first (unless `opts.no_cache` is set). On a cache
/// hit the cached OCI layout is copied to `output_dir` and the pipeline is
/// skipped. On a cache miss the pipeline runs and the result is stored.
async fn build_single(def: &Definition, opts: &BuildOptions, output_dir: &Path) -> Result<()> {
    // ── Cache lookup (whole-definition) ──────────────────────────────────────
    if !opts.no_cache {
        if let Some(entry) = cache::lookup(def).context("checking build cache")? {
            info!(
                cached_at = entry.built_at,
                digest = entry.index_digest,
                "using cached build — skipping pipeline (use --no-cache to force rebuild)"
            );
            if entry.oci_layout_path != output_dir {
                cache::copy_dir_all_pub(&entry.oci_layout_path, output_dir)
                    .context("copying cached layout to output dir")?;
            }
            return Ok(());
        }

        // ── Stage-level cache lookup ──────────────────────────────────────────
        // If the full pipeline rootfs snapshot is cached (same source + actions
        // + packages + files), restore it and skip straight to OCI commit.
        if let Ok(stage_key) = pipeline_stage_key(def) {
            if cache::stage_hit(&stage_key).unwrap_or(false) {
                info!(
                    key = stage_key,
                    "stage cache hit — restoring rootfs, skipping container pipeline"
                );
                let rootfs_dir =
                    tempfile::TempDir::new().context("creating temp dir for cached rootfs")?;
                cache::stage_restore(&stage_key, rootfs_dir.path())
                    .context("restoring stage cache")?;
                commit_rootfs(rootfs_dir.path(), output_dir, def)
                    .context("committing cached rootfs to OCI layout")?;

                // Apply output format conversion.
                let final_output = convert(
                    output_dir,
                    opts.output_format,
                    &def.effective_name(),
                    &def.effective_tag(),
                )
                .context("converting output format (stage cache path)")?;

                // Push if configured (--push flag takes precedence over definition).
                let effective_registry = opts
                    .push_registry
                    .as_deref()
                    .filter(|r| !r.is_empty())
                    .or_else(|| {
                        def.oci
                            .as_ref()
                            .map(|o| o.registry.as_str())
                            .filter(|r| !r.is_empty())
                    });
                if let Some(registry) = effective_registry {
                    push(
                        output_dir,
                        registry,
                        &def.effective_name(),
                        &def.effective_tag(),
                    )
                    .await
                    .context("pushing OCI image (stage cache path)")?;
                }

                // Store the whole-definition cache entry so next run is even faster.
                if let Err(e) = cache::store(def, output_dir) {
                    tracing::warn!("failed to write build cache after stage restore: {e}");
                }

                info!(output = %final_output.display(), "build complete (from stage cache)");
                return Ok(());
            }
        }
    }

    // ── Full pipeline ─────────────────────────────────────────────────────────
    let client = IncusClient::new().context("connecting to Incus daemon")?;

    let instance_name = format!("iob-{}", &Uuid::new_v4().to_string()[..8]);
    info!(
        instance = instance_name,
        arch = def.image.architecture,
        "starting build"
    );

    let result = run_pipeline(&client, def, opts, &instance_name, output_dir).await;

    if !opts.keep_container {
        cleanup(&client, &instance_name).await;
    }

    result?;

    // ── Cache store ───────────────────────────────────────────────────────────
    if !opts.no_cache {
        if let Err(e) = cache::store(def, output_dir) {
            // Cache write failure is non-fatal — warn and continue.
            tracing::warn!("failed to write build cache: {e}");
        }
    }

    Ok(())
}

async fn run_pipeline(
    client: &IncusClient,
    def: &Definition,
    opts: &BuildOptions,
    instance: &str,
    output_dir: &Path,
) -> Result<()> {
    // ── Stage 1: Create and start the build container ─────────────────────────
    info!(instance, "creating build container");

    // For host-side bootstrappers we build the rootfs on the host first, import
    // it into Incus via the REST API, and track the fingerprint so we can delete
    // the image after the container is created (task 5: bootstrap image cleanup).
    let mut bootstrap_image_fingerprint: Option<String> = None;

    let source = match &def.source.downloader {
        Downloader::Incus => InstanceSource::from_remote_alias(&def.source.image),

        Downloader::Debootstrap => {
            let rootfs_tmp =
                tempfile::TempDir::new().context("creating temp dir for debootstrap rootfs")?;
            let mirror = if def.source.url.is_empty() {
                None
            } else {
                Some(def.source.url.as_str())
            };
            debootstrap(
                &def.source.suite,
                rootfs_tmp.path(),
                mirror,
                &def.source.components,
            )
            .await
            .context("debootstrap failed")?;
            let (src, fp) =
                import_rootfs_as_incus_image(client, rootfs_tmp.path(), instance).await?;
            bootstrap_image_fingerprint = Some(fp);
            src
        }

        Downloader::Rpmbootstrap => {
            let rootfs_tmp =
                tempfile::TempDir::new().context("creating temp dir for rpmbootstrap rootfs")?;
            let mirror = if def.source.url.is_empty() {
                None
            } else {
                Some(def.source.url.as_str())
            };
            rpmbootstrap(
                &def.image.release,
                rootfs_tmp.path(),
                mirror,
                &def.source.seed_packages,
            )
            .await
            .context("rpmbootstrap failed")?;
            let (src, fp) =
                import_rootfs_as_incus_image(client, rootfs_tmp.path(), instance).await?;
            bootstrap_image_fingerprint = Some(fp);
            src
        }

        Downloader::RootfsHttp => {
            if def.source.url.is_empty() {
                anyhow::bail!("rootfs-http downloader requires source.url to be set");
            }
            let rootfs_tmp =
                tempfile::TempDir::new().context("creating temp dir for http rootfs")?;
            let checksum = if def.source.checksum.is_empty() {
                None
            } else {
                Some(def.source.checksum.as_str())
            };
            crate::incus::bootstrap::rootfs_http(
                &def.source.url,
                rootfs_tmp.path(),
                checksum,
                def.source.http_auth.as_ref(),
            )
            .await
            .context("rootfs-http download failed")?;
            let (src, fp) =
                import_rootfs_as_incus_image(client, rootfs_tmp.path(), instance).await?;
            bootstrap_image_fingerprint = Some(fp);
            src
        }
    };

    let config = InstanceConfig {
        name: instance.to_string(),
        instance_type: "container".to_string(),
        source,
        ephemeral: true,
        config: Default::default(),
        // Pass the target architecture so Incus can use the right kernel/emulation.
        architecture: def.image.architecture.clone(),
    };

    client
        .create_instance(&config)
        .await
        .context("creating instance")?;

    // ── Bootstrap image cleanup (task 5) ──────────────────────────────────────
    // Delete the imported bootstrap image now that the container exists.
    // The container holds its own copy of the rootfs; the image is no longer
    // needed and would otherwise accumulate across builds.
    if let Some(fp) = &bootstrap_image_fingerprint {
        if let Err(e) = client.delete_image(fp).await {
            tracing::warn!("failed to delete bootstrap image {fp}: {e}");
        } else {
            info!(fingerprint = fp, "bootstrap image deleted");
        }
    }

    client
        .start_instance(instance)
        .await
        .context("starting instance")?;

    // ── Stage 2: Post-unpack actions ──────────────────────────────────────────
    run_actions(client, instance, def, &ActionTrigger::PostUnpack).await?;

    let layered = def.oci.as_ref().map(|o| o.layered).unwrap_or(false);
    let mut snap_idx: usize = 0;

    // Take a snapshot after post-unpack if layered mode is enabled.
    if layered {
        client
            .create_snapshot(instance, &format!("iob-snap-{snap_idx}"))
            .await
            .context("snapshot after post-unpack")?;
        snap_idx += 1;
    }

    // ── Stage 3: Packages ─────────────────────────────────────────────────────
    if let Some(pkgs) = &def.packages {
        let manager = &pkgs.manager;

        if pkgs.update {
            upgrade_packages(client, instance, manager)
                .await
                .context("upgrading packages")?;
        }

        for set in &pkgs.sets {
            // Filter by release and architecture if specified.
            if !set.releases.is_empty() && !set.releases.contains(&def.image.release) {
                continue;
            }
            if !set.architectures.is_empty() && !set.architectures.contains(&def.image.architecture)
            {
                continue;
            }

            match set.action {
                PackageAction::Install => {
                    install_packages(client, instance, manager, &set.packages)
                        .await
                        .context("installing packages")?;
                }
                PackageAction::Remove => {
                    remove_packages(client, instance, manager, &set.packages)
                        .await
                        .context("removing packages")?;
                }
            }
        }

        if pkgs.cleanup {
            cleanup_packages(client, instance, manager)
                .await
                .context("cleaning up package caches")?;
        }
    }

    // ── Stage 4: Post-packages actions ────────────────────────────────────────
    run_actions(client, instance, def, &ActionTrigger::PostPackages).await?;

    if layered {
        client
            .create_snapshot(instance, &format!("iob-snap-{snap_idx}"))
            .await
            .context("snapshot after post-packages")?;
        snap_idx += 1;
    }

    // ── Stage 5: File generators ──────────────────────────────────────────────
    for file in &def.files {
        apply_file_generator(client, instance, file)
            .await
            .with_context(|| {
                format!(
                    "applying file generator {:?} for {}",
                    file.generator, file.path
                )
            })?;
    }

    // ── Stage 6: Post-files actions ───────────────────────────────────────────
    run_actions(client, instance, def, &ActionTrigger::PostFiles).await?;

    if layered {
        client
            .create_snapshot(instance, &format!("iob-snap-{snap_idx}"))
            .await
            .context("snapshot after post-files")?;
        // snap_idx not incremented further — commit_layered reads until it
        // finds no more snapshots, then appends the final container state.
    }

    // ── Stage 7: Export rootfs and commit OCI image ───────────────────────────
    client
        .stop_instance(instance)
        .await
        .context("stopping instance")?;

    let layered = def.oci.as_ref().map(|o| o.layered).unwrap_or(false);

    if layered {
        // Snapshot-based layered build: each snapshot taken during the build
        // becomes a separate OCI layer. Produces smaller incremental layers
        // and enables layer cache reuse on re-builds.
        commit_layered(client, instance, output_dir, def).await?;
    } else {
        // Single-layer build: export the full rootfs and pack it as one layer.
        let rootfs_dir = tempfile::TempDir::new().context("creating temp dir for rootfs")?;
        export_rootfs_to_dir(client, instance, rootfs_dir.path())
            .await
            .context("exporting rootfs")?;

        // Store a stage-level rootfs snapshot so future builds with the same
        // pipeline inputs can skip the container execution entirely.
        if !opts.no_cache {
            let stage_key = pipeline_stage_key(def);
            if let Ok(key) = stage_key {
                if let Err(e) = cache::stage_store(&key, rootfs_dir.path()) {
                    tracing::warn!("failed to write stage cache: {e}");
                } else {
                    info!(key, "stage rootfs snapshot stored");
                }
            }
        }

        commit_rootfs(rootfs_dir.path(), output_dir, def)
            .context("committing rootfs to OCI layout")?;
    }

    // ── Stage 8b: Convert output format (optional) ────────────────────────────
    let final_output = convert(
        output_dir,
        opts.output_format,
        &def.effective_name(),
        &def.effective_tag(),
    )
    .context("converting output format")?;

    // ── Stage 9: Push (optional) ──────────────────────────────────────────────
    // --push CLI flag takes precedence over oci.registry in the definition.
    let effective_registry = opts
        .push_registry
        .as_deref()
        .filter(|r| !r.is_empty())
        .or_else(|| {
            def.oci
                .as_ref()
                .map(|o| o.registry.as_str())
                .filter(|r| !r.is_empty())
        });

    if let Some(registry) = effective_registry {
        push(
            output_dir,
            registry,
            &def.effective_name(),
            &def.effective_tag(),
        )
        .await
        .context("pushing OCI image")?;
    }

    info!(output = %final_output.display(), "build complete");
    Ok(())
}

async fn run_actions(
    client: &IncusClient,
    instance: &str,
    def: &Definition,
    trigger: &ActionTrigger,
) -> Result<()> {
    for action in def.actions.iter().filter(|a| &a.trigger == trigger) {
        if !action.releases.is_empty() && !action.releases.contains(&def.image.release) {
            continue;
        }
        if !action.architectures.is_empty()
            && !action.architectures.contains(&def.image.architecture)
        {
            continue;
        }
        run_script(client, instance, &action.action)
            .await
            .with_context(|| format!("running {:?} action", trigger))?;
    }
    Ok(())
}

async fn apply_file_generator(
    client: &IncusClient,
    instance: &str,
    file: &crate::definition::FileDef,
) -> Result<()> {
    match file.generator {
        FileGenerator::Dump => {
            // Write literal content to the path inside the container.
            let escaped = file.content.replace('\'', "'\\''");
            let script = format!(
                "mkdir -p $(dirname '{}') && printf '%s' '{}' > '{}'",
                file.path, escaped, file.path
            );
            run_script(client, instance, &script).await?;
        }
        FileGenerator::Remove => {
            run_script(client, instance, &format!("rm -rf '{}'", file.path)).await?;
        }
        FileGenerator::Hostname => {
            let hostname = if file.content.is_empty() {
                "localhost"
            } else {
                &file.content
            };
            run_script(
                client,
                instance,
                &format!("echo '{}' > /etc/hostname", hostname),
            )
            .await?;
        }
        FileGenerator::Hosts => {
            let content = if file.content.is_empty() {
                "127.0.0.1 localhost\n::1 localhost\n".to_string()
            } else {
                file.content.clone()
            };
            let escaped = content.replace('\'', "'\\''");
            run_script(
                client,
                instance,
                &format!("printf '%s' '{}' > /etc/hosts", escaped),
            )
            .await?;
        }
        FileGenerator::Copy => {
            // Read the host-side source file and push it into the container
            // via the Incus file API.
            if file.source.is_empty() {
                anyhow::bail!(
                    "copy generator for '{}' requires a 'source' field",
                    file.path
                );
            }
            let content = std::fs::read(&file.source)
                .with_context(|| format!("reading source file '{}'", file.source))?;

            // Parse mode string (e.g. "0644") or default to 0644.
            let mode = if file.mode.is_empty() {
                0o644u32
            } else {
                u32::from_str_radix(file.mode.trim_start_matches("0o"), 8)
                    .with_context(|| format!("parsing mode '{}'", file.mode))?
            };

            // Ensure the parent directory exists inside the container first.
            if let Some(parent) = std::path::Path::new(&file.path).parent() {
                if parent != std::path::Path::new("/") && !parent.as_os_str().is_empty() {
                    run_script(
                        client,
                        instance,
                        &format!("mkdir -p '{}'", parent.display()),
                    )
                    .await
                    .with_context(|| format!("creating parent dir for '{}'", file.path))?;
                }
            }

            client
                .push_file(
                    instance,
                    &file.path,
                    bytes::Bytes::from(content),
                    mode,
                    0,
                    0,
                )
                .await
                .with_context(|| format!("pushing file to '{}'", file.path))?;
        }
    }
    Ok(())
}

/// Build a layered OCI image by diffing successive Incus snapshots.
///
/// Snapshots are taken at each pipeline stage boundary. Each snapshot pair
/// produces a tar diff layer. The final image has one layer per stage that
/// changed the filesystem, enabling layer cache reuse on incremental rebuilds.
async fn commit_layered(
    client: &IncusClient,
    instance: &str,
    output_dir: &std::path::Path,
    def: &Definition,
) -> Result<()> {
    info!(instance, "building layered OCI image from snapshots");

    // List snapshots taken during the build (named iob-snap-0, iob-snap-1, …).
    // Export each snapshot's rootfs and compute diffs between consecutive ones.
    let snap_prefix = "iob-snap-";
    let mut snap_dirs: Vec<tempfile::TempDir> = Vec::new();

    // Export the base snapshot (snap-0) as the first layer.
    let base_snap = format!("{snap_prefix}0");
    let base_dir = tempfile::TempDir::new().context("creating temp dir for base snapshot")?;
    export_rootfs_to_dir(client, &format!("{instance}/{base_snap}"), base_dir.path())
        .await
        .with_context(|| format!("exporting snapshot {base_snap}"))?;
    snap_dirs.push(base_dir);

    // Export subsequent snapshots.
    let mut i = 1usize;
    loop {
        let snap = format!("{snap_prefix}{i}");
        let dir = tempfile::TempDir::new()
            .with_context(|| format!("creating temp dir for snapshot {snap}"))?;
        match export_rootfs_to_dir(client, &format!("{instance}/{snap}"), dir.path()).await {
            Ok(()) => {
                snap_dirs.push(dir);
                i += 1;
            }
            Err(_) => break, // no more snapshots
        }
    }

    // Also export the final container state as the last layer.
    let final_dir = tempfile::TempDir::new().context("creating temp dir for final rootfs")?;
    export_rootfs_to_dir(client, instance, final_dir.path())
        .await
        .context("exporting final rootfs")?;
    snap_dirs.push(final_dir);

    let layer_dirs: Vec<&std::path::Path> = snap_dirs.iter().map(|d| d.path()).collect();
    commit_layered_rootfs(&layer_dirs, output_dir, def).context("committing layered OCI image")?;

    // Clean up snapshots.
    for j in 0..i {
        let snap = format!("{snap_prefix}{j}");
        if let Err(e) = client.delete_snapshot(instance, &snap).await {
            tracing::warn!("failed to delete snapshot {snap}: {e}");
        }
    }

    Ok(())
}

/// Pack a host-side rootfs directory into a squashfs and import it into the
/// Incus image store via the REST API (`POST /1.0/images`).
///
/// Returns the `InstanceSource` and the image fingerprint so the caller can
/// delete the image after the container has been created.
async fn import_rootfs_as_incus_image(
    client: &IncusClient,
    rootfs: &std::path::Path,
    tag: &str,
) -> Result<(InstanceSource, String)> {
    let squashfs_tmp = tempfile::NamedTempFile::new().context("creating temp squashfs file")?;
    let mksquashfs =
        which::which("mksquashfs").context("mksquashfs not found — install squashfs-tools")?;

    info!(rootfs = %rootfs.display(), "packing rootfs into squashfs");
    let status = tokio::process::Command::new(&mksquashfs)
        .arg(rootfs)
        .arg(squashfs_tmp.path())
        .arg("-noappend")
        .arg("-comp")
        .arg("xz")
        .status()
        .await
        .context("running mksquashfs")?;
    if !status.success() {
        anyhow::bail!("mksquashfs failed with status {status}");
    }

    let squashfs_bytes =
        bytes::Bytes::from(std::fs::read(squashfs_tmp.path()).context("reading squashfs file")?);

    let alias = format!("iob-bootstrap-{tag}");
    let fingerprint = client
        .import_image(squashfs_bytes, &alias)
        .await
        .context("importing rootfs image via Incus REST API")?;

    Ok((InstanceSource::from_local_alias(&alias), fingerprint))
}

/// Compute the cumulative stage key for the full pipeline (through post-files).
///
/// This key changes only when the source, actions, packages, or files sections
/// of the definition change — not when OCI labels or registry settings change.
/// Used to key the stage-level rootfs snapshot cache.
fn pipeline_stage_key(def: &Definition) -> Result<String> {
    // Use a fixed root so the stage key is independent of the full def hash.
    // This means changing a label doesn't bust the rootfs snapshot.
    let root = "pipeline-v1";
    let k = cache::stage_key(root, Stage::Source, def)?;
    let k = cache::stage_key(&k, Stage::PostUnpack, def)?;
    let k = cache::stage_key(&k, Stage::Packages, def)?;
    let k = cache::stage_key(&k, Stage::PostPackages, def)?;
    let k = cache::stage_key(&k, Stage::Files, def)?;
    cache::stage_key(&k, Stage::PostFiles, def)
}

/// Best-effort cleanup — log errors but don't propagate them.
async fn cleanup(client: &IncusClient, instance: &str) {
    info!(instance, "cleaning up build container");
    if let Err(e) = client.stop_instance(instance).await {
        // May already be stopped — not an error.
        tracing::debug!(instance, "stop (cleanup): {e}");
    }
    if let Err(e) = client.delete_instance(instance).await {
        error!(instance, "failed to delete instance during cleanup: {e}");
    }
}
