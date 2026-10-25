//! Client libraries the app installs from their vendors' sites: Oracle's Instant Client, which
//! the native Oracle driver loads, and IBM's Data Server Driver for ODBC and CLI. The archives
//! are checked against the SHA-256 recorded here and unpacked under
//! `<data>/drivers/vendor/<name>/<version>/`; IBM's driver is then registered with the
//! system's ODBC driver manager by IBM's own program, which on Windows needs administrator
//! rights.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::install::{MAX_FILE, TMP_PREFIX, check, download};
use crate::registry::client;
use crate::{DriverError, Installed, drivers_dir, io_error, platform};

/// An archive of a package, as the vendor publishes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Archive {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// A program of the package, with its arguments, run from its own folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Program {
    /// Relative to the installed package's folder.
    pub path: String,
    pub args: Vec<String>,
}

/// A vendor's client library for this platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorPackage {
    /// `oracle` or `db2` (`DriverKind::vendor_package`).
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    /// The vendor's download page.
    pub vendor_url: String,
    /// Unpacked into the same folder, in order.
    pub archives: Vec<Archive>,
    /// The library, relative to the package's folder.
    pub library: String,
    /// Registers an ODBC driver with the system, as administrator; `None` for a library
    /// the app loads itself.
    pub register: Option<Program>,
    pub unregister: Option<Program>,
}

impl VendorPackage {
    /// The bytes to download.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.archives.iter().map(|a| a.size).sum()
    }

    /// The folder the library sits in, once installed as `installed`.
    #[must_use]
    pub fn library_dir(installed: &Installed) -> Option<&Path> {
        installed.library.parent()
    }
}

fn archive(url: String, sha256: &str, size: u64) -> Archive {
    Archive {
        url,
        sha256: sha256.to_owned(),
        size,
    }
}

fn program(path: &str, args: &[&str]) -> Program {
    Program {
        path: path.to_owned(),
        args: args.iter().map(|&a| a.to_owned()).collect(),
    }
}

/// Oracle Instant Client Basic Light: the client in English, with Unicode (AL32UTF8) data.
/// macOS gets it as a disk image, which is not unpacked here.
fn oracle() -> Option<VendorPackage> {
    const BASE: &str = "https://download.oracle.com/otn_software";
    let (archive, library) = match platform() {
        "windows_amd64" => (
            archive(
                format!(
                    "{BASE}/nt/instantclient/2326400/instantclient-basiclite-windows.x64-23.26.4.0.0.zip"
                ),
                "7586c02a2926f619a0b16a9ae32e85bb3605798aa177155b065fffb48a1ee0b8",
                60_406_433,
            ),
            "oci.dll",
        ),
        "linux_amd64" => (
            archive(
                format!(
                    "{BASE}/linux/instantclient/2326400/instantclient-basiclite-linux.x64-23.26.4.0.0.zip"
                ),
                "d049e593bba0041feb15c14ea894e2f1a437377462b99d9bdb6b7ab30b2dd660",
                76_612_271,
            ),
            "libclntsh.so",
        ),
        "linux_arm64" => (
            archive(
                format!(
                    "{BASE}/linux/instantclient/2326400/instantclient-basiclite-linux.arm64-23.26.4.0.0.zip"
                ),
                "61d55b028e9ba5792fbf497e9bcb8f2f14116ed81fc7a8625afc3dd5d0f28c27",
                68_005_804,
            ),
            "libclntsh.so",
        ),
        _ => return None,
    };
    Some(VendorPackage {
        name: "oracle".to_owned(),
        title: "Oracle Instant Client".to_owned(),
        version: "23.26.4".to_owned(),
        license: "Oracle Free Use Terms and Conditions".to_owned(),
        license_url: "https://www.oracle.com/downloads/licenses/oracle-free-license.html"
            .to_owned(),
        vendor_url: "https://www.oracle.com/database/technologies/instant-client/downloads.html"
            .to_owned(),
        archives: vec![archive],
        library: format!("instantclient_23_26/{library}"),
        register: None,
        unregister: None,
    })
}

