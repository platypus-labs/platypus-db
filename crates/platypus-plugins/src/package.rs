//! Installed plugins: `<data>/plugins/<id>/<version>/` holds a package's `plugin.toml`,
//! `plugin.wasm` and a theme's fonts, and `<data>/plugins/plugins.json` which are turned on, the
//! permissions the user approved at install and what each module described.
//!
//! A package is a folder or a `.zip` with those files at its root or in a single folder.
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
use crate::extensions::{GridDecl, SettingField, SettingKind};
use crate::frames::{self, PackageFiles};
use crate::grid::GridLimits;
use crate::hooks::HookLimits;
use crate::manifest::{
    Capability, Kind, MANIFEST_FILE, MODULE_FILE, Manifest, Permissions, is_plain_name,
};
use crate::runtime::{self, DESCRIBE, Description, GRID, Grants, HOOK, Limits, Sandbox};
use crate::settings::{SettingState, SettingsAccess, SettingsFiles, secret_keys};
use crate::{API_VERSION, PluginError, driver, log, theme};

pub(crate) const STATE_FILE: &str = "plugins.json";
pub(crate) const STATE_TMP: &str = "plugins.json.tmp";
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
    /// A hash of the package's files, given back to [`Plugins::install`]: what is
    /// installed is exactly what the user reviewed, even if the package changes in between.
    pub hash: String,
    /// The installed version has another signer, or none where this one has one: it may be
    /// another publisher, and installing clears the secrets and the state of its pages.
    pub signer_change: Option<SignerChange>,
}

