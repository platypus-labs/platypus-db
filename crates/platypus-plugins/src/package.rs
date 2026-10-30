//! Installed plugins: `<data>/plugins/<id>/<version>/` holds a package's `plugin.toml` and
//! `plugin.wasm`, and `<data>/plugins/plugins.json` which are turned on, the permissions the
//! user approved at install and what each module described.
//!
//! A package is a folder or a `.zip` with the two files at its root or in a single folder.
//! Installing reads the module's description, so a module that does not load is refused
//! before it is copied; the user approves the manifest's permissions by installing it
//! (the app shows them first, from [`Plugins::inspect`]).

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::{self, ActionInfo, ActionOutcome};
use crate::manifest::{Kind, MANIFEST_FILE, MODULE_FILE, Manifest, Permissions};
use crate::runtime::{self, DESCRIBE, Description, Grants, Limits, Sandbox};
use crate::{PluginError, driver};

const STATE_FILE: &str = "plugins.json";
/// The largest manifest read.
const MAX_MANIFEST: u64 = 64 << 10;
/// The largest module read.
const MAX_MODULE: u64 = 32 << 20;
/// Prefix of the folders an install writes before moving them in place.
const TMP_PREFIX: &str = ".tmp-";

/// One change to `plugins.json` at a time.
static STATE_LOCK: Mutex<()> = Mutex::new(());

/// A package read before installing it: what the user approves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspected {
    pub manifest: Manifest,
    pub description: Description,
    /// The version of the same plugin already installed, which this one replaces.
    pub replaces: Option<String>,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub enabled: bool,
    /// The permissions the user approved: the manifest's, at install.
    pub approved: Permissions,
    pub description: Description,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    version: String,
    enabled: bool,
    approved: Permissions,
    description: Description,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    plugins: BTreeMap<String, Entry>,
}

/// The plugins of a data folder.
#[derive(Debug, Clone)]
pub struct Plugins {
    dir: PathBuf,
    limits: Limits,
}

