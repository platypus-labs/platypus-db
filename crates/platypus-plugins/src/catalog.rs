//! The catalog: a list of plugins from an index (JSON) at an address the user set, and the checks a
//! package goes through before it reaches the same review as a manual install.
//!
//! Nothing here runs by itself: an index is fetched when the user opens the page or asks, and a
//! package downloaded when the user asks to install it. The checks, in order, each refusing with
//! its reason: the address is HTTPS (or on this computer, for an index that is too) and no nearer
//! than the index, which holds for a name too, whatever it resolves to; the download fits the limit; its SHA-256 is the one in the index; it has a detached minisign signature by a
//! key the user trusts, unless the user confirmed installing it as unsigned; its names stay inside
//! the `.zip`; its id and version are the entry's; it is not older than what is installed, unless
//! asked. Only then the module is read for the review, and installing takes the hash of what was
//! reviewed, as any install does ([`Plugins::install`]).

use std::cmp::Ordering;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use minisign_verify::{Error as SignatureError, PublicKey, Signature};
use reqwest::dns::{Name, Resolve, Resolving};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::PluginError;
use crate::http::{client_builder, describe};
use crate::manifest::is_plain_name;
use crate::package::{Inspected, Installed, Plugins, Signer, hex};

/// The folder under the plugins' where a package waits between its checks and the user's answer.
const DOWNLOADS: &str = ".downloads";
/// Beside a download, the file that says who signed it.
const SIGNER_EXTENSION: &str = "signer";
/// The format of the index this version reads.
const INDEX_FORMAT: u32 = 1;
/// Redirects followed for one download.
const MAX_REDIRECTS: usize = 5;
/// The largest signature text of an entry.
const MAX_SIGNATURE: usize = 4 << 10;
/// The longest name of a plugin in the index, in characters.
const MAX_NAME: usize = 200;
/// The longest description or author of a plugin in the index, in characters.
const MAX_TEXT: usize = 2000;

/// How much a catalog downloads, and for how long.
#[derive(Debug, Clone, Copy)]
pub struct CatalogLimits {
    /// The largest index.
    pub index: u64,
    /// The largest package.
    pub package: u64,
    pub connect_timeout: Duration,
    pub timeout: Duration,
}

impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            index: 2 << 20,
            package: 64 << 20,
            connect_timeout: Duration::from_secs(15),
            timeout: Duration::from_secs(120),
        }
    }
}

/// One plugin of an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The package's address, absolute after [`parse_index`].
    pub url: String,
    /// The package's SHA-256, in hexadecimal.
    pub sha256: String,
    /// The text of the package's detached minisign signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// An entry of an index that was left out, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexProblem {
    /// Its id, if it has a readable one.
    pub id: Option<String>,
    pub reason: String,
}

/// How an entry of the index stands against what is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryStatus {
    /// Not installed.
    New,
    /// Newer than the installed version.
    Update,
    /// The installed version.
    Current,
    /// Older than the installed version.
    Older,
}

/// An entry with what the app has of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogItem {
    pub entry: IndexEntry,
    pub installed: Option<String>,
    pub status: EntryStatus,
}

/// What the page shows: the index's address, its plugins and the entries it left out. An empty
/// address contacts nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CatalogView {
    pub url: String,
    pub items: Vec<CatalogItem>,
    pub problems: Vec<IndexProblem>,
}

/// A public key the user trusts for packages, with a name of their choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedKey {
    pub name: String,
    /// The minisign public key, in base64.
    pub key: String,
}

impl TrustedKey {
    /// A key from what the user pasted: the base64 line of a minisign public key, or the whole
    /// `.pub` file. A blank name becomes the start of the key.
    pub fn parse(name: &str, text: &str) -> Result<Self, PluginError> {
        let key = text
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or_default();
        PublicKey::from_base64(key).map_err(|_| {
            PluginError::Catalog("this is not a minisign public key (a line of base64)".to_owned())
        })?;
        let name = name.trim();
        let name = if name.is_empty() {
            key.chars().take(12).collect()
        } else {
            name.chars().take(MAX_NAME).collect()
        };
        Ok(Self {
            name,
            key: key.to_owned(),
        })
    }
}