/// How the signer of a package differs from the installed version's: the names the user gave the
/// keys, `None` for a package with no signature, as any manual install is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignerChange {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The trusted key that signed a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Signer {
    /// The public key, what is compared.
    pub(crate) key: String,
    /// The name the user gave it.
    pub(crate) name: String,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub enabled: bool,
    /// The permissions the user approved: the manifest's, at install.
    pub approved: Permissions,
    pub description: Description,
    /// Its files are missing or are not its own: it is listed so that it can be removed, its
    /// manifest stands for nothing but its name, and nothing of it runs.
    pub broken: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    version: String,
    enabled: bool,
    approved: Permissions,
    description: Description,
    /// The keys of its secret settings: its manifest may be gone when they have to be forgotten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    secrets: Vec<String>,
    /// Who signed the package installed; nobody, for a manual or an unsigned install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signer: Option<Signer>,
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

    /// The folder of the plugins.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
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
        self.inspect_signed(source, None).await
    }

    /// [`Self::inspect`] of a package `signer` is known to have signed.
    pub(crate) async fn inspect_signed(
        &self,
        source: &Path,
        signer: Option<&Signer>,
    ) -> Result<Inspected, PluginError> {
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
            signer_change: current
                .filter(|e| !same_signer(e.signer.as_ref(), signer))
                .map(|e| SignerChange {
                    from: e.signer.as_ref().map(|s| s.name.clone()),
                    to: signer.map(|s| s.name.clone()),
                }),
            manifest,
            description,
            hash,
        })
    }

    /// Installs a package, replacing any version of the same plugin, with its manifest's
    /// permissions approved: turned on, or as the user left the version it replaces. `reviewed` is
    /// the [`Inspected::hash`] of the package the user approved: a package that changed since (other
    /// permissions, another module) is refused.
    pub async fn install(&self, source: &Path, reviewed: &str) -> Result<Installed, PluginError> {
        self.install_signed(source, reviewed, None).await
    }

    /// [`Self::install`] of a package `signer` is known to have signed. When that is not who signed
    /// the version it replaces, nothing the user gave that one reaches this one: its secrets and
    /// the state of its pages go, as the id alone does not say that it is the same publisher.
    pub(crate) async fn install_signed(
        &self,
        source: &Path,
        reviewed: &str,
        signer: Option<Signer>,
    ) -> Result<Installed, PluginError> {
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
        let previous = self.read_state()?.plugins.remove(&manifest.id);
        let old_fields = previous
            .as_ref()
            .and_then(|e| self.installed(&manifest.id, e).ok())
            .map_or_else(Vec::new, |p| p.manifest.settings);
        let home = self.dir.join(&manifest.id);
        std::fs::create_dir_all(&home).map_err(PluginError::io(&home))?;
        let tmp = home.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
        let target = home.join(&manifest.version);
        let swapped = write_package(&tmp, &manifest, &wasm, &files)
            .and_then(|()| swap_in(&tmp, &target, &home));
        let aside = match swapped {
            Ok(aside) => aside,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
        };
        let enabled = previous.as_ref().is_none_or(|e| e.enabled);
        let entry = Entry {
            version: manifest.version.clone(),
            enabled,
            approved: manifest.permissions.clone(),
            description: description.clone(),
            secrets: secret_keys(&manifest.settings),
            signer: signer.clone(),
        };
        let saved = self
            .clear_for_signer(&manifest, previous.as_ref(), &old_fields, signer.as_ref())
            .and_then(|()| {
                self.update_state(|state| {
                    state.plugins.insert(manifest.id.clone(), entry);
                    Ok(())
                })
            });
        if let Err(e) = saved {
            swap_back(&target, aside.as_deref());
            return Err(e);
        }
        // The other versions and leftovers of failed installs go.
        if let Ok(entries) = std::fs::read_dir(&home) {
            for entry in entries.flatten() {
                if entry.file_name() != manifest.version.as_str() {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        self.forget_dropped(&manifest, previous.as_ref(), old_fields);
        Ok(Installed {
            approved: manifest.permissions.clone(),
            manifest,
            enabled,
            description,
            broken: false,
        })
    }

    /// What the plugin `manifest` replaces was given by the user, when it has another signer than
    /// `signer`: its secrets and the state of its pages go. A failure refuses the install, as the
    /// next publisher would read them.
    fn clear_for_signer(
        &self,
        manifest: &Manifest,
        previous: Option<&Entry>,
        old_fields: &[SettingField],
        signer: Option<&Signer>,
    ) -> Result<(), PluginError> {
        let Some(previous) = previous.filter(|e| !same_signer(e.signer.as_ref(), signer)) else {
            return Ok(());
        };
        let mut keys = previous.secrets.clone();
        keys.extend(secret_keys(old_fields));
        keys.extend(secret_keys(&manifest.settings));
        self.settings.forget_secrets(&manifest.id, &keys)?;
        frames::forget_state(&self.dir, &manifest.id)
    }

    /// A field the new version no longer has, or has as another kind, leaves nothing behind: text
    /// that became a secret is not left in the file, nor a secret that became text in the keychain.
    /// Either is out of the module's reach already: the rest of the install stands.
    fn forget_dropped(
        &self,
        manifest: &Manifest,
        previous: Option<&Entry>,
        old_fields: Vec<SettingField>,
    ) {
        let kept = |key: &str, kind| {
            manifest
                .settings
                .iter()
                .any(|f| f.key == key && f.kind == kind)
        };
        let dropped: Vec<_> = old_fields
            .into_iter()
            .filter(|old| !kept(&old.key, old.kind))
            .collect();
        let _ = self.settings.forget(&manifest.id, &dropped);
        let orphans: Vec<_> = previous
            .iter()
            .flat_map(|e| &e.secrets)
            .filter(|key| !kept(key, SettingKind::Secret))
            .cloned()
            .collect();
        let _ = self.settings.forget_secrets(&manifest.id, &orphans);
    }

    /// The installed plugins, by id. One whose files are gone, or are not its own, is still listed,
    /// as `broken`: what is not listed cannot be removed.
    pub fn list(&self) -> Result<Vec<Installed>, PluginError> {
        Ok(self
            .read_state()?
            .plugins
            .into_iter()
            .map(|(id, entry)| {
                self.installed(&id, &entry).unwrap_or_else(|e| {
                    log("WARN", &format!("plugin {id}: {e}"));
                    broken(id, entry)
                })
            })
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
        let installed = self.installed_any(id)?;
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

    /// The bytes of the font a plugin's theme has for `usage` (`ui`, `mono`), for the theme in
    /// use: nothing reads them before. Checked again, as anything may have changed the file.
    pub fn theme_font(&self, id: &str, usage: &str) -> Result<Vec<u8>, PluginError> {
        let installed = self.installed_any(id)?;
        if !installed.enabled {
            return Err(PluginError::Disabled(id.to_owned()));
        }
        let path = installed
            .manifest
            .theme
            .as_ref()
            .and_then(|theme| theme.fonts.get(usage))
            .ok_or_else(|| {
                PluginError::Invalid(format!("the plugin {id:?} has no font for {usage:?}"))
            })?;
        let bytes = read_file(
            &self.module_path(&installed.manifest).join(path),
            theme::MAX_FONT,
        )?;
        theme::check_font(path, &bytes).map_err(PluginError::Invalid)?;
        Ok(bytes)
    }

    /// Removes a plugin, its files, its settings and its secrets.
    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        self.remove_with(id, false)
    }

    /// Removes a plugin and keeps the values of its settings page for a later install. Its
    /// secrets still leave the keychain: nothing would read them until then.
    pub fn remove_keeping_settings(&self, id: &str) -> Result<(), PluginError> {
        self.remove_with(id, true)
    }

    /// Takes a plugin off the list, then clears all there is of it: one thing that fails does not
    /// keep the others, as a second try would find no plugin to remove. A
    /// [`PluginError::PartlyRemoved`] says what is left.
    fn remove_with(&self, id: &str, keep_settings: bool) -> Result<(), PluginError> {
        let entry = self
            .read_state()?
            .plugins
            .remove(id)
            .ok_or_else(|| PluginError::NotInstalled(id.to_owned()))?;
        // The fields are in its manifest, if it is there still; what it installed with is kept too.
        let fields = self
            .installed(id, &entry)
            .map(|p| p.manifest.settings)
            .unwrap_or_default();
        let mut secrets = entry.secrets;
        secrets.extend(secret_keys(&fields));
        self.update_state(|state| {
            state.plugins.remove(id);
            Ok(())
        })?;
        let mut left = Vec::new();
        // An id from a list written before it had to be plain, or by hand, is no path to follow:
        // `foo.` is the folder of `foo` on Windows.
        if is_plain_name(id) {
            let home = self.dir.join(id);
            if home.exists()
                && let Err(e) = std::fs::remove_dir_all(&home)
            {
                left.push(format!("its folder ({e})"));
            }
            if let Err(e) = frames::forget_state(&self.dir, id) {
                left.push(format!("the state of its pages ({e})"));
            }
        }
        let forgotten = if keep_settings {
            self.settings.forget_secrets(id, &secrets)
        } else {
            self.settings.forget_plugin(id, &secrets)
        };
        if let Err(e) = forgotten {
            left.push(format!("its settings or secrets ({e})"));
        }
        if left.is_empty() {
            return Ok(());
        }
        Err(PluginError::PartlyRemoved(format!(
            "The plugin {id:?} is removed, but not {}.",
            left.join(", ")
        )))
    }

    /// An installed plugin, turned on or not.
    fn installed_any(&self, id: &str) -> Result<Installed, PluginError> {
        let state = self.read_state()?;
        let entry = state
            .plugins
            .get(id)
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

    /// The plugin of an entry of the list, from its files. The list and the files must agree: the
    /// permissions there were approved for this id and this version, and for no other's files.
    fn installed(&self, id: &str, entry: &Entry) -> Result<Installed, PluginError> {
        if !is_plain_name(id) || !is_plain_name(&entry.version.to_ascii_lowercase()) {
            return Err(PluginError::Invalid(format!(
                "the list of plugins names {id:?}, version {:?}, which cannot be a folder",
                entry.version
            )));
        }
        let path = self.dir.join(id).join(&entry.version).join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(PluginError::io(&path))?;
        let manifest = Manifest::parse(&text)?;
        if manifest.id != id || manifest.version != entry.version {
            return Err(PluginError::Invalid(format!(
                "{} is the plugin {:?}, version {:?}, and the list says {id:?}, version {:?}",
                path.display(),
                manifest.id,
                manifest.version,
                entry.version
            )));
        }
        Ok(Installed {
            manifest,
            enabled: entry.enabled,
            approved: entry.approved.clone(),
            description: entry.description.clone(),
            broken: false,
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
        let tmp = self.dir.join(STATE_TMP);
        let json =
            serde_json::to_vec_pretty(&state).map_err(|e| PluginError::Invalid(e.to_string()))?;
        std::fs::write(&tmp, json).map_err(PluginError::io(&tmp))?;
        std::fs::rename(&tmp, &path).map_err(PluginError::io(&path))
    }
}

/// Whether two packages have the same signer: the key, whatever the user calls it, or both none.
fn same_signer(a: Option<&Signer>, b: Option<&Signer>) -> bool {
    a.map(|s| &s.key) == b.map(|s| &s.key)
}

/// A plugin of the list whose files cannot be read: its name and version are all there is to show,
/// and it provides nothing.
fn broken(id: String, entry: Entry) -> Installed {
    Installed {
        manifest: Manifest {
            name: id.clone(),
            id,
            version: entry.version,
            author: String::new(),
            description: String::new(),
            api: API_VERSION,
            kinds: Vec::new(),
            hooks: Vec::new(),
            permissions: entry.approved.clone(),
            tabs: Vec::new(),
            panels: Vec::new(),
            grid: GridDecl::default(),
            settings: Vec::new(),
            theme: None,
        },
        enabled: entry.enabled,
        approved: entry.approved,
        description: Description::default(),
        broken: true,
    }
}

/// Puts the folder `new` at `target`. What was there moves aside in `home` first, and is given
/// back to be deleted or restored ([`swap_back`]): a failure at any point leaves it where the list
/// expects it.
fn swap_in(new: &Path, target: &Path, home: &Path) -> Result<Option<PathBuf>, PluginError> {
    let aside = if target.exists() {
        let aside = home.join(format!("{TMP_PREFIX}{}", uuid::Uuid::new_v4()));
        std::fs::rename(target, &aside).map_err(PluginError::io(target))?;
        Some(aside)
    } else {
        None
    };
    if let Err(e) = std::fs::rename(new, target) {
        if let Some(aside) = &aside {
            let _ = std::fs::rename(aside, target);
        }
        return Err(PluginError::io(target)(e));
    }
    Ok(aside)
}

/// Undoes a [`swap_in`] whose install did not go through.
fn swap_back(target: &Path, aside: Option<&Path>) {
    let _ = std::fs::remove_dir_all(target);
    if let Some(aside) = aside {
        let _ = std::fs::rename(aside, target);
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
/// they load and a theme's fonts, and the hash of them all.
struct Package {
    manifest: Manifest,
    wasm: Vec<u8>,
    files: PackageFiles,
    hash: String,
}

/// The lowercase hexadecimal of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
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
    hex(ctx.finish().as_ref())
}

/// The manifest, the module and the fonts of a folder or a `.zip`. A package that needs no module
/// has none, even if a file is there: nothing of it is read, copied or run.
fn read_package(source: &Path) -> Result<Package, PluginError> {
    let mut from = PackageSource::open(source)?;
    let manifest = from.read(MANIFEST_FILE, MAX_MANIFEST)?;
    let text = std::str::from_utf8(&manifest)
        .map_err(|_| PluginError::Invalid(format!("{MANIFEST_FILE} is not UTF-8 text")))?;
    let parsed = Manifest::parse(text)?;
    let wasm = if parsed.needs_module() {
        from.read(MODULE_FILE, MAX_MODULE)?
    } else {
        Vec::new()
    };
    let mut files = from.frame_files(&parsed)?;
    for path in parsed.theme.iter().flat_map(|t| t.fonts.values()) {
        let bytes = from.read(path, theme::MAX_FONT)?;
        theme::check_font(path, &bytes).map_err(PluginError::Invalid)?;
        files.insert(path.clone(), bytes);
    }
    Ok(Package {
        hash: package_hash(&manifest, &wasm, &files),
        manifest: parsed,
        wasm,
        files,
    })
}

pub(crate) fn read_file(path: &Path, max: u64) -> Result<Vec<u8>, PluginError> {
    let file = std::fs::File::open(path).map_err(PluginError::io(path))?;
    read_capped(file, max, path, &path.display().to_string())
}

/// All of `reader`, at most `max` bytes; `at` is where I/O errors are blamed, `label` names it
/// in the size error.
fn read_capped(
    reader: impl std::io::Read,
    max: u64,
    at: &Path,
    label: &str,
) -> Result<Vec<u8>, PluginError> {
    let mut bytes = Vec::new();
    reader
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(PluginError::io(at))?;
    if bytes.len() as u64 > max {
        return Err(PluginError::Invalid(format!(
            "{label} is larger than {} KB",
            max >> 10
        )));
    }
    Ok(bytes)
}

/// Where the files of a package are read from.
enum PackageSource {
    Folder(PathBuf),
    Zip {
        archive: zip::ZipArchive<std::fs::File>,
        source: PathBuf,
        /// The names in it, as written.
        names: Vec<String>,
        /// The folder the manifest is in, with its slash; empty at the root.
        prefix: String,
    },
}

impl PackageSource {
    /// A folder, or a `.zip` whose names all stay inside it and with the manifest at its root or
    /// in its one folder.
    fn open(source: &Path) -> Result<Self, PluginError> {
        if source.is_dir() {
            return Ok(Self::Folder(source.to_owned()));
        }
        let file = std::fs::File::open(source).map_err(PluginError::io(source))?;
        let archive = zip::ZipArchive::new(file).map_err(|e| {
            PluginError::Invalid(format!(
                "{} is neither a folder nor a .zip package: {e}",
                source.display()
            ))
        })?;
        let names = archive
            .file_names()
            .map(|name| {
                name.map(std::borrow::Cow::into_owned).map_err(|e| {
                    PluginError::Invalid(format!("the .zip has a name it cannot read: {e}"))
                })
            })
            .collect::<Result<Vec<String>, _>>()?;
        if let Some(name) = names.iter().find(|name| climbs_out(name)) {
            return Err(PluginError::Invalid(format!(
                "the .zip has a path that climbs out of it: {name:?}"
            )));
        }
        let manifest_name = names
            .iter()
            .filter(|n| n.as_str() == MANIFEST_FILE || n.ends_with(&format!("/{MANIFEST_FILE}")))
            .min_by_key(|n| n.matches('/').count())
            .ok_or_else(|| PluginError::Invalid(format!("the .zip has no {MANIFEST_FILE}")))?;
        if manifest_name.matches('/').count() > 1 {
            return Err(PluginError::Invalid(format!(
                "{MANIFEST_FILE} must be at the root of the .zip or in its one folder"
            )));
        }
        let prefix = manifest_name[..manifest_name.len() - MANIFEST_FILE.len()].to_owned();
        Ok(Self::Zip {
            archive,
            source: source.to_owned(),
            names,
            prefix,
        })
    }

    /// The file at `path` of the package, at most `max` bytes.
    fn read(&mut self, path: &str, max: u64) -> Result<Vec<u8>, PluginError> {
        let (archive, source, prefix) = match self {
            Self::Folder(folder) => return read_file(&folder.join(path), max),
            Self::Zip {
                archive,
                source,
                prefix,
                ..
            } => (archive, source, prefix),
        };
        let name = format!("{prefix}{path}");
        let entry = archive
            .by_name(&name)
            .map_err(|_| PluginError::Invalid(format!("the .zip has no {name}")))?;
        read_capped(entry, max, source, &name)
    }

    /// The files of the tabs and panels of `manifest`, by their paths in the package.
    fn frame_files(&mut self, manifest: &Manifest) -> Result<PackageFiles, PluginError> {
        let inside: Vec<String> = match self {
            Self::Folder(folder) => return frames::read_folder_files(folder, manifest),
            Self::Zip { names, prefix, .. } => names
                .iter()
                .filter_map(|n| n.strip_prefix(prefix.as_str()))
                .map(str::to_owned)
                .collect(),
        };
        let wanted = frames::choose_files(manifest, &inside)?;
        let mut files = PackageFiles::new();
        let mut budget = frames::Budget::default();
        for path in wanted {
            let bytes = self.read(&path, frames::MAX_FILE)?;
            budget.spend(&path, bytes.len())?;
            files.insert(path, bytes);
        }
        Ok(files)
    }
}

/// A name in a `.zip` that would land outside the folder it is unpacked in: absolute, with a
/// drive, or going up, with either kind of slash.
fn climbs_out(name: &str) -> bool {
    let name = name.replace('\\', "/");
    name.starts_with('/')
        || name.as_bytes().get(1) == Some(&b':')
        || name.split('/').any(|part| part == "..")
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