impl Plugins {
    /// The plugins kept in `<data_dir>/plugins`.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("plugins"),
            limits: Limits::default(),
        }
    }

    /// The plugins of the app's data folder (`platypus_drivers::set_data_dir`).
    pub fn shared() -> Result<Self, PluginError> {
        platypus_drivers::data_dir()
            .map(Self::new)
            .ok_or_else(|| PluginError::Invalid("the data folder is not set".to_owned()))
    }

    /// The same plugins, run with other limits.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }

    /// Reads a package and its module's description, without installing it.
    pub async fn inspect(&self, source: &Path) -> Result<Inspected, PluginError> {
        let (manifest, wasm) = read_package(source)?;
        let description = describe(&manifest, wasm, self.limits).await?;
        let replaces = self
            .read_state()?
            .plugins
            .get(&manifest.id)
            .map(|e| e.version.clone());
        Ok(Inspected {
            manifest,
            description,
            replaces,
        })
    }

    /// Installs a package, replacing any version of the same plugin, turned on and with
    /// its manifest's permissions approved.
    pub async fn install(&self, source: &Path) -> Result<Installed, PluginError> {
        let (manifest, wasm) = read_package(source)?;
        let description = describe(&manifest, wasm.clone(), self.limits).await?;
        let home = self.dir.join(&manifest.id);
        std::fs::create_dir_all(&home).map_err(PluginError::io(&home))?;
        let tmp = home.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
        let written = write_package(&tmp, &manifest, &wasm);
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        let target = home.join(&manifest.version);
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(PluginError::io(&target))?;
        }
        std::fs::rename(&tmp, &target).map_err(PluginError::io(&target))?;
        // The other versions and leftovers of failed installs go.
        if let Ok(entries) = std::fs::read_dir(&home) {
            for entry in entries.flatten() {
                if entry.file_name() != manifest.version.as_str() {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        let entry = Entry {
            version: manifest.version.clone(),
            enabled: true,
            approved: manifest.permissions.clone(),
            description: description.clone(),
        };
        self.update_state(|state| {
            state.plugins.insert(manifest.id.clone(), entry);
            Ok(())
        })?;
        Ok(Installed {
            approved: manifest.permissions.clone(),
            manifest,
            enabled: true,
            description,
        })
    }

    /// The installed plugins, by id; one whose files are gone is left out.
    pub fn list(&self) -> Result<Vec<Installed>, PluginError> {
        Ok(self
            .read_state()?
            .plugins
            .into_iter()
            .filter_map(|(id, entry)| self.installed(&id, entry).ok())
            .collect())
    }

    /// Turns a plugin on or off: off, its actions and driver are not offered.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PluginError> {
        self.update_state(|state| {
            let entry = state
                .plugins
                .get_mut(id)
                .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))?;
            entry.enabled = enabled;
            Ok(())
        })
    }

    /// Removes a plugin and its files.
    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        self.update_state(|state| {
            state
                .plugins
                .remove(id)
                .map(|_| ())
                .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))
        })?;
        let home = self.dir.join(id);
        if !id.is_empty() && !id.contains(['/', '\\']) && home.exists() {
            std::fs::remove_dir_all(&home).map_err(PluginError::io(&home))?;
        }
        Ok(())
    }

    /// An installed plugin that is turned on, with its module's bytes.
    pub(crate) fn load(&self, id: &str) -> Result<(Installed, Vec<u8>), PluginError> {
        let entry = self
            .read_state()?
            .plugins
            .remove(id)
            .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))?;
        let installed = self.installed(id, entry)?;
        if !installed.enabled {
            return Err(PluginError::Disabled(id.to_owned()));
        }
        let path = self
            .dir
            .join(id)
            .join(&installed.manifest.version)
            .join(MODULE_FILE);
        let wasm = std::fs::read(&path).map_err(PluginError::io(&path))?;
        Ok((installed, wasm))
    }

    /// Runs action `action` of plugin `id` on `context`, as the place it belongs to gives it.
    pub async fn run_action(
        &self,
        id: &str,
        action: &str,
        context: Value,
    ) -> Result<ActionOutcome, PluginError> {
        let (installed, wasm) = self.load(id)?;
        let info: ActionInfo = installed
            .description
            .actions
            .iter()
            .find(|a| a.id == action)
            .cloned()
            .ok_or_else(|| {
                PluginError::Invalid(format!("the plugin {id:?} has no action {action:?}"))
            })?;
        let grants = Grants {
            plugin: id.to_owned(),
            http: installed.approved.http.clone(),
            proxy: None,
        };
        let clipboard = installed.approved.clipboard;
        let limits = self.limits;
        blocking(move || {
            let module = runtime::compile(&wasm)?;
            action::run(&module, &grants, limits, clipboard, &info, context)
        })
        .await
    }

    fn installed(&self, id: &str, entry: Entry) -> Result<Installed, PluginError> {
        let path = self.dir.join(id).join(&entry.version).join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(PluginError::io(&path))?;
        Ok(Installed {
            manifest: Manifest::parse(&text)?,
            enabled: entry.enabled,
            approved: entry.approved,
            description: entry.description,
        })
    }

    fn read_state(&self) -> Result<State, PluginError> {
        let path = self.dir.join(STATE_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| PluginError::Invalid(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(PluginError::io(&path)(e)),
        }
    }

    fn update_state(
        &self,
        change: impl FnOnce(&mut State) -> Result<(), PluginError>,
    ) -> Result<(), PluginError> {
        let _guard = STATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self.read_state()?;
        change(&mut state)?;
        std::fs::create_dir_all(&self.dir).map_err(PluginError::io(&self.dir))?;
        let path = self.dir.join(STATE_FILE);
        let tmp = self.dir.join(format!("{STATE_FILE}.tmp"));
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| PluginError::Invalid(e.to_string()))?;
        std::fs::write(&tmp, json).map_err(PluginError::io(&tmp))?;
        std::fs::rename(&tmp, &path).map_err(PluginError::io(&path))
    }
}

/// Runs `f` on a blocking thread, where sandboxes run.
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, PluginError> + Send + 'static,
) -> Result<T, PluginError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| PluginError::Trap(e.to_string()))?
}

/// Loads the module and checks that its description matches the manifest's kinds.
async fn describe(
    manifest: &Manifest,
    wasm: Vec<u8>,
    limits: Limits,
) -> Result<Description, PluginError> {
    let grants = Grants {
        plugin: manifest.id.clone(),
        ..Grants::default()
    };
    let description: Description = blocking(move || {
        let module = runtime::compile(&wasm)?;
        Sandbox::new(&module, &grants, limits)?.call(DESCRIBE, &serde_json::json!({}))
    })
    .await?;
    matches_manifest(manifest, &description)?;
    Ok(description)
}