/// What the user asked to install, and what they allowed.
#[derive(Debug, Clone)]
pub struct CatalogRequest {
    pub entry: IndexEntry,
    /// The address of the index it came from: a package on this computer needs an index there too.
    pub index_url: String,
    pub keys: Vec<TrustedKey>,
    /// The user confirmed installing it without a valid signature.
    pub allow_unsigned: bool,
    /// The user asked for this version although a newer one is installed.
    pub allow_downgrade: bool,
}

/// What the signature check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SignatureState {
    /// Signed by this trusted key.
    Verified { key: String },
    /// No valid signature, for this reason, and the user confirmed going on.
    Unsigned { reason: String },
}

/// A package that passed the checks and waits for the user's answer: the same review as a manual
/// install, then [`Plugins::catalog_install`] or [`Plugins::catalog_discard`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prepared {
    pub ticket: String,
    pub inspected: Inspected,
    pub signature: SignatureState,
}

fn refused(why: impl Into<String>) -> PluginError {
    PluginError::Catalog(why.into())
}

/// Where an address leads, the nearest last. An index reaches nothing nearer than itself, so one on
/// the network cannot send the app to this computer's services or to those of its network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Network,
    PrivateNetwork,
    ThisComputer,
}

fn place_of_v4(ip: Ipv4Addr) -> Place {
    let [a, b, c, _] = ip.octets();
    if ip.is_loopback() || a == 0 {
        Place::ThisComputer
    } else if ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
    {
        Place::PrivateNetwork
    } else {
        Place::Network
    }
}

/// The IPv4 address an IPv6 one carries: mapped, compatible, NAT64 or 6to4.
fn carried_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let [s0, s1, s2, s3, s4, s5, s6, s7] = ip.segments();
    let tail = |high: u16, low: u16| {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        Some(Ipv4Addr::new(a, b, c, d))
    };
    match (s0, s1, s2, s3, s4, s5) {
        (0, 0, 0, 0, 0, 0 | 0xffff) | (0x64, 0xff9b, 0, 0, 0, 0) => tail(s6, s7),
        (0x2002, ..) => tail(s1, s2),
        _ => None,
    }
}

fn place_of_ip(ip: IpAddr) -> Place {
    let ip = match ip {
        IpAddr::V4(ip) => return place_of_v4(ip),
        IpAddr::V6(ip) => ip,
    };
    if ip.is_loopback() || ip.is_unspecified() {
        return Place::ThisComputer;
    }
    if let Some(v4) = carried_v4(ip) {
        return place_of_v4(v4);
    }
    let first = ip.segments()[0];
    let unique_local = first & 0xfe00 == 0xfc00;
    let link_or_site_local = first & 0xff80 == 0xfe80;
    if unique_local || link_or_site_local || ip.is_multicast() {
        Place::PrivateNetwork
    } else {
        Place::Network
    }
}

/// Where the URL's host leads, as far as it says by itself: a name is the network here, until it is
/// resolved ([`ResolvedPlace`]). `localhost` and what is under it are this computer, with or without
/// the dot of a full name.
fn place_of(url: &Url) -> Place {
    match url.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            if host == "localhost" || host.ends_with(".localhost") {
                Place::ThisComputer
            } else {
                Place::Network
            }
        }
        Some(url::Host::Ipv4(ip)) => place_of_v4(ip),
        Some(url::Host::Ipv6(ip)) => place_of_ip(IpAddr::V6(ip)),
        None => Place::Network,
    }
}

/// Resolves a name to its addresses no nearer than `from`: what a name says now counts, not what it
/// said when the address was checked, so neither a name that points inside nor one that changes its
/// mind between two requests gets further than the address itself would. None left is an error that
/// does not say which ones there were.
#[derive(Debug, Clone, Copy)]
struct ResolvedPlace {
    from: Place,
}

fn reachable(addresses: impl Iterator<Item = SocketAddr>, from: Place) -> Vec<SocketAddr> {
    addresses
        .filter(|address| place_of_ip(address.ip()) <= from)
        .collect()
}

