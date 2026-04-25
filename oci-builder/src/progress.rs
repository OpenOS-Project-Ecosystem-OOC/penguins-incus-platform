//! Structured build progress reporting.
//!
//! Wraps `indicatif` multi-progress bars with a `tracing-indicatif` layer so
//! that `tracing` log messages are printed above the active progress bars
//! rather than interleaving with them.
//!
//! Usage:
//! ```no_run
//! use incus_oci_builder::progress::{init_logging, BuildProgress};
//!
//! init_logging("info");
//! let bp = BuildProgress::new("ubuntu/noble", "20240101");
//! bp.stage("packages");
//! // … do work …
//! bp.finish("build complete");
//! ```

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

// ── Logging initialisation ────────────────────────────────────────────────────

/// Initialise the global tracing subscriber with an `indicatif` layer.
///
/// Log messages are printed above any active progress bars. `RUST_LOG`
/// overrides `log_level`.
pub fn init_logging(log_level: &str) {
    let indicatif_layer = IndicatifLayer::new();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();
}

// ── Build progress ────────────────────────────────────────────────────────────

/// Named build stages in pipeline order.
pub const STAGES: &[&str] = &[
    "preflight",
    "bootstrap",
    "post-unpack",
    "packages",
    "post-packages",
    "files",
    "post-files",
    "export",
    "commit",
    "push",
];

/// A multi-bar progress display for a single build.
pub struct BuildProgress {
    multi: MultiProgress,
    /// The overall build spinner (top-level bar).
    overall: ProgressBar,
    /// The current stage spinner (nested bar).
    stage_bar: Option<ProgressBar>,
}

impl BuildProgress {
    /// Create a new `BuildProgress` for the given image.
    pub fn new(image_name: &str, image_tag: &str) -> Self {
        let multi = MultiProgress::new();

        let overall_style = ProgressStyle::with_template("{spinner:.cyan} {prefix:.bold} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]);

        let overall = multi.add(ProgressBar::new_spinner());
        overall.set_style(overall_style);
        overall.set_prefix(format!("{image_name}:{image_tag}"));
        overall.set_message("starting");
        overall.enable_steady_tick(std::time::Duration::from_millis(80));

        Self {
            multi,
            overall,
            stage_bar: None,
        }
    }

    /// Advance to a named build stage.
    pub fn stage(&mut self, name: &str) {
        // Finish the previous stage bar if any.
        if let Some(prev) = self.stage_bar.take() {
            prev.finish_and_clear();
        }

        let stage_style = ProgressStyle::with_template("  {spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]);

        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(stage_style);
        bar.set_message(name.to_string());
        bar.enable_steady_tick(std::time::Duration::from_millis(80));

        self.overall.set_message(name.to_string());
        self.stage_bar = Some(bar);
    }

    /// Mark the current stage as done with a status message.
    pub fn stage_done(&mut self, msg: &str) {
        if let Some(bar) = self.stage_bar.take() {
            bar.finish_with_message(format!("✓ {msg}"));
        }
    }

    /// Mark the overall build as complete.
    pub fn finish(&mut self, msg: &str) {
        if let Some(bar) = self.stage_bar.take() {
            bar.finish_and_clear();
        }
        self.overall.finish_with_message(format!("✓ {msg}"));
    }

    /// Mark the overall build as failed.
    pub fn fail(&mut self, msg: &str) {
        if let Some(bar) = self.stage_bar.take() {
            bar.finish_and_clear();
        }
        self.overall.finish_with_message(format!("✗ {msg}"));
    }
}

impl Drop for BuildProgress {
    fn drop(&mut self) {
        // Ensure bars are cleaned up even if finish/fail wasn't called.
        if let Some(bar) = self.stage_bar.take() {
            bar.finish_and_clear();
        }
    }
}
