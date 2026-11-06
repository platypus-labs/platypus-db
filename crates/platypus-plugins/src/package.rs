//! Installed plugins: `<data>/plugins/<id>/<version>/` holds a package's `plugin.toml` and
//! `plugin.wasm`, and `<data>/plugins/plugins.json` which are turned on, the permissions the
//! user approved at install and what each module described.
//!
//! A package is a folder or a `.zip` with the two files at its root or in a single folder.
//! Installing reads the module's description, so a module that does not load is refused
//! before it is copied; the user approves the manifest's permissions by installing it
//! (the app shows them first, from [`Plugins::inspect`]), and the install takes the hash of
//! the package inspected, so it installs exactly what was shown. A package that is only a theme
//! has no module, and nothing runs for it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use platypus_store::SecretStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::{self, ActionInfo, ActionOutcome};
use crate::frames::{self, PackageFiles};
use crate::grid::GridLimits;
use crate::hooks::HookLimits;
use crate::manifest::{Capability, Kind, MANIFEST_FILE, MODULE_FILE, Manifest, Permissions};
use crate::runtime::{self, DESCRIBE, Description, GRID, Grants, HOOK, Limits, Sandbox};
use crate::settings::{SettingState, SettingsAccess, SettingsFiles};
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
    /// What it asks beyond what the user approved for that version; all it asks on a first
    /// install. Installing approves it, so the app shows it before.
    pub added: Permissions,
    /// A hash of the package's two files, given back to [`Plugins::install`]: what is
    /// installed is exactly what the user reviewed, even if the package changes in between.
    pub hash: String,
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
    hook_limits: HookLimits,
    grid_limits: GridLimits,
    settings: SettingsFiles,
}