async fn resolve_from(
    name: Name,
    from: Place,
) -> Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> {
    let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
    let left = reachable(found, from);
    if left.is_empty() {
        return Err("this name leads to an address the catalog may not reach".into());
    }
    Ok(Box::new(left.into_iter()))
}

impl Resolve for ResolvedPlace {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(resolve_from(name, self.from))
    }
}

/// An address the catalog may reach from where the request starts: HTTPS, or HTTP or HTTPS on this
/// computer, and no nearer than `from`.
fn check_url(text: &str, from: Place) -> Result<Url, PluginError> {
    let url = Url::parse(text).map_err(|e| refused(format!("{text:?} is not an address: {e}")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refused(format!(
            "{url} has a user name or a password in it"
        )));
    }
    let place = place_of(&url);
    if place > from {
        let there = if place == Place::ThisComputer {
            "on this computer"
        } else {
            "on a private network"
        };
        return Err(refused(format!("{url} is {there}, and the index is not")));
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if place == Place::ThisComputer => Ok(url),
        _ => Err(refused(format!(
            "{url} is not HTTPS: the catalog only uses HTTPS, or an address on this computer"
        ))),
    }
}

/// Checks the address of an index as the user typed it; an empty one is no index.
pub fn check_index_url(text: &str) -> Result<String, PluginError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    check_url(text, Place::ThisComputer).map(|_| text.to_owned())
}

/// Where requests made for the index at `index_url` start: where the index is by its address. By a
/// name it is on the network, whatever it resolves to, as that is the name's owner to say.
fn start_of(index_url: &str) -> Place {
    Url::parse(index_url).map_or(Place::Network, |url| place_of(&url))
}

/// The client for one download of `url`: redirects and resolved names go through the same checks as
/// the address. A proxy resolves names by itself, so none is used for what it could not reach.
fn client_for(
    url: &Url,
    from: Place,
    limits: CatalogLimits,
) -> Result<reqwest::Client, PluginError> {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match check_url(attempt.url().as_str(), from) {
            Ok(_) => attempt.follow(),
            Err(e) => attempt.error(e.to_string()),
        }
    });
    let mut builder = client_builder(limits.connect_timeout)?.redirect(policy);
    if place_of(url) > Place::Network {
        builder = builder.no_proxy();
    }
    if from < Place::ThisComputer {
        builder = builder.dns_resolver(Arc::new(ResolvedPlace { from }));
    }
    builder
        .build()
        .map_err(|e| refused(format!("cannot set up HTTP: {e}")))
}

