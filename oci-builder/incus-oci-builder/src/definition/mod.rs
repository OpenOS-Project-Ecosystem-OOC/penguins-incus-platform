//! Build definition types.
//!
//! The definition file is a YAML document that describes how to build a rootfs
//! and what OCI image to produce from it. The schema is intentionally close to
//! distrobuilder's so that existing definition files can be reused with minimal
//! changes.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Image ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDef {
    pub distribution: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub description: String,
    /// OCI image name written into the manifest (e.g. "ubuntu/noble")
    #[serde(default)]
    pub name: String,
    /// OCI image tag (defaults to the serial / date stamp)
    #[serde(default)]
    pub tag: String,
}

// ── Source ───────────────────────────────────────────────────────────────────

/// How the base rootfs is obtained before the Incus container is started.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Downloader {
    /// Pull a base image from an Incus image server and use it as the starting
    /// point (e.g. `images:ubuntu/noble`).
    Incus,
    /// Bootstrap via debootstrap (Debian/Ubuntu).
    Debootstrap,
    /// Bootstrap via dnf/rpm (Fedora/RHEL/etc.).
    Rpmbootstrap,
    /// Pull a rootfs tarball over HTTP.
    RootfsHttp,
}

/// Authentication configuration for the `rootfs-http` downloader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuth {
    /// HTTP Bearer token authentication (`Authorization: Bearer <token>`).
    /// The token value can reference an environment variable with `$VAR` syntax.
    Bearer { token: String },
    /// HTTP Basic authentication (`Authorization: Basic <base64>`).
    /// Passwords can reference environment variables with `$VAR` syntax.
    Basic { username: String, password: String },
    /// Arbitrary header injection (e.g. `X-Auth-Token: <value>`).
    /// The value can reference environment variables with `$VAR` syntax.
    Header { name: String, value: String },
}

impl HttpAuth {
    /// Resolve any `$VAR` or `${VAR}` references in credential strings.
    pub fn resolve_env(&self) -> Self {
        match self {
            Self::Bearer { token } => Self::Bearer {
                token: expand_env(token),
            },
            Self::Basic { username, password } => Self::Basic {
                username: expand_env(username),
                password: expand_env(password),
            },
            Self::Header { name, value } => Self::Header {
                name: name.clone(),
                value: expand_env(value),
            },
        }
    }
}

/// Expand `$VAR` and `${VAR}` references in `s` using the process environment.
fn expand_env(s: &str) -> String {
    // Simple single-pass expansion: find `$` followed by identifier chars or `{…}`.
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            result.push(c);
            continue;
        }
        // Collect the variable name.
        let braced = chars.peek() == Some(&'{');
        if braced {
            chars.next(); // consume '{'
        }
        let var_name: String = chars
            .by_ref()
            .take_while(|&ch| {
                if braced {
                    ch != '}'
                } else {
                    ch.is_alphanumeric() || ch == '_'
                }
            })
            .collect();
        let val = std::env::var(&var_name).unwrap_or_default();
        result.push_str(&val);
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDef {
    pub downloader: Downloader,
    /// Remote image reference for the `incus` downloader, e.g. `images:ubuntu/noble`.
    #[serde(default)]
    pub image: String,
    /// URL used by HTTP-based downloaders.
    #[serde(default)]
    pub url: String,
    /// Optional checksum for the `rootfs-http` downloader (`sha256:<hex>`).
    /// The download is rejected if the digest does not match.
    #[serde(default)]
    pub checksum: String,
    /// Suite/release passed to debootstrap.
    #[serde(default)]
    pub suite: String,
    /// Components passed to debootstrap (e.g. `["main", "universe"]`).
    #[serde(default)]
    pub components: Vec<String>,
    /// Seed packages for the `rpmbootstrap` downloader.
    /// Defaults to `["basesystem", "bash", "coreutils", "dnf"]` when empty.
    #[serde(default)]
    pub seed_packages: Vec<String>,
    /// Optional authentication for the `rootfs-http` downloader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_auth: Option<HttpAuth>,
}

// ── Packages ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageAction {
    Install,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSet {
    pub action: PackageAction,
    pub packages: Vec<String>,
    /// Only apply this set on matching releases.
    #[serde(default)]
    pub releases: Vec<String>,
    /// Only apply this set on matching architectures.
    #[serde(default)]
    pub architectures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagesDef {
    /// Package manager to use inside the container (apt, dnf, apk, pacman, …).
    #[serde(default)]
    pub manager: String,
    /// Run a full upgrade before installing packages.
    #[serde(default)]
    pub update: bool,
    /// Remove package manager caches after installation.
    #[serde(default = "default_true")]
    pub cleanup: bool,
    #[serde(default)]
    pub sets: Vec<PackageSet>,
    /// Extra repositories to add before installing packages.
    #[serde(default)]
    pub repositories: Vec<Repository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub key: String,
}

// ── Actions ──────────────────────────────────────────────────────────────────

/// When a custom action script is executed relative to the build lifecycle.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ActionTrigger {
    /// After the base rootfs is unpacked into the container.
    PostUnpack,
    /// After package installation.
    PostPackages,
    /// After file generators have run.
    PostFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub trigger: ActionTrigger,
    /// Shell script executed via `incus exec` inside the build container.
    pub action: String,
    #[serde(default)]
    pub releases: Vec<String>,
    #[serde(default)]
    pub architectures: Vec<String>,
}

