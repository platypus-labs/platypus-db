//! The driver registry: an `index.yaml` listing each driver's versions and, per platform,
//! the package to download.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use platypus_core::{ConnectionConfig, DriverKind, SslMode};
use serde::{Deserialize, Serialize};

use crate::{DriverError, PUBLIC_REGISTRY, drivers_dir, io_error, network_error, platform};

/// A package of a driver, ready to download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    /// Its name in the registry, e.g. `snowflake`.
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    pub url: String,
    pub sha256: Option<String>,
}

#[derive(Debug)]
pub struct Registry {
    pub(crate) url: String,
    pub(crate) data_dir: PathBuf,
}

#[derive(Deserialize)]
struct Index {
    drivers: Vec<IndexDriver>,
}

#[derive(Deserialize)]
struct IndexDriver {
    name: String,
    license: String,
    path: String,
    #[serde(default)]
    pkginfo: Vec<IndexVersion>,
}

#[derive(Deserialize)]
struct IndexVersion {
    version: String,
    #[serde(default)]
    packages: Vec<IndexPackage>,
}

#[derive(Deserialize)]
struct IndexPackage {
    platform: String,
    url: String,
    /// `sha256:<hex>`.
    #[serde(default)]
    hash: Option<String>,
}

/// An HTTP client that names the app, verifying certificates when `https`.
pub(crate) fn client(https: bool) -> Result<reqwest::Client, DriverError> {
    let mut config = ConnectionConfig::new(DriverKind::Adbc);
    config.ssl_mode = SslMode::VerifyFull;
    // reqwest has no crypto provider of its own: it needs a configuration even for http.
    let tls = if https {
        platypus_tls::client_config(&config)
            .map_err(network_error)?
            .ok_or_else(|| DriverError::Network("no TLS configuration".to_owned()))?
    } else {
        platypus_tls::plaintext_config().map_err(network_error)?
    };
    reqwest::Client::builder()
        .user_agent(concat!("platypus-db/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .tls_backend_preconfigured(tls)
        .build()
        .map_err(network_error)
}

/// Orders versions like semantic versions: `v1.10.0` after `v1.9.0` (numbers compared as
/// numbers), and a release after its prereleases (`1.0.0` after `1.0.0-beta`). Build
/// metadata (`+…`) only breaks ties.
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    /// The release numbers and the prerelease identifiers, if any.
    fn parts(v: &str) -> (Vec<u64>, Option<Vec<&str>>) {
        let v = v.trim_start_matches('v');
        let v = v.split_once('+').map_or(v, |(v, _)| v);
        let (release, pre) = match v.split_once('-') {
            Some((release, pre)) => (release, Some(pre.split('.').collect())),
            None => (v, None),
        };
        let numbers = release.split('.').map_while(|p| p.parse().ok()).collect();
        (numbers, pre)
    }
    /// Numeric identifiers sort below text ones, and by value among themselves.
    fn identifier(a: &str, b: &str) -> Ordering {
        match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        }
    }
    let ((release_a, pre_a), (release_b, pre_b)) = (parts(a), parts(b));
    release_a
        .cmp(&release_b)
        .then_with(|| match (pre_a, pre_b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(pa), Some(pb)) => pa
                .iter()
                .zip(&pb)
                .map(|(x, y)| identifier(x, y))
                .find(|o| o.is_ne())
                .unwrap_or_else(|| pa.len().cmp(&pb.len())),
        })
        .then_with(|| a.cmp(b))
}

/// The hex digits of an index hash, `sha256:<hex>` (the prefix in any case).
pub(crate) fn sha256_hex(hash: &str) -> Option<String> {
    let hash = hash.trim();
    let prefix = hash.get(..7)?;
    prefix
        .eq_ignore_ascii_case("sha256:")
        .then(|| hash[7..].trim().to_owned())
        .filter(|hex| !hex.is_empty())
}

impl Registry {
    /// The public registry, with its index cached under `data_dir`.
    #[must_use]
    pub fn public(data_dir: &Path) -> Self {
        Self::with_url(PUBLIC_REGISTRY, data_dir)
    }

    #[must_use]
    pub fn with_url(url: &str, data_dir: &Path) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            data_dir: data_dir.to_owned(),
        }
    }

    fn cache(&self) -> PathBuf {
        drivers_dir(&self.data_dir).join("index.yaml")
    }

    /// An HTTP client that names the app, as the registry asks of tools other than `dbc`.
    pub(crate) fn http(&self) -> Result<reqwest::Client, DriverError> {
        client(self.url.starts_with("https:"))
    }

    /// The index from the registry, saved as the cache; the cached one when the registry
    /// cannot be reached.
    async fn index(&self) -> Result<Index, DriverError> {
        let text = match self.fetch_index().await {
            Ok(text) => {
                let cache = self.cache();
                if let Some(dir) = cache.parent() {
                    tokio::fs::create_dir_all(dir)
                        .await
                        .map_err(io_error(dir))?;
                }
                tokio::fs::write(&cache, &text)
                    .await
                    .map_err(io_error(&cache))?;
                text
            }
            Err(e) => tokio::fs::read_to_string(self.cache())
                .await
                .map_err(|_| e)?,
        };
        serde_yaml_ng::from_str(&text).map_err(|e| DriverError::Format(e.to_string()))
    }

    async fn fetch_index(&self) -> Result<String, DriverError> {
        self.http()?
            .get(format!("{}/index.yaml", self.url))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(network_error)?
            .text()
            .await
            .map_err(network_error)
    }

    /// The newest package of driver `name` for this platform.
    pub async fn package(&self, name: &str) -> Result<Package, DriverError> {
        let index = self.index().await?;
        let no_package = || DriverError::NoPackage {
            name: name.to_owned(),
            platform: platform().to_owned(),
        };
        let driver = index
            .drivers
            .into_iter()
            .find(|d| d.path == name)
            .ok_or_else(no_package)?;
        // Licenses that are not free (`LicenseRef-…`) come with terms to accept elsewhere.
        if driver.license.starts_with("LicenseRef-") {
            return Err(DriverError::License(driver.license));
        }
        let (version, package) = driver
            .pkginfo
            .into_iter()
            .filter_map(|v| {
                let p = v.packages.into_iter().find(|p| p.platform == platform())?;
                Some((v.version, p))
            })
            .max_by(|(a, _), (b, _)| compare_versions(a, b))
            .ok_or_else(no_package)?;
        Ok(Package {
            name: name.to_owned(),
            title: driver.name,
            version,
            license: driver.license,
            url: format!("{}/{}", self.url, package.url.trim_start_matches('/')),
            sha256: package.hash.as_deref().and_then(sha256_hex),
        })
    }
}