/// Downloads `url` into memory, at most `max` bytes; `what` names it in the refusals. A redirect
/// goes through the same check as the address.
async fn download(
    url: Url,
    from: Place,
    max: u64,
    limits: CatalogLimits,
    what: &str,
) -> Result<Vec<u8>, PluginError> {
    let client = client_for(&url, from, limits)?;
    let failed = |e: &reqwest::Error| refused(format!("cannot download {what}: {}", describe(e)));
    let mut response = client
        .get(url.clone())
        .timeout(limits.timeout)
        .send()
        .await
        .map_err(|e| failed(&e))?;
    if !response.status().is_success() {
        return Err(refused(format!(
            "cannot download {what}: {url} answered {}",
            response.status()
        )));
    }
    let too_large = || refused(format!("{what} is larger than {} KB", max >> 10));
    if response.content_length().is_some_and(|length| length > max) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| failed(&e))? {
        if (body.len() + chunk.len()) as u64 > max {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reads an index: its known format, and each entry that is well formed, with the package's address
/// made absolute against the index's. An entry that is not goes in `problems` with its reason; the
/// others stay usable.
pub fn parse_index(
    text: &[u8],
    index_url: &str,
) -> Result<(Vec<IndexEntry>, Vec<IndexProblem>), PluginError> {
    #[derive(Deserialize)]
    struct Raw {
        format: u32,
        plugins: Vec<Value>,
    }
    let raw: Raw = serde_json::from_slice(text)
        .map_err(|e| refused(format!("the index is not a plugin index: {e}")))?;
    if raw.format != INDEX_FORMAT {
        return Err(refused(format!(
            "the index has format {}, and this version of the app reads format {INDEX_FORMAT}",
            raw.format
        )));
    }
    let base = Url::parse(index_url).map_err(|e| refused(format!("{index_url:?}: {e}")))?;
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut problems = Vec::new();
    for value in raw.plugins {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let read = serde_json::from_value::<IndexEntry>(value)
            .map_err(|e| e.to_string())
            .and_then(|entry| check_entry(entry, &base));
        match read {
            Ok(entry) if entries.iter().any(|e| e.id == entry.id) => problems.push(IndexProblem {
                id,
                reason: "the index lists this plugin twice".to_owned(),
            }),
            Ok(entry) => entries.push(entry),
            Err(reason) => problems.push(IndexProblem { id, reason }),
        }
    }
    Ok((entries, problems))
}

/// An entry that can be offered, its address absolute.
fn check_entry(mut entry: IndexEntry, base: &Url) -> Result<IndexEntry, String> {
    if !is_plain_name(&entry.id) {
        return Err(format!("the id {:?} is not valid", entry.id));
    }
    if !is_plain_name(&entry.version.to_ascii_lowercase()) {
        return Err(format!("the version {:?} is not valid", entry.version));
    }
    if entry.name.trim().is_empty() || entry.name.chars().count() > MAX_NAME {
        return Err("the name is empty or too long".to_owned());
    }
    for text in [&entry.author, &entry.description].into_iter().flatten() {
        if text.chars().count() > MAX_TEXT {
            return Err("the author or the description is too long".to_owned());
        }
    }
    entry.sha256 = entry.sha256.trim().to_ascii_lowercase();
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("`sha256` is not a SHA-256 in hexadecimal".to_owned());
    }
    if entry
        .signature
        .as_ref()
        .is_some_and(|s| s.len() > MAX_SIGNATURE)
    {
        return Err("`signature` is too long".to_owned());
    }
    let url = base
        .join(&entry.url)
        .map_err(|e| format!("{:?}: {e}", entry.url))?;
    let url = check_url(url.as_str(), place_of(base)).map_err(|e| e.to_string())?;
    entry.url = url.into();
    Ok(entry)
}

/// Orders two versions: their dotted parts, as numbers where both are, a missing part being 0, and
/// a pre-release (`1.0.0-rc1`) before its release.
#[must_use]
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    let (a_release, a_pre) = split_release(&a);
    let (b_release, b_pre) = split_release(&b);
    compare_dotted(a_release, b_release).then_with(|| match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => compare_dotted(a, b),
    })
}

fn split_release(version: &str) -> (&str, Option<&str>) {
    version
        .split_once('-')
        .map_or((version, None), |(release, pre)| (release, Some(pre)))
}

fn compare_dotted(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            return Ordering::Equal;
        }
        let (x, y) = (x.unwrap_or("0"), y.unwrap_or("0"));
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Greater,
            (Err(_), Ok(_)) => Ordering::Less,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if order.is_ne() {
            return order;
        }
    }
}

/// The trusted key that signed `bytes`, or why none did.
fn check_signature(
    bytes: &[u8],
    signature: Option<&str>,
    keys: &[TrustedKey],
) -> Result<Signer, String> {
    let Some(text) = signature.filter(|s| !s.trim().is_empty()) else {
        return Err("The index gives no signature for this package.".to_owned());
    };
    let signature =
        Signature::decode(text).map_err(|_| "Its signature cannot be read.".to_owned())?;
    if keys.is_empty() {
        return Err("You trust no key yet.".to_owned());
    }
    let mut known_key = false;
    for key in keys {
        let Ok(public) = PublicKey::from_base64(&key.key) else {
            continue;
        };
        match public.verify(bytes, &signature, false) {
            Ok(()) => {
                return Ok(Signer {
                    key: key.key.clone(),
                    name: key.name.clone(),
                });
            }
            Err(SignatureError::UnexpectedKeyId) => {}
            Err(_) => known_key = true,
        }
    }
    Err(if known_key {
        "The signature does not match the package: it may have been altered.".to_owned()
    } else {
        "It is not signed by a key you trust.".to_owned()
    })
}