/// IBM's ODBC (CLI) driver, registered with `db2cli install -setup`; Windows only, as ODBC.
fn db2() -> Option<VendorPackage> {
    const BASE: &str =
        "https://public.dhe.ibm.com/ibmdl/export/pub/software/data/db2/drivers/odbc_cli";
    (platform() == "windows_amd64").then(|| VendorPackage {
        name: "db2".to_owned(),
        title: "IBM Data Server Driver for ODBC and CLI".to_owned(),
        version: "12.1.4".to_owned(),
        license: "IBM's terms for the Data Server Driver".to_owned(),
        license_url:
            "https://www.ibm.com/docs/en/db2/12.1.0?topic=commands-db2cli-db2-interactive-cli"
                .to_owned(),
        vendor_url: format!("{BASE}/"),
        archives: vec![archive(
            format!("{BASE}/v12.1.4/ntx64_odbc_cli.zip"),
            "28748e451e628c41a49676548439c980f02155890d4126342c167fed27a0d63f",
            26_859_193,
        )],
        library: "clidriver/bin/db2app64.dll".to_owned(),
        register: Some(program("clidriver/bin/db2cli.exe", &["install", "-setup"])),
        unregister: Some(program(
            "clidriver/bin/db2cli.exe",
            &["install", "-cleanup"],
        )),
    })
}

/// The package `name` for this platform; `None` where the app does not install it.
#[must_use]
pub fn vendor_package(name: &str) -> Option<VendorPackage> {
    match name {
        "oracle" => oracle(),
        "db2" => db2(),
        _ => None,
    }
}

fn vendor_dir(data_dir: &Path) -> PathBuf {
    drivers_dir(data_dir).join("vendor")
}

/// The package `name` as installed: the newest version unpacked.
#[must_use]
pub fn installed_vendor(data_dir: &Path, name: &str) -> Option<Installed> {
    crate::installed_in(&vendor_dir(data_dir), name)
}

/// Downloads the archives of `package`, checks them and unpacks them. `progress` gets the
/// bytes received of all archives and their total.
pub async fn install_vendor(
    data_dir: &Path,
    package: &VendorPackage,
    progress: impl Fn(u64, Option<u64>),
    cancel: &CancellationToken,
) -> Result<Installed, DriverError> {
    let total = package.size();
    let mut done = 0;
    let mut downloaded = Vec::with_capacity(package.archives.len());
    for archive in &package.archives {
        let http = client(archive.url.starts_with("https:"))?;
        let bytes = download(
            &http,
            &archive.url,
            |n, _| progress(done + n, Some(total)),
            cancel,
        )
        .await?;
        check(&bytes, &archive.sha256)?;
        done += bytes.len() as u64;
        downloaded.push(bytes);
    }
    let tmp = drivers_dir(data_dir).join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
    let target = vendor_dir(data_dir).join(&package.name);
    let record = Installed {
        name: package.name.clone(),
        title: package.title.clone(),
        version: package.version.clone(),
        license: package.license.clone(),
        library: PathBuf::from(&package.library),
    };
    tokio::task::spawn_blocking(move || {
        let result = downloaded
            .iter()
            .try_for_each(|bytes| unzip(bytes, &tmp))
            .and_then(|()| {
                if tmp.join(&record.library).is_file() {
                    crate::install::place(&tmp, &target, &record)
                } else {
                    Err(DriverError::Format(format!(
                        "the archives lack `{}`",
                        record.library.display()
                    )))
                }
            });
        let _ = std::fs::remove_dir_all(&tmp);
        result
    })
    .await
    .map_err(|e| DriverError::Format(e.to_string()))?
}

/// `link`'s target inside `dir`, or `None` when it points outside.
fn link_target(dir: &Path, link: &Path, target: &str) -> Option<PathBuf> {
    let mut path = link.parent()?.to_owned();
    for component in Path::new(target).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    path.starts_with(dir).then_some(path)
}