// ── Files ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileGenerator {
    /// Write literal content to a path.
    Dump,
    /// Copy a file from the host into the container.
    Copy,
    /// Remove a path from the container.
    Remove,
    /// Write /etc/hostname.
    Hostname,
    /// Write /etc/hosts.
    Hosts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDef {
    pub generator: FileGenerator,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    /// Host-side source path (used by the `copy` generator).
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub mode: String,
}

// ── OCI output ───────────────────────────────────────────────────────────────

/// Controls how the final OCI image is assembled from the container rootfs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciDef {
    /// Registry to push the finished image to (e.g. `registry.example.com`).
    /// Leave empty to only write to a local OCI layout directory.
    #[serde(default)]
    pub registry: String,
    /// Additional labels written into the OCI image config.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Default command for the image (OCI `Cmd`).
    #[serde(default)]
    pub cmd: Vec<String>,
    /// Default entrypoint for the image (OCI `Entrypoint`).
    #[serde(default)]
    pub entrypoint: Vec<String>,
    /// Ports to expose (informational, written into image config).
    #[serde(default)]
    pub exposed_ports: Vec<String>,
    /// Whether to capture intermediate Incus snapshots as separate OCI layers.
    /// Enables layer caching but requires more disk space during the build.
    #[serde(default)]
    pub layered: bool,
}

// ── Top-level definition ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub image: ImageDef,
    pub source: SourceDef,
    #[serde(default)]
    pub packages: Option<PackagesDef>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub files: Vec<FileDef>,
    #[serde(default)]
    pub oci: Option<OciDef>,
}