impl Plugins {
    /// The catalog of the index at `url`, against what is installed. An empty address gives an
    /// empty catalog and contacts nothing.
    pub async fn catalog_fetch(
        &self,
        url: &str,
        limits: CatalogLimits,
    ) -> Result<CatalogView, PluginError> {
        if url.trim().is_empty() {
            return Ok(CatalogView::default());
        }
        let from = start_of(url);
        let address = check_url(url, from)?;
        let body = download(address, from, limits.index, limits, "the index").await?;
        let (entries, problems) = parse_index(&body, url)?;
        let installed = self.list()?;
        let items = entries
            .into_iter()
            .map(|entry| {
                let current = installed
                    .iter()
                    .find(|p| p.manifest.id == entry.id)
                    .map(|p| p.manifest.version.clone());
                let status = current.as_deref().map_or(EntryStatus::New, |version| {
                    match compare_versions(&entry.version, version) {
                        Ordering::Greater => EntryStatus::Update,
                        Ordering::Equal => EntryStatus::Current,
                        Ordering::Less => EntryStatus::Older,
                    }
                });
                CatalogItem {
                    entry,
                    installed: current,
                    status,
                }
            })
            .collect();
        Ok(CatalogView {
            url: url.to_owned(),
            items,
            problems,
        })
    }

    fn downloads(&self) -> PathBuf {
        self.dir().join(DOWNLOADS)
    }

    fn download_path(&self, ticket: &str) -> Result<PathBuf, PluginError> {
        let ticket = Uuid::parse_str(ticket)
            .map_err(|_| refused("this download is gone: ask to install it again"))?;
        Ok(self.downloads().join(format!("{ticket}.zip")))
    }

    /// Who signed the download at `path`, as its check found: known to no one but this file, which
    /// the page does not name. Without one, it is not signed.
    fn signer_of(path: &Path) -> Option<Signer> {
        let text = std::fs::read(path.with_extension(SIGNER_EXTENSION)).ok()?;
        serde_json::from_slice(&text).ok()
    }