/// Unpacks a zip archive into `dir`, keeping its folders and symbolic links (Linux libraries
/// are links to their versioned files; where links cannot be made, the file is copied).
/// Entries that would land outside `dir` are skipped; files larger than [`MAX_FILE`] fail
/// the archive.
fn unzip(bytes: &[u8], dir: &Path) -> Result<(), DriverError> {
    let format = |e: zip::result::ZipError| DriverError::Format(e.to_string());
    let read_error = |e: std::io::Error| DriverError::Format(e.to_string());
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(format)?;
    let mut links = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(format)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out = dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(io_error(&out))?;
            continue;
        }
        if entry.size() > MAX_FILE {
            return Err(DriverError::TooLarge);
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE + 1)
            .read_to_end(&mut data)
            .map_err(read_error)?;
        if data.len() as u64 > MAX_FILE {
            return Err(DriverError::TooLarge);
        }
        if entry.is_symlink() {
            links.push((out, String::from_utf8_lossy(&data).into_owned()));
            continue;
        }
        std::fs::write(&out, data).map_err(io_error(&out))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt as _;
            let permissions = std::fs::Permissions::from_mode(mode & 0o755);
            std::fs::set_permissions(&out, permissions).map_err(io_error(&out))?;
        }
    }
    for (link, target) in links {
        let Some(resolved) = link_target(dir, &link, &target) else {
            continue;
        };
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).map_err(io_error(&link))?;
        #[cfg(not(unix))]
        if resolved.is_file() {
            std::fs::copy(&resolved, &link).map_err(io_error(&link))?;
        }
        let _ = resolved;
    }
    Ok(())
}

/// The folder an installed package was unpacked into.
fn package_dir(installed: &Installed, package: &VendorPackage) -> Option<PathBuf> {
    let mut dir = installed.library.as_path();
    for _ in Path::new(&package.library).components() {
        dir = dir.parent()?;
    }
    Some(dir.to_owned())
}

/// Registers the installed ODBC driver with the system's driver manager, through the
/// vendor's program. On Windows it runs as administrator: Windows asks the user first
/// (UAC). Nothing to do for a library the app loads itself.
pub async fn register(installed: &Installed, package: &VendorPackage) -> Result<(), DriverError> {
    match &package.register {
        Some(program) => run_program(installed, package, program).await,
        None => Ok(()),
    }
}

/// Unregisters the driver, as administrator like [`register`].
pub async fn unregister(installed: &Installed, package: &VendorPackage) -> Result<(), DriverError> {
    match &package.unregister {
        Some(program) => run_program(installed, package, program).await,
        None => Ok(()),
    }
}

async fn run_program(
    installed: &Installed,
    package: &VendorPackage,
    program: &Program,
) -> Result<(), DriverError> {
    let dir = package_dir(installed, package)
        .ok_or_else(|| DriverError::Format("the installed driver has no folder".to_owned()))?;
    let path = dir.join(&program.path);
    let args = program.args.clone();
    tokio::task::spawn_blocking(move || run_elevated(&path, &args))
        .await
        .map_err(|e| DriverError::Register(e.to_string()))?
}

/// The Windows error of a UAC prompt the user declined (`ERROR_CANCELLED`).
const DECLINED: i32 = 1223;

/// `s` as a PowerShell string literal.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The PowerShell script that runs `program` as administrator from its folder, waits for
/// it and exits with its code, or with [`DECLINED`] when the UAC prompt is declined.
fn elevation_script(program: &Path, args: &[String]) -> String {
    let dir = program.parent().unwrap_or(program);
    let mut start = format!(
        "Start-Process -FilePath {} -WorkingDirectory {} -Verb RunAs -Wait -PassThru \
         -WindowStyle Hidden -ErrorAction Stop",
        ps_quote(&program.to_string_lossy()),
        ps_quote(&dir.to_string_lossy())
    );
    if !args.is_empty() {
        let list: Vec<String> = args.iter().map(|a| ps_quote(a)).collect();
        start.push_str(" -ArgumentList ");
        start.push_str(&list.join(","));
    }
    format!(
        "try {{ $p = {start} }} catch {{ $e = $_.Exception; \
         while ($e.InnerException) {{ $e = $e.InnerException }}; \
         if ($e.NativeErrorCode -eq {DECLINED}) {{ exit {DECLINED} }}; \
         [Console]::Error.WriteLine($_); exit 1 }}; exit $p.ExitCode"
    )
}