/// The description provides what the manifest's `kinds` say, and nothing else.
fn matches_manifest(manifest: &Manifest, description: &Description) -> Result<(), PluginError> {
    action::check(&description.actions)?;
    let why = match (
        manifest.provides(Kind::Actions),
        !description.actions.is_empty(),
        manifest.provides(Kind::Driver),
        &description.driver,
    ) {
        (true, false, ..) => "the manifest promises actions and the module has none",
        (false, true, ..) => "the module has actions and the manifest's `kinds` lacks \"actions\"",
        (.., true, None) => "the manifest promises a driver and the module has none",
        (.., false, Some(_)) => {
            "the module has a driver and the manifest's `kinds` lacks \"driver\""
        }
        (.., Some(info)) => return driver::check(info),
        _ => return Ok(()),
    };
    Err(PluginError::Invalid(why.to_owned()))
}

/// The manifest and the module of a folder or a `.zip`.
fn read_package(source: &Path) -> Result<(Manifest, Vec<u8>), PluginError> {
    let (manifest, wasm) = if source.is_dir() {
        (
            read_file(&source.join(MANIFEST_FILE), MAX_MANIFEST)?,
            read_file(&source.join(MODULE_FILE), MAX_MODULE)?,
        )
    } else {
        read_zip(source)?
    };
    let text = String::from_utf8(manifest)
        .map_err(|_| PluginError::Invalid(format!("{MANIFEST_FILE} is not UTF-8 text")))?;
    Ok((Manifest::parse(&text)?, wasm))
}

fn read_file(path: &Path, max: u64) -> Result<Vec<u8>, PluginError> {
    let file = std::fs::File::open(path).map_err(PluginError::io(path))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(PluginError::io(path))?;
    if bytes.len() as u64 > max {
        return Err(PluginError::Invalid(format!(
            "{} is larger than {} KB",
            path.display(),
            max >> 10
        )));
    }
    Ok(bytes)
}

/// The two files of a `.zip`, at its root or in its one folder.
fn read_zip(source: &Path) -> Result<(Vec<u8>, Vec<u8>), PluginError> {
    let file = std::fs::File::open(source).map_err(PluginError::io(source))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        PluginError::Invalid(format!(
            "{} is neither a folder nor a .zip package: {e}",
            source.display()
        ))
    })?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok().map(std::borrow::Cow::into_owned))
        .collect();
    let manifest_name = names
        .iter()
        .filter(|n| n.as_str() == MANIFEST_FILE || n.ends_with(&format!("/{MANIFEST_FILE}")))
        .min_by_key(|n| n.matches('/').count())
        .ok_or_else(|| PluginError::Invalid(format!("the .zip has no {MANIFEST_FILE}")))?
        .clone();
    if manifest_name.matches('/').count() > 1 {
        return Err(PluginError::Invalid(format!(
            "{MANIFEST_FILE} must be at the root of the .zip or in its one folder"
        )));
    }
    let module_name = format!(
        "{}{MODULE_FILE}",
        &manifest_name[..manifest_name.len() - MANIFEST_FILE.len()]
    );
    let mut read = |name: &str, max: u64| -> Result<Vec<u8>, PluginError> {
        let entry = archive
            .by_name(name)
            .map_err(|_| PluginError::Invalid(format!("the .zip has no {name}")))?;
        let mut bytes = Vec::new();
        entry
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(PluginError::io(source))?;
        if bytes.len() as u64 > max {
            return Err(PluginError::Invalid(format!(
                "{name} is larger than {} KB",
                max >> 10
            )));
        }
        Ok(bytes)
    };
    let manifest = read(&manifest_name, MAX_MANIFEST)?;
    let module = read(&module_name, MAX_MODULE)?;
    Ok((manifest, module))
}

fn write_package(dir: &Path, manifest: &Manifest, wasm: &[u8]) -> Result<(), PluginError> {
    std::fs::create_dir_all(dir).map_err(PluginError::io(dir))?;
    let text = toml::to_string_pretty(manifest).map_err(|e| PluginError::Invalid(e.to_string()))?;
    let path = dir.join(MANIFEST_FILE);
    std::fs::write(&path, text).map_err(PluginError::io(&path))?;
    let path = dir.join(MODULE_FILE);
    std::fs::write(&path, wasm).map_err(PluginError::io(&path))
}

#[cfg(test)]
mod tests;