    /// Downloads and checks the package of `request.entry` for the user's review. Nothing of it is
    /// installed, and nothing of it is read before its hash and signature are checked.
    pub async fn catalog_prepare(
        &self,
        request: CatalogRequest,
        limits: CatalogLimits,
    ) -> Result<Prepared, PluginError> {
        let from = start_of(&request.index_url);
        let base = Url::parse(&request.index_url)
            .map_err(|e| refused(format!("{:?}: {e}", request.index_url)))?;
        let entry = check_entry(request.entry, &base).map_err(refused)?;
        let url = check_url(&entry.url, from)?;
        let body = download(url, from, limits.package, limits, "the package").await?;
        let digest = hex(ring::digest::digest(&ring::digest::SHA256, &body).as_ref());
        if digest != entry.sha256 {
            return Err(refused(format!(
                "the download's SHA-256 is {digest}, and the index says {}: it is not the package the index lists",
                entry.sha256
            )));
        }
        let (signature, signer) =
            match check_signature(&body, entry.signature.as_deref(), &request.keys) {
                Ok(signer) => {
                    let key = signer.name.clone();
                    (SignatureState::Verified { key }, Some(signer))
                }
                Err(reason) if request.allow_unsigned => {
                    (SignatureState::Unsigned { reason }, None)
                }
                Err(reason) => return Err(PluginError::Unsigned(reason)),
            };
        // One download waits at a time: what an earlier answer left goes.
        let folder = self.downloads();
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).map_err(PluginError::io(&folder))?;
        let ticket = Uuid::new_v4().to_string();
        let path = self.download_path(&ticket)?;
        std::fs::write(&path, &body).map_err(PluginError::io(&path))?;
        if let Some(signer) = &signer {
            let beside = path.with_extension(SIGNER_EXTENSION);
            let text = serde_json::to_vec(signer).map_err(|e| refused(e.to_string()))?;
            std::fs::write(&beside, text).map_err(PluginError::io(&beside))?;
        }
        let reviewed = self
            .review_download(&path, &entry, signer.as_ref(), request.allow_downgrade)
            .await;
        match reviewed {
            Ok(inspected) => Ok(Prepared {
                ticket,
                inspected,
                signature,
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&folder);
                Err(e)
            }
        }
    }

    /// The review of a download whose hash and signature are good: it is the package the entry
    /// says, and not a step back unless the user asked for one.
    async fn review_download(
        &self,
        path: &Path,
        entry: &IndexEntry,
        signer: Option<&Signer>,
        allow_downgrade: bool,
    ) -> Result<Inspected, PluginError> {
        let inspected = self.inspect_signed(path, signer).await?;
        let manifest = &inspected.manifest;
        if manifest.id != entry.id {
            return Err(refused(format!(
                "the package is the plugin {:?}, and the index lists {:?}",
                manifest.id, entry.id
            )));
        }
        if manifest.version != entry.version {
            return Err(refused(format!(
                "the package is version {}, and the index lists {}",
                manifest.version, entry.version
            )));
        }
        if let Some(installed) = &inspected.replaces
            && !allow_downgrade
            && compare_versions(&entry.version, installed) == Ordering::Less
        {
            return Err(PluginError::Downgrade {
                installed: installed.clone(),
                offered: entry.version.clone(),
            });
        }
        Ok(inspected)
    }

    /// Installs the package the user reviewed. `hash` is the review's: as in a manual install, a
    /// package changed since is refused. The download is gone afterwards, either way.
    pub async fn catalog_install(
        &self,
        ticket: &str,
        hash: &str,
    ) -> Result<Installed, PluginError> {
        let path = self.download_path(ticket)?;
        if !path.exists() {
            return Err(refused("this download is gone: ask to install it again"));
        }
        let installed = self
            .install_signed(&path, hash, Self::signer_of(&path))
            .await;
        self.catalog_discard(ticket);
        installed
    }

    /// Forgets a download the user turned down.
    pub fn catalog_discard(&self, ticket: &str) {
        if let Ok(path) = self.download_path(ticket) {
            let _ = std::fs::remove_file(path.with_extension(SIGNER_EXTENSION));
            let _ = std::fs::remove_file(path);
        }
        // The folder goes with its last file.
        let _ = std::fs::remove_dir(self.downloads());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "ab";

    fn entry() -> serde_json::Value {
        serde_json::json!({
            "id": "sea",
            "name": "Sea",
            "version": "1.0.0",
            "url": "sea.zip",
            "sha256": HASH.repeat(32),
        })
    }

    fn index(plugins: &[Value]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "format": 1, "plugins": plugins })).unwrap()
    }

    #[test]
    fn only_https_except_on_this_computer() {
        for good in [
            "https://example.com/index.json",
            "http://localhost:8080/index.json",
            "http://127.0.0.1:8080/index.json",
            "http://[::1]:8080/index.json",
            "https://localhost/index.json",
        ] {
            assert_eq!(check_index_url(good).unwrap(), good);
        }
        assert_eq!(check_index_url("  ").unwrap(), "");
        // What the user types may be anywhere: only an index on the network is held back.
        for good in [
            "http://localhost.:8080/i.json",
            "http://[::ffff:127.0.0.1]:8080/i.json",
            "http://0.0.0.0:8080/i.json",
            "https://192.168.1.2/i.json",
        ] {
            assert_eq!(check_index_url(good).unwrap(), good);
        }
        for (bad, why) in [
            ("http://example.com/index.json", "not HTTPS"),
            ("ftp://example.com/index.json", "not HTTPS"),
            ("file:///c:/index.json", "not HTTPS"),
            ("http://localhost.example.com/index.json", "not HTTPS"),
            ("http://127.0.0.1.example.com/index.json", "not HTTPS"),
            ("http://192.168.1.2/index.json", "not HTTPS"),
            ("https://user:secret@example.com/index.json", "password"),
            ("example.com/index.json", "not an address"),
        ] {
            let e = check_index_url(bad).unwrap_err().to_string();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }

    #[test]
    fn an_index_on_the_network_cannot_reach_this_computer() {
        let plugin = {
            let mut e = entry();
            e["url"] = "http://127.0.0.1:9000/secret".into();
            e
        };
        let (entries, problems) = parse_index(
            &index(std::slice::from_ref(&plugin)),
            "https://example.com/i.json",
        )
        .unwrap();
        assert_eq!(entries.len(), 0);
        assert!(problems[0].reason.contains("this computer"), "{problems:?}");
        // One on this computer may.
        let (entries, _) = parse_index(&index(&[plugin]), "http://localhost:1/i.json").unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn every_way_to_write_a_near_address_is_near() {
        for near in [
            "https://localhost/",
            "https://localhost./",
            "https://LOCALHOST/",
            "https://a.localhost/",
            "https://127.0.0.1/",
            "https://127.1/",
            "https://2130706433/",
            "https://0x7f.0.0.1/",
            "https://0.0.0.0/",
            "https://0.1.2.3/",
            "https://[::1]/",
            "https://[::]/",
            "https://[::ffff:127.0.0.1]/",
            "https://[::127.0.0.1]/",
            "https://[64:ff9b::7f00:1]/",
            "https://[2002:7f00:1::]/",
        ] {
            let url = Url::parse(near).unwrap();
            assert_eq!(place_of(&url), Place::ThisComputer, "{near}");
            let e = check_url(near, Place::Network).unwrap_err().to_string();
            assert!(e.contains("on this computer"), "{near}: {e}");
            assert!(check_url(near, Place::PrivateNetwork).is_err(), "{near}");
            assert!(check_url(near, Place::ThisComputer).is_ok(), "{near}");
        }
        for private in [
            "https://10.0.0.1/",
            "https://172.16.0.1/",
            "https://172.31.255.255/",
            "https://192.168.0.1/",
            "https://169.254.169.254/",
            "https://100.64.0.1/",
            "https://198.18.0.1/",
            "https://192.0.0.1/",
            "https://224.0.0.1/",
            "https://240.0.0.1/",
            "https://255.255.255.255/",
            "https://[fc00::1]/",
            "https://[fd12:3456::1]/",
            "https://[fe80::1]/",
            "https://[fec0::1]/",
            "https://[ff02::1]/",
            "https://[::ffff:10.0.0.1]/",
            "https://[::ffff:169.254.169.254]/",
            "https://[64:ff9b::a00:1]/",
            "https://[2002:c0a8:1::]/",
        ] {
            let url = Url::parse(private).unwrap();
            assert_eq!(place_of(&url), Place::PrivateNetwork, "{private}");
            let e = check_url(private, Place::Network).unwrap_err().to_string();
            assert!(e.contains("private network"), "{private}: {e}");
            assert!(
                check_url(private, Place::PrivateNetwork).is_ok(),
                "{private}"
            );
        }
        for far in [
            "https://example.com/",
            "https://localhost.example.com/",
            "https://8.8.8.8/",
            "https://172.32.0.1/",
            "https://100.128.0.1/",
            "https://[2606:4700::1111]/",
            "https://[::ffff:8.8.8.8]/",
        ] {
            let url = Url::parse(far).unwrap();
            assert_eq!(place_of(&url), Place::Network, "{far}");
            assert!(check_url(far, Place::Network).is_ok(), "{far}");
        }
    }

    #[test]
    fn a_name_keeps_only_the_addresses_it_may_reach() {
        let addresses: Vec<SocketAddr> = ["8.8.8.8:0", "127.0.0.1:0", "10.0.0.1:0", "[::1]:0"]
            .iter()
            .map(|a| a.parse().unwrap())
            .collect();
        let kept = |from| reachable(addresses.iter().copied(), from).len();
        assert_eq!(kept(Place::Network), 1);
        assert_eq!(kept(Place::PrivateNetwork), 2);
        assert_eq!(kept(Place::ThisComputer), 4);
    }

    /// A server on this computer, reached by a name: an index on the network never gets there,
    /// and the refusal says nothing of where it leads.
    #[tokio::test]
    async fn a_name_that_resolves_near_is_refused_before_anything_answers() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0; 1024];
                let _ = socket.read(&mut request).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            }
        });
        let url = Url::parse(&format!("http://localhost:{port}/")).unwrap();
        let limits = CatalogLimits::default();

        let from_network = client_for(&url, Place::Network, limits).unwrap();
        let e = from_network.get(url.clone()).send().await.unwrap_err();
        let said = describe(&e);
        assert!(said.contains("may not reach"), "{said}");
        assert!(!said.contains("127.0.0.1"), "{said}");

        let from_here = client_for(&url, Place::ThisComputer, limits).unwrap();
        assert!(
            from_here
                .get(url)
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }

    #[test]
    fn reads_an_index_and_says_what_it_left_out() {
        let bad_hash = {
            let mut e = entry();
            e["id"] = "bad-hash".into();
            e["sha256"] = "nope".into();
            e
        };
        let climbing_id = {
            let mut e = entry();
            e["id"] = "../up".into();
            e
        };
        let http = {
            let mut e = entry();
            e["id"] = "plain".into();
            e["url"] = "http://example.com/plain.zip".into();
            e
        };
        let future_fields = {
            let mut e = entry();
            e["id"] = "future".into();
            e["homepage"] = "https://example.com".into();
            e
        };
        let (entries, problems) = parse_index(
            &index(&[
                entry(),
                entry(),
                bad_hash,
                climbing_id,
                http,
                future_fields,
                serde_json::json!("x"),
            ]),
            "https://example.com/plugins/index.json",
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com/plugins/sea.zip");
        assert_eq!(entries[0].sha256, HASH.repeat(32));
        let reasons: Vec<_> = problems.iter().map(|p| p.reason.as_str()).collect();
        assert_eq!(reasons.len(), 5, "{reasons:?}");
        assert!(reasons[0].contains("twice"));
        assert!(reasons[1].contains("sha256"));
        assert!(reasons[2].contains("id"));
        assert!(reasons[3].contains("not HTTPS"));

        for (text, why) in [
            (&b"[1]"[..], "not a plugin index"),
            (&b"{nope"[..], "not a plugin index"),
            (&br#"{"format":2,"plugins":[]}"#[..], "format 2"),
        ] {
            let e = parse_index(text, "https://example.com/").unwrap_err();
            assert!(e.to_string().contains(why), "{e}");
        }
    }

    #[test]
    fn orders_versions() {
        for (a, b, order) in [
            ("1.0.0", "1.0.0", Ordering::Equal),
            ("1.0", "1.0.0", Ordering::Equal),
            ("1.10.0", "1.9.0", Ordering::Greater),
            ("2", "10", Ordering::Less),
            ("1.0.0-rc1", "1.0.0", Ordering::Less),
            ("1.0.0", "1.0.0-rc1", Ordering::Greater),
            ("1.0.0-rc2", "1.0.0-rc10", Ordering::Greater),
            ("1.0.0-rc.2", "1.0.0-rc.10", Ordering::Less),
            ("1.0.a", "1.0.1", Ordering::Less),
            ("1.0.0", "1.0.0-RC1", Ordering::Greater),
        ] {
            assert_eq!(compare_versions(a, b), order, "{a} {b}");
        }
    }

    #[test]
    fn a_trusted_key_is_a_key() {
        const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let key = TrustedKey::parse(" Jane ", KEY).unwrap();
        assert_eq!((key.name.as_str(), key.key.as_str()), ("Jane", KEY));
        let file = format!("untrusted comment: minisign public key\n{KEY}\n\n");
        let key = TrustedKey::parse("", &file).unwrap();
        assert_eq!(key.key, KEY);
        assert_eq!(key.name, &KEY[..12]);
        for bad in ["", "nope", "untrusted comment: only this"] {
            let e = TrustedKey::parse("x", bad).unwrap_err().to_string();
            assert!(e.contains("not a minisign public key"), "{bad}: {e}");
        }
    }
}
