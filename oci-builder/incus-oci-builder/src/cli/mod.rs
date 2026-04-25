//! CLI definition using clap derive macros.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "incus-oci-builder",
    version,
    about = "Build OCI images using Incus system containers as the build environment",
    long_about = None,
)]
pub struct Cli {
    /// Incus Unix socket path.
    #[arg(
        long,
        default_value = "/var/lib/incus/unix.socket",
        env = "INCUS_SOCKET"
    )]
    pub socket: PathBuf,

    /// Log level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info", env = "LOG_LEVEL")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Build an OCI image from a definition file.
    Build(BuildArgs),

    /// Validate a definition file without running a build.
    Validate(ValidateArgs),

    /// Generate a starter definition file.
    Init(InitArgs),

    /// Manage the local build cache.
    Cache(CacheArgs),

    /// Print an example definition file.
    Example,
}

#[derive(clap::Args, Debug)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommands,
}

#[derive(Subcommand, Debug)]
pub enum CacheCommands {
    /// Remove stage snapshots and build entries older than a given age.
    Prune {
        /// Remove entries older than this many days (default: 30).
        #[arg(long, default_value = "30")]
        older_than: u64,
    },
    /// Remove all cached build entries and stage snapshots.
    Clear,
    /// Show cache location and disk usage.
    Info,
}

#[derive(clap::Args, Debug)]
pub struct BuildArgs {
    /// Path to the build definition YAML file.
    pub definition: PathBuf,

    /// Directory to write the OCI image layout into.
    #[arg(short, long, default_value = "./oci-output")]
    pub output: PathBuf,

    /// Keep the Incus container after the build (useful for debugging).
    #[arg(long)]
    pub keep_container: bool,

    /// Override image tag (default: today's date stamp YYYYMMDD).
    #[arg(long)]
    pub tag: Option<String>,

    /// Output format: oci-dir (default), oci-archive, docker-archive.
    ///
    /// oci-dir      — OCI Image Layout directory (default)
    /// oci-archive  — OCI Image Layout packed into a single .tar file
    /// docker-archive — Docker save-compatible tar (docker load / podman load)
    #[arg(long, default_value = "oci-dir")]
    pub output_format: String,

    /// Target platform(s) for the build, comma-separated (e.g. linux/amd64,linux/arm64).
    ///
    /// When a single platform is given, the build runs for that arch.
    /// When multiple platforms are given, the build runs once per arch and
    /// the results are merged into a multi-arch OCI image index.
    /// Defaults to the host architecture when not set.
    #[arg(long)]
    pub platform: Option<String>,

    /// Bypass the build cache and always run the full pipeline.
    #[arg(long)]
    pub no_cache: bool,

    /// Push the built image to this registry, overriding oci.registry in the
    /// definition file (e.g. registry.example.com or ghcr.io/myorg).
    /// The image name and tag come from the definition (or --tag).
    #[arg(long)]
    pub push: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ValidateArgs {
    /// Path to the build definition YAML file.
    pub definition: PathBuf,

    /// Skip pre-flight checks (Incus socket, host tools, registry reachability).
    /// Useful in CI environments where Incus is not available.
    #[arg(long)]
    pub skip_preflight: bool,
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Output path for the generated definition file.
    /// Defaults to `./image.yaml`. Use `-` to write to stdout.
    #[arg(default_value = "image.yaml")]
    pub output: String,

    /// OS distribution (e.g. ubuntu, fedora, debian, alpine).
    #[arg(long, default_value = "ubuntu")]
    pub distribution: String,

    /// Distribution release (e.g. noble, 40, bookworm, 3.19).
    #[arg(long, default_value = "noble")]
    pub release: String,

    /// Package manager (apt, dnf, apk, pacman, zypper, xbps).
    #[arg(long)]
    pub manager: Option<String>,

    /// Incus source image (e.g. images:ubuntu/noble).
    /// Defaults to images:<distribution>/<release>.
    #[arg(long)]
    pub image: Option<String>,

    /// OCI registry to push to after build.
    #[arg(long)]
    pub registry: Option<String>,

    /// Overwrite the output file if it already exists.
    #[arg(long)]
    pub force: bool,
}