#[cfg(windows)]
fn run_elevated(program: &Path, args: &[String]) -> Result<(), DriverError> {
    use std::os::windows::process::CommandExt as _;
    /// No console window for PowerShell.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if !program.is_file() {
        return Err(DriverError::Register(format!(
            "{} is missing",
            program.display()
        )));
    }
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(elevation_script(program, args))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| DriverError::Register(format!("cannot start PowerShell: {e}")))?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(DECLINED) => Err(DriverError::ElevationDeclined),
        code => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let name = program
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let code = code.map_or_else(|| "no exit code".to_owned(), |c| format!("exit code {c}"));
            let detail = stderr.trim();
            Err(DriverError::Register(if detail.is_empty() {
                format!("{name} failed ({code})")
            } else {
                format!("{name} failed ({code}): {detail}")
            }))
        }
    }
}

#[cfg(not(windows))]
fn run_elevated(_program: &Path, _args: &[String]) -> Result<(), DriverError> {
    Err(DriverError::Register(
        "ODBC drivers are registered on Windows only for now".to_owned(),
    ))
}

/// Deletes an installed package; like [`crate::remove`], files Windows keeps locked are
/// deleted at the next start.
pub fn remove_vendor(data_dir: &Path, name: &str) -> Result<(), DriverError> {
    crate::remove_in(&vendor_dir(data_dir), name)
}

/// Deletes the packages marked removed; called at start by [`crate::purge_removed`].
pub(crate) fn purge_removed_vendor(data_dir: &Path) {
    crate::purge_marked(&vendor_dir(data_dir));
}