impl Definition {
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading definition file {}", path.display()))?;
        let def: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("parsing definition file {}", path.display()))?;
        def.validate()?;
        Ok(def)
    }

    pub fn validate(&self) -> Result<()> {
        if self.image.distribution.trim().is_empty() {
            anyhow::bail!("image.distribution must not be empty");
        }
        if let Some(pkgs) = &self.packages {
            if pkgs.manager.trim().is_empty() {
                anyhow::bail!(
                    "packages.manager must not be empty when packages section is present"
                );
            }
        }
        Ok(())
    }

    /// Effective image tag: explicit tag, or today's date stamp.
    pub fn effective_tag(&self) -> String {
        if !self.image.tag.is_empty() {
            self.image.tag.clone()
        } else {
            chrono::Utc::now().format("%Y%m%d").to_string()
        }
    }

    /// Effective image name: explicit name, or `distribution/release`.
    pub fn effective_name(&self) -> String {
        if !self.image.name.is_empty() {
            self.image.name.clone()
        } else if !self.image.release.is_empty() {
            format!("{}/{}", self.image.distribution, self.image.release)
        } else {
            self.image.distribution.clone()
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_definition() {
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: incus
  image: "images:ubuntu/noble"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.image.distribution, "ubuntu");
        assert_eq!(def.image.release, "noble");
        def.validate().unwrap();
    }

    #[test]
    fn effective_name_fallback() {
        let yaml = r#"
image:
  distribution: alpine
  release: "3.19"
source:
  downloader: incus
  image: "images:alpine/3.19"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.effective_name(), "alpine/3.19");
    }

    // ── http_auth parsing ─────────────────────────────────────────────────────

    #[test]
    fn http_auth_bearer_parses() {
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
  http_auth:
    type: bearer
    token: "my-secret-token"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        match def.source.http_auth.unwrap() {
            HttpAuth::Bearer { token } => assert_eq!(token, "my-secret-token"),
            other => panic!("expected Bearer, got {other:?}"),
        }
    }

    #[test]
    fn http_auth_basic_parses() {
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
  http_auth:
    type: basic
    username: myuser
    password: mypass
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        match def.source.http_auth.unwrap() {
            HttpAuth::Basic { username, password } => {
                assert_eq!(username, "myuser");
                assert_eq!(password, "mypass");
            }
            other => panic!("expected Basic, got {other:?}"),
        }
    }

    #[test]
    fn http_auth_header_parses() {
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
  http_auth:
    type: header
    name: "X-Auth-Token"
    value: "tok123"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        match def.source.http_auth.unwrap() {
            HttpAuth::Header { name, value } => {
                assert_eq!(name, "X-Auth-Token");
                assert_eq!(value, "tok123");
            }
            other => panic!("expected Header, got {other:?}"),
        }
    }

    #[test]
    fn http_auth_absent_is_none() {
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        assert!(def.source.http_auth.is_none());
    }

    #[test]
    fn http_auth_bearer_env_expansion() {
        std::env::set_var("IOB_DEF_TEST_TOKEN", "expanded-value");
        let auth = HttpAuth::Bearer {
            token: "$IOB_DEF_TEST_TOKEN".to_string(),
        };
        match auth.resolve_env() {
            HttpAuth::Bearer { token } => assert_eq!(token, "expanded-value"),
            other => panic!("expected Bearer after resolve, got {other:?}"),
        }
        std::env::remove_var("IOB_DEF_TEST_TOKEN");
    }

    #[test]
    fn http_auth_basic_env_expansion() {
        std::env::set_var("IOB_DEF_TEST_PASS", "secret123");
        let auth = HttpAuth::Basic {
            username: "admin".to_string(),
            password: "${IOB_DEF_TEST_PASS}".to_string(),
        };
        match auth.resolve_env() {
            HttpAuth::Basic { username, password } => {
                assert_eq!(username, "admin");
                assert_eq!(password, "secret123");
            }
            other => panic!("expected Basic after resolve, got {other:?}"),
        }
        std::env::remove_var("IOB_DEF_TEST_PASS");
    }

    #[test]
    fn http_auth_header_env_expansion() {
        std::env::set_var("IOB_DEF_TEST_HDR", "hdr-value");
        let auth = HttpAuth::Header {
            name: "X-Custom".to_string(),
            value: "$IOB_DEF_TEST_HDR".to_string(),
        };
        match auth.resolve_env() {
            HttpAuth::Header { name, value } => {
                assert_eq!(name, "X-Custom");
                assert_eq!(value, "hdr-value");
            }
            other => panic!("expected Header after resolve, got {other:?}"),
        }
        std::env::remove_var("IOB_DEF_TEST_HDR");
    }

    #[test]
    fn http_auth_missing_env_var_expands_to_empty() {
        // Ensure the var is not set.
        std::env::remove_var("IOB_DEF_NONEXISTENT_VAR_XYZ");
        let auth = HttpAuth::Bearer {
            token: "$IOB_DEF_NONEXISTENT_VAR_XYZ".to_string(),
        };
        match auth.resolve_env() {
            HttpAuth::Bearer { token } => assert_eq!(token, ""),
            other => panic!("expected Bearer, got {other:?}"),
        }
    }

    #[test]
    fn http_auth_bearer_round_trips_yaml() {
        let auth = HttpAuth::Bearer {
            token: "tok".to_string(),
        };
        let yaml = serde_yaml::to_string(&auth).unwrap();
        let back: HttpAuth = serde_yaml::from_str(&yaml).unwrap();
        match back {
            HttpAuth::Bearer { token } => assert_eq!(token, "tok"),
            other => panic!("round-trip failed: {other:?}"),
        }
    }

    #[test]
    fn http_auth_basic_round_trips_yaml() {
        let auth = HttpAuth::Basic {
            username: "u".to_string(),
            password: "p".to_string(),
        };
        let yaml = serde_yaml::to_string(&auth).unwrap();
        let back: HttpAuth = serde_yaml::from_str(&yaml).unwrap();
        match back {
            HttpAuth::Basic { username, password } => {
                assert_eq!(username, "u");
                assert_eq!(password, "p");
            }
            other => panic!("round-trip failed: {other:?}"),
        }
    }

    #[test]
    fn http_auth_header_round_trips_yaml() {
        let auth = HttpAuth::Header {
            name: "X-Foo".to_string(),
            value: "bar".to_string(),
        };
        let yaml = serde_yaml::to_string(&auth).unwrap();
        let back: HttpAuth = serde_yaml::from_str(&yaml).unwrap();
        match back {
            HttpAuth::Header { name, value } => {
                assert_eq!(name, "X-Foo");
                assert_eq!(value, "bar");
            }
            other => panic!("round-trip failed: {other:?}"),
        }
    }

    #[test]
    fn definition_with_http_auth_serialises_without_auth_when_absent() {
        // When http_auth is None, it must not appear in the serialised YAML
        // (skip_serializing_if = "Option::is_none").
        let yaml = r#"
image:
  distribution: ubuntu
  release: noble
source:
  downloader: rootfs-http
  url: "https://example.com/rootfs.tar.gz"
"#;
        let def: Definition = serde_yaml::from_str(yaml).unwrap();
        let out = serde_yaml::to_string(&def).unwrap();
        assert!(
            !out.contains("http_auth"),
            "http_auth should not appear in output when absent, got:\n{out}"
        );
    }
}
