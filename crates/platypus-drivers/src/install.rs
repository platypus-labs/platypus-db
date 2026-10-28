//! Downloading a package, checking it and unpacking it where drivers are kept. Nothing reaches
//! the driver's folder until the package is whole and checked.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::{
    DriverError, Installed, Package, REMOVED_MARK, Registry, adbc_dir, drivers_dir, io_error,
    is_plain_name,
};

/// Names of the folders a download unpacks into before it is moved in place.
pub(crate) const TMP_PREFIX: &str = ".tmp-";

/// Far more than any driver library; a bigger file in a package is refused, not unpacked.
pub(crate) const MAX_FILE: u64 = 512 << 20;

/// The package's own description of its files.
#[derive(Deserialize)]
struct Manifest {
    #[serde(rename = "Files")]
    files: ManifestFiles,
}

#[derive(Deserialize)]
struct ManifestFiles {
    driver: String,
}

impl Registry {
    /// Downloads `package`, checks its SHA-256, unpacks it and records it. `progress` gets
    /// the bytes received and the total when known. A package without a SHA-256 is refused:
    /// nothing would vouch for the library loaded into the app.
    pub async fn install(
        &self,
        package: &Package,
        progress: impl Fn(u64, Option<u64>),
        cancel: &CancellationToken,
    ) -> Result<Installed, DriverError> {
        let Some(expected) = &package.sha256 else {
            return Err(DriverError::NoChecksum);
        };
        for name in [&package.name, &package.version] {
            if !is_plain_name(name) {
                return Err(DriverError::InvalidName(name.clone()));
            }
        }
        let bytes = download(&self.http()?, &package.url, progress, cancel).await?;
        check(&bytes, expected)?;
        let tmp = drivers_dir(&self.data_dir).join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
        let package = package.clone();
        let target = adbc_dir(&self.data_dir).join(&package.name);
        tokio::task::spawn_blocking(move || {
            let result = unpack(&bytes, &tmp).and_then(|library| {
                let record = Installed {
                    name: package.name.clone(),
                    title: package.title.clone(),
                    version: package.version.clone(),
                    license: package.license.clone(),
                    library: PathBuf::from(library),
                };
                place(&tmp, &target, record)
            });
            let _ = std::fs::remove_dir_all(&tmp);
            result
        })
        .await
        .map_err(|e| DriverError::Format(e.to_string()))?
    }
}

fn download_error(e: reqwest::Error) -> DriverError {
    DriverError::Download(e.to_string())
}

/// Downloads `url` whole; `progress` gets the bytes received and the total when known.
pub(crate) async fn download(
    client: &reqwest::Client,
    url: &str,
    progress: impl Fn(u64, Option<u64>),
    cancel: &CancellationToken,
) -> Result<Vec<u8>, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled);
    }
    let response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(download_error)?;
    let total = response.content_length();
    let mut bytes = Vec::with_capacity(usize::try_from(total.unwrap_or(0)).unwrap_or(0));
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            () = cancel.cancelled() => return Err(DriverError::Cancelled),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        bytes.extend_from_slice(&chunk.map_err(download_error)?);
        if bytes.len() as u64 > MAX_FILE {
            return Err(DriverError::TooLarge);
        }
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

/// Fails unless `bytes` have the SHA-256 `expected` (hex digits).
pub(crate) fn check(bytes: &[u8], expected: &str) -> Result<(), DriverError> {
    if hex(&Sha256::digest(bytes)).eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(DriverError::Checksum)
    }
}

/// Lower-case hex digits of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Unpacks the files at the top of a `.tar.gz` into `dir`; folders and links are skipped,
/// files larger than [`MAX_FILE`] fail the archive.
pub(crate) fn untar(bytes: &[u8], dir: &Path) -> Result<(), DriverError> {
    std::fs::create_dir_all(dir).map_err(io_error(dir))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let format = |e: std::io::Error| DriverError::Format(e.to_string());
    for entry in archive.entries().map_err(format)? {
        let mut entry = entry.map_err(format)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(format)?.into_owned();
        // Only plain names: a package cannot write outside its folder.
        let Some(name) = path.file_name().filter(|_| path.components().count() == 1) else {
            continue;
        };
        if entry.header().size().map_err(format)? > MAX_FILE {
            return Err(DriverError::TooLarge);
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE + 1)
            .read_to_end(&mut data)
            .map_err(format)?;
        if data.len() as u64 > MAX_FILE {
            return Err(DriverError::TooLarge);
        }
        let out = dir.join(name);
        std::fs::write(&out, data).map_err(io_error(&out))?;
    }
    Ok(())
}

/// Unpacks the `.tar.gz` into `dir`; returns the library's file name from `MANIFEST`.
fn unpack(bytes: &[u8], dir: &Path) -> Result<String, DriverError> {
    untar(bytes, dir)?;
    let manifest = dir.join("MANIFEST");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|_| DriverError::Format("the package has no MANIFEST".to_owned()))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|e| DriverError::Format(format!("MANIFEST: {e}")))?;
    let library = manifest.files.driver;
    if Path::new(&library).components().count() != 1 || !dir.join(&library).is_file() {
        return Err(DriverError::Format(format!(
            "the package lacks its library `{library}`"
        )));
    }
    Ok(library)
}

/// Records the unpacked package (`record.library` relative to `tmp`) and moves it to
/// `target/<version>`.
pub(crate) fn place(
    tmp: &Path,
    target: &Path,
    record: Installed,
) -> Result<Installed, DriverError> {
    let toml = toml::to_string(&record).map_err(|e| DriverError::Format(e.to_string()))?;
    let file = tmp.join("driver.toml");
    std::fs::write(&file, toml).map_err(io_error(&file))?;
    std::fs::create_dir_all(target).map_err(io_error(target))?;
    // Installed again after a removal that could not delete the files.
    let _ = std::fs::remove_file(target.join(REMOVED_MARK));
    let dir = target.join(&record.version);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(io_error(&dir))?;
    }
    std::fs::rename(tmp, &dir).map_err(io_error(&dir))?;
    Ok(Installed {
        library: dir.join(&record.library),
        ..record
    })
}
