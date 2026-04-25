//! Architecture detection and normalisation.
//!
//! Provides:
//! - `host_arch()` — detect the current host architecture in Incus/OCI notation
//! - `parse_platforms()` — parse `linux/amd64,linux/arm64` platform strings
//! - `incus_arch()` / `oci_arch()` — normalise arch strings between naming schemes

/// Detect the host CPU architecture and return it in Incus notation
/// (e.g. "x86_64", "aarch64").
pub fn host_arch() -> String {
    // std::env::consts::ARCH uses Rust's naming (x86_64, aarch64, arm, …)
    // which already matches Incus notation for the common cases.
    match std::env::consts::ARCH {
        "x86_64" => "x86_64".to_string(),
        "aarch64" => "aarch64".to_string(),
        "arm" => "armv7l".to_string(),
        "x86" => "i686".to_string(),
        "powerpc64" => "ppc64le".to_string(),
        "s390x" => "s390x".to_string(),
        "riscv64" => "riscv64".to_string(),
        other => other.to_string(),
    }
}

/// A parsed `os/arch` platform specifier (e.g. `linux/amd64`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

impl Platform {
    /// Return the architecture in Incus notation.
    pub fn incus_arch(&self) -> String {
        oci_to_incus(&self.arch)
    }

    /// Return the architecture in OCI notation.
    pub fn oci_arch(&self) -> String {
        incus_to_oci(&self.arch)
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.os, self.arch)
    }
}

/// Parse a comma-separated list of `os/arch` platform strings.
///
/// ```
/// # use incus_oci_builder::arch::parse_platforms;
/// let platforms = parse_platforms("linux/amd64,linux/arm64").unwrap();
/// assert_eq!(platforms.len(), 2);
/// assert_eq!(platforms[0].arch, "amd64");
/// ```
pub fn parse_platforms(s: &str) -> anyhow::Result<Vec<Platform>> {
    s.split(',')
        .map(|p| {
            let p = p.trim();
            let (os, arch) = p
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("invalid platform {p:?}: expected os/arch"))?;
            Ok(Platform {
                os: os.to_string(),
                arch: arch.to_string(),
            })
        })
        .collect()
}

/// Convert an OCI architecture name to Incus notation.
pub fn oci_to_incus(arch: &str) -> String {
    match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        "arm" => "armv7l",
        "386" => "i686",
        "ppc64le" => "ppc64le",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        other => other,
    }
    .to_string()
}

/// Convert an Incus architecture name to OCI notation.
pub fn incus_to_oci(arch: &str) -> String {
    match arch {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        "armv7l" | "armhf" => "arm",
        "i686" | "i386" => "386",
        "ppc64le" => "ppc64le",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_arch_is_non_empty() {
        assert!(!host_arch().is_empty());
    }

    #[test]
    fn parse_single_platform() {
        let platforms = parse_platforms("linux/amd64").unwrap();
        assert_eq!(platforms.len(), 1);
        assert_eq!(platforms[0].os, "linux");
        assert_eq!(platforms[0].arch, "amd64");
    }

    #[test]
    fn parse_multi_platform() {
        let platforms = parse_platforms("linux/amd64,linux/arm64").unwrap();
        assert_eq!(platforms.len(), 2);
        assert_eq!(platforms[1].arch, "arm64");
    }

    #[test]
    fn invalid_platform_is_error() {
        assert!(parse_platforms("amd64").is_err());
    }

    #[test]
    fn oci_incus_roundtrip() {
        for arch in [
            "amd64", "arm64", "arm", "386", "ppc64le", "s390x", "riscv64",
        ] {
            let incus = oci_to_incus(arch);
            let back = incus_to_oci(&incus);
            assert_eq!(back, arch, "roundtrip failed for {arch}");
        }
    }

    #[test]
    fn platform_display() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "amd64".to_string(),
        };
        assert_eq!(p.to_string(), "linux/amd64");
    }
}
