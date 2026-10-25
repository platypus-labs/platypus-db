//! The driver registry: an `index.yaml` listing each driver's versions and, per platform,
//! the package to download.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use platypus_core::{ConnectionConfig, DriverKind, SslMode};
use serde::Deserialize;

use crate::{DriverError, PUBLIC_REGISTRY, drivers_dir, io_error, platform};

/// A package of a driver, ready to download.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Orders `v1.10.0` after `v1.9.0`: numbers compared as numbers, the rest as text.
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    parts(a).cmp(&parts(b)).then_with(|| a.cmp(b))
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
        let mut config = ConnectionConfig::new(DriverKind::Adbc);
        config.ssl_mode = SslMode::VerifyFull;
        // reqwest has no crypto provider of its own: it needs a configuration even for http.
        let tls = if self.url.starts_with("https:") {
            platypus_tls::client_config(&config)
                .map_err(|e| DriverError::Network(e.to_string()))?
                .ok_or_else(|| DriverError::Network("no TLS configuration".to_owned()))?
        } else {
            platypus_tls::plaintext_config().map_err(|e| DriverError::Network(e.to_string()))?
        };
        reqwest::Client::builder()
            .user_agent(concat!("platypus-db/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .tls_backend_preconfigured(tls)
            .build()
            .map_err(|e| DriverError::Network(e.to_string()))
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
        let network = |e: reqwest::Error| DriverError::Network(e.to_string());
        self.http()?
            .get(format!("{}/index.yaml", self.url))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(network)?
            .text()
            .await
            .map_err(network)
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
            sha256: package
                .hash
                .and_then(|h| h.trim().strip_prefix("sha256:").map(str::to_owned)),
        })
    }
}
