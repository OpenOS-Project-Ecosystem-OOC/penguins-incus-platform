// The library crate exposes builder, definition, incus, and oci.
// main.rs only needs the CLI module (not part of the public library API).
mod cli;

use anyhow::{Context, Result};
use clap::Parser;

use cli::{CacheCommands, Cli, Commands, InitArgs};
use incus_oci_builder::arch::parse_platforms;
use incus_oci_builder::builder;
use incus_oci_builder::definition;
use incus_oci_builder::oci::convert::OutputFormat;
use incus_oci_builder::preflight;
use incus_oci_builder::progress;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise logging with indicatif progress bars.
    // RUST_LOG overrides --log-level.
    progress::init_logging(&cli.log_level);

    match cli.command {
        Commands::Build(args) => {
            let mut def = definition::Definition::from_file(&args.definition)?;

            // CLI --tag overrides the definition's tag field.
            if let Some(tag) = args.tag {
                def.image.tag = tag;
            }

            let output_format: OutputFormat = args
                .output_format
                .parse()
                .context("invalid --output-format")?;

            let platforms = match args.platform {
                Some(ref s) => parse_platforms(s).context("invalid --platform")?,
                None => Vec::new(),
            };

            let opts = builder::BuildOptions {
                output_dir: args.output,
                keep_container: args.keep_container,
                output_format,
                platforms,
                no_cache: args.no_cache,
                push_registry: args.push,
            };

            let mut bp = progress::BuildProgress::new(&def.effective_name(), &def.effective_tag());

            match builder::build(&def, &opts).await {
                Ok(()) => bp.finish("done"),
                Err(e) => {
                    bp.fail(&e.to_string());
                    return Err(e);
                }
            }
        }

        Commands::Validate(args) => {
            let def = definition::Definition::from_file(&args.definition)?;

            if args.skip_preflight {
                println!(
                    "✅ {} — definition is valid (pre-flight skipped)",
                    args.definition.display()
                );
            } else {
                preflight::run(&def).await?;
                println!("✅ {} — definition is valid", args.definition.display());
            }

            println!("   image: {}:{}", def.effective_name(), def.effective_tag());
        }

        Commands::Init(args) => {
            run_init(args)?;
        }

        Commands::Cache(args) => match args.command {
            CacheCommands::Prune { older_than } => {
                let pruned = incus_oci_builder::cache::prune_stage_cache(older_than)
                    .context("pruning stage cache")?;
                let removed = incus_oci_builder::cache::prune_entries(older_than)
                    .context("pruning build cache entries")?;
                println!(
                    "✅ pruned {pruned} stage snapshot(s) and {removed} build entry/entries \
                     older than {older_than} day(s)"
                );
            }
            CacheCommands::Clear => {
                incus_oci_builder::cache::clear_all().context("clearing cache")?;
                println!("✅ cache cleared");
            }
            CacheCommands::Info => {
                let info = incus_oci_builder::cache::cache_info().context("reading cache info")?;
                println!("Cache location : {}", info.path.display());
                println!("Build entries  : {}", info.entry_count);
                println!("Stage snapshots: {}", info.stage_count);
                println!("Disk usage     : {}", format_bytes(info.total_bytes));
            }
        },

        Commands::Example => {
            print!("{}", EXAMPLE_DEFINITION);
        }
    }

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    while unit_idx + 1 < units.len() && value >= 1024.0 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit_idx])
    }
}

// ── init subcommand ───────────────────────────────────────────────────────────

fn run_init(args: InitArgs) -> Result<()> {
    let image_ref = args
        .image
        .unwrap_or_else(|| format!("images:{}/{}", args.distribution, args.release));

    let registry_line = match &args.registry {
        Some(r) => format!("  registry: {r}\n"),
        None => "  # registry: registry.example.com\n".to_string(),
    };

    // Infer a sensible default package manager from the distribution.
    let manager = args.manager.unwrap_or_else(|| {
        match args.distribution.as_str() {
            "ubuntu" | "debian" | "linuxmint" | "pop" => "apt",
            "fedora" | "rhel" | "centos" | "rocky" | "alma" => "dnf",
            "alpine" => "apk",
            "arch" | "manjaro" | "endeavouros" => "pacman",
            "opensuse" | "suse" => "zypper",
            "void" => "xbps",
            _ => "apt",
        }
        .to_string()
    });

    let content = format!(
        r#"# incus-oci-builder definition
# Generated by: incus-oci-builder init
# Reference:    docs/definition.md

image:
  distribution: {dist}
  release: {release}
  # architecture: x86_64   # defaults to host arch
  # name: myorg/{dist}-{release}
  # tag: "1.0"

source:
  downloader: incus
  image: "{image_ref}"

packages:
  manager: {manager}
  update: true
  cleanup: true
  sets:
    - action: install
      packages:
        - ca-certificates

actions:
  - trigger: post-packages
    action: |
      # Remove docs to reduce image size
      find /usr/share/doc -type f -delete 2>/dev/null || true

files:
  - generator: hostname
    content: "{dist}-{release}"
  - generator: hosts

oci:
{registry_line}  cmd: ["/bin/sh"]
  labels:
    org.opencontainers.image.description: "{dist} {release} built with incus-oci-builder"
"#,
        dist = args.distribution,
        release = args.release,
        image_ref = image_ref,
        manager = manager,
        registry_line = registry_line,
    );

    if args.output == "-" {
        print!("{content}");
        return Ok(());
    }

    let path = std::path::Path::new(&args.output);
    if path.exists() && !args.force {
        anyhow::bail!("{} already exists — use --force to overwrite", args.output);
    }

    std::fs::write(path, content.as_bytes()).with_context(|| format!("writing {}", args.output))?;

    println!("✅ {} written", args.output);
    println!(
        "   Edit it, then run: incus-oci-builder build {}",
        args.output
    );
    Ok(())
}

const EXAMPLE_DEFINITION: &str = r#"# incus-oci-builder definition example
#
# Build a minimal Ubuntu Noble OCI image using an Incus container.

image:
  distribution: ubuntu
  release: noble
  architecture: x86_64
  description: "Ubuntu 24.04 LTS (Noble Numbat)"
  # name and tag are optional; defaults to distribution/release and YYYYMMDD
  # name: myorg/ubuntu-noble
  # tag: "24.04"

source:
  downloader: incus
  image: "images:ubuntu/noble"

packages:
  manager: apt
  update: true
  cleanup: true
  sets:
    - action: install
      packages:
        - ca-certificates
        - curl
    - action: remove
      packages:
        - snapd

actions:
  - trigger: post-unpack
    action: |
      echo "Container is ready"
  - trigger: post-packages
    action: |
      # Remove docs to reduce image size
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
  # registry: registry.example.com
  cmd: ["/bin/bash"]
  labels:
    org.opencontainers.image.vendor: "My Org"
"#;