impl Plugins {
    /// The plugins kept in `<data_dir>/plugins`.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("plugins");
        Self {
            settings: SettingsFiles::new(&dir),
            dir,
            limits: Limits::default(),
            hook_limits: HookLimits::default(),
            grid_limits: GridLimits::default(),
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

    /// The same plugins, their hooks run with other limits.
    #[must_use]
    pub fn with_hook_limits(mut self, limits: HookLimits) -> Self {
        self.hook_limits = limits;
        self
    }

    /// The same plugins, their grid extensions run with other limits.
    #[must_use]
    pub fn with_grid_limits(mut self, limits: GridLimits) -> Self {
        self.grid_limits = limits;
        self
    }

    /// The same plugins, their secret settings kept in `secrets` and not the system's keychain.
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.settings = self.settings.with_secrets(secrets);
        self
    }

    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }

    pub(crate) fn hook_limits(&self) -> HookLimits {
        self.hook_limits
    }

    pub(crate) fn grid_limits(&self) -> GridLimits {
        self.grid_limits
    }

    /// The folder holding the plugins of this data folder.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder of an installed plugin's version.
    pub(crate) fn module_path(&self, manifest: &Manifest) -> PathBuf {
        self.dir.join(&manifest.id).join(&manifest.version)
    }

    /// What a sandbox of an installed plugin may reach: what the user approved.
    pub(crate) fn grants(&self, installed: &Installed) -> Grants {
        let manifest = &installed.manifest;
        Grants {
            plugin: manifest.id.clone(),
            http: installed.approved.http.clone(),
            proxy: None,
            api: manifest.api,
            settings: (!manifest.settings.is_empty()).then(|| {
                SettingsAccess::new(&manifest.id, &manifest.settings, self.settings.clone())
            }),
        }
    }

    /// Reads a package and its module's description, without installing it.
    pub async fn inspect(&self, source: &Path) -> Result<Inspected, PluginError> {
        let Package {
            manifest,
            wasm,
            hash,
            ..
        } = read_package(source)?;
        let description = describe(&manifest, wasm, self.limits).await?;
        let state = self.read_state()?;
        let current = state.plugins.get(&manifest.id);
        let approved = current.map(|e| e.approved.clone()).unwrap_or_default();
        Ok(Inspected {
            added: manifest.permissions.beyond(&approved),
            replaces: current.map(|e| e.version.clone()),
            manifest,
            description,
            hash,
        })
    }

    /// Installs a package, replacing any version of the same plugin, turned on and with
    /// its manifest's permissions approved. `reviewed` is the [`Inspected::hash`] of the
    /// package the user approved: a package that changed since (other permissions, another
    /// module) is refused.
    pub async fn install(&self, source: &Path, reviewed: &str) -> Result<Installed, PluginError> {
        let Package {
            manifest,
            wasm,
            files,
            hash,
        } = read_package(source)?;
        if hash != reviewed {
            return Err(PluginError::Invalid(
                "the package changed since it was reviewed: review it again before installing"
                    .to_owned(),
            ));
        }
        let description = describe(&manifest, wasm.clone(), self.limits).await?;
        let previous = self
            .list()?
            .into_iter()
            .find(|p| p.manifest.id == manifest.id);
        let home = self.dir.join(&manifest.id);
        std::fs::create_dir_all(&home).map_err(PluginError::io(&home))?;
        let tmp = home.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
        let written = write_package(&tmp, &manifest, &wasm, &files);
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
        // A field the new version no longer has leaves nothing behind. Its secret is out of
        // the module's reach already, as is any value: the rest of the install stands.
        if let Some(previous) = previous {
            let dropped: Vec<_> = previous
                .manifest
                .settings
                .into_iter()
                .filter(|old| !manifest.settings.iter().any(|f| f.key == old.key))
                .collect();
            let _ = self.settings.forget(&manifest.id, &dropped);
        }
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

    /// Whether the user approved `capability` for the plugin `id`; an error that says so when
    /// not, for what is about to use it. Nothing of the plugin has run then.
    pub fn require(&self, id: &str, capability: Capability) -> Result<Installed, PluginError> {
        let entry = self
            .read_state()?
            .plugins
            .remove(id)
            .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))?;
        let installed = self.installed(id, entry)?;
        if !installed.enabled {
            return Err(PluginError::Disabled(id.to_owned()));
        }
        if installed.approved.allows(capability) {
            Ok(installed)
        } else {
            Err(PluginError::Denied(format!(
                "the plugin {id:?} has no permission for {capability:?}"
            )))
        }
    }

    /// The fields of a plugin's settings page with what they hold; a secret only says if it
    /// is set.
    pub fn settings_of(&self, id: &str) -> Result<Vec<SettingState>, PluginError> {
        let installed = self.installed_any(id)?;
        self.settings.states(id, &installed.manifest.settings)
    }

    /// Saves a value of a plugin's settings page; `null` goes back to the default. A key its
    /// manifest does not declare, or a value that does not fit the field, is refused.
    pub fn set_setting(&self, id: &str, key: &str, value: &Value) -> Result<(), PluginError> {
        let installed = self.installed_any(id)?;
        let field = installed
            .manifest
            .settings
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| {
                PluginError::Invalid(format!("the plugin {id:?} has no setting {key:?}"))
            })?;
        self.settings.set(id, field, value)
    }

    /// Removes a plugin, its files and its settings.
    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        // Its fields are in its manifest, about to go.
        let fields = self
            .installed_any(id)
            .map(|p| p.manifest.settings)
            .unwrap_or_default();
        self.update_state(|state| {
            state
                .plugins
                .remove(id)
                .map(|_| ())
                .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))
        })?;
        // The state holds checked ids only (see `Manifest::parse`): a plain folder name.
        let home = self.dir.join(id);
        if home.exists() {
            std::fs::remove_dir_all(&home).map_err(PluginError::io(&home))?;
        }
        frames::forget_state(&self.dir, id);
        self.settings.forget(id, &fields)
    }

    /// An installed plugin, turned on or not.
    fn installed_any(&self, id: &str) -> Result<Installed, PluginError> {
        let entry = self
            .read_state()?
            .plugins
            .remove(id)
            .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))?;
        self.installed(id, entry)
    }

    /// An installed plugin that is turned on, with its module's bytes.
    pub(crate) fn load(&self, id: &str) -> Result<(Installed, Vec<u8>), PluginError> {
        let installed = self.installed_any(id)?;
        if !installed.enabled {
            return Err(PluginError::Disabled(id.to_owned()));
        }
        if !installed.manifest.needs_module() {
            return Err(PluginError::Invalid(format!(
                "the plugin {id:?} has no module"
            )));
        }
        let path = self.module_path(&installed.manifest).join(MODULE_FILE);
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
        let grants = self.grants(&installed);
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

/// Loads the module and checks that its description matches the manifest's kinds, and that it
/// has the entry point of the hooks the manifest declares. A package without a module provides
/// nothing to describe.
async fn describe(
    manifest: &Manifest,
    wasm: Vec<u8>,
    limits: Limits,
) -> Result<Description, PluginError> {
    if !manifest.needs_module() {
        return Ok(Description::default());
    }
    let grants = Grants {
        plugin: manifest.id.clone(),
        api: manifest.api,
        ..Grants::default()
    };
    let hooks = !manifest.hooks.is_empty();
    let grid = !manifest.grid.is_empty();
    let description: Description = blocking(move || {
        let module = runtime::compile(&wasm)?;
        let mut sandbox = Sandbox::new(&module, &grants, limits)?;
        if hooks && !sandbox.exports(HOOK) {
            return Err(PluginError::Invalid(format!(
                "the manifest declares hooks and the module does not export `{HOOK}`"
            )));
        }
        if grid && !sandbox.exports(GRID) {
            return Err(PluginError::Invalid(format!(
                "the manifest declares grid extensions and the module does not export `{GRID}`"
            )));
        }
        sandbox.call(DESCRIBE, &serde_json::json!({}))
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

/// A package as read: its manifest, its module, the documents of its tabs and panels with what
/// they load, and the hash of them all.
struct Package {
    manifest: Manifest,
    wasm: Vec<u8>,
    files: PackageFiles,
    hash: String,
}

/// The SHA-256 of the manifest's bytes, the module's and those of the files with their names,
/// each preceded by its length so the boundaries between them count too.
fn package_hash(manifest: &[u8], wasm: &[u8], files: &PackageFiles) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let named = files
        .iter()
        .flat_map(|(name, bytes)| [name.as_bytes(), bytes.as_slice()]);
    for part in [manifest, wasm].into_iter().chain(named) {
        ctx.update(&(part.len() as u64).to_le_bytes());
        ctx.update(part);
    }
    ctx.finish()
        .as_ref()
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The manifest and the module of a folder or a `.zip`; a package that needs no module has
/// none, even if a file is there: nothing of it is read, copied or run.
fn read_package(source: &Path) -> Result<Package, PluginError> {
    if source.is_dir() {
        let manifest = read_file(&source.join(MANIFEST_FILE), MAX_MANIFEST)?;
        let parsed = parse_manifest(&manifest)?;
        let wasm = if parsed.needs_module() {
            read_file(&source.join(MODULE_FILE), MAX_MODULE)?
        } else {
            Vec::new()
        };
        let files = frames::read_folder_files(source, &parsed)?;
        return Ok(package(&manifest, parsed, wasm, files));
    }
    let mut zip = ZipPackage::open(source)?;
    let manifest = zip.read_manifest()?;
    let parsed = parse_manifest(&manifest)?;
    let wasm = if parsed.needs_module() {
        zip.read_module()?
    } else {
        Vec::new()
    };
    let files = zip.read_files(&parsed)?;
    Ok(package(&manifest, parsed, wasm, files))
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, PluginError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PluginError::Invalid(format!("{MANIFEST_FILE} is not UTF-8 text")))?;
    Manifest::parse(text)
}

fn package(manifest: &[u8], parsed: Manifest, wasm: Vec<u8>, files: PackageFiles) -> Package {
    Package {
        hash: package_hash(manifest, &wasm, &files),
        manifest: parsed,
        wasm,
        files,
    }
}

pub(crate) fn read_file(path: &Path, max: u64) -> Result<Vec<u8>, PluginError> {
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

/// A `.zip` package: the manifest at its root or in its one folder, the rest beside it.
struct ZipPackage<'a> {
    source: &'a Path,
    archive: zip::ZipArchive<std::fs::File>,
    names: Vec<String>,
    /// What the names of its files start with: the folder of the manifest, with its `/`.
    prefix: String,
}

impl<'a> ZipPackage<'a> {
    fn open(source: &'a Path) -> Result<Self, PluginError> {
        let file = std::fs::File::open(source).map_err(PluginError::io(source))?;
        let archive = zip::ZipArchive::new(file).map_err(|e| {
            PluginError::Invalid(format!(
                "{} is neither a folder nor a .zip package: {e}",
                source.display()
            ))
        })?;
        let names: Vec<String> = archive
            .file_names()
            .filter_map(|n| n.ok().map(std::borrow::Cow::into_owned))
            .collect();
        let manifest = names
            .iter()
            .filter(|n| n.as_str() == MANIFEST_FILE || n.ends_with(&format!("/{MANIFEST_FILE}")))
            .min_by_key(|n| n.matches('/').count())
            .ok_or_else(|| PluginError::Invalid(format!("the .zip has no {MANIFEST_FILE}")))?;
        if manifest.matches('/').count() > 1 {
            return Err(PluginError::Invalid(format!(
                "{MANIFEST_FILE} must be at the root of the .zip or in its one folder"
            )));
        }
        let prefix = manifest[..manifest.len() - MANIFEST_FILE.len()].to_owned();
        Ok(Self {
            source,
            archive,
            names,
            prefix,
        })
    }

    fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>, PluginError> {
        let entry = self
            .archive
            .by_name(name)
            .map_err(|_| PluginError::Invalid(format!("the .zip has no {name}")))?;
        let mut bytes = Vec::new();
        entry
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(PluginError::io(self.source))?;
        if bytes.len() as u64 > max {
            return Err(PluginError::Invalid(format!(
                "{name} is larger than {} KB",
                max >> 10
            )));
        }
        Ok(bytes)
    }

    fn read_manifest(&mut self) -> Result<Vec<u8>, PluginError> {
        self.read(&format!("{}{MANIFEST_FILE}", self.prefix), MAX_MANIFEST)
    }

    fn read_module(&mut self) -> Result<Vec<u8>, PluginError> {
        let name = format!("{}{MODULE_FILE}", self.prefix);
        if !self.names.contains(&name) {
            return Err(PluginError::Invalid(format!(
                "the .zip has no {MODULE_FILE}"
            )));
        }
        self.read(&name, MAX_MODULE)
    }

    /// The files of the tabs and panels of `manifest`, by their paths in the package.
    fn read_files(&mut self, manifest: &Manifest) -> Result<PackageFiles, PluginError> {
        let inside: Vec<String> = self
            .names
            .iter()
            .filter_map(|n| n.strip_prefix(self.prefix.as_str()))
            .map(str::to_owned)
            .collect();
        let wanted = frames::choose_files(manifest, &inside)?;
        let mut files = PackageFiles::new();
        let mut budget = frames::Budget::default();
        for path in wanted {
            let bytes = self.read(&format!("{}{path}", self.prefix), frames::MAX_FILE)?;
            budget.spend(&path, bytes.len())?;
            files.insert(path, bytes);
        }
        Ok(files)
    }
}

fn write_package(
    dir: &Path,
    manifest: &Manifest,
    wasm: &[u8],
    files: &PackageFiles,
) -> Result<(), PluginError> {
    std::fs::create_dir_all(dir).map_err(PluginError::io(dir))?;
    let text = toml::to_string_pretty(manifest).map_err(|e| PluginError::Invalid(e.to_string()))?;
    let path = dir.join(MANIFEST_FILE);
    std::fs::write(&path, text).map_err(PluginError::io(&path))?;
    if manifest.needs_module() {
        let path = dir.join(MODULE_FILE);
        std::fs::write(&path, wasm).map_err(PluginError::io(&path))?;
    }
    frames::write_files(dir, files)
}

#[cfg(test)]
mod tests;