/// The marker [`remove_vendor`] leaves when it cannot delete the files yet.
#[cfg(test)]
pub(crate) fn removed_marker(data_dir: &Path, name: &str) -> PathBuf {
    vendor_dir(data_dir).join(name).join(crate::REMOVED_MARK)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in files {
            if let Some(dir) = name.strip_suffix('/') {
                out.add_directory(dir, options).unwrap();
            } else if let Some((link, target)) = name.split_once(" -> ") {
                out.add_symlink(link, target, options).unwrap();
            } else {
                out.start_file(*name, options).unwrap();
                out.write_all(data).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    fn package(urls: &[(&str, &[u8])], register: bool) -> VendorPackage {
        VendorPackage {
            name: "oracle".to_owned(),
            title: "Test client".to_owned(),
            version: "1.0".to_owned(),
            license: "test".to_owned(),
            license_url: String::new(),
            vendor_url: String::new(),
            archives: urls
                .iter()
                .map(|(url, body)| {
                    archive(
                        (*url).to_owned(),
                        &crate::install::hex(&<sha2::Sha256 as sha2::Digest>::digest(body)),
                        body.len() as u64,
                    )
                })
                .collect(),
            library: "ic/libclntsh.so".to_owned(),
            register: register.then(|| program("ic/install.exe", &[])),
            unregister: None,
        }
    }

    #[test]
    fn packages_by_platform() {
        match platform() {
            "windows_amd64" => {
                let oracle = vendor_package("oracle").unwrap();
                assert_eq!(oracle.library, "instantclient_23_26/oci.dll");
                assert!(oracle.register.is_none());
                let db2 = vendor_package("db2").unwrap();
                assert_eq!(
                    db2.register.as_ref().map(|p| p.args.clone()),
                    Some(vec!["install".to_owned(), "-setup".to_owned()])
                );
            }
            "linux_amd64" | "linux_arm64" => {
                assert!(
                    vendor_package("oracle")
                        .unwrap()
                        .library
                        .ends_with("libclntsh.so")
                );
                assert!(vendor_package("db2").is_none());
            }
            _ => assert!(vendor_package("oracle").is_none()),
        }
        for p in ["oracle", "db2"].into_iter().filter_map(vendor_package) {
            assert!(
                p.archives
                    .iter()
                    .all(|a| a.sha256.len() == 64 && a.url.starts_with("https://") && a.size > 0)
            );
        }
        assert!(vendor_package("snowflake").is_none());
    }

    #[tokio::test]
    async fn installs_and_removes_a_package() {
        let first = zip(&[("ic/", b""), ("ic/oci.dll", b"oci"), ("../evil.dll", b"x")]);
        let second = zip(&[
            ("ic/libclntsh.so.23.1", b"client"),
            ("ic/libclntsh.so -> libclntsh.so.23.1", b""),
            ("ic/outside -> ../../evil", b""),
        ]);
        let url = crate::tests::serve(vec![
            ("/first.zip", first.clone()),
            ("/second.zip", second.clone()),
        ])
        .await;
        let pkg = package(
            &[
                (&format!("{url}/first.zip"), &first),
                (&format!("{url}/second.zip"), &second),
            ],
            false,
        );
        let dir = tempfile::tempdir().unwrap();
        let seen = std::sync::Mutex::new(Vec::new());
        let installed = install_vendor(
            dir.path(),
            &pkg,
            |done, total| seen.lock().unwrap().push((done, total)),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.last(), Some(&(pkg.size(), Some(pkg.size()))));
        assert!(installed.library.ends_with("ic/libclntsh.so"));
        assert_eq!(
            std::fs::read(&installed.library).unwrap(),
            b"client",
            "the link's target"
        );
        let home = package_dir(&installed, &pkg).unwrap();
        assert!(
            home.join("ic/oci.dll").is_file(),
            "both archives in one folder"
        );
        assert!(!home.join("ic/outside").exists());
        assert!(!home.parent().unwrap().join("evil.dll").exists());
        assert_eq!(
            VendorPackage::library_dir(&installed),
            Some(home.join("ic").as_path())
        );
        assert_eq!(
            installed_vendor(dir.path(), "oracle"),
            Some(installed.clone())
        );
        // Nothing to register for a library the app loads.
        register(&installed, &pkg).await.unwrap();
        remove_vendor(dir.path(), "oracle").unwrap();
        assert_eq!(installed_vendor(dir.path(), "oracle"), None);
        assert!(!removed_marker(dir.path(), "oracle").exists());
    }

    #[tokio::test]
    async fn refuses_a_corrupted_or_incomplete_archive() {
        let body = zip(&[("ic/libclntsh.so", b"client")]);
        let url = crate::tests::serve(vec![("/a.zip", body.clone())]).await;
        let mut pkg = package(&[(&format!("{url}/a.zip"), &body)], false);
        pkg.archives[0].sha256 = "00".repeat(32);
        let dir = tempfile::tempdir().unwrap();
        let err = install_vendor(dir.path(), &pkg, |_, _| {}, &CancellationToken::new()).await;
        assert!(matches!(err, Err(DriverError::Checksum)), "{err:?}");
        assert_eq!(installed_vendor(dir.path(), "oracle"), None);
        let other = zip(&[("ic/readme.txt", b"no library")]);
        let url = crate::tests::serve(vec![("/b.zip", other.clone())]).await;
        let pkg = package(&[(&format!("{url}/b.zip"), &other)], true);
        let err = install_vendor(dir.path(), &pkg, |_, _| {}, &CancellationToken::new()).await;
        assert!(matches!(err, Err(DriverError::Format(_))), "{err:?}");
    }

    #[test]
    fn link_targets_stay_inside() {
        let dir = Path::new("/d");
        assert_eq!(
            link_target(dir, Path::new("/d/ic/a.so"), "a.so.1"),
            Some(PathBuf::from("/d/ic/a.so.1"))
        );
        assert_eq!(link_target(dir, Path::new("/d/ic/a"), "../../x"), None);
        assert_eq!(link_target(dir, Path::new("/d/ic/a"), "/etc/passwd"), None);
    }

    #[test]
    fn elevation_script_quotes_and_waits() {
        let script = elevation_script(
            Path::new(r"C:\Users\O'Brien\drivers\ic\db2cli.exe"),
            &["install".to_owned(), "-setup".to_owned()],
        );
        assert!(script.contains(r"-FilePath 'C:\Users\O''Brien\drivers\ic\db2cli.exe'"));
        assert!(script.contains(r"-WorkingDirectory 'C:\Users\O''Brien\drivers\ic'"));
        assert!(script.contains("-Verb RunAs -Wait"));
        assert!(script.contains("-ArgumentList 'install','-setup'"));
        assert!(script.contains("exit 1223"));
        assert!(!elevation_script(Path::new("C:/a.exe"), &[]).contains("-ArgumentList"));
    }
}
